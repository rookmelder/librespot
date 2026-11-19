//! The engine: owns the Spotify session and the player, and runs on Tokio in the background.
//!
//! Whoever controls it (the terminal loop now, the egui window in Phase 3) only talks to it through two
//! channels: it sends [`Command`]s in and receives [`Event`]s back. The engine never waits on its controller
//! and the controller never waits on the network.

use std::{path::PathBuf, sync::Arc};

use crate::webapi::{PlaylistSummary, Stats, TrackSummary, WebApi};
use librespot::core::SpotifyUri;
use librespot::metadata::audio::{AudioItem, UniqueFields};
use librespot::playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{self, Mixer, MixerConfig},
    player::{Player, PlayerEvent},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, error::SendError};

/// The engine's end of the event channel. After every event it calls `notify`, so a window can redraw at
/// once instead of polling (egui: `ctx.request_repaint()`). The terminal passes a `notify` that does nothing.
#[derive(Clone)]
pub struct EventSender {
    tx: UnboundedSender<Event>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

impl EventSender {
    pub fn new(tx: UnboundedSender<Event>, notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            tx,
            notify: Arc::new(notify),
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
    /// Load the user's playlists through the Web API (logs in through the browser the first time).
    LoadPlaylists,
    /// Load one playlist's tracks through the Web API (by playlist ID).
    LoadPlaylistTracks(String),
    /// Load the user's Liked Songs through the Web API.
    LoadLikedTracks,
    /// Load recent listening and short-term favorites through the Web API.
    LoadStats,
    /// Search tracks through the Web API.
    Search(String),
    /// Stop playing, log out and end `run_engine`.
    Quit,
}

/// What the engine tells the controller.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    LoggedIn {
        username: String,
    },
    /// Playback (re)started at `position_ms` into the track.
    Playing {
        track_id: String,
        position_ms: u32,
    },
    Paused {
        position_ms: u32,
    },
    /// The position jumped (seek or correction) while playing or paused.
    Position {
        position_ms: u32,
    },
    /// Info about the track that was just loaded.
    TrackInfo(TrackInfo),
    Stopped,
    EndOfTrack,
    VolumeChanged(u16),
    /// Answer to `Command::LoadPlaylists`.
    PlaylistsLoaded(Vec<PlaylistSummary>),
    /// Answer to `Command::LoadPlaylistTracks`.
    PlaylistTracksLoaded {
        playlist_id: String,
        tracks: Vec<TrackSummary>,
    },
    /// Answer to `Command::LoadLikedTracks`.
    LikedTracksLoaded(Vec<TrackSummary>),
    /// Answer to `Command::LoadStats`.
    StatsLoaded(Stats),
    /// Answer to `Command::Search`.
    SearchResults {
        query: String,
        tracks: Vec<TrackSummary>,
    },
    Error(String),
}

/// What the window shows about the current track.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackInfo {
    pub title: String,
    /// Artist names joined with ", ". Empty for podcast episodes without a show name.
    pub artists: String,
    /// Album name (or the show name for a podcast episode).
    pub album: String,
    pub duration_ms: u32,
    /// The widest cover image librespot knows, if any.
    pub cover_url: Option<String>,
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

    let web_api: SharedWebApi = Arc::new(tokio::sync::Mutex::new(None));

    // Commands that arrived during login, in the order they were sent.
    let mut quit = false;
    for command in pending {
        if let Flow::Quit = handle_command(command, &player, mixer.as_ref(), &evt_tx, &web_api) {
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
                            if let Flow::Quit = handle_command(command, &player, mixer.as_ref(), &evt_tx, &web_api) {
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
    web_api: &SharedWebApi,
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
        // Web API calls run as their own task, so the player keeps reacting to commands meanwhile.
        Command::LoadPlaylists => {
            spawn_web_call(web_api, evt_tx, |api| {
                Box::pin(async move { api.playlists().await.map(Event::PlaylistsLoaded) })
            });
            Ok(())
        }
        Command::LoadPlaylistTracks(playlist_id) => {
            spawn_web_call(web_api, evt_tx, move |api| {
                Box::pin(async move {
                    let tracks = api.playlist_tracks(&playlist_id).await?;
                    Ok(Event::PlaylistTracksLoaded {
                        playlist_id,
                        tracks,
                    })
                })
            });
            Ok(())
        }
        Command::LoadLikedTracks => {
            spawn_web_call(web_api, evt_tx, |api| {
                Box::pin(async move { api.liked_tracks().await.map(Event::LikedTracksLoaded) })
            });
            Ok(())
        }
        Command::LoadStats => {
            spawn_web_call(web_api, evt_tx, |api| {
                Box::pin(async move { api.stats().await.map(Event::StatsLoaded) })
            });
            Ok(())
        }
        Command::Search(query) => {
            spawn_web_call(web_api, evt_tx, move |api| {
                Box::pin(async move {
                    let tracks = api.search_tracks(&query).await?;
                    Ok(Event::SearchResults { query, tracks })
                })
            });
            Ok(())
        }
        Command::Quit => return Flow::Quit,
    };
    match sent {
        Ok(()) => Flow::Continue,
        Err(_) => Flow::Quit,
    }
}

/// The Web API client, created on first use. A tokio `Mutex` because it is held across `.await`
/// (login, requests); one request at a time is fine for a single user.
type SharedWebApi = Arc<tokio::sync::Mutex<Option<WebApi>>>;

type WebCall = Box<
    dyn for<'a> FnOnce(
            &'a mut WebApi,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Event>> + Send + 'a>,
        > + Send,
>;

/// Run one Web API call as its own task (logging in first if needed) and send its result as an event.
fn spawn_web_call(
    web_api: &SharedWebApi,
    evt_tx: &EventSender,
    call: impl for<'a> FnOnce(
        &'a mut WebApi,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Event>> + Send + 'a>,
    > + Send
    + 'static,
) {
    let web_api = Arc::clone(web_api);
    let evt_tx = evt_tx.clone();
    let call: WebCall = Box::new(call);
    tokio::spawn(async move {
        let event = match with_web_api(&web_api, call).await {
            Ok(event) => event,
            Err(error) => Event::Error(format!("{error:#}")),
        };
        let _ = evt_tx.send(event);
    });
}

async fn with_web_api(web_api: &SharedWebApi, call: WebCall) -> anyhow::Result<Event> {
    let mut guard = web_api.lock().await;
    if guard.is_none() {
        *guard = Some(WebApi::login().await?);
    }
    match guard.as_mut() {
        Some(api) => call(api).await,
        None => anyhow::bail!("Web API login did not finish"),
    }
}

fn map_player_event(ev: &PlayerEvent) -> Option<Event> {
    match ev {
        PlayerEvent::Playing {
            track_id,
            position_ms,
            ..
        } => Some(Event::Playing {
            track_id: player_track_uri(track_id),
            position_ms: *position_ms,
        }),
        PlayerEvent::Paused { position_ms, .. } => Some(Event::Paused {
            position_ms: *position_ms,
        }),
        PlayerEvent::Seeked { position_ms, .. }
        | PlayerEvent::PositionCorrection { position_ms, .. } => Some(Event::Position {
            position_ms: *position_ms,
        }),
        PlayerEvent::TrackChanged { audio_item } => Some(Event::TrackInfo(track_info(audio_item))),
        PlayerEvent::Stopped { .. } => Some(Event::Stopped),
        PlayerEvent::EndOfTrack { .. } => Some(Event::EndOfTrack),
        PlayerEvent::Unavailable { track_id, .. } => Some(Event::Error(format!(
            "track unavailable: {}",
            player_track_uri(track_id)
        ))),
        _ => None,
    }
}

fn track_info(item: &AudioItem) -> TrackInfo {
    let (artists, album) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => (
            artists
                .iter()
                .map(|artist| artist.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            album.clone(),
        ),
        UniqueFields::Local { artists, album, .. } => (
            artists.clone().unwrap_or_default(),
            album.clone().unwrap_or_default(),
        ),
        UniqueFields::Episode { show_name, .. } => (String::new(), show_name.clone()),
    };
    TrackInfo {
        title: item.name.clone(),
        artists,
        album,
        duration_ms: item.duration_ms,
        cover_url: item
            .covers
            .iter()
            .max_by_key(|cover| cover.width)
            .map(|cover| cover.url.clone()),
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
                position_ms: 123,
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
            Some(Event::Paused { position_ms: 123 })
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
    fn map_player_event_seeked_gives_position() {
        assert_eq!(
            map_player_event(&PlayerEvent::Seeked {
                play_request_id: 1,
                track_id: track_uri(),
                position_ms: 4567,
            }),
            Some(Event::Position { position_ms: 4567 })
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
