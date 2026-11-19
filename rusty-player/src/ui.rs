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
use crate::queue::{Queue, QueueSource};
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
            egui_extras::install_image_loaders(&cc.egui_ctx);
            crate::imagecache::install(&cc.egui_ctx);
            load_fonts(&cc.egui_ctx);
            apply_theme(&cc.egui_ctx);
            let app = PlayerApp::new(cmd_tx, evt_rx, ui_settings);
            Ok(Box::new(app))
        }),
    );

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
}

pub fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    filled_circle: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hovered = response.hovered();
        let enabled = ui.is_enabled();
        let center = rect.center();

        let (icon_color, r) = if filled_circle {
            let radius = size / 2.0;
            let circle_color = if !enabled {
                egui::Color32::from_rgb(60, 60, 60)
            } else if size >= 40.0 {
                if hovered {
                    egui::Color32::from_rgb(30, 215, 96)
                } else {
                    GREEN
                }
            } else if hovered {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_rgb(225, 225, 225)
            };
            painter.circle_filled(center, radius, circle_color);
            let icon_col = if !enabled {
                egui::Color32::from_rgb(120, 120, 120)
            } else {
                egui::Color32::BLACK
            };
            (icon_col, size * 0.26)
        } else {
            let icon_col = if !enabled {
                egui::Color32::from_rgb(70, 70, 70)
            } else if hovered {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_rgb(179, 179, 179)
            };
            (icon_col, size * 0.32)
        };

        match icon {
            Icon::Play => {
                let p1 = egui::pos2(center.x + r * 1.05, center.y);
                let p2 = egui::pos2(center.x - r * 0.65, center.y - r * 0.95);
                let p3 = egui::pos2(center.x - r * 0.65, center.y + r * 0.95);
                painter.add(egui::Shape::convex_polygon(
                    vec![p2, p1, p3],
                    icon_color,
                    egui::Stroke::NONE,
                ));
            }
            Icon::Pause => {
                let bar_w = (r * 0.5).max(2.0);
                let bar_h = r * 1.9;
                let gap = r * 0.45;
                let left_x = center.x - (gap + bar_w) / 2.0;
                let right_x = center.x + (gap + bar_w) / 2.0;
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(left_x, center.y),
                        egui::vec2(bar_w, bar_h),
                    ),
                    1.0,
                    icon_color,
                );
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(right_x, center.y),
                        egui::vec2(bar_w, bar_h),
                    ),
                    1.0,
                    icon_color,
                );
            }
            Icon::Next => {
                let p1 = egui::pos2(center.x + r * 0.35, center.y);
                let p2 = egui::pos2(center.x - r * 0.75, center.y - r * 0.85);
                let p3 = egui::pos2(center.x - r * 0.75, center.y + r * 0.85);
                painter.add(egui::Shape::convex_polygon(
                    vec![p2, p1, p3],
                    icon_color,
                    egui::Stroke::NONE,
                ));
                let bar_w = (r * 0.35).max(2.0);
                let bar_h = r * 1.7;
                let bar_x = center.x + r * 0.65;
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(bar_x, center.y),
                        egui::vec2(bar_w, bar_h),
                    ),
                    1.0,
                    icon_color,
                );
            }
            Icon::Prev => {
                let bar_w = (r * 0.35).max(2.0);
                let bar_h = r * 1.7;
                let bar_x = center.x - r * 0.65;
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(bar_x, center.y),
                        egui::vec2(bar_w, bar_h),
                    ),
                    1.0,
                    icon_color,
                );
                let p1 = egui::pos2(center.x - r * 0.35, center.y);
                let p2 = egui::pos2(center.x + r * 0.75, center.y - r * 0.85);
                let p3 = egui::pos2(center.x + r * 0.75, center.y + r * 0.85);
                painter.add(egui::Shape::convex_polygon(
                    vec![p2, p1, p3],
                    icon_color,
                    egui::Stroke::NONE,
                ));
            }
        }
    }
    response
}

pub fn player_bar_zones(total_width: f32) -> (f32, f32, f32) {
    let left = total_width * 0.3;
    let center = total_width * 0.4;
    let right = total_width * 0.3;
    (left, center, right)
}

pub fn row_icon(is_hovered: bool, is_playing_row: bool, status_playing: bool) -> Option<Icon> {
    if is_playing_row {
        if status_playing {
            Some(Icon::Pause)
        } else {
            Some(Icon::Play)
        }
    } else if is_hovered {
        Some(Icon::Play)
    } else {
        None
    }
}

fn draw_purple_heart(
    painter: &egui::Painter,
    rect: egui::Rect,
    corner_radius: f32,
    font_size: f32,
) {
    let purple = egui::Color32::from_rgb(80, 56, 160);
    painter.rect_filled(rect, corner_radius, purple);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "♥",
        egui::FontId::proportional(font_size),
        egui::Color32::WHITE,
    );
}

fn draw_volume_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let body = egui::Rect::from_min_size(egui::pos2(c.x - 6.0, c.y - 3.0), egui::vec2(3.0, 6.0));
    painter.rect_filled(body, 0.0, color);
    let p1 = egui::pos2(c.x - 3.0, c.y - 3.0);
    let p2 = egui::pos2(c.x + 1.0, c.y - 6.0);
    let p3 = egui::pos2(c.x + 1.0, c.y + 6.0);
    let p4 = egui::pos2(c.x - 3.0, c.y + 3.0);
    painter.add(egui::Shape::convex_polygon(
        vec![p1, p2, p3, p4],
        color,
        egui::Stroke::NONE,
    ));
    let arc_x = c.x + 4.0;
    let wave_top = egui::pos2(arc_x, c.y - 4.0);
    let wave_mid = egui::pos2(arc_x + 2.0, c.y);
    let wave_bot = egui::pos2(arc_x, c.y + 4.0);
    painter.line_segment([wave_top, wave_mid], egui::Stroke::new(1.5_f32, color));
    painter.line_segment([wave_mid, wave_bot], egui::Stroke::new(1.5_f32, color));
}

fn draw_shuffle_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let stroke = egui::Stroke::new(1.8_f32, color);
    let x0 = c.x - 7.0;
    let x1 = c.x + 7.0;
    let y0 = c.y - 5.0;
    let y1 = c.y + 5.0;

    painter.line_segment([egui::pos2(x0, y0), egui::pos2(x1, y1)], stroke);
    painter.line_segment([egui::pos2(x0, y1), egui::pos2(x1, y0)], stroke);

    painter.line_segment([egui::pos2(x1 - 4.0, y0), egui::pos2(x1, y0)], stroke);
    painter.line_segment([egui::pos2(x1, y0 + 4.0), egui::pos2(x1, y0)], stroke);

    painter.line_segment([egui::pos2(x1 - 4.0, y1), egui::pos2(x1, y1)], stroke);
    painter.line_segment([egui::pos2(x1, y1 - 4.0), egui::pos2(x1, y1)], stroke);
}

fn draw_queue_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let left = c.x - 7.0;
    let right = c.x + 7.0;
    let stroke = egui::Stroke::new(1.8_f32, color);
    for y_off in [-5.0, 0.0, 5.0] {
        painter.line_segment(
            [
                egui::pos2(left, c.y + y_off),
                egui::pos2(right, c.y + y_off),
            ],
            stroke,
        );
    }
}

fn draw_now_playing_bars(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let bar_w = 2.0;
    let gap = 2.0;
    let heights = [6.0, 14.0, 10.0];
    let total_w = 3.0 * bar_w + 2.0 * gap;
    let start_x = rect.center().x - total_w / 2.0;
    let bot_y = rect.center().y + 7.0;
    for (i, &h) in heights.iter().enumerate() {
        let x = start_x + i as f32 * (bar_w + gap);
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, bot_y - h), egui::vec2(bar_w, h)),
            1.0,
            color,
        );
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
    queue: Option<Queue>,
    queue_open: bool,
    queue_scroll_to_current: bool,
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
            queue: None,
            queue_open: false,
            queue_scroll_to_current: false,
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
                if self.playlists.is_empty()
                    && !self.playlists_loading
                    && crate::webapi::has_saved_token()
                {
                    self.playlists_loading = true;
                    self.send(Command::LoadPlaylists);
                }
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
                if let Some(queue) = self.queue.as_mut() {
                    if let Some(track) = queue.advance() {
                        let uri = track.uri.clone();
                        self.send(Command::Play(uri));
                    } else {
                        self.status = Status::Ended;
                    }
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
                    } else {
                        if let Some(queue) = self.queue.as_mut() {
                            if queue.source() == &QueueSource::Liked {
                                queue.append(page.tracks.clone());
                            }
                        }
                        self.liked.extend(page.tracks);
                    }
                }
                self.liked_loading = false;
                if self.should_background_load_liked() {
                    self.load_liked();
                }
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

    fn play_tracks(&mut self, tracks: &[TrackSummary], index: usize, source: QueueSource) {
        if tracks.is_empty() {
            return;
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let mut q = Queue::new(tracks.to_vec(), index, source, seed);
        if self.settings.shuffle {
            q.set_shuffle(true);
        }
        if let Some(track) = q.current() {
            let uri = track.uri.clone();
            self.send(Command::Play(uri));
        }
        self.queue = Some(q);
        if self.should_background_load_liked() {
            self.load_liked();
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.settings.shuffle = !self.settings.shuffle;
        if let Some(queue) = self.queue.as_mut() {
            queue.set_shuffle(self.settings.shuffle);
        }
        self.settings_saved = false;
        self.send(Command::SaveSettings(self.settings.clone()));
        if self.should_background_load_liked() {
            self.load_liked();
        }
    }

    fn next(&mut self) {
        if let Some(queue) = self.queue.as_mut() {
            if let Some(track) = queue.advance() {
                let uri = track.uri.clone();
                self.send(Command::Play(uri));
            }
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
        } else if let Some(queue) = self.queue.as_mut() {
            if let Some(track) = queue.back() {
                let uri = track.uri.clone();
                self.send(Command::Play(uri));
            }
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

    fn should_background_load_liked(&self) -> bool {
        let queue_is_liked = self
            .queue
            .as_ref()
            .is_some_and(|q| q.source() == &QueueSource::Liked);
        queue_is_liked
            && self.settings.shuffle
            && self.username.is_some()
            && !self.liked_loading
            && !self.liked_paging_failed
            && self
                .liked_total
                .is_some_and(|total| self.liked.len() < total as usize)
    }

    fn library_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("playlists")
            .resizable(true)
            .default_width(260.0)
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
                ui.add_space(4.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(self.username.is_some(), |ui| {
                        let liked_selected = self.view == View::Liked;
                        let count_str = self.liked_total.map_or_else(
                            || format!("Playlist · {} songs", self.liked.len()),
                            |t| format!("Playlist · {t} songs"),
                        );
                        if library_entry(ui, "Liked Songs", &count_str, None, true, liked_selected)
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
                        let subtitle = format!("Playlist · {} songs", playlist.track_count);
                        let response = ui
                            .add_enabled_ui(self.username.is_some(), |ui| {
                                library_entry(
                                    ui,
                                    &playlist.name,
                                    &subtitle,
                                    playlist.image_url.as_deref(),
                                    false,
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
            let track = TrackSummary {
                uri: text.clone(),
                name: text.clone(),
                artists: String::new(),
                album: String::new(),
                duration_ms: 0,
                image_url: None,
            };
            self.play_tracks(&[track], 0, QueueSource::Link);
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

    fn queue_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("queue")
            .resizable(true)
            .default_width(280.0)
            .frame(side_frame())
            .show(ctx, |ui| {
                ui.heading("Queue");
                let Some(queue) = self.queue.as_ref() else {
                    ui.weak("No queue");
                    return;
                };

                let source_str = match queue.source() {
                    QueueSource::Playlist(name) => name.clone(),
                    QueueSource::Liked => "Liked Songs".to_string(),
                    QueueSource::Search => "Search".to_string(),
                    QueueSource::Stats => "Stats".to_string(),
                    QueueSource::Link => "Link".to_string(),
                };
                ui.weak(format!("Playing from: {source_str}"));
                ui.add_space(8.0);

                let current_pos = queue.pos();
                let mut queue_move = None;
                let mut queue_jump = None;
                let mut scrolled_to_current = false;

                let row_height = 48.0;
                let total_rows = queue.len();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, row_height, total_rows, |ui, rows| {
                        for idx in rows {
                            let Some(entry) = queue.entries().get(idx) else {
                                continue;
                            };
                            let is_past = current_pos.is_some_and(|pos| idx < pos);
                            let is_current = current_pos == Some(idx);
                            let is_next_up = current_pos.is_some_and(|pos| idx == pos + 1);

                            if is_current {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("Now playing").weak().small());
                            } else if is_next_up {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("Next up").weak().small());
                            }

                            let id = egui::Id::new("queue_row").with(entry.id);
                            let drag_resp = ui
                                .dnd_drag_source(id, idx, |ui| {
                                    let available_w = ui.available_width();
                                    let (row_rect, row_resp) = ui.allocate_exact_size(
                                        egui::vec2(available_w, row_height),
                                        egui::Sense::hover(),
                                    );
                                    let is_hovered = row_resp.hovered();
                                    let bg_color = if is_current {
                                        CARD
                                    } else if is_hovered {
                                        HOVER
                                    } else {
                                        egui::Color32::TRANSPARENT
                                    };
                                    ui.painter().rect_filled(row_rect, 4.0, bg_color);

                                    let mut row_ui = ui.new_child(
                                        egui::UiBuilder::new()
                                            .max_rect(row_rect.shrink2(egui::vec2(4.0, 4.0)))
                                            .layout(egui::Layout::left_to_right(
                                                egui::Align::Center,
                                            )),
                                    );

                                    let thumb_size = egui::vec2(40.0, 40.0);
                                    if let Some(url) = &entry.track.image_url {
                                        let mut img = egui::Image::new(url.as_str())
                                            .fit_to_exact_size(thumb_size)
                                            .corner_radius(4.0);
                                        if is_past {
                                            img = img.tint(egui::Color32::from_white_alpha(128));
                                        }
                                        row_ui.add(img);
                                    } else {
                                        let (t_rect, _) = row_ui
                                            .allocate_exact_size(thumb_size, egui::Sense::hover());
                                        let fill = if is_past {
                                            egui::Color32::from_rgb(20, 20, 20)
                                        } else {
                                            CARD
                                        };
                                        row_ui.painter().rect_filled(t_rect, 4.0, fill);
                                    }

                                    row_ui.add_space(6.0);

                                    if is_current {
                                        row_ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let (bars_rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(16.0, 16.0),
                                                    egui::Sense::hover(),
                                                );
                                                draw_now_playing_bars(
                                                    ui.painter(),
                                                    bars_rect,
                                                    GREEN,
                                                );
                                            },
                                        );
                                    }

                                    let text_w = row_ui.available_width();
                                    row_ui.allocate_ui_with_layout(
                                        egui::vec2(text_w, 40.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.add_space(2.0);
                                            let title_color = if is_current {
                                                GREEN
                                            } else if is_past {
                                                egui::Color32::from_rgb(140, 140, 140)
                                            } else {
                                                egui::Color32::WHITE
                                            };
                                            let mut title_rt =
                                                egui::RichText::new(&entry.track.name)
                                                    .size(13.0)
                                                    .color(title_color);
                                            if !is_past {
                                                title_rt = title_rt.strong();
                                            } else {
                                                title_rt = title_rt.weak();
                                            }
                                            ui.add(egui::Label::new(title_rt).truncate());

                                            let mut artist_rt =
                                                egui::RichText::new(&entry.track.artists).small();
                                            if is_past {
                                                artist_rt = artist_rt
                                                    .color(egui::Color32::from_rgb(110, 110, 110));
                                            } else {
                                                artist_rt = artist_rt.weak();
                                            }
                                            ui.add(egui::Label::new(artist_rt).truncate());
                                        },
                                    );
                                })
                                .response;

                            let response = ui.interact(
                                drag_resp.rect,
                                drag_resp.id,
                                egui::Sense::click_and_drag(),
                            );

                            if is_current && self.queue_scroll_to_current {
                                response.scroll_to_me(Some(egui::Align::Center));
                                scrolled_to_current = true;
                            }

                            if let Some(from) = response.dnd_release_payload::<usize>() {
                                queue_move = Some((*from, idx));
                            }
                            if let Some(from) = response.dnd_hover_payload::<usize>() {
                                let y = if *from <= idx {
                                    response.rect.bottom()
                                } else {
                                    response.rect.top()
                                };
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(response.rect.left(), y),
                                        egui::pos2(response.rect.right(), y),
                                    ],
                                    egui::Stroke::new(2.0_f32, GREEN),
                                );
                            }
                            if response.double_clicked() {
                                queue_jump = Some(idx);
                            }
                        }
                    });

                if self.queue_scroll_to_current {
                    if !scrolled_to_current {
                        if let Some(pos) = current_pos {
                            let approx_y = pos as f32 * row_height;
                            ui.scroll_to_rect(
                                egui::Rect::from_min_size(
                                    egui::pos2(ui.min_rect().min.x, approx_y),
                                    egui::vec2(10.0, row_height),
                                ),
                                Some(egui::Align::Center),
                            );
                        }
                    }
                    self.queue_scroll_to_current = false;
                }

                if let Some((from, to)) = queue_move {
                    if let Some(queue) = self.queue.as_mut() {
                        queue.move_entry(from, to);
                    }
                }
                if let Some(idx) = queue_jump {
                    if let Some(queue) = self.queue.as_mut() {
                        if let Some(track) = queue.jump_to(idx) {
                            let uri = track.uri.clone();
                            self.send(Command::Play(uri));
                        }
                    }
                }
            });
    }

    fn stats_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("stats")
            .resizable(true)
            .default_width(240.0)
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
                                if stats_track_entry(ui, track) && self.username.is_some() {
                                    clicked_stats_track =
                                        Some((stats.recently_played.clone(), idx));
                                }
                            }
                        });

                        ui.collapsing("Top tracks", |ui| {
                            for (idx, track) in stats.top_tracks.iter().enumerate() {
                                if stats_track_entry(ui, track) && self.username.is_some() {
                                    clicked_stats_track = Some((stats.top_tracks.clone(), idx));
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

                if let Some((tracks, idx)) = clicked_stats_track {
                    self.play_tracks(&tracks, idx, QueueSource::Stats);
                }
            });
    }
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).color(egui::Color32::BLACK)).fill(GREEN))
}

fn side_frame() -> egui::Frame {
    egui::Frame::new().fill(BACKGROUND).inner_margin(8)
}

fn library_entry(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    image_url: Option<&str>,
    is_liked: bool,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::click());
    let bg_color = if selected || response.hovered() {
        HOVER
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, bg_color);

    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(6.0, 4.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    if is_liked {
        let (heart_rect, _) =
            row_ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
        draw_purple_heart(row_ui.painter(), heart_rect, 4.0, 20.0);
    } else if let Some(url) = image_url {
        row_ui.add(
            egui::Image::new(url)
                .fit_to_exact_size(egui::vec2(48.0, 48.0))
                .corner_radius(4.0),
        );
    } else {
        let (p_rect, _) = row_ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
        row_ui.painter().rect_filled(p_rect, 4.0, CARD);
    }

    row_ui.add_space(8.0);

    let text_w = row_ui.available_width();
    row_ui.allocate_ui_with_layout(
        egui::vec2(text_w, 48.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.add_space(6.0);
            let mut title_rt = egui::RichText::new(title).strong();
            if selected {
                title_rt = title_rt.color(GREEN);
            } else {
                title_rt = title_rt.color(egui::Color32::WHITE);
            }
            ui.add(egui::Label::new(title_rt).truncate());
            ui.add(egui::Label::new(egui::RichText::new(subtitle).weak().small()).truncate());
        },
    );

    response
}

fn stats_track_entry(ui: &mut egui::Ui, track: &TrackSummary) -> bool {
    let row_height = 40.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        egui::Sense::click(),
    );
    let is_hovered = response.hovered();
    if is_hovered {
        ui.painter().rect_filled(rect, 4.0, HOVER);
    }

    let mut play_clicked = false;
    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(4.0, 4.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    let thumb_resp = if let Some(url) = &track.image_url {
        row_ui.add(
            egui::Image::new(url.as_str())
                .fit_to_exact_size(egui::vec2(32.0, 32.0))
                .corner_radius(4.0),
        )
    } else {
        let (t_rect, resp) =
            row_ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
        row_ui.painter().rect_filled(t_rect, 4.0, CARD);
        resp
    };
    let thumb_rect = thumb_resp.rect;

    if is_hovered {
        row_ui.painter().rect_filled(
            thumb_rect,
            4.0,
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
        );
        let mut overlay_ui = row_ui.new_child(egui::UiBuilder::new().max_rect(thumb_rect).layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        ));
        if icon_button(&mut overlay_ui, Icon::Play, 18.0, false).clicked() {
            play_clicked = true;
        }
    }

    row_ui.add_space(6.0);
    let text_w = row_ui.available_width();
    row_ui.allocate_ui_with_layout(
        egui::vec2(text_w, 32.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(&track.name).strong().size(12.0)).truncate(),
            );
            ui.add(egui::Label::new(egui::RichText::new(&track.artists).weak().small()).truncate());
        },
    );

    play_clicked || response.double_clicked()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackAction {
    Play(usize),
    Toggle,
}

fn track_list(
    ui: &mut egui::Ui,
    tracks: &[TrackSummary],
    playing_uri: Option<&str>,
    status_is_playing: bool,
    loading: bool,
    paging_failed: bool,
) -> (Option<TrackAction>, usize, bool) {
    let available_w = ui.available_width();
    let col_hash_w = 40.0;
    let col_dur_w = 70.0;
    let col_spacing = 16.0;
    let rem_w = (available_w - col_hash_w - col_dur_w - col_spacing * 3.0).max(100.0);
    let col_title_w = rem_w * 0.60;
    let col_album_w = rem_w * 0.40;

    ui.horizontal(|ui| {
        ui.add_sized(
            [col_hash_w, 20.0],
            egui::Label::new(egui::RichText::new("#").weak()),
        );
        ui.add_sized(
            [col_title_w, 20.0],
            egui::Label::new(egui::RichText::new("Title").weak()),
        );
        ui.add_sized(
            [col_album_w, 20.0],
            egui::Label::new(egui::RichText::new("Album").weak()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_sized(
                [col_dur_w, 20.0],
                egui::Label::new(egui::RichText::new("Duration").weak()),
            );
        });
    });
    ui.separator();

    let row_height = 56.0;
    let mut action = None;
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
                    let (row_rect, row_resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_height),
                        egui::Sense::click(),
                    );
                    let is_hovered = row_resp.hovered();

                    if is_hovered {
                        ui.painter().rect_filled(row_rect, 4.0, HOVER);
                    }

                    if row_resp.double_clicked() {
                        action = Some(TrackAction::Play(idx));
                    }

                    let mut row_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(row_rect.shrink2(egui::vec2(8.0, 4.0)))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );

                    let icon_opt = row_icon(is_hovered, is_current, status_is_playing);
                    row_ui.allocate_ui_with_layout(
                        egui::vec2(col_hash_w, 48.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            if let Some(icon) = icon_opt {
                                let btn_resp = icon_button(ui, icon, 20.0, false);
                                if btn_resp.clicked() {
                                    if is_current {
                                        action = Some(TrackAction::Toggle);
                                    } else {
                                        action = Some(TrackAction::Play(idx));
                                    }
                                }
                            } else {
                                let mut num_text =
                                    egui::RichText::new((idx + 1).to_string()).weak();
                                if is_current {
                                    num_text = num_text.color(GREEN);
                                }
                                ui.add(egui::Label::new(num_text));
                            }
                        },
                    );

                    if let Some(url) = &track.image_url {
                        row_ui.add(
                            egui::Image::new(url.as_str())
                                .fit_to_exact_size(egui::vec2(40.0, 40.0))
                                .corner_radius(4.0),
                        );
                    } else {
                        let (t_rect, _) = row_ui
                            .allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                        row_ui.painter().rect_filled(t_rect, 4.0, CARD);
                    }
                    row_ui.add_space(8.0);

                    let title_actual_w = (col_title_w - 48.0).max(20.0);
                    row_ui.allocate_ui_with_layout(
                        egui::vec2(title_actual_w, 48.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.add_space(4.0);
                            let title_text = if is_current {
                                egui::RichText::new(&track.name).strong().color(GREEN)
                            } else {
                                egui::RichText::new(&track.name)
                                    .strong()
                                    .color(egui::Color32::WHITE)
                            };
                            ui.add(egui::Label::new(title_text).truncate());
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&track.artists).weak().small(),
                                )
                                .truncate(),
                            );
                        },
                    );

                    row_ui.allocate_ui_with_layout(
                        egui::vec2(col_album_w, 48.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&track.album).weak().small())
                                    .truncate(),
                            );
                        },
                    );

                    row_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(
                            egui::RichText::new(format_time(track.duration_ms))
                                .weak()
                                .small(),
                        ));
                    });
                }
            },
        );

    (action, visible_end, retry)
}

enum HeaderAction {
    Play,
    Toggle,
}

fn render_header(
    ui: &mut egui::Ui,
    view: View,
    title: &str,
    subtitle_type: &str,
    count: usize,
    image_url: Option<&str>,
) {
    let header_height = if view == View::Search { 90.0 } else { 180.0 };
    let (header_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), header_height),
        egui::Sense::hover(),
    );

    let top_color = match view {
        View::Liked => egui::Color32::from_rgb(55, 35, 90),
        _ => egui::Color32::from_rgb(38, 38, 38),
    };
    let bot_color = BACKGROUND;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(header_rect.left_top(), top_color);
    mesh.colored_vertex(header_rect.right_top(), top_color);
    mesh.colored_vertex(header_rect.right_bottom(), bot_color);
    mesh.colored_vertex(header_rect.left_bottom(), bot_color);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);

    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(header_rect.shrink2(egui::vec2(16.0, 12.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Max)),
    );

    if view != View::Search {
        if view == View::Liked {
            let (cover_rect, _) =
                content_ui.allocate_exact_size(egui::vec2(160.0, 160.0), egui::Sense::hover());
            draw_purple_heart(content_ui.painter(), cover_rect, 4.0, 64.0);
        } else if let Some(url) = image_url {
            content_ui.add(
                egui::Image::new(url)
                    .fit_to_exact_size(egui::vec2(160.0, 160.0))
                    .corner_radius(4.0),
            );
        } else {
            let (cover_rect, _) =
                content_ui.allocate_exact_size(egui::vec2(160.0, 160.0), egui::Sense::hover());
            content_ui.painter().rect_filled(cover_rect, 4.0, CARD);
        }
        content_ui.add_space(16.0);
    }

    let text_height = if view == View::Search { 70.0 } else { 160.0 };
    let text_w = content_ui.available_width();
    content_ui.allocate_ui_with_layout(
        egui::vec2(text_w, text_height),
        egui::Layout::bottom_up(egui::Align::Min),
        |ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!("{count} songs")).weak().small());
            ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .size(32.0)
                        .strong()
                        .color(egui::Color32::WHITE),
                )
                .truncate(),
            );
            ui.label(egui::RichText::new(subtitle_type).weak().small());
        },
    );
}

fn render_header_actions(
    ui: &mut egui::Ui,
    tracks: &[TrackSummary],
    source: &QueueSource,
    queue: Option<&Queue>,
    status: &Status,
    current_uri: Option<&str>,
    has_user: bool,
) -> Option<HeaderAction> {
    ui.add_space(8.0);
    let mut action = None;
    ui.horizontal(|ui| {
        let is_this_list = queue.is_some_and(|q| q.source() == source && current_uri.is_some())
            && tracks.iter().any(|t| Some(t.uri.as_str()) == current_uri);
        let is_playing = is_this_list && matches!(status, Status::Playing(_));
        let btn_icon = if is_playing { Icon::Pause } else { Icon::Play };
        let can_play = has_user && !tracks.is_empty();

        let btn_resp = ui
            .add_enabled_ui(can_play, |ui| icon_button(ui, btn_icon, 48.0, true))
            .inner;

        if btn_resp.clicked() {
            if is_this_list {
                action = Some(HeaderAction::Toggle);
            } else {
                action = Some(HeaderAction::Play);
            }
        }
    });
    ui.add_space(8.0);
    action
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

        if self.should_background_load_liked() {
            self.load_liked();
        }

        egui::TopBottomPanel::top("top_bar")
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
                let total_w = ui.available_width();
                let (left_w, center_w, right_w) = player_bar_zones(total_w);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                    // Left 30%
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_w, 66.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(left_w);
                            if let Some(track) = &self.track {
                                if let Some(url) = &track.cover_url {
                                    ui.add(
                                        egui::Image::new(url.as_str())
                                            .fit_to_exact_size(egui::vec2(56.0, 56.0))
                                            .corner_radius(4.0),
                                    );
                                } else {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(56.0, 56.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(rect, 4.0, CARD);
                                }
                                ui.add_space(8.0);
                                let text_w = (left_w - 74.0).max(10.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(text_w, 56.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.add_space(8.0);
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&track.title)
                                                    .strong()
                                                    .color(egui::Color32::WHITE),
                                            )
                                            .truncate(),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&track.artists).weak().small(),
                                            )
                                            .truncate(),
                                        );
                                    },
                                );
                            } else {
                                ui.weak(status_text(&self.status));
                            }
                        },
                    );

                    // Center 40%
                    ui.allocate_ui_with_layout(
                        egui::vec2(center_w, 66.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(center_w);
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                let btn_row_w = 28.0 + 16.0 + 28.0 + 16.0 + 36.0 + 16.0 + 28.0;
                                ui.add_space(((center_w - btn_row_w) / 2.0).max(0.0));

                                // Shuffle button
                                let can_shuffle = self.username.is_some();
                                let (shuf_rect, shuf_resp) = ui.allocate_exact_size(
                                    egui::vec2(28.0, 28.0),
                                    egui::Sense::click(),
                                );
                                let shuf_hovered = shuf_resp.hovered();
                                let shuf_color = if !can_shuffle {
                                    egui::Color32::from_rgb(70, 70, 70)
                                } else if self.settings.shuffle {
                                    GREEN
                                } else if shuf_hovered {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::from_rgb(179, 179, 179)
                                };
                                draw_shuffle_icon(ui.painter(), shuf_rect, shuf_color);
                                if can_shuffle && shuf_resp.on_hover_text("Shuffle").clicked() {
                                    self.toggle_shuffle();
                                }

                                ui.add_space(16.0);

                                let can_back = self
                                    .queue
                                    .as_ref()
                                    .is_some_and(|q| q.pos().is_some_and(|pos| pos > 0));
                                let can_prev =
                                    self.username.is_some() && (self.can_restart(now) || can_back);
                                let prev_resp = ui
                                    .add_enabled_ui(can_prev, |ui| {
                                        icon_button(ui, Icon::Prev, 28.0, false)
                                    })
                                    .inner;
                                if prev_resp.on_hover_text("Previous").clicked() {
                                    self.previous(now);
                                }

                                ui.add_space(16.0);

                                let can_toggle = self.username.is_some()
                                    && toggle_command(&self.status).is_some();
                                let is_playing = matches!(self.status, Status::Playing(_));
                                let play_icon = if is_playing { Icon::Pause } else { Icon::Play };
                                let play_resp = ui
                                    .add_enabled_ui(can_toggle, |ui| {
                                        icon_button(ui, play_icon, 36.0, true)
                                    })
                                    .inner;
                                if play_resp
                                    .on_hover_text(if is_playing { "Pause" } else { "Resume" })
                                    .clicked()
                                {
                                    self.toggle();
                                }

                                ui.add_space(16.0);

                                let can_next = self.username.is_some()
                                    && self.queue.as_ref().and_then(|q| q.peek_next()).is_some();
                                let next_resp = ui
                                    .add_enabled_ui(can_next, |ui| {
                                        icon_button(ui, Icon::Next, 28.0, false)
                                    })
                                    .inner;
                                if next_resp.on_hover_text("Next").clicked() {
                                    self.next();
                                }
                            });

                            ui.add_space(6.0);

                            let duration = self.track.as_ref().map_or(0, |t| t.duration_ms);
                            let position = self.progress.map_or(0, |p| p.at(now)).min(duration);
                            let fraction = if duration == 0 {
                                0.0
                            } else {
                                position as f32 / duration as f32
                            };

                            ui.horizontal(|ui| {
                                let time_label_w = 34.0;
                                let bar_w = (center_w - (time_label_w * 2.0 + 24.0)).max(20.0);
                                let total_bar_w = time_label_w * 2.0 + bar_w + 16.0;
                                ui.add_space(((center_w - total_bar_w) / 2.0).max(0.0));

                                ui.add_sized(
                                    [time_label_w, 12.0],
                                    egui::Label::new(
                                        egui::RichText::new(format_time(position)).weak().small(),
                                    ),
                                );

                                let (bar_rect, bar_resp) = ui.allocate_exact_size(
                                    egui::vec2(bar_w, 12.0),
                                    egui::Sense::hover(),
                                );
                                let track_rect = egui::Rect::from_center_size(
                                    bar_rect.center(),
                                    egui::vec2(bar_rect.width(), 4.0),
                                );
                                ui.painter().rect_filled(
                                    track_rect,
                                    2.0,
                                    egui::Color32::from_rgb(77, 77, 77),
                                );
                                let fill_w =
                                    (track_rect.width() * fraction.clamp(0.0, 1.0)).max(0.0);
                                let fill_rect = egui::Rect::from_min_size(
                                    track_rect.min,
                                    egui::vec2(fill_w, 4.0),
                                );
                                let fill_col = if bar_resp.hovered() {
                                    GREEN
                                } else {
                                    egui::Color32::WHITE
                                };
                                ui.painter().rect_filled(fill_rect, 2.0, fill_col);
                                if bar_resp.hovered() {
                                    let thumb_center =
                                        egui::pos2(fill_rect.max.x, track_rect.center().y);
                                    ui.painter().circle_filled(
                                        thumb_center,
                                        4.0,
                                        egui::Color32::WHITE,
                                    );
                                }

                                ui.add_sized(
                                    [time_label_w, 12.0],
                                    egui::Label::new(
                                        egui::RichText::new(format_time(duration)).weak().small(),
                                    ),
                                );
                            });
                        },
                    );

                    // Right 30%
                    ui.allocate_ui_with_layout(
                        egui::vec2(right_w, 66.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.add_space(8.0);
                            ui.add_enabled_ui(self.username.is_some(), |ui| {
                                // Queue toggle button
                                let (q_rect, q_resp) = ui.allocate_exact_size(
                                    egui::vec2(24.0, 24.0),
                                    egui::Sense::click(),
                                );
                                let q_color = if self.queue_open {
                                    GREEN
                                } else if q_resp.hovered() {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::from_rgb(179, 179, 179)
                                };
                                draw_queue_icon(ui.painter(), q_rect, q_color);
                                if q_resp.on_hover_text("Queue").clicked() {
                                    self.queue_open = !self.queue_open;
                                    if self.queue_open {
                                        self.queue_scroll_to_current = true;
                                    }
                                }

                                ui.add_space(16.0);

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
                                let (vol_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(18.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                draw_volume_icon(
                                    ui.painter(),
                                    vol_rect,
                                    egui::Color32::from_rgb(179, 179, 179),
                                );
                            });
                        },
                    );
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

        if self.queue_open {
            self.queue_panel(ctx);
        } else {
            self.stats_panel(ctx);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(16))
            .show(ctx, |ui| {
                let playing_uri = self.current_uri.clone();
                let playing_uri_ref = playing_uri.as_deref();

                match self.view {
                    View::Search => {
                        let heading_text = match &self.search_query {
                            Some(q) => format!("Search: {q}"),
                            None => "Search".to_string(),
                        };
                        render_header(
                            ui,
                            View::Search,
                            &heading_text,
                            "Search",
                            self.search_results.len(),
                            None,
                        );
                        let source = QueueSource::Search;
                        if let Some(action) = render_header_actions(
                            ui,
                            &self.search_results,
                            &source,
                            self.queue.as_ref(),
                            &self.status,
                            self.current_uri.as_deref(),
                            self.username.is_some(),
                        ) {
                            match action {
                                HeaderAction::Play => {
                                    let tracks = self.search_results.clone();
                                    self.play_tracks(&tracks, 0, source);
                                }
                                HeaderAction::Toggle => self.toggle(),
                            }
                        }

                        if self.search_loading {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Searching...");
                            });
                        } else if self.search_results.is_empty() {
                            ui.label(egui::RichText::new("No results").weak());
                        } else {
                            let is_playing = matches!(self.status, Status::Playing(_));
                            let (action, _, _) = track_list(
                                ui,
                                &self.search_results,
                                playing_uri_ref,
                                is_playing,
                                false,
                                false,
                            );
                            if self.username.is_some() {
                                match action {
                                    Some(TrackAction::Play(idx)) => {
                                        let tracks = self.search_results.clone();
                                        self.play_tracks(&tracks, idx, QueueSource::Search);
                                    }
                                    Some(TrackAction::Toggle) => {
                                        self.toggle();
                                    }
                                    None => {}
                                }
                            }
                        }
                    }
                    View::Liked => {
                        let total = self.liked_total.map_or(self.liked.len(), |n| n as usize);
                        render_header(ui, View::Liked, "Liked Songs", "Playlist", total, None);
                        let source = QueueSource::Liked;
                        if let Some(action) = render_header_actions(
                            ui,
                            &self.liked,
                            &source,
                            self.queue.as_ref(),
                            &self.status,
                            self.current_uri.as_deref(),
                            self.username.is_some(),
                        ) {
                            match action {
                                HeaderAction::Play => {
                                    let tracks = self.liked.clone();
                                    self.play_tracks(&tracks, 0, source);
                                }
                                HeaderAction::Toggle => self.toggle(),
                            }
                        }

                        if ui
                            .add_enabled(
                                self.username.is_some() && !self.liked_loading,
                                egui::Button::new("Reload").small(),
                            )
                            .clicked()
                        {
                            self.reload_liked();
                        }
                        if self.liked.is_empty() && !self.liked_loading {
                            ui.label(egui::RichText::new("No liked tracks").weak());
                        }
                        let is_playing = matches!(self.status, Status::Playing(_));
                        let (action, visible_end, retry) = track_list(
                            ui,
                            &self.liked,
                            playing_uri_ref,
                            is_playing,
                            self.liked_loading,
                            self.liked_paging_failed,
                        );
                        if self.username.is_some() {
                            match action {
                                Some(TrackAction::Play(idx)) => {
                                    let tracks = self.liked.clone();
                                    self.play_tracks(&tracks, idx, QueueSource::Liked);
                                }
                                Some(TrackAction::Toggle) => {
                                    self.toggle();
                                }
                                None => {}
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
                        let (playlist_name, track_count, image_url) = self
                            .selected_playlist
                            .as_ref()
                            .and_then(|id| self.playlists.iter().find(|p| &p.id == id))
                            .map(|p| (p.name.clone(), p.track_count as usize, p.image_url.clone()))
                            .unwrap_or_else(|| {
                                ("Select a playlist".to_string(), self.tracks.len(), None)
                            });

                        render_header(
                            ui,
                            View::Playlist,
                            &playlist_name,
                            "Playlist",
                            track_count,
                            image_url.as_deref(),
                        );
                        let source = QueueSource::Playlist(playlist_name.clone());
                        if let Some(action) = render_header_actions(
                            ui,
                            &self.tracks,
                            &source,
                            self.queue.as_ref(),
                            &self.status,
                            self.current_uri.as_deref(),
                            self.username.is_some(),
                        ) {
                            match action {
                                HeaderAction::Play => {
                                    let tracks = self.tracks.clone();
                                    self.play_tracks(&tracks, 0, source);
                                }
                                HeaderAction::Toggle => self.toggle(),
                            }
                        }

                        if self.tracks_loading {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Loading tracks...");
                            });
                        } else if self.selected_playlist.is_some() {
                            if self.tracks.is_empty() {
                                ui.label(egui::RichText::new("No playable tracks").weak());
                            } else {
                                let is_playing = matches!(self.status, Status::Playing(_));
                                let (action, _, _) = track_list(
                                    ui,
                                    &self.tracks,
                                    playing_uri_ref,
                                    is_playing,
                                    false,
                                    false,
                                );
                                if self.username.is_some() {
                                    match action {
                                        Some(TrackAction::Play(idx)) => {
                                            let tracks = self.tracks.clone();
                                            self.play_tracks(
                                                &tracks,
                                                idx,
                                                QueueSource::Playlist(playlist_name.clone()),
                                            );
                                        }
                                        Some(TrackAction::Toggle) => {
                                            self.toggle();
                                        }
                                        None => {}
                                    }
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
    fn test_player_bar_zones() {
        let (left, center, right) = player_bar_zones(1000.0);
        assert!((left - 300.0).abs() < f32::EPSILON);
        assert!((center - 400.0).abs() < f32::EPSILON);
        assert!((right - 300.0).abs() < f32::EPSILON);
        assert!((left + center + right - 1000.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_row_icon() {
        assert_eq!(row_icon(false, false, false), None);
        assert_eq!(row_icon(false, false, true), None);
        assert_eq!(row_icon(true, false, false), Some(Icon::Play));
        assert_eq!(row_icon(true, false, true), Some(Icon::Play));
        assert_eq!(row_icon(false, true, true), Some(Icon::Pause));
        assert_eq!(row_icon(true, true, true), Some(Icon::Pause));
        assert_eq!(row_icon(false, true, false), Some(Icon::Play));
        assert_eq!(row_icon(true, true, false), Some(Icon::Play));
    }

    #[test]
    fn test_progress_at() {
        let t0 = Instant::now();
        let p_running = Progress {
            position_ms: 1000,
            since: t0,
            running: true,
        };
        assert_eq!(p_running.at(t0 + Duration::from_millis(2500)), 3500);

        let p_paused = Progress {
            position_ms: 1000,
            since: t0,
            running: false,
        };
        assert_eq!(p_paused.at(t0 + Duration::from_millis(2500)), 1000);

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

        app.apply_event(Event::Position { position_ms: 2000 }, t0);
        assert_eq!(app.progress, None);

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

        assert_eq!(app.status, Status::LoggingIn);
        app.apply_event(Event::Error("login failed".into()), t0);
        assert_eq!(app.status, Status::LoggingIn);
        assert_eq!(app.last_error.as_deref(), Some("login failed"));

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
            image_url: None,
            snapshot_id: None,
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
                image_url: None,
                snapshot_id: None,
            },
            PlaylistSummary {
                id: "2".into(),
                name: "Jazz".into(),
                track_count: 20,
                image_url: None,
                snapshot_id: None,
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
            album: String::new(),
            duration_ms: 61_000,
            image_url: None,
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
                album: String::new(),
                duration_ms: 120_000,
                image_url: None,
            },
            TrackSummary {
                uri: "spotify:track:2".into(),
                name: "Track Two".into(),
                artists: "Artist B".into(),
                album: String::new(),
                duration_ms: 180_000,
                image_url: None,
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
            album: String::new(),
            duration_ms: 60_000,
            image_url: None,
        }];
        app.tracks = original_tracks.clone();
        app.tracks_for = Some("pl1".into());

        let new_tracks = vec![TrackSummary {
            uri: "spotify:track:other".into(),
            name: "Other Track".into(),
            artists: "Other Artist".into(),
            album: String::new(),
            duration_ms: 90_000,
            image_url: None,
        }];

        app.apply_event(
            Event::PlaylistTracksLoaded {
                playlist_id: "pl2".into(),
                tracks: new_tracks,
            },
            t0,
        );

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
            album: String::new(),
            duration_ms: 354_000,
            image_url: None,
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
            album: String::new(),
            duration_ms: 200_000,
            image_url: None,
        }];
        app.search_results = original_results.clone();

        let old_results = vec![TrackSummary {
            uri: "spotify:track:old".into(),
            name: "Old Result".into(),
            artists: "Old Artist".into(),
            album: String::new(),
            duration_ms: 180_000,
            image_url: None,
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
            album: String::new(),
            duration_ms: 180_000,
            image_url: None,
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
            album: String::new(),
            duration_ms: 1000,
            image_url: None,
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
    fn test_previous_restarts_after_three_seconds_and_otherwise_moves_back() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        let now = Instant::now();

        let tracks = vec![
            TrackSummary {
                uri: "first".into(),
                name: "1".into(),
                artists: "".into(),
                album: "".into(),
                duration_ms: 5000,
                image_url: None,
            },
            TrackSummary {
                uri: "second".into(),
                name: "2".into(),
                artists: "".into(),
                album: "".into(),
                duration_ms: 5000,
                image_url: None,
            },
        ];
        app.play_tracks(&tracks, 1, QueueSource::Link);
        let _ = cmd_rx.try_recv();

        app.apply_event(
            Event::Playing {
                track_id: "second".into(),
                position_ms: 2800,
            },
            now,
        );
        app.previous(now + Duration::from_millis(201));
        assert_eq!(cmd_rx.try_recv(), Ok(Command::Play("second".into())));
        assert_eq!(app.queue.as_ref().unwrap().pos(), Some(1));
        app.previous(now + Duration::from_millis(200));
        assert_eq!(cmd_rx.try_recv(), Ok(Command::Play("first".into())));
        assert_eq!(app.queue.as_ref().unwrap().pos(), Some(0));
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
        assert!(app.queue.is_none());
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
            album: String::new(),
            duration_ms: 1000,
            image_url: None,
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
            album: String::new(),
            duration_ms: 1000,
            image_url: None,
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
            album: String::new(),
            duration_ms: 1000,
            image_url: None,
        }];
        let tracks = app.tracks.clone();
        app.play_tracks(&tracks, 0, QueueSource::Search);
        assert_eq!(
            app.queue
                .as_ref()
                .unwrap()
                .current()
                .map(|t| t.uri.as_str()),
            Some("fixture")
        );
        assert_eq!(app.queue.as_ref().unwrap().pos(), Some(0));
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
                    album: String::new(),
                    duration_ms: 1000,
                    image_url: None,
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
            shuffle: false,
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
                album: String::new(),
                duration_ms: 120_000,
                image_url: None,
            }],
            top_tracks: vec![TrackSummary {
                uri: "spotify:track:top".into(),
                name: "Top".into(),
                artists: "Artist".into(),
                album: String::new(),
                duration_ms: 200_000,
                image_url: None,
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
    fn test_stopped_never_sends_play_and_does_not_advance_queue() {
        let t0 = Instant::now();
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_evt_tx, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());

        let tracks = vec![
            TrackSummary {
                uri: "spotify:track:1".into(),
                name: "1".into(),
                artists: "".into(),
                album: "".into(),
                duration_ms: 1000,
                image_url: None,
            },
            TrackSummary {
                uri: "spotify:track:2".into(),
                name: "2".into(),
                artists: "".into(),
                album: "".into(),
                duration_ms: 1000,
                image_url: None,
            },
        ];
        app.play_tracks(&tracks, 0, QueueSource::Link);
        let _ = cmd_rx.try_recv();

        app.status = Status::Playing("spotify:track:1".into());

        app.apply_event(Event::Stopped, t0);

        assert_eq!(app.status, Status::Stopped);
        assert_eq!(app.queue.as_ref().unwrap().pos(), Some(0));
        assert_eq!(cmd_rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn test_starting_from_list_builds_queue_with_right_current() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        let tracks = vec![
            TrackSummary {
                uri: "spotify:track:1".into(),
                name: "Track 1".into(),
                artists: "Artist 1".into(),
                album: String::new(),
                duration_ms: 1000,
                image_url: None,
            },
            TrackSummary {
                uri: "spotify:track:2".into(),
                name: "Track 2".into(),
                artists: "Artist 2".into(),
                album: String::new(),
                duration_ms: 2000,
                image_url: None,
            },
        ];
        app.play_tracks(&tracks, 1, QueueSource::Search);
        assert!(app.queue.is_some());
        let q = app.queue.as_ref().unwrap();
        assert_eq!(q.pos(), Some(1));
        assert_eq!(q.current().map(|t| t.uri.as_str()), Some("spotify:track:2"));
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:2".into()))
        );
    }

    #[test]
    fn test_starting_with_shuffle_is_shuffled_and_current_still_clicked() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_, evt_rx) = unbounded_channel();
        let settings = Settings {
            shuffle: true,
            ..Default::default()
        };
        let mut app = PlayerApp::new(cmd_tx, evt_rx, settings);

        let tracks: Vec<TrackSummary> = (0..10)
            .map(|i| TrackSummary {
                uri: format!("spotify:track:{i}"),
                name: format!("Track {i}"),
                artists: "Artist".into(),
                album: String::new(),
                duration_ms: 1000,
                image_url: None,
            })
            .collect();

        app.play_tracks(&tracks, 3, QueueSource::Liked);
        let q = app.queue.as_ref().unwrap();
        assert!(q.is_shuffled());
        assert_eq!(q.pos(), Some(3));
        assert_eq!(q.current().map(|t| t.uri.as_str()), Some("spotify:track:3"));
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:3".into()))
        );
    }

    #[test]
    fn test_end_of_track_advances_and_sets_ended_at_end() {
        let t0 = Instant::now();
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());

        let tracks = vec![
            TrackSummary {
                uri: "spotify:track:1".into(),
                name: "Track 1".into(),
                artists: "Artist".into(),
                album: String::new(),
                duration_ms: 1000,
                image_url: None,
            },
            TrackSummary {
                uri: "spotify:track:2".into(),
                name: "Track 2".into(),
                artists: "Artist".into(),
                album: String::new(),
                duration_ms: 1000,
                image_url: None,
            },
        ];
        app.play_tracks(&tracks, 0, QueueSource::Playlist("Test".into()));
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:1".into()))
        );

        app.apply_event(Event::EndOfTrack, t0);
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:2".into()))
        );
        assert_eq!(app.queue.as_ref().unwrap().pos(), Some(1));

        app.apply_event(Event::EndOfTrack, t0);
        assert_eq!(app.status, Status::Ended);
        assert_eq!(cmd_rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn test_next_and_previous_through_queue() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        let now = Instant::now();

        let tracks = vec![
            TrackSummary {
                uri: "spotify:track:1".into(),
                name: "Track 1".into(),
                artists: "Artist".into(),
                album: String::new(),
                duration_ms: 1000,
                image_url: None,
            },
            TrackSummary {
                uri: "spotify:track:2".into(),
                name: "Track 2".into(),
                artists: "Artist".into(),
                album: String::new(),
                duration_ms: 1000,
                image_url: None,
            },
        ];
        app.play_tracks(&tracks, 0, QueueSource::Liked);
        let _ = cmd_rx.try_recv();

        app.next();
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:2".into()))
        );
        assert_eq!(app.queue.as_ref().unwrap().pos(), Some(1));

        app.previous(now);
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::Play("spotify:track:1".into()))
        );
        assert_eq!(app.queue.as_ref().unwrap().pos(), Some(0));
    }

    #[test]
    fn test_liked_page_loaded_appends_to_liked_queue() {
        let (cmd_tx, _cmd_rx) = unbounded_channel();
        let (_, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());

        let initial = vec![TrackSummary {
            uri: "spotify:track:1".into(),
            name: "Track 1".into(),
            artists: "Artist".into(),
            album: String::new(),
            duration_ms: 1000,
            image_url: None,
        }];
        app.liked = initial.clone();
        app.play_tracks(&initial, 0, QueueSource::Liked);

        let new_track = TrackSummary {
            uri: "spotify:track:2".into(),
            name: "Track 2".into(),
            artists: "Artist".into(),
            album: String::new(),
            duration_ms: 1000,
            image_url: None,
        };

        app.apply_event(
            Event::LikedPageLoaded(crate::webapi::LikedPage {
                offset: 1,
                tracks: vec![new_track],
                total: 2,
            }),
            Instant::now(),
        );

        let q = app.queue.as_ref().unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q.entries()[1].track.uri, "spotify:track:2");
    }

    #[test]
    fn test_shuffle_toggle_flips_setting_shuffles_queue_and_sends_save() {
        let (cmd_tx, mut cmd_rx) = unbounded_channel();
        let (_, evt_rx) = unbounded_channel();
        let mut app = PlayerApp::new(cmd_tx, evt_rx, Settings::default());
        assert!(!app.settings.shuffle);

        let tracks: Vec<TrackSummary> = (0..8)
            .map(|i| TrackSummary {
                uri: format!("spotify:track:{i}"),
                name: format!("Track {i}"),
                artists: "Artist".into(),
                album: String::new(),
                duration_ms: 1000,
                image_url: None,
            })
            .collect();
        app.play_tracks(&tracks, 0, QueueSource::Search);
        let _ = cmd_rx.try_recv();

        assert!(!app.queue.as_ref().unwrap().is_shuffled());

        // Toggle shuffle ON
        app.toggle_shuffle();
        assert!(app.settings.shuffle);
        assert!(app.queue.as_ref().unwrap().is_shuffled());
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::SaveSettings(app.settings.clone()))
        );

        // Toggle shuffle OFF
        app.toggle_shuffle();
        assert!(!app.settings.shuffle);
        assert!(!app.queue.as_ref().unwrap().is_shuffled());
        assert_eq!(
            cmd_rx.try_recv(),
            Ok(Command::SaveSettings(app.settings.clone()))
        );
    }
}
