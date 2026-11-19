//! The window (egui via eframe). Runs on the main thread (Windows requires it); the engine runs on a
//! background thread with its own Tokio runtime. They only talk through the Command/Event channels.

use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use eframe::egui;
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, error::TryRecvError, unbounded_channel,
};

use crate::cli::{percent_to_volume, volume_to_percent};
use crate::engine::{self, Command, EngineConfig, Event, EventSender, TrackInfo};
use crate::webapi::{PlaylistSummary, TrackSummary};

/// Open the window and run until it is closed; then stop the engine and wait for it.
pub fn run(to_file: Option<PathBuf>) -> Result<()> {
    let (cmd_tx, cmd_rx) = unbounded_channel::<Command>();
    let (evt_tx, evt_rx) = unbounded_channel::<Event>();

    // The engine wakes the window after every event. The egui context only exists once the window is
    // created, so the engine gets a slot that is filled in then; events before that are still queued and
    // shown on the first frame.
    let ctx_slot: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
    let notify = {
        let ctx_slot = Arc::clone(&ctx_slot);
        move || {
            if let Some(ctx) = ctx_slot.get() {
                ctx.request_repaint();
            }
        }
    };

    let runtime = tokio::runtime::Runtime::new().context("could not start the Tokio runtime")?;
    let engine_thread = std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || {
            runtime.block_on(engine::run_engine(
                EngineConfig { to_file },
                cmd_rx,
                EventSender::new(evt_tx, notify),
            ));
            // A first-time Web API login waits on a blocking thread for the browser. Don't let it keep
            // the app alive after the window is closed.
            runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        })
        .context("could not start the engine thread")?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rusty-player")
            .with_inner_size([760.0, 400.0]),
        ..Default::default()
    };
    let window_result = eframe::run_native(
        "rusty-player",
        options,
        Box::new(move |cc| {
            let _ = ctx_slot.set(cc.egui_ctx.clone());
            // Lets `ui.image(url)` download and show the album cover (http + jpeg/png).
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(PlayerApp::new(cmd_tx, evt_rx)))
        }),
    );

    // The window is closed and `PlayerApp` (with the only command sender) is dropped, so the engine sees
    // its command channel close and shuts down, also if it was still logging in.
    let engine_result = engine_thread
        .join()
        .map_err(|_| anyhow!("the engine thread panicked"));
    window_result.map_err(|e| anyhow!("window error: {e}"))?;
    engine_result
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Progress {
    position_ms: u32,
    since: Instant,
    running: bool,
}

impl Progress {
    fn at(&self, now: Instant) -> u32 {
        if self.running {
            let elapsed = u32::try_from(now.saturating_duration_since(self.since).as_millis())
                .unwrap_or(u32::MAX);
            self.position_ms.saturating_add(elapsed)
        } else {
            self.position_ms
        }
    }
}

pub fn format_time(ms: u32) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

pub fn track_row_label(n: usize, t: &TrackSummary) -> String {
    format!(
        "{n}. {} — {}   {}",
        t.name,
        t.artists,
        format_time(t.duration_ms)
    )
}

pub fn playlist_label(p: &PlaylistSummary) -> String {
    format!("{} ({})", p.name, p.track_count)
}

#[derive(Debug, Clone, PartialEq)]
enum Status {
    LoggingIn,
    Idle,
    Playing(String),
    Paused,
    Stopped,
    Ended,
}

fn status_text(status: &Status) -> String {
    match status {
        Status::LoggingIn => "Logging in...".to_string(),
        Status::Idle => "Ready: paste a track and press Play".to_string(),
        Status::Playing(id) => format!("Playing {id}"),
        Status::Paused => "Paused".to_string(),
        Status::Stopped => "Stopped".to_string(),
        Status::Ended => "Track ended".to_string(),
    }
}

pub struct PlayerApp {
    cmd_tx: UnboundedSender<Command>,
    evt_rx: UnboundedReceiver<Event>,
    track_input: String,
    volume_percent: u8,
    status: Status,
    username: Option<String>,
    last_error: Option<String>,
    progress: Option<Progress>,
    track: Option<TrackInfo>,
    playlists: Vec<PlaylistSummary>,
    playlists_loading: bool,
    selected_playlist: Option<String>,
    tracks: Vec<TrackSummary>,
    tracks_loading: bool,
    tracks_for: Option<String>,
}

impl PlayerApp {
    pub fn new(cmd_tx: UnboundedSender<Command>, evt_rx: UnboundedReceiver<Event>) -> Self {
        Self {
            cmd_tx,
            evt_rx,
            track_input: String::new(),
            volume_percent: 100,
            status: Status::LoggingIn,
            username: None,
            last_error: None,
            progress: None,
            track: None,
            playlists: Vec::new(),
            playlists_loading: false,
            selected_playlist: None,
            tracks: Vec::new(),
            tracks_loading: false,
            tracks_for: None,
        }
    }

    fn apply_event(&mut self, event: Event, now: Instant) {
        match event {
            Event::LoggedIn { username } => {
                self.username = Some(username);
                self.status = Status::Idle;
            }
            Event::Playing {
                track_id,
                position_ms,
            } => {
                self.status = Status::Playing(track_id);
                self.last_error = None;
                self.progress = Some(Progress {
                    position_ms,
                    since: now,
                    running: true,
                });
            }
            Event::Paused { position_ms } => {
                self.status = Status::Paused;
                self.progress = Some(Progress {
                    position_ms,
                    since: now,
                    running: false,
                });
            }
            Event::Position { position_ms } => {
                if let Some(ref mut progress) = self.progress {
                    progress.position_ms = position_ms;
                    progress.since = now;
                }
            }
            Event::TrackInfo(info) => {
                self.track = Some(info);
                self.progress = None;
            }
            Event::Stopped => {
                self.status = Status::Stopped;
                self.progress = None;
            }
            Event::EndOfTrack => {
                self.status = Status::Ended;
                self.progress = None;
            }
            Event::VolumeChanged(v) => {
                let percent = volume_to_percent(v).min(100) as u8;
                self.volume_percent = percent;
            }
            Event::PlaylistsLoaded(list) => {
                self.playlists = list;
                self.playlists_loading = false;
            }
            Event::PlaylistTracksLoaded {
                playlist_id,
                tracks,
            } => {
                if Some(&playlist_id) == self.selected_playlist.as_ref() {
                    self.tracks = tracks;
                    self.tracks_for = Some(playlist_id);
                    self.tracks_loading = false;
                }
            }
            Event::Error(msg) => {
                self.playlists_loading = false;
                self.tracks_loading = false;
                self.last_error = Some(msg);
            }
        }
    }

    /// Called once the engine's event channel is closed.
    fn engine_gone(&mut self) {
        if let Some(progress) = self.progress.as_mut() {
            progress.running = false;
        }
        if self.last_error.is_none() {
            self.last_error = Some("the engine has stopped".into());
        }
    }

    fn send(&mut self, command: Command) {
        if self.cmd_tx.send(command).is_err() {
            self.last_error = Some("the engine has stopped".into());
        }
    }
}

impl eframe::App for PlayerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        loop {
            match self.evt_rx.try_recv() {
                Ok(event) => self.apply_event(event, now),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The engine has ended (login failed, player died): freeze the clock so the bar stops
                    // moving and nothing keeps repainting.
                    self.engine_gone();
                    break;
                }
            }
        }

        egui::SidePanel::left("playlists")
            .resizable(true)
            .default_width(200.0)
            .show(ctx, |ui| {
                match &self.username {
                    Some(name) => {
                        ui.label(format!("Logged in as {name}"));
                    }
                    None => {
                        ui.label("Logging in...");
                    }
                }

                ui.heading("Playlists");

                let button_text = if self.playlists.is_empty() {
                    "Load playlists"
                } else {
                    "Reload"
                };
                let can_load = !self.playlists_loading && self.username.is_some();
                if ui
                    .add_enabled(can_load, egui::Button::new(button_text))
                    .clicked()
                {
                    self.playlists_loading = true;
                    self.send(Command::LoadPlaylists);
                }

                if self.playlists_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading... (a browser may open the first time)");
                    });
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut clicked_playlist = None;
                    for playlist in &self.playlists {
                        let selected = self.selected_playlist.as_deref() == Some(&playlist.id);
                        if ui
                            .selectable_label(selected, playlist_label(playlist))
                            .clicked()
                        {
                            clicked_playlist = Some(playlist.id.clone());
                        }
                    }

                    if let Some(id) = clicked_playlist {
                        let same_selected = self.selected_playlist.as_deref() == Some(&id);
                        // Load unless these tracks are shown already or are loading right now.
                        let is_different = !same_selected
                            || (self.tracks_for.as_deref() != Some(&id) && !self.tracks_loading);
                        self.selected_playlist = Some(id.clone());
                        if is_different {
                            self.tracks_loading = true;
                            self.tracks.clear();
                            self.tracks_for = None;
                            self.send(Command::LoadPlaylistTracks(id));
                        }
                    }

                    if self.playlists.is_empty() && !self.playlists_loading {
                        ui.label(egui::RichText::new("No playlists loaded").weak());
                    }
                });
            });

        egui::TopBottomPanel::bottom("player").show(ctx, |ui| {
            if let Some(track) = &self.track {
                ui.horizontal(|ui| {
                    if let Some(ref url) = track.cover_url {
                        ui.add(
                            egui::Image::new(url.as_str())
                                .fit_to_exact_size(egui::vec2(64.0, 64.0))
                                .corner_radius(4.0),
                        );
                    } else {
                        ui.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
                    }

                    ui.vertical(|ui| {
                        ui.heading(&track.title);
                        ui.label(&track.artists);
                        ui.label(egui::RichText::new(&track.album).weak());
                    });
                });

                if track.duration_ms > 0 {
                    let position = self
                        .progress
                        .map(|p| p.at(now))
                        .unwrap_or(0)
                        .min(track.duration_ms);
                    let fraction = (position as f32 / track.duration_ms as f32).clamp(0.0, 1.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::ProgressBar::new(fraction).desired_width(300.0));
                        ui.label(format!(
                            "{} / {}",
                            format_time(position),
                            format_time(track.duration_ms)
                        ));
                    });
                }
            }

            ui.add_enabled_ui(self.username.is_some(), |ui| {
                ui.horizontal(|ui| {
                    let text_edit = egui::TextEdit::singleline(&mut self.track_input)
                        .hint_text("Track ID or Spotify link")
                        .desired_width(260.0);
                    let response = ui.add(text_edit);

                    let enter_pressed =
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let play_clicked = ui.button("Play").clicked();

                    if play_clicked || enter_pressed {
                        let trimmed = self.track_input.trim();
                        if !trimmed.is_empty() {
                            self.send(Command::Play(trimmed.to_string()));
                        }
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("Pause").clicked() {
                        self.send(Command::Pause);
                    }
                    if ui.button("Resume").clicked() {
                        self.send(Command::Resume);
                    }
                    if ui.button("Stop").clicked() {
                        self.send(Command::Stop);
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Volume");
                    let slider = egui::Slider::new(&mut self.volume_percent, 0..=100).suffix("%");
                    if ui.add(slider).changed() {
                        self.send(Command::SetVolume(percent_to_volume(self.volume_percent)));
                    }
                });
            });

            ui.label(status_text(&self.status));

            if let Some(msg) = &self.last_error {
                ui.label(egui::RichText::new(format!("Error: {msg}")).color(egui::Color32::RED));
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let heading_text = self
                .selected_playlist
                .as_ref()
                .and_then(|id| self.playlists.iter().find(|p| &p.id == id))
                .map(|p| p.name.as_str())
                .unwrap_or("Select a playlist");
            ui.heading(heading_text);

            if self.tracks_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading tracks...");
                });
            } else if self.selected_playlist.is_some() {
                if self.tracks.is_empty() {
                    ui.label(egui::RichText::new("No playable tracks").weak());
                } else {
                    // Only the visible rows are built each frame (`show_rows`), so long playlists stay cheap.
                    let row_height = ui.spacing().interact_size.y;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, row_height, self.tracks.len(), |ui, rows| {
                            let playing_uri = match &self.status {
                                Status::Playing(uri) => Some(uri.clone()),
                                _ => None,
                            };
                            let mut play_track = None;
                            let first = rows.start;
                            for (offset, track) in self.tracks[rows].iter().enumerate() {
                                let idx = first + offset;
                                let is_current = playing_uri.as_deref() == Some(track.uri.as_str());
                                let text = track_row_label(idx + 1, track);
                                let response = ui.selectable_label(is_current, text);
                                if response.double_clicked() {
                                    play_track = Some(track.uri.clone());
                                }
                            }
                            if let Some(uri) = play_track {
                                self.send(Command::Play(uri));
                            }
                        });
                }
            }
        });

        if self.progress.is_some_and(|p| p.running) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_app() -> (UnboundedSender<Event>, PlayerApp) {
        let (cmd_tx, _cmd_rx) = unbounded_channel();
        let (evt_tx, evt_rx) = unbounded_channel();
        (evt_tx, PlayerApp::new(cmd_tx, evt_rx))
    }

    #[test]
    fn test_progress_at() {
        let t0 = Instant::now();
        let p_running = Progress {
            position_ms: 1000,
            since: t0,
            running: true,
        };
        // running adds elapsed: position 1000, 2500 ms later -> 3500
        assert_eq!(p_running.at(t0 + Duration::from_millis(2500)), 3500);

        // not running ignores elapsed
        let p_paused = Progress {
            position_ms: 1000,
            since: t0,
            running: false,
        };
        assert_eq!(p_paused.at(t0 + Duration::from_millis(2500)), 1000);

        // now earlier than since gives position_ms (no panic)
        assert_eq!(p_running.at(t0 - Duration::from_millis(500)), 1000);
        assert_eq!(p_paused.at(t0 - Duration::from_millis(500)), 1000);
    }

    #[test]
    fn test_format_time() {
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(61_000), "1:01");
        assert_eq!(format_time(3_599_999), "59:59");
        assert_eq!(format_time(3_600_000), "60:00");
    }

    #[test]
    fn test_apply_event_logged_in() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        assert_eq!(app.status, Status::LoggingIn);
        assert_eq!(app.username, None);

        app.apply_event(
            Event::LoggedIn {
                username: "testuser".into(),
            },
            t0,
        );
        assert_eq!(app.username.as_deref(), Some("testuser"));
        assert_eq!(app.status, Status::Idle);
    }

    #[test]
    fn test_apply_event_playing() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.last_error = Some("previous error".into());

        app.apply_event(
            Event::Playing {
                track_id: "spotify:track:xyz".into(),
                position_ms: 500,
            },
            t0,
        );
        assert_eq!(app.status, Status::Playing("spotify:track:xyz".into()));
        assert_eq!(app.last_error, None);
        assert_eq!(
            app.progress,
            Some(Progress {
                position_ms: 500,
                since: t0,
                running: true,
            })
        );
    }

    #[test]
    fn test_apply_event_paused() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();

        app.apply_event(Event::Paused { position_ms: 1200 }, t0);
        assert_eq!(app.status, Status::Paused);
        assert_eq!(
            app.progress,
            Some(Progress {
                position_ms: 1200,
                since: t0,
                running: false,
            })
        );
    }

    #[test]
    fn test_apply_event_position() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(500);
        let (_evt_tx, mut app) = create_test_app();

        // Position with no progress is ignored
        app.apply_event(Event::Position { position_ms: 2000 }, t0);
        assert_eq!(app.progress, None);

        // Position keeps running state and moves position
        app.progress = Some(Progress {
            position_ms: 1000,
            since: t0,
            running: true,
        });
        app.apply_event(Event::Position { position_ms: 3000 }, t1);
        assert_eq!(
            app.progress,
            Some(Progress {
                position_ms: 3000,
                since: t1,
                running: true,
            })
        );

        app.progress = Some(Progress {
            position_ms: 1000,
            since: t0,
            running: false,
        });
        app.apply_event(Event::Position { position_ms: 4000 }, t1);
        assert_eq!(
            app.progress,
            Some(Progress {
                position_ms: 4000,
                since: t1,
                running: false,
            })
        );
    }

    #[test]
    fn test_apply_event_track_info() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.progress = Some(Progress {
            position_ms: 500,
            since: t0,
            running: true,
        });

        let info = TrackInfo {
            title: "Test Title".into(),
            artists: "Test Artist".into(),
            album: "Test Album".into(),
            duration_ms: 180_000,
            cover_url: Some("https://example.com/cover.jpg".into()),
        };

        app.apply_event(Event::TrackInfo(info.clone()), t0);
        assert_eq!(app.track, Some(info));
        assert_eq!(app.progress, None);
    }

    #[test]
    fn test_apply_event_stopped_and_end_of_track() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.progress = Some(Progress {
            position_ms: 1000,
            since: t0,
            running: true,
        });

        app.apply_event(Event::Stopped, t0);
        assert_eq!(app.status, Status::Stopped);
        assert_eq!(app.progress, None);

        app.progress = Some(Progress {
            position_ms: 2000,
            since: t0,
            running: true,
        });
        app.apply_event(Event::EndOfTrack, t0);
        assert_eq!(app.status, Status::Ended);
        assert_eq!(app.progress, None);
    }

    #[test]
    fn test_apply_event_volume_changed() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();

        app.apply_event(Event::VolumeChanged(32767), t0);
        assert_eq!(app.volume_percent, 50);

        app.apply_event(Event::VolumeChanged(65535), t0);
        assert_eq!(app.volume_percent, 100);

        app.apply_event(Event::VolumeChanged(0), t0);
        assert_eq!(app.volume_percent, 0);
    }

    #[test]
    fn test_apply_event_error_handling() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();

        // Error while LoggingIn keeps LoggingIn and sets last_error
        assert_eq!(app.status, Status::LoggingIn);
        app.apply_event(Event::Error("login failed".into()), t0);
        assert_eq!(app.status, Status::LoggingIn);
        assert_eq!(app.last_error.as_deref(), Some("login failed"));

        // Error while Playing keeps Playing
        app.status = Status::Playing("spotify:track:123".into());
        app.apply_event(Event::Error("stream warning".into()), t0);
        assert_eq!(app.status, Status::Playing("spotify:track:123".into()));
        assert_eq!(app.last_error.as_deref(), Some("stream warning"));
    }

    #[test]
    fn test_playlist_label() {
        let p = PlaylistSummary {
            id: "pl1".into(),
            name: "Chill".into(),
            track_count: 12,
        };
        assert_eq!(playlist_label(&p), "Chill (12)");
    }

    #[test]
    fn test_apply_event_playlists_loaded() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.playlists_loading = true;

        let playlists = vec![
            PlaylistSummary {
                id: "1".into(),
                name: "Rock".into(),
                track_count: 50,
            },
            PlaylistSummary {
                id: "2".into(),
                name: "Jazz".into(),
                track_count: 20,
            },
        ];

        app.apply_event(Event::PlaylistsLoaded(playlists.clone()), t0);
        assert_eq!(app.playlists, playlists);
        assert!(!app.playlists_loading);
    }

    #[test]
    fn test_apply_event_error_clears_playlists_loading() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.playlists_loading = true;

        app.apply_event(Event::Error("failed to fetch playlists".into()), t0);
        assert!(!app.playlists_loading);
        assert_eq!(app.last_error.as_deref(), Some("failed to fetch playlists"));
    }

    #[test]
    fn test_track_row_label() {
        let t = TrackSummary {
            uri: "spotify:track:xyz".into(),
            name: "Song".into(),
            artists: "A, B".into(),
            duration_ms: 61_000,
        };
        assert_eq!(track_row_label(1, &t), "1. Song — A, B   1:01");
    }

    #[test]
    fn test_apply_event_playlist_tracks_loaded_selected() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.selected_playlist = Some("pl1".into());
        app.tracks_loading = true;

        let tracks = vec![
            TrackSummary {
                uri: "spotify:track:1".into(),
                name: "Track One".into(),
                artists: "Artist A".into(),
                duration_ms: 120_000,
            },
            TrackSummary {
                uri: "spotify:track:2".into(),
                name: "Track Two".into(),
                artists: "Artist B".into(),
                duration_ms: 180_000,
            },
        ];

        app.apply_event(
            Event::PlaylistTracksLoaded {
                playlist_id: "pl1".into(),
                tracks: tracks.clone(),
            },
            t0,
        );

        assert_eq!(app.tracks, tracks);
        assert_eq!(app.tracks_for.as_deref(), Some("pl1"));
        assert!(!app.tracks_loading);
    }

    #[test]
    fn test_apply_event_playlist_tracks_loaded_other_ignored() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.selected_playlist = Some("pl1".into());
        app.tracks_loading = true;
        let original_tracks = vec![TrackSummary {
            uri: "spotify:track:original".into(),
            name: "Original".into(),
            artists: "Original Artist".into(),
            duration_ms: 60_000,
        }];
        app.tracks = original_tracks.clone();
        app.tracks_for = Some("pl1".into());

        let new_tracks = vec![TrackSummary {
            uri: "spotify:track:other".into(),
            name: "Other Track".into(),
            artists: "Other Artist".into(),
            duration_ms: 90_000,
        }];

        app.apply_event(
            Event::PlaylistTracksLoaded {
                playlist_id: "pl2".into(),
                tracks: new_tracks,
            },
            t0,
        );

        // Tracks and tracks_loading unchanged
        assert_eq!(app.tracks, original_tracks);
        assert_eq!(app.tracks_for.as_deref(), Some("pl1"));
        assert!(app.tracks_loading);
    }

    #[test]
    fn test_apply_event_error_clears_tracks_loading() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.tracks_loading = true;

        app.apply_event(Event::Error("something went wrong".into()), t0);
        assert!(!app.tracks_loading);
        assert_eq!(app.last_error.as_deref(), Some("something went wrong"));
    }

    #[test]
    fn test_status_text() {
        assert_eq!(status_text(&Status::LoggingIn), "Logging in...");
        assert_eq!(
            status_text(&Status::Idle),
            "Ready: paste a track and press Play"
        );
        assert_eq!(
            status_text(&Status::Playing("spotify:track:abc".into())),
            "Playing spotify:track:abc"
        );
        assert_eq!(status_text(&Status::Paused), "Paused");
        assert_eq!(status_text(&Status::Stopped), "Stopped");
        assert_eq!(status_text(&Status::Ended), "Track ended");
    }

    #[test]
    fn test_send_dropped_receiver() {
        let (cmd_tx, cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx);

        drop(cmd_rx);
        assert_eq!(app.last_error, None);
        app.send(Command::Pause);
        assert_eq!(app.last_error.as_deref(), Some("the engine has stopped"));
    }

    #[test]
    fn test_send_live_receiver() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx);

        app.send(Command::Play("spotify:track:xyz".into()));
        assert_eq!(app.last_error, None);
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:xyz".into()))
        );
    }
}
