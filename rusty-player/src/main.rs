//! rusty-player: a lightweight Spotify desktop player for Windows, built on librespot.
//!
//! Phase 1: a command-line program.
//! - `rusty-player`                              log in with the cached credentials, show who you are, exit. No sound.
//! - `rusty-player play <track>`                 play one track on the speakers, exit when it ends (or on Ctrl+C).
//! - `rusty-player play <track> --to-file <f>`   decode the track into a raw PCM file instead: silent.
//!
//! `<track>` is a Spotify track ID, a `spotify:track:` URI, or an `open.spotify.com/track/` link.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use librespot::{
    core::{
        SpotifyUri, authentication::Credentials, cache::Cache, config::SessionConfig,
        session::Session, spotify_id::SpotifyId,
    },
    playback::{
        audio_backend,
        config::{AudioFormat, PlayerConfig},
        mixer::NoOpVolume,
        player::Player,
    },
};
use log::info;

/// What the user asked for on the command line.
#[derive(Debug, PartialEq)]
enum Action {
    /// Only log in and print the account: never plays anything.
    Login,
    /// Play one track, on the speakers (`to_file: None`) or into a file.
    Play {
        track: SpotifyId,
        to_file: Option<PathBuf>,
    },
}

const USAGE: &str = "usage:
  rusty-player                              log in and show the account (no sound)
  rusty-player play <track>                 play one track on the speakers
  rusty-player play <track> --to-file <f>   write the decoded audio to a file (no sound)";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("rusty_player=info,librespot=warn"),
    )
    .init();

    // `args_os` instead of `args`: `args` panics on an argument that is not valid Unicode.
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string().map_err(|bad| {
                anyhow::anyhow!("argument is not valid text: {}", bad.to_string_lossy())
            })
        })
        .collect::<Result<Vec<String>>>()?;
    let action = parse_args(&args)?;

    let session = connect().await?;
    info!("Logged in as {}", session.username());

    let result = match action {
        Action::Login => {
            println!("Logged in as {}. Nothing to play.", session.username());
            Ok(())
        }
        Action::Play { track, to_file } => play(&session, track, to_file).await,
    };

    // Close the connection even when playing failed, then report the error (if any).
    session.shutdown();
    result
}

/// Where librespot keeps our login, volume and downloaded audio: `%APPDATA%\rusty-player\cache`.
fn cache_dir() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join("rusty-player").join("cache"))
}

/// Log in with the credentials saved in the cache by an earlier OAuth login.
async fn connect() -> Result<Session> {
    let dir = cache_dir()?;
    let cache = Cache::new(Some(&dir), Some(&dir), Some(&dir.join("files")), None)
        .with_context(|| format!("could not open the cache in {}", dir.display()))?;

    let credentials: Credentials = cache.credentials().with_context(|| {
        format!(
            "no saved login in {}. Log in once with OAuth:\n  cargo run -- --enable-oauth --cache \"{}\"",
            dir.display(),
            dir.display()
        )
    })?;

    let session = Session::new(SessionConfig::default(), Some(cache));
    // `true`: save the refreshed credentials back to the cache, so the next run logs in too.
    session
        .connect(credentials, true)
        .await
        .context("could not log in to Spotify")?;
    Ok(session)
}

/// Play one track and return when it has ended, or when the user presses Ctrl+C.
async fn play(session: &Session, track: SpotifyId, to_file: Option<PathBuf>) -> Result<()> {
    // "pipe" writes raw PCM to the file given as its device; no device = the default (speakers).
    let (backend_name, device) = match &to_file {
        Some(path) => (Some("pipe".to_string()), Some(path.display().to_string())),
        None => (None, None),
    };
    let backend = audio_backend::find(backend_name).context("audio backend not found")?;

    let player = Player::new(
        PlayerConfig::default(),
        session.clone(),
        Box::new(NoOpVolume),
        move || backend(device, AudioFormat::default()),
    );

    player.load(SpotifyUri::Track { id: track }, true, 0);
    match &to_file {
        Some(path) => info!("Writing track to {}...", path.display()),
        None => info!("Playing... (Ctrl+C to stop)"),
    }

    tokio::select! {
        _ = player.await_end_of_track() => info!("Track ended."),
        signal = tokio::signal::ctrl_c() => {
            player.stop();
            signal.context("could not listen for Ctrl+C")?;
            info!("Stopped by Ctrl+C.");
        }
    }
    Ok(())
}

/// Turn the command-line arguments (without the program name) into an [`Action`].
fn parse_args(args: &[String]) -> Result<Action> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => Ok(Action::Login),
        ["play", track] => Ok(Action::Play {
            track: parse_track(track)?,
            to_file: None,
        }),
        ["play", track, "--to-file", path] => Ok(Action::Play {
            track: parse_track(track)?,
            to_file: Some(PathBuf::from(path)),
        }),
        _ => bail!("{USAGE}"),
    }
}

/// Accept a bare track ID, `spotify:track:<id>`, or `https://open.spotify.com/track/<id>?si=...`.
fn parse_track(input: &str) -> Result<SpotifyId> {
    let input = input.trim();
    // Drop the "?si=..." share tracking part first, so the "/track/" match only looks at the path.
    let path = input.split(['?', '#']).next().unwrap_or(input);
    let id = if let Some(rest) = input.strip_prefix("spotify:track:") {
        rest
    } else if let Some((_, rest)) = path.split_once("open.spotify.com/track/") {
        rest.trim_end_matches('/')
    } else {
        input
    };
    SpotifyId::from_base62(id).with_context(|| format!("not a Spotify track ID or link: {input}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "4uLU6hMCjMI75M1A2tKUQC";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parse_track_accepts_id_uri_and_link() {
        let expected = SpotifyId::from_base62(ID).unwrap();
        assert_eq!(parse_track(ID).unwrap(), expected);
        assert_eq!(
            parse_track(&format!("spotify:track:{ID}")).unwrap(),
            expected
        );
        assert_eq!(
            parse_track(&format!("https://open.spotify.com/track/{ID}?si=abc123")).unwrap(),
            expected
        );
        assert_eq!(
            parse_track(&format!("  https://open.spotify.com/track/{ID}/  ")).unwrap(),
            expected
        );
    }

    #[test]
    fn parse_track_rejects_garbage() {
        assert!(parse_track("").is_err());
        assert!(parse_track("not an id!").is_err());
        assert!(parse_track("https://open.spotify.com/track/").is_err());
        // "/track/" hidden in the query part is not a track link.
        assert!(
            parse_track(&format!(
                "https://open.spotify.com/?si=open.spotify.com/track/{ID}"
            ))
            .is_err()
        );
    }

    #[test]
    fn no_arguments_only_logs_in() {
        assert_eq!(parse_args(&[]).unwrap(), Action::Login);
    }

    #[test]
    fn play_needs_a_track_and_to_file_needs_a_path() {
        let track = SpotifyId::from_base62(ID).unwrap();
        assert_eq!(
            parse_args(&args(&["play", ID])).unwrap(),
            Action::Play {
                track,
                to_file: None
            }
        );
        assert_eq!(
            parse_args(&args(&["play", ID, "--to-file", "out.pcm"])).unwrap(),
            Action::Play {
                track,
                to_file: Some(PathBuf::from("out.pcm"))
            }
        );
        assert!(parse_args(&args(&["play"])).is_err());
        assert!(parse_args(&args(&["play", ID, "--to-file"])).is_err());
        assert!(parse_args(&args(&[ID])).is_err());
    }
}
