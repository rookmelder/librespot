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
use crate::settings::Settings;
use crate::webapi::{PlaylistSummary, Stats, TrackSummary};

const GREEN: egui::Color32 = egui::Color32::from_rgb(29, 185, 84);
const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(18, 18, 18);
const CARD: egui::Color32 = egui::Color32::from_rgb(31, 31, 31);
const HOVER: egui::Color32 = egui::Color32::from_rgb(42, 42, 42);

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts
            .font_data
            .insert("Segoe UI".into(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Segoe UI".into());
    }
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguiemj.ttf") {
        fonts.font_data.insert(
            "Segoe Emoji".into(),
            egui::FontData::from_owned(bytes).into(),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("Segoe Emoji".into());
        }
    }
    ctx.set_fonts(fonts);
}

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = BACKGROUND;
    style.visuals.override_text_color = Some(egui::Color32::WHITE);
    style.visuals.weak_text_color = Some(egui::Color32::from_rgb(179, 179, 179));
    style.visuals.selection.bg_fill = GREEN;
    style.visuals.selection.stroke.color = egui::Color32::BLACK;
    style.visuals.window_corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.inactive.bg_fill = CARD;
    style.visuals.widgets.inactive.weak_bg_fill = CARD;
    style.visuals.widgets.hovered.bg_fill = HOVER;
    style.visuals.widgets.hovered.weak_bg_fill = HOVER;
    style.visuals.widgets.active.bg_fill = GREEN;
    style.visuals.widgets.active.weak_bg_fill = GREEN;
    for widget in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(6);
        widget.fg_stroke.color = egui::Color32::WHITE;
    }
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    for (text_style, size) in [
        (egui::TextStyle::Heading, 22.0),
        (egui::TextStyle::Body, 14.0),
        (egui::TextStyle::Button, 14.0),
        (egui::TextStyle::Small, 12.0),
    ] {
        style
            .text_styles
            .insert(text_style, egui::FontId::proportional(size));
    }
    ctx.set_style(style);
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum MediaAction {
    Play,
    Pause,
    Stop,
    Toggle,
    Next,
    Previous,
}

/// Open the window and run until it is closed; then stop the engine and wait for it.
pub fn run(to_file: Option<PathBuf>, settings: crate::settings::Settings) -> Result<()> {
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

    let ui_settings = settings.clone();
    let runtime = tokio::runtime::Runtime::new().context("could not start the Tokio runtime")?;
    let engine_thread = std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || {
            runtime.block_on(engine::run_engine(
                EngineConfig { to_file, settings },
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
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    let window_result = eframe::run_native(
        "rusty-player",
        options,
        Box::new(move |cc| {
            let _ = ctx_slot.set(cc.egui_ctx.clone());
            // Lets `ui.image(url)` download and show the album cover (http + jpeg/png).
            egui_extras::install_image_loaders(&cc.egui_ctx);
            load_fonts(&cc.egui_ctx);
            apply_theme(&cc.egui_ctx);
            let app = PlayerApp::new(cmd_tx, evt_rx, ui_settings);
            Ok(Box::new(app))
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

#[cfg(test)]
fn track_row_label(n: usize, t: &TrackSummary) -> String {
    format!(
        "{n}. {} — {}   {}",
        t.name,
        t.artists,
        format_time(t.duration_ms)
    )
}

#[cfg(test)]
fn playlist_label(p: &PlaylistSummary) -> String {
    format!("{} ({})", p.name, p.track_count)
}

pub fn next_in_queue(queue: &[String], pos: Option<usize>) -> Option<usize> {
    match pos {
        Some(i) if i + 1 < queue.len() => Some(i + 1),
        _ => None,
    }
}

pub fn prev_in_queue(queue: &[String], pos: Option<usize>) -> Option<usize> {
    match pos {
        Some(i) if i > 0 && i < queue.len() => Some(i - 1),
        _ => None,
    }
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

fn toggle_command(status: &Status) -> Option<Command> {
    match status {
        Status::Playing(_) => Some(Command::Pause),
        Status::Paused => Some(Command::Resume),
        _ => None,
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum View {
    Playlist,
    Liked,
    Search,
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
    liked: Vec<TrackSummary>,
    liked_loading: bool,
    liked_paging_failed: bool,
    liked_total: Option<u32>,
    settings: Settings,
    settings_open: bool,
    settings_saved: bool,
    current_uri: Option<String>,
    #[cfg(windows)]
    media_controls: Option<souvlaki::MediaControls>,
    #[cfg(windows)]
    media_rx: Option<std::sync::mpsc::Receiver<MediaAction>>,
    #[cfg(windows)]
    media_init_tried: bool,
    stats: Option<Stats>,
    stats_loading: bool,
    search_input: String,
    search_loading: bool,
    search_query: Option<String>,
    search_results: Vec<TrackSummary>,
    queue: Vec<String>,
    queue_pos: Option<usize>,
    view: View,
}

impl PlayerApp {
    pub fn new(
        cmd_tx: UnboundedSender<Command>,
        evt_rx: UnboundedReceiver<Event>,
        settings: Settings,
    ) -> Self {
        Self {
            cmd_tx,
            evt_rx,
            track_input: String::new(),
            volume_percent: settings.volume_percent.min(100),
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
            liked: Vec::new(),
            liked_loading: false,
            liked_paging_failed: false,
            liked_total: None,
            settings,
            settings_open: false,
            settings_saved: false,
            current_uri: None,
            #[cfg(windows)]
            media_controls: None,
            #[cfg(windows)]
            media_rx: None,
            #[cfg(windows)]
            media_init_tried: false,
            stats: None,
            stats_loading: false,
            search_input: String::new(),
            search_loading: false,
            search_query: None,
            search_results: Vec::new(),
            queue: Vec::new(),
            queue_pos: None,
            view: View::Playlist,
        }
    }

    fn apply_event(&mut self, event: Event, now: Instant) {
        #[cfg(windows)]
        self.update_media(&event);
        match event {
            Event::LoggedIn { username } => {
                self.username = Some(username);
                self.status = Status::Idle;
            }
            Event::Playing {
                track_id,
                position_ms,
            } => {
                self.current_uri = Some(track_id.clone());
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
                self.progress = None;
                if let Some(next_idx) = next_in_queue(&self.queue, self.queue_pos) {
                    self.queue_pos = Some(next_idx);
                    let uri = self.queue[next_idx].clone();
                    self.send(Command::Play(uri));
                } else {
                    self.status = Status::Ended;
                }
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
            Event::LikedPageLoaded(page) => {
                self.liked_total = Some(page.total);
                if page.offset as usize == self.liked.len() {
                    if page.tracks.is_empty() {
                        self.liked_total = Some(self.liked.len() as u32);
                    }
                    self.liked.extend(page.tracks);
                }
                self.liked_loading = false;
            }
            Event::SettingsSaved => self.settings_saved = true,
            Event::StatsLoaded(stats) => {
                self.stats = Some(stats);
                self.stats_loading = false;
            }
            Event::SearchResults { query, tracks } => {
                if Some(&query) == self.search_query.as_ref() {
                    self.search_results = tracks;
                    self.search_loading = false;
                }
            }
            Event::Error(msg) => {
                if self.liked_loading {
                    self.liked_paging_failed = true;
                }
                self.playlists_loading = false;
                self.tracks_loading = false;
                self.liked_loading = false;
                self.stats_loading = false;
                self.search_loading = false;
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

    fn play_queue(&mut self, queue: Vec<String>, index: usize) {
        self.queue = queue;
        self.queue_pos = Some(index);
        if let Some(uri) = self.queue.get(index).cloned() {
            self.send(Command::Play(uri));
        }
    }

    fn next(&mut self) {
        if let Some(index) = next_in_queue(&self.queue, self.queue_pos) {
            self.queue_pos = Some(index);
            self.send(Command::Play(self.queue[index].clone()));
        }
    }

    fn can_restart(&self, now: Instant) -> bool {
        self.current_uri.is_some() && self.progress.is_some_and(|p| p.at(now) > 3000)
    }

    fn previous(&mut self, now: Instant) {
        if self.can_restart(now) {
            if let Some(uri) = self.current_uri.clone() {
                self.send(Command::Play(uri));
            }
        } else if let Some(index) = prev_in_queue(&self.queue, self.queue_pos) {
            self.queue_pos = Some(index);
            self.send(Command::Play(self.queue[index].clone()));
        }
    }

    fn toggle(&mut self) {
        if let Some(command) = toggle_command(&self.status) {
            self.send(command);
        }
    }

    fn load_liked(&mut self) {
        self.liked_loading = true;
        self.send(Command::LoadLikedPage {
            offset: self.liked.len() as u32,
        });
    }

    fn reload_liked(&mut self) {
        self.liked.clear();
        self.liked_total = None;
        self.liked_paging_failed = false;
        self.load_liked();
    }

    fn should_load_more_liked(&self, visible_end: usize) -> bool {
        self.username.is_some()
            && !self.liked_loading
            && !self.liked_paging_failed
            && self
                .liked_total
                .is_some_and(|total| self.liked.len() < total as usize)
            && visible_end.saturating_add(10) >= self.liked.len()
    }

    fn library_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("playlists")
            .resizable(true)
            .default_width(240.0)
            .frame(side_frame())
            .show(ctx, |ui| {
                ui.heading("Your Library");
                ui.weak(self.username.as_deref().unwrap_or("Logging in..."));
                let button_text = if self.playlists.is_empty() {
                    "Load playlists"
                } else {
                    "Reload"
                };
                if ui
                    .add_enabled(
                        !self.playlists_loading && self.username.is_some(),
                        egui::Button::new(egui::RichText::new(button_text).small()),
                    )
                    .clicked()
                {
                    self.playlists_loading = true;
                    self.send(Command::LoadPlaylists);
                }
                if self.playlists_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak("Loading... (a browser may open the first time)");
                    });
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(self.username.is_some(), |ui| {
                        if library_entry(
                            ui,
                            "♥ Liked Songs",
                            "Your saved songs",
                            self.view == View::Liked,
                        )
                        .clicked()
                        {
                            self.view = View::Liked;
                            if self.liked.is_empty()
                                && !self.liked_loading
                                && !self.liked_paging_failed
                                && self.liked_total != Some(0)
                            {
                                self.load_liked();
                            }
                        }
                    });
                    let mut clicked_playlist = None;
                    for playlist in &self.playlists {
                        let selected = self.view == View::Playlist
                            && self.selected_playlist.as_deref() == Some(&playlist.id);
                        let response = ui
                            .add_enabled_ui(self.username.is_some(), |ui| {
                                library_entry(
                                    ui,
                                    &playlist.name,
                                    &format!("Playlist · {} songs", playlist.track_count),
                                    selected,
                                )
                            })
                            .inner;
                        if response.clicked() {
                            clicked_playlist = Some(playlist.id.clone());
                        }
                    }
                    if let Some(id) = clicked_playlist {
                        self.view = View::Playlist;
                        let same_selected = self.selected_playlist.as_deref() == Some(&id);
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
                        ui.weak("No playlists loaded");
                    }
                });
            });
    }

    fn play_input(&mut self) {
        let text = self.track_input.trim().to_string();
        if !text.is_empty() {
            self.queue.clear();
            self.queue_pos = None;
            self.send(Command::Play(text));
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open;
        egui::Window::new("Settings")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Audio bitrate");
                let mut changed = false;
                ui.horizontal(|ui| {
                    for bitrate in [96, 160, 320] {
                        changed |= ui
                            .radio_value(
                                &mut self.settings.bitrate,
                                bitrate,
                                format!("{bitrate} kbps"),
                            )
                            .changed();
                    }
                });
                changed |= ui
                    .checkbox(&mut self.settings.normalisation, "Volume normalisation")
                    .changed();
                ui.weak("Applies at next start");
                if changed {
                    self.settings_saved = false;
                }
                ui.add_enabled_ui(self.username.is_some(), |ui| {
                    ui.horizontal(|ui| {
                        if primary_button(ui, "Save").clicked() {
                            self.settings.volume_percent = self.volume_percent;
                            self.settings_saved = false;
                            self.send(Command::SaveSettings(self.settings.clone()));
                        }
                        if self.settings_saved {
                            ui.label("Saved");
                        }
                    });
                    ui.separator();
                    ui.label("Play by link");
                    ui.horizontal(|ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.track_input)
                                .hint_text("Track ID or Spotify link")
                                .desired_width(280.0),
                        );
                        let enter =
                            response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if primary_button(ui, "Play").clicked() || enter {
                            self.play_input();
                        }
                    });
                });
            });
        self.settings_open = open;
    }

    #[cfg(windows)]
    fn init_media(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        self.media_init_tried = true;
        // Escape hatch: RUSTY_PLAYER_NO_MEDIA_KEYS=1 starts without media keys.
        if std::env::var_os("RUSTY_PLAYER_NO_MEDIA_KEYS").is_some() {
            return;
        }
        let hwnd = match frame.window_handle() {
            Ok(handle) => match handle.as_raw() {
                RawWindowHandle::Win32(handle) => handle.hwnd.get() as *mut std::ffi::c_void,
                _ => return,
            },
            Err(error) => {
                log::warn!("could not get window handle for media controls: {error}");
                return;
            }
        };
        let mut controls = match souvlaki::MediaControls::new(souvlaki::PlatformConfig {
            display_name: "rusty-player",
            dbus_name: "rusty_player",
            hwnd: Some(hwnd),
        }) {
            Ok(controls) => controls,
            Err(error) => {
                log::warn!("could not create media controls: {error}");
                return;
            }
        };
        let (media_tx, media_rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        if let Err(error) = controls.attach(move |event| {
            let action = match event {
                souvlaki::MediaControlEvent::Play => Some(MediaAction::Play),
                souvlaki::MediaControlEvent::Pause => Some(MediaAction::Pause),
                souvlaki::MediaControlEvent::Stop => Some(MediaAction::Stop),
                souvlaki::MediaControlEvent::Toggle => Some(MediaAction::Toggle),
                souvlaki::MediaControlEvent::Next => Some(MediaAction::Next),
                souvlaki::MediaControlEvent::Previous => Some(MediaAction::Previous),
                _ => None,
            };
            if let Some(action) = action {
                if media_tx.send(action).is_err() {
                    log::warn!("could not send media action: the window has closed");
                }
                repaint.request_repaint();
            }
        }) {
            log::warn!("could not attach media controls: {error}");
            return;
        }
        self.media_controls = Some(controls);
        self.media_rx = Some(media_rx);
    }

    #[cfg(windows)]
    fn update_media(&mut self, event: &Event) {
        let Some(controls) = self.media_controls.as_mut() else {
            return;
        };
        let result = match event {
            Event::TrackInfo(info) => controls.set_metadata(souvlaki::MediaMetadata {
                title: Some(&info.title),
                artist: Some(&info.artists),
                album: Some(&info.album),
                cover_url: info.cover_url.as_deref(),
                duration: Some(std::time::Duration::from_millis(u64::from(
                    info.duration_ms,
                ))),
            }),
            Event::Playing { position_ms, .. } => {
                controls.set_playback(souvlaki::MediaPlayback::Playing {
                    progress: Some(souvlaki::MediaPosition(std::time::Duration::from_millis(
                        u64::from(*position_ms),
                    ))),
                })
            }
            Event::Paused { position_ms } => {
                controls.set_playback(souvlaki::MediaPlayback::Paused {
                    progress: Some(souvlaki::MediaPosition(std::time::Duration::from_millis(
                        u64::from(*position_ms),
                    ))),
                })
            }
            Event::Stopped | Event::EndOfTrack => {
                controls.set_playback(souvlaki::MediaPlayback::Stopped)
            }
            _ => return,
        };
        if let Err(error) = result {
            log::warn!("could not update media controls: {error}");
            self.media_controls = None;
            self.media_rx = None;
        }
    }
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).color(egui::Color32::BLACK)).fill(GREEN))
}

fn side_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::BLACK)
        .inner_margin(8)
}

fn library_entry(ui: &mut egui::Ui, title: &str, subtitle: &str, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.0), egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        6.0,
        if selected || response.hovered() {
            HOVER
        } else {
            egui::Color32::BLACK
        },
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(8.0))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.add(egui::Label::new(egui::RichText::new(title).strong()).truncate());
    child.add(egui::Label::new(egui::RichText::new(subtitle).weak().small()).truncate());
    response
}

fn queue_uris(tracks: &[TrackSummary]) -> Vec<String> {
    tracks.iter().map(|track| track.uri.clone()).collect()
}

fn track_list(
    ui: &mut egui::Ui,
    tracks: &[TrackSummary],
    playing_uri: Option<&str>,
    loading: bool,
    paging_failed: bool,
) -> (Option<usize>, usize, bool) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [32.0, 22.0],
            egui::Label::new(egui::RichText::new("#").weak()),
        );
        ui.weak("Title");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak("Duration");
        });
    });
    ui.separator();
    let row_height = 54.0;
    let mut clicked_idx = None;
    let mut visible_end = 0;
    let mut retry = false;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(
            ui,
            row_height,
            tracks.len() + usize::from(loading || paging_failed),
            |ui, rows| {
                visible_end = rows.end.min(tracks.len());
                for idx in rows {
                    let Some(track) = tracks.get(idx) else {
                        if paging_failed {
                            ui.horizontal(|ui| {
                                ui.weak("Couldn't load more ·");
                                retry = ui.small_button("Retry").clicked();
                            });
                        } else {
                            ui.add_sized([ui.available_width(), row_height], egui::Spinner::new());
                        }
                        continue;
                    };
                    let is_current = playing_uri == Some(track.uri.as_str());
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_height),
                        egui::Sense::click(),
                    );
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 6.0, HOVER);
                    }
                    let mut row = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect.shrink2(egui::vec2(6.0, 4.0)))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    row.add_sized(
                        [26.0, 30.0],
                        egui::Label::new(egui::RichText::new((idx + 1).to_string()).weak()),
                    );
                    let title_width = (row.available_width() - 80.0).max(0.0);
                    row.allocate_ui_with_layout(
                        egui::vec2(title_width, 44.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let mut title = egui::RichText::new(&track.name).strong();
                            if is_current {
                                title = title.color(GREEN);
                            }
                            ui.add(egui::Label::new(title).truncate());
                            ui.add(
                                egui::Label::new(egui::RichText::new(&track.artists).weak())
                                    .truncate(),
                            );
                        },
                    );
                    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(format_time(track.duration_ms));
                    });
                    if response.double_clicked() {
                        clicked_idx = Some(idx);
                    }
                }
            },
        );
    (clicked_idx, visible_end, retry)
}

impl eframe::App for PlayerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(windows)]
        if !self.media_init_tried {
            self.init_media(ctx, _frame);
        }
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

        #[cfg(windows)]
        loop {
            let action = self.media_rx.as_ref().and_then(|rx| rx.try_recv().ok());
            let Some(action) = action else { break };
            if self.username.is_none() || self.cmd_tx.is_closed() {
                continue;
            }
            match action {
                MediaAction::Play => self.send(Command::Resume),
                MediaAction::Pause => self.send(Command::Pause),
                MediaAction::Stop => self.send(Command::Stop),
                MediaAction::Toggle => self.toggle(),
                MediaAction::Next => self.next(),
                MediaAction::Previous => self.previous(now),
            }
        }

        egui::TopBottomPanel::top("search")
            .frame(side_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() * 0.28).max(0.0));
                    ui.add_enabled_ui(self.username.is_some(), |ui| {
                        ui.horizontal(|ui| {
                            let text_edit = egui::TextEdit::singleline(&mut self.search_input)
                                .hint_text("What do you want to play?")
                                .desired_width(340.0);
                            let response = ui.add(text_edit);

                            let enter_pressed = response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let search_clicked = primary_button(ui, "Search").clicked();

                            if enter_pressed || search_clicked {
                                let trimmed = self.search_input.trim();
                                if !trimmed.is_empty() {
                                    self.search_query = Some(trimmed.to_string());
                                    self.search_loading = true;
                                    self.view = View::Search;
                                    self.search_results.clear();
                                    self.send(Command::Search(trimmed.to_string()));
                                }
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⚙").on_hover_text("Settings").clicked() {
                            self.settings_open = true;
                        }
                    });
                });
            });

        egui::TopBottomPanel::bottom("player")
            .exact_height(90.0)
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(12))
            .show(ctx, |ui| {
                let width = ui.available_width();
                let left_width = width * 0.28;
                let center_width = width * 0.44;
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_width, 64.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            if let Some(track) = &self.track {
                                if let Some(url) = &track.cover_url {
                                    ui.add(
                                        egui::Image::new(url.as_str())
                                            .fit_to_exact_size(egui::vec2(56.0, 56.0))
                                            .corner_radius(6.0),
                                    );
                                } else {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(56.0, 56.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(rect, 6.0, CARD);
                                }
                                ui.vertical(|ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&track.title).strong(),
                                        )
                                        .truncate(),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&track.artists).weak(),
                                        )
                                        .truncate(),
                                    );
                                });
                            } else {
                                ui.weak(status_text(&self.status));
                            }
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(center_width, 64.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_enabled_ui(self.username.is_some(), |ui| {
                                ui.horizontal(|ui| {
                                    ui.add_space(((center_width - 156.0) / 2.0).max(0.0));
                                    let previous = self.can_restart(now)
                                        || prev_in_queue(&self.queue, self.queue_pos).is_some();
                                    if ui
                                        .add_enabled(previous, egui::Button::new("⏮"))
                                        .on_hover_text("Previous")
                                        .clicked()
                                    {
                                        self.previous(now);
                                    }
                                    if ui
                                        .add_enabled(
                                            toggle_command(&self.status).is_some(),
                                            egui::Button::new(
                                                egui::RichText::new("⏯")
                                                    .color(egui::Color32::BLACK),
                                            )
                                            .fill(GREEN),
                                        )
                                        .on_hover_text(
                                            if matches!(self.status, Status::Playing(_)) {
                                                "Pause"
                                            } else {
                                                "Resume"
                                            },
                                        )
                                        .clicked()
                                    {
                                        self.toggle();
                                    }
                                    if ui
                                        .add_enabled(
                                            next_in_queue(&self.queue, self.queue_pos).is_some(),
                                            egui::Button::new("⏭"),
                                        )
                                        .on_hover_text("Next")
                                        .clicked()
                                    {
                                        self.next();
                                    }
                                });
                            });
                            let duration = self.track.as_ref().map_or(0, |t| t.duration_ms);
                            let position = self.progress.map_or(0, |p| p.at(now)).min(duration);
                            let fraction = if duration == 0 {
                                0.0
                            } else {
                                position as f32 / duration as f32
                            };
                            ui.horizontal(|ui| {
                                ui.weak(format_time(position));
                                ui.add(
                                    egui::ProgressBar::new(fraction)
                                        .fill(GREEN)
                                        .desired_height(5.0)
                                        .desired_width((center_width - 105.0).max(30.0)),
                                );
                                ui.weak(format_time(duration));
                            });
                        },
                    );
                    ui.add_enabled_ui(self.username.is_some(), |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().slider_width = 120.0;
                            if ui
                                .add(
                                    egui::Slider::new(&mut self.volume_percent, 0..=100)
                                        .show_value(false)
                                        .trailing_fill(true),
                                )
                                .on_hover_text(format!("{}%", self.volume_percent))
                                .changed()
                            {
                                self.settings_saved = false;
                                self.send(Command::SetVolume(percent_to_volume(
                                    self.volume_percent,
                                )));
                            }
                            ui.label("🔊");
                        });
                    });
                });
            });

        if let Some(message) = self.last_error.clone() {
            egui::TopBottomPanel::bottom("error")
                .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(6))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message)
                                    .small()
                                    .color(egui::Color32::LIGHT_RED),
                            )
                            .truncate(),
                        );
                        if ui.small_button("×").clicked() {
                            self.last_error = None;
                        }
                    });
                });
        }

        self.library_panel(ctx);

        egui::SidePanel::right("stats")
            .resizable(true)
            .default_width(220.0)
            .frame(side_frame())
            .show(ctx, |ui| {
                ui.heading("Stats");

                let button_text = if self.stats.is_none() {
                    "Load stats"
                } else {
                    "Reload"
                };
                let can_load = !self.stats_loading && self.username.is_some();
                if ui
                    .add_enabled(can_load, egui::Button::new(button_text))
                    .clicked()
                {
                    self.stats_loading = true;
                    self.send(Command::LoadStats);
                }

                if self.stats_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading stats...");
                    });
                }

                let mut clicked_stats_track = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(stats) = &self.stats {
                        ui.collapsing("Recently played", |ui| {
                            for (idx, track) in stats.recently_played.iter().enumerate() {
                                let label_text = format!("{} — {}", track.name, track.artists);
                                let response = ui.selectable_label(false, label_text);
                                if response.double_clicked() && self.username.is_some() {
                                    clicked_stats_track =
                                        Some((queue_uris(&stats.recently_played), idx));
                                }
                            }
                        });

                        ui.collapsing("Top tracks", |ui| {
                            for (idx, track) in stats.top_tracks.iter().enumerate() {
                                let label_text = format!("{} — {}", track.name, track.artists);
                                let response = ui.selectable_label(false, label_text);
                                if response.double_clicked() && self.username.is_some() {
                                    clicked_stats_track =
                                        Some((queue_uris(&stats.top_tracks), idx));
                                }
                            }
                        });

                        ui.collapsing("Top artists", |ui| {
                            for (idx, artist) in stats.top_artists.iter().enumerate() {
                                ui.label(format!("{}. {artist}", idx + 1));
                            }
                        });
                    }
                });

                if let Some((list, idx)) = clicked_stats_track {
                    self.play_queue(list, idx);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            let playing_uri = self.current_uri.clone();
            let playing_uri_ref = playing_uri.as_deref();

            match self.view {
                View::Search => {
                    let heading_text = match &self.search_query {
                        Some(q) => format!("Search: {q}"),
                        None => "Search".to_string(),
                    };
                    ui.heading(heading_text);
                    ui.weak(format!("{} songs", self.search_results.len()));

                    if self.search_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Searching...");
                        });
                    } else if self.search_results.is_empty() {
                        ui.label(egui::RichText::new("No results").weak());
                    } else if let (Some(idx), _, _) =
                        track_list(ui, &self.search_results, playing_uri_ref, false, false)
                    {
                        if self.username.is_some() {
                            let queue = queue_uris(&self.search_results);
                            self.play_queue(queue, idx);
                        }
                    }
                }
                View::Liked => {
                    let mut reload_clicked = false;
                    ui.horizontal(|ui| {
                        ui.heading("Liked Songs");
                        if ui
                            .add_enabled(
                                self.username.is_some() && !self.liked_loading,
                                egui::Button::new("Reload").small(),
                            )
                            .clicked()
                        {
                            reload_clicked = true;
                        }
                    });

                    if reload_clicked {
                        self.reload_liked();
                    }
                    ui.weak(format!(
                        "{} songs",
                        self.liked_total.map_or(self.liked.len(), |n| n as usize)
                    ));
                    if self.liked.is_empty() && !self.liked_loading {
                        ui.label(egui::RichText::new("No liked tracks").weak());
                    }
                    let (clicked, visible_end, retry) = track_list(
                        ui,
                        &self.liked,
                        playing_uri_ref,
                        self.liked_loading,
                        self.liked_paging_failed,
                    );
                    if self.username.is_some() {
                        if let Some(idx) = clicked {
                            let queue = queue_uris(&self.liked);
                            self.play_queue(queue, idx);
                        }
                        if retry && !self.liked_loading {
                            self.liked_paging_failed = false;
                            self.load_liked();
                        } else if self.should_load_more_liked(visible_end) {
                            self.load_liked();
                        }
                    }
                }
                View::Playlist => {
                    let heading_text = self
                        .selected_playlist
                        .as_ref()
                        .and_then(|id| self.playlists.iter().find(|p| &p.id == id))
                        .map(|p| p.name.as_str())
                        .unwrap_or("Select a playlist");
                    ui.heading(heading_text);
                    let count = self
                        .selected_playlist
                        .as_ref()
                        .and_then(|id| self.playlists.iter().find(|p| &p.id == id))
                        .map_or(self.tracks.len(), |p| p.track_count as usize);
                    ui.weak(format!("{count} songs"));

                    if self.tracks_loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Loading tracks...");
                        });
                    } else if self.selected_playlist.is_some() {
                        if self.tracks.is_empty() {
                            ui.label(egui::RichText::new("No playable tracks").weak());
                        } else if let (Some(idx), _, _) =
                            track_list(ui, &self.tracks, playing_uri_ref, false, false)
                        {
                            if self.username.is_some() {
                                let queue = queue_uris(&self.tracks);
                                self.play_queue(queue, idx);
                            }
                        }
                    }
                }
            }
        });

        self.settings_window(ctx);

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
        (evt_tx, PlayerApp::new(cmd_tx, evt_rx, Settings::default()))
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
    fn test_apply_event_search_results_matching_query() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.search_query = Some("queen".into());
        app.search_loading = true;

        let tracks = vec![TrackSummary {
            uri: "spotify:track:bohemian".into(),
            name: "Bohemian Rhapsody".into(),
            artists: "Queen".into(),
            duration_ms: 354_000,
        }];

        app.apply_event(
            Event::SearchResults {
                query: "queen".into(),
                tracks: tracks.clone(),
            },
            t0,
        );

        assert_eq!(app.search_results, tracks);
        assert!(!app.search_loading);
    }

    #[test]
    fn test_apply_event_search_results_older_query_ignored() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.search_query = Some("queen".into());
        app.search_loading = true;
        let original_results = vec![TrackSummary {
            uri: "spotify:track:orig".into(),
            name: "Original".into(),
            artists: "Queen".into(),
            duration_ms: 200_000,
        }];
        app.search_results = original_results.clone();

        let old_results = vec![TrackSummary {
            uri: "spotify:track:old".into(),
            name: "Old Result".into(),
            artists: "Old Artist".into(),
            duration_ms: 180_000,
        }];

        app.apply_event(
            Event::SearchResults {
                query: "beatles".into(),
                tracks: old_results,
            },
            t0,
        );

        assert_eq!(app.search_results, original_results);
        assert!(app.search_loading);
    }

    #[test]
    fn test_apply_event_error_clears_search_loading() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.search_loading = true;

        app.apply_event(Event::Error("search failed".into()), t0);
        assert!(!app.search_loading);
        assert_eq!(app.last_error.as_deref(), Some("search failed"));
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
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());

        drop(cmd_rx);
        assert_eq!(app.last_error, None);
        app.send(Command::Pause);
        assert_eq!(app.last_error.as_deref(), Some("the engine has stopped"));
    }

    #[test]
    fn test_send_live_receiver() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());

        app.send(Command::Play("spotify:track:xyz".into()));
        assert_eq!(app.last_error, None);
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:xyz".into()))
        );
    }

    #[test]
    fn test_apply_event_liked_page_loaded() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.liked_loading = true;

        let liked = vec![TrackSummary {
            uri: "spotify:track:liked1".into(),
            name: "Liked Song".into(),
            artists: "Artist".into(),
            duration_ms: 180_000,
        }];

        app.apply_event(
            Event::LikedPageLoaded(crate::webapi::LikedPage {
                offset: 0,
                tracks: liked.clone(),
                total: 42,
            }),
            t0,
        );
        assert_eq!(app.liked, liked);
        assert_eq!(app.liked_total, Some(42));
        assert!(!app.liked_loading);
    }

    #[test]
    fn test_liked_page_wrong_offset_ignored() {
        let (_, mut app) = create_test_app();
        let track = TrackSummary {
            uri: "spotify:track:fixture".into(),
            name: "Fixture".into(),
            artists: "Artist".into(),
            duration_ms: 1000,
        };
        app.liked = vec![track.clone()];
        for offset in [0, 2] {
            app.liked_loading = true;
            app.apply_event(
                Event::LikedPageLoaded(crate::webapi::LikedPage {
                    offset,
                    tracks: vec![track.clone()],
                    total: 8,
                }),
                Instant::now(),
            );
            assert_eq!(app.liked, vec![track.clone()]);
            assert_eq!(app.liked_total, Some(8));
            assert!(!app.liked_loading);
        }
        app.apply_event(
            Event::LikedPageLoaded(crate::webapi::LikedPage {
                offset: 1,
                tracks: vec![track],
                total: 8,
            }),
            Instant::now(),
        );
        assert_eq!(app.liked.len(), 2);
    }

    #[test]
    fn test_toggle_command() {
        assert_eq!(
            toggle_command(&Status::Playing("fixture".into())),
            Some(Command::Pause)
        );
        assert_eq!(toggle_command(&Status::Paused), Some(Command::Resume));
        for status in [
            Status::LoggingIn,
            Status::Idle,
            Status::Stopped,
            Status::Ended,
        ] {
            assert_eq!(toggle_command(&status), None);
        }
    }

    #[test]
    fn test_prev_in_queue() {
        let queue = vec!["first".into(), "second".into(), "third".into()];
        assert_eq!(prev_in_queue(&queue, Some(2)), Some(1));
        assert_eq!(prev_in_queue(&queue, Some(1)), Some(0));
        assert_eq!(prev_in_queue(&queue, Some(0)), None);
        assert_eq!(prev_in_queue(&queue, None), None);
        assert_eq!(prev_in_queue(&queue, Some(3)), None);
        assert_eq!(prev_in_queue(&[], None), None);
        assert_eq!(prev_in_queue(&[], Some(0)), None);
        assert_eq!(prev_in_queue(&[], Some(1)), None);
    }

    #[test]
    fn test_previous_restarts_after_three_seconds_and_otherwise_moves_back() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        let now = Instant::now();
        app.queue = vec!["first".into(), "second".into()];
        app.queue_pos = Some(1);
        app.apply_event(
            Event::Playing {
                track_id: "second".into(),
                position_ms: 2800,
            },
            now,
        );
        app.previous(now + Duration::from_millis(201));
        assert_eq!(cmd_rx.try_recv(), Ok(Command::Play("second".into())));
        assert_eq!(app.queue_pos, Some(1));
        app.previous(now + Duration::from_millis(200));
        assert_eq!(cmd_rx.try_recv(), Ok(Command::Play("first".into())));
        assert_eq!(app.queue_pos, Some(0));
        app.progress = None;
        app.previous(now);
        assert_eq!(cmd_rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn test_previous_restarts_a_paused_track_without_a_queue() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        let now = Instant::now();
        app.apply_event(
            Event::Playing {
                track_id: "fixture".into(),
                position_ms: 0,
            },
            now,
        );
        app.apply_event(Event::Paused { position_ms: 3001 }, now);
        app.previous(now);
        assert_eq!(cmd_rx.try_recv(), Ok(Command::Play("fixture".into())));
        assert_eq!(app.queue_pos, None);
    }

    #[test]
    fn test_liked_paging_error_stops_automatic_loading() {
        let (_, mut app) = create_test_app();
        app.username = Some("testuser".into());
        app.liked_total = Some(42);
        assert!(app.should_load_more_liked(0));
        app.liked_loading = true;
        assert!(!app.should_load_more_liked(0));

        app.apply_event(Event::Error("page failed".into()), Instant::now());

        assert!(app.liked_paging_failed);
        assert!(!app.liked_loading);
        assert!(!app.should_load_more_liked(0));
    }

    #[test]
    fn test_error_without_liked_loading_does_not_block_paging() {
        let (_, mut app) = create_test_app();
        app.username = Some("testuser".into());
        app.liked_total = Some(42);

        app.apply_event(Event::Error("unrelated failure".into()), Instant::now());

        assert!(!app.liked_paging_failed);
        assert!(app.should_load_more_liked(0));
    }

    #[test]
    fn test_reload_liked_clears_paging_failure_and_requests_first_page() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        app.liked = vec![TrackSummary {
            uri: "fixture".into(),
            name: "Song".into(),
            artists: "Artist".into(),
            duration_ms: 1000,
        }];
        app.liked_total = Some(42);
        app.liked_paging_failed = true;

        app.reload_liked();

        assert!(!app.liked_paging_failed);
        assert!(app.liked.is_empty());
        assert_eq!(app.liked_total, None);
        assert!(app.liked_loading);
        assert_eq!(cmd_rx.try_recv(), Ok(Command::LoadLikedPage { offset: 0 }));
    }

    #[test]
    fn test_empty_liked_page_at_current_offset_stops_paging() {
        let (_, mut app) = create_test_app();
        app.username = Some("testuser".into());
        app.liked = vec![TrackSummary {
            uri: "fixture".into(),
            name: "Song".into(),
            artists: "Artist".into(),
            duration_ms: 1000,
        }];
        app.liked_loading = true;

        app.apply_event(
            Event::LikedPageLoaded(crate::webapi::LikedPage {
                offset: 1,
                tracks: Vec::new(),
                total: 42,
            }),
            Instant::now(),
        );

        assert_eq!(app.liked_total, Some(app.liked.len() as u32));
        assert!(!app.liked_loading);
        assert!(!app.should_load_more_liked(app.liked.len()));
    }

    #[test]
    fn test_empty_initial_liked_page_stops_paging() {
        let (_, mut app) = create_test_app();
        app.username = Some("testuser".into());
        app.liked_loading = true;
        app.apply_event(
            Event::LikedPageLoaded(crate::webapi::LikedPage {
                offset: 0,
                tracks: Vec::new(),
                total: 42,
            }),
            Instant::now(),
        );
        assert_eq!(app.liked_total, Some(0));
        assert!(!app.should_load_more_liked(0));
    }

    #[test]
    fn test_queue_from_borrowed_tracks_plays_selected_uri() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        app.tracks = vec![TrackSummary {
            uri: "fixture".into(),
            name: "Song".into(),
            artists: "Artist".into(),
            duration_ms: 1000,
        }];
        let queue = queue_uris(&app.tracks);
        app.play_queue(queue, 0);
        assert_eq!(app.queue, vec!["fixture".to_string()]);
        assert_eq!(app.queue_pos, Some(0));
        assert_eq!(app.tracks.len(), 1);
        assert_eq!(cmd_rx.try_recv(), Ok(Command::Play("fixture".into())));
    }

    #[test]
    fn test_liked_load_uses_loaded_count() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        app.load_liked();
        assert!(app.liked_loading);
        assert_eq!(cmd_rx.try_recv(), Ok(Command::LoadLikedPage { offset: 0 }));
        app.apply_event(
            Event::LikedPageLoaded(crate::webapi::LikedPage {
                offset: 0,
                tracks: vec![TrackSummary {
                    uri: "fixture".into(),
                    name: "Song".into(),
                    artists: "Artist".into(),
                    duration_ms: 1000,
                }],
                total: 2,
            }),
            Instant::now(),
        );
        app.load_liked();
        assert!(app.liked_loading);
        assert_eq!(cmd_rx.try_recv(), Ok(Command::LoadLikedPage { offset: 1 }));
    }

    #[test]
    fn test_loaded_settings_and_saved_event() {
        let (cmd_tx, _cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let settings = Settings {
            bitrate: 96,
            normalisation: true,
            volume_percent: 35,
        };
        let mut app = PlayerApp::new(cmd_tx, evt_rx, settings.clone());
        assert_eq!(app.settings, settings);
        assert_eq!(app.volume_percent, 35);
        assert!(!app.settings_saved);
        app.apply_event(Event::SettingsSaved, Instant::now());
        assert!(app.settings_saved);
    }

    #[test]
    fn test_apply_event_stats_loaded() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.stats_loading = true;

        let stats = Stats {
            recently_played: vec![TrackSummary {
                uri: "spotify:track:recent".into(),
                name: "Recent".into(),
                artists: "Artist".into(),
                duration_ms: 120_000,
            }],
            top_tracks: vec![TrackSummary {
                uri: "spotify:track:top".into(),
                name: "Top".into(),
                artists: "Artist".into(),
                duration_ms: 200_000,
            }],
            top_artists: vec!["Top Artist".into()],
        };

        app.apply_event(Event::StatsLoaded(stats.clone()), t0);
        assert_eq!(app.stats, Some(stats));
        assert!(!app.stats_loading);
    }

    #[test]
    fn test_apply_event_error_clears_liked_and_stats_loading() {
        let t0 = Instant::now();
        let (_evt_tx, mut app) = create_test_app();
        app.liked_loading = true;
        app.stats_loading = true;

        app.apply_event(Event::Error("fetch failed".into()), t0);
        assert!(!app.liked_loading);
        assert!(!app.stats_loading);
        assert_eq!(app.last_error.as_deref(), Some("fetch failed"));
    }

    #[test]
    fn test_next_in_queue() {
        let q = vec![
            "spotify:track:1".to_string(),
            "spotify:track:2".to_string(),
            "spotify:track:3".to_string(),
        ];

        // middle -> next
        assert_eq!(next_in_queue(&q, Some(0)), Some(1));
        assert_eq!(next_in_queue(&q, Some(1)), Some(2));
        // last -> None
        assert_eq!(next_in_queue(&q, Some(2)), None);
        // None -> None
        assert_eq!(next_in_queue(&q, None), None);
        // empty -> None
        let empty: Vec<String> = Vec::new();
        assert_eq!(next_in_queue(&empty, None), None);
        assert_eq!(next_in_queue(&empty, Some(0)), None);
    }

    #[test]
    fn test_end_of_track_with_queue_advances_and_plays() {
        let t0 = Instant::now();
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());

        app.queue = vec![
            "spotify:track:1".into(),
            "spotify:track:2".into(),
            "spotify:track:3".into(),
        ];
        app.queue_pos = Some(0);
        app.status = Status::Playing("spotify:track:1".into());

        app.apply_event(Event::EndOfTrack, t0);

        assert_eq!(app.queue_pos, Some(1));
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:2".into()))
        );
        // status is not ended because next play command was sent
        assert_ne!(app.status, Status::Ended);
    }

    #[test]
    fn test_end_of_track_at_end_of_queue_ends_and_sends_nothing() {
        let t0 = Instant::now();
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());

        app.queue = vec!["spotify:track:1".into(), "spotify:track:2".into()];
        app.queue_pos = Some(1);
        app.status = Status::Playing("spotify:track:2".into());

        app.apply_event(Event::EndOfTrack, t0);

        assert_eq!(app.status, Status::Ended);
        assert_eq!(app.queue_pos, Some(1));
        assert_eq!(cmd_rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn test_stopped_never_sends_play_and_does_not_advance_queue() {
        let t0 = Instant::now();
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());

        app.queue = vec!["spotify:track:1".into(), "spotify:track:2".into()];
        app.queue_pos = Some(0);
        app.status = Status::Playing("spotify:track:1".into());

        app.apply_event(Event::Stopped, t0);

        assert_eq!(app.status, Status::Stopped);
        assert_eq!(app.queue_pos, Some(0));
        assert_eq!(cmd_rx.try_recv(), Err(TryRecvError::Empty));
    }
}
