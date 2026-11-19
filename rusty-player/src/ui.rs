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
        })
        .context("could not start the engine thread")?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rusty-player")
            .with_inner_size([520.0, 360.0]),
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
            Event::Error(msg) => {
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

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("rusty-player");
            match &self.username {
                Some(name) => {
                    ui.label(format!("Logged in as {name}"));
                }
                None => {
                    ui.label("Logging in...");
                }
            }

            if let Some(track) = &self.track {
                ui.horizontal(|ui| {
                    if let Some(ref url) = track.cover_url {
                        ui.add(
                            egui::Image::new(url.as_str())
                                .fit_to_exact_size(egui::vec2(120.0, 120.0))
                                .corner_radius(4.0),
                        );
                    } else {
                        ui.allocate_exact_size(egui::vec2(120.0, 120.0), egui::Sense::hover());
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
