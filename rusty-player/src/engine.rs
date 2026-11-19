//! The engine: owns the Spotify session and the player, and runs on Tokio in the background.
//!
//! Whoever controls it (the terminal loop now, the egui window in Phase 3) only talks to it through two
//! channels: it sends [`Command`]s in and receives [`Event`]s back. The engine never waits on its controller
//! and the controller never waits on the network.

use std::{path::PathBuf, sync::Arc};

use librespot::core::SpotifyUri;
use librespot::playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{self, Mixer, MixerConfig},
    player::{Player, PlayerEvent},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::SendError};

/// The engine's end of the event channel. After every event it calls `notify`, so a window can redraw at
/// once instead of polling (egui: `ctx.request_repaint()`). The terminal passes a `notify` that does nothing.
pub struct EventSender {
    tx: UnboundedSender<Event>,
    notify: Box<dyn Fn() + Send + Sync>,
}

impl EventSender {
    pub fn new(tx: UnboundedSender<Event>, notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            tx,
            notify: Box::new(notify),
        }
    }

    /// Fails only when the controller dropped its receiver: then the engine should stop.
    pub fn send(&self, event: Event) -> Result<(), SendError<Event>> {
        self.tx.send(event)?;
        (self.notify)();
        Ok(())
    }
}

/// What to do after handling one command.
enum Flow {
    Continue,
    Quit,
}

/// What the controller asks the engine to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Load and start a track: an ID, `spotify:track:` URI or share link, as typed.
    Play(String),
    Pause,
    Resume,
    Stop,
    /// Volume on librespot's scale: 0 (silent) ..= 65535 (full).
    SetVolume(u16),
    /// Stop playing, log out and end `run_engine`.
    Quit,
}

/// What the engine tells the controller.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    LoggedIn { username: String },
    Playing { track_id: String },
    Paused,
    Stopped,
    EndOfTrack,
    VolumeChanged(u16),
    Error(String),
}

/// How the engine plays audio.
#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    /// `None`: the default audio output (speakers). `Some(path)`: librespot's "pipe" backend writes raw PCM
    /// to this file instead, so nothing is heard.
    pub to_file: Option<PathBuf>,
}

/// Run the engine until it gets [`Command::Quit`], the command channel closes, or login fails.
pub async fn run_engine(
    config: EngineConfig,
    cmd_rx: UnboundedReceiver<Command>,
    evt_tx: EventSender,
) {
    let mut cmd_rx = cmd_rx;

    // Log in, but keep listening for commands: Quit (or a closed channel, e.g. the window was closed) must
    // end the engine right away, even while the login is still waiting on the network. Other commands that
    // arrive during login are kept and run once the player exists.
    let mut pending = Vec::new();
    let login = crate::login::connect();
    tokio::pin!(login);
    let session = loop {
        tokio::select! {
            result = &mut login => match result {
                Ok(session) => break session,
                Err(error) => {
                    if evt_tx.send(Event::Error(format!("{error:#}"))).is_err() {
                        log::debug!("engine event receiver dropped");
                    }
                    return;
                }
            },
            command = cmd_rx.recv() => match command {
                Some(Command::Quit) | None => return,
                Some(command) => pending.push(command),
            },
        }
    };
    if evt_tx
        .send(Event::LoggedIn {
            username: session.username(),
        })
        .is_err()
    {
        session.shutdown();
        return;
    }

    let Some(mixer_factory) = mixer::find(None) else {
        if evt_tx
            .send(Event::Error("no software mixer available".into()))
            .is_err()
        {
            log::debug!("engine event receiver dropped");
        }
        session.shutdown();
        return;
    };
    let mixer: Arc<dyn Mixer> = match mixer_factory(MixerConfig::default()) {
        Ok(mixer) => mixer,
        Err(error) => {
            if evt_tx
                .send(Event::Error(format!(
                    "could not create audio mixer: {error:#}"
                )))
                .is_err()
            {
                log::debug!("engine event receiver dropped");
            }
            session.shutdown();
            return;
        }
    };
    let (backend_name, device) = match config.to_file {
        Some(path) => (Some("pipe".to_string()), Some(path.display().to_string())),
        None => (None, None),
    };
    let Some(backend) = audio_backend::find(backend_name) else {
        if evt_tx
            .send(Event::Error(
                "requested audio backend is unavailable".into(),
            ))
            .is_err()
        {
            log::debug!("engine event receiver dropped");
        }
        session.shutdown();
        return;
    };
    let player = Player::new(
        PlayerConfig::default(),
        session.clone(),
        mixer.get_soft_volume(),
        move || backend(device, AudioFormat::default()),
    );
    let mut player_events = player.get_player_event_channel();

    // Commands that arrived during login, in the order they were sent.
    let mut quit = false;
    for command in pending {
        if let Flow::Quit = handle_command(command, &player, mixer.as_ref(), &evt_tx) {
            quit = true;
            break;
        }
    }

    if !quit {
        loop {
            tokio::select! {
                command = cmd_rx.recv() => {
                    match command {
                        Some(command) => {
                            if let Flow::Quit = handle_command(command, &player, mixer.as_ref(), &evt_tx) {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                event = player_events.recv() => {
                    match event {
                        Some(event) => {
                            if let Some(event) = map_player_event(&event) {
                                if evt_tx.send(event).is_err() {
                                    break;
                                }
                            }
                        }
                        None => {
                            if evt_tx
                                .send(Event::Error("player stopped unexpectedly".into()))
                                .is_err()
                            {
                                log::debug!("engine event receiver dropped");
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    player.stop();
    session.shutdown();
}

/// Run one command on the player. `Flow::Quit` on `Command::Quit` or when the controller is gone.
fn handle_command(
    command: Command,
    player: &Player,
    mixer: &dyn Mixer,
    evt_tx: &EventSender,
) -> Flow {
    let sent = match command {
        Command::Play(text) => match crate::track::parse_track(&text) {
            Ok(id) => {
                player.load(SpotifyUri::Track { id }, true, 0);
                Ok(())
            }
            Err(error) => evt_tx.send(Event::Error(format!("{error:#}"))),
        },
        Command::Pause => {
            player.pause();
            Ok(())
        }
        Command::Resume => {
            player.play();
            Ok(())
        }
        Command::Stop => {
            player.stop();
            Ok(())
        }
        Command::SetVolume(volume) => {
            mixer.set_volume(volume);
            evt_tx.send(Event::VolumeChanged(volume))
        }
        Command::Quit => return Flow::Quit,
    };
    match sent {
        Ok(()) => Flow::Continue,
        Err(_) => Flow::Quit,
    }
}

fn map_player_event(ev: &PlayerEvent) -> Option<Event> {
    match ev {
        PlayerEvent::Playing { track_id, .. } => Some(Event::Playing {
            track_id: player_track_uri(track_id),
        }),
        PlayerEvent::Paused { .. } => Some(Event::Paused),
        PlayerEvent::Stopped { .. } => Some(Event::Stopped),
        PlayerEvent::EndOfTrack { .. } => Some(Event::EndOfTrack),
        PlayerEvent::Unavailable { track_id, .. } => Some(Event::Error(format!(
            "track unavailable: {}",
            player_track_uri(track_id)
        ))),
        _ => None,
    }
}

fn player_track_uri(track_id: &SpotifyUri) -> String {
    track_id.to_uri()
}

#[cfg(test)]
mod tests {
    use super::*;
    use librespot::core::spotify_id::SpotifyId;

    fn track_uri() -> SpotifyUri {
        SpotifyUri::Track {
            id: SpotifyId::from_base62("4uLU6hMCjMI75M1A2tKUQC").unwrap(),
        }
    }

    #[test]
    fn map_player_event_playing() {
        let event = PlayerEvent::Playing {
            play_request_id: 1,
            track_id: track_uri(),
            position_ms: 123,
        };
        assert_eq!(
            map_player_event(&event),
            Some(Event::Playing {
                track_id: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into(),
            })
        );
    }

    #[test]
    fn map_player_event_paused() {
        assert_eq!(
            map_player_event(&PlayerEvent::Paused {
                play_request_id: 1,
                track_id: track_uri(),
                position_ms: 123,
            }),
            Some(Event::Paused)
        );
    }

    #[test]
    fn map_player_event_stopped() {
        assert_eq!(
            map_player_event(&PlayerEvent::Stopped {
                play_request_id: 1,
                track_id: track_uri(),
            }),
            Some(Event::Stopped)
        );
    }

    #[test]
    fn map_player_event_end_of_track() {
        assert_eq!(
            map_player_event(&PlayerEvent::EndOfTrack {
                play_request_id: 1,
                track_id: track_uri(),
            }),
            Some(Event::EndOfTrack)
        );
    }

    #[test]
    fn map_player_event_unavailable() {
        let event = map_player_event(&PlayerEvent::Unavailable {
            play_request_id: 1,
            track_id: track_uri(),
        });
        assert_eq!(
            event,
            Some(Event::Error(
                "track unavailable: spotify:track:4uLU6hMCjMI75M1A2tKUQC".into()
            ))
        );
    }

    #[test]
    fn map_player_event_loading_is_ignored() {
        assert_eq!(
            map_player_event(&PlayerEvent::Loading {
                play_request_id: 1,
                track_id: track_uri(),
                position_ms: 0,
            }),
            None
        );
    }

    #[test]
    fn map_player_event_volume_changed_is_ignored() {
        assert_eq!(
            map_player_event(&PlayerEvent::VolumeChanged { volume: 1234 }),
            None
        );
    }
}
