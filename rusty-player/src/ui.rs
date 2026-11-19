//! The window (egui via eframe). Runs on the main thread (Windows requires it); the engine runs on a
//! background thread with its own Tokio runtime. They only talk through the Command/Event channels.

use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result, anyhow};
use eframe::egui;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::cli::{percent_to_volume, volume_to_percent};
use crate::engine::{self, Command, EngineConfig, Event, EventSender};

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
            .with_inner_size([480.0, 260.0]),
        ..Default::default()
    };
    let window_result = eframe::run_native(
        "rusty-player",
        options,
        Box::new(move |cc| {
            let _ = ctx_slot.set(cc.egui_ctx.clone());
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
        }
    }

    fn apply_event(&mut self, event: Event) {
        match event {
            Event::LoggedIn { username } => {
                self.username = Some(username);
                self.status = Status::Idle;
            }
            Event::Playing { track_id } => {
                self.status = Status::Playing(track_id);
                self.last_error = None;
            }
            Event::Paused => {
                self.status = Status::Paused;
            }
            Event::Stopped => {
                self.status = Status::Stopped;
            }
            Event::EndOfTrack => {
                self.status = Status::Ended;
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

    fn send(&mut self, command: Command) {
        if self.cmd_tx.send(command).is_err() {
            self.last_error = Some("the engine has stopped".into());
        }
    }
}

impl eframe::App for PlayerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.evt_rx.try_recv() {
            self.apply_event(event);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_app() -> (UnboundedSender<Event>, PlayerApp) {
        let (cmd_tx, _cmd_rx) = unbounded_channel();
        let (evt_tx, evt_rx) = unbounded_channel();
        (evt_tx, PlayerApp::new(cmd_tx, evt_rx))
    }

    #[test]
    fn test_apply_event_logged_in() {
        let (_evt_tx, mut app) = create_test_app();
        assert_eq!(app.status, Status::LoggingIn);
        assert_eq!(app.username, None);

        app.apply_event(Event::LoggedIn {
            username: "testuser".into(),
        });
        assert_eq!(app.username.as_deref(), Some("testuser"));
        assert_eq!(app.status, Status::Idle);
    }

    #[test]
    fn test_apply_event_playing_and_clears_last_error() {
        let (_evt_tx, mut app) = create_test_app();
        app.last_error = Some("previous error".into());

        app.apply_event(Event::Playing {
            track_id: "spotify:track:xyz".into(),
        });
        assert_eq!(app.status, Status::Playing("spotify:track:xyz".into()));
        assert_eq!(app.last_error, None);
    }

    #[test]
    fn test_apply_event_playback_states() {
        let (_evt_tx, mut app) = create_test_app();

        app.apply_event(Event::Paused);
        assert_eq!(app.status, Status::Paused);

        app.apply_event(Event::Stopped);
        assert_eq!(app.status, Status::Stopped);

        app.apply_event(Event::EndOfTrack);
        assert_eq!(app.status, Status::Ended);
    }

    #[test]
    fn test_apply_event_volume_changed() {
        let (_evt_tx, mut app) = create_test_app();

        app.apply_event(Event::VolumeChanged(32767));
        assert_eq!(app.volume_percent, 50);

        app.apply_event(Event::VolumeChanged(65535));
        assert_eq!(app.volume_percent, 100);

        app.apply_event(Event::VolumeChanged(0));
        assert_eq!(app.volume_percent, 0);
    }

    #[test]
    fn test_apply_event_error_handling() {
        let (_evt_tx, mut app) = create_test_app();

        // Error while LoggingIn keeps LoggingIn and sets last_error
        assert_eq!(app.status, Status::LoggingIn);
        app.apply_event(Event::Error("login failed".into()));
        assert_eq!(app.status, Status::LoggingIn);
        assert_eq!(app.last_error.as_deref(), Some("login failed"));

        // Error while Playing keeps Playing
        app.status = Status::Playing("spotify:track:123".into());
        app.apply_event(Event::Error("stream warning".into()));
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
