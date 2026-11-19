//! rusty-player: a lightweight Spotify desktop player for Windows, built on librespot.

mod engine;
mod login;
mod track;

use anyhow::{Context, Result, bail};
use engine::{Command, EngineConfig, Event};
use std::path::PathBuf;

const USAGE: &str = "usage: rusty-player [--to-file <path>]\n  --to-file <path>   write decoded audio to a file instead of the speakers (silent)";

pub fn parse_args(args: &[String]) -> Result<Option<PathBuf>> {
    match args {
        [] => Ok(None),
        [flag, path] if flag == "--to-file" => Ok(Some(PathBuf::from(path))),
        _ => bail!("{USAGE}"),
    }
}

pub fn percent_to_volume(percent: u8) -> u16 {
    (percent as u32 * 65535 / 100) as u16
}

pub fn volume_to_percent(volume: u16) -> u16 {
    ((volume as u32 * 100 + 32767) / 65535) as u16
}

pub fn parse_line(line: &str) -> Result<Option<Command>, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if trimmed.eq_ignore_ascii_case("p") {
        return Ok(Some(Command::Pause));
    }
    if trimmed.eq_ignore_ascii_case("r") {
        return Ok(Some(Command::Resume));
    }
    if trimmed.eq_ignore_ascii_case("s") {
        return Ok(Some(Command::Stop));
    }
    if trimmed.eq_ignore_ascii_case("q") {
        return Ok(Some(Command::Quit));
    }

    // A volume command is "v" alone or "v" + space + number. A track ID can start with "v" too
    // ("vXyz..."), so a "v" glued to more text is a track, not a volume.
    let (first, rest) = trimmed
        .split_once(char::is_whitespace)
        .unwrap_or((trimmed, ""));
    if first.eq_ignore_ascii_case("v") {
        let rest = rest.trim();
        match rest.parse::<u8>() {
            Ok(n) if n <= 100 => Ok(Some(Command::SetVolume(percent_to_volume(n)))),
            _ => Err("volume must be a number from 0 to 100".to_string()),
        }
    } else {
        Ok(Some(Command::Play(trimmed.to_string())))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("rusty_player=info,librespot=warn"),
    )
    .init();

    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(s) => args.push(s),
            Err(_) => bail!("{USAGE}"),
        }
    }

    let to_file = parse_args(&args)?;

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
    let (evt_tx, mut evt_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

    let engine_handle = tokio::spawn(engine::run_engine(EngineConfig { to_file }, cmd_rx, evt_tx));

    println!(
        "Commands: <track id or link> = play, p = pause, r = resume, s = stop, v <0-100> = volume, q = quit"
    );

    std::thread::spawn(move || {
        for line_result in std::io::stdin().lines() {
            match line_result {
                Ok(line) => match parse_line(&line) {
                    Ok(Some(cmd)) => {
                        let is_quit = cmd == Command::Quit;
                        if cmd_tx.send(cmd).is_err() {
                            return;
                        }
                        if is_quit {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(message) => eprintln!("{message}"),
                },
                Err(error) => {
                    eprintln!("Error: could not read input ({error}), quitting");
                    let _ = cmd_tx.send(Command::Quit);
                    return;
                }
            }
        }
        let _ = cmd_tx.send(Command::Quit);
    });

    while let Some(event) = evt_rx.recv().await {
        match event {
            Event::LoggedIn { username } => println!("Logged in as {username}"),
            Event::Playing { track_id } => println!("Playing {track_id}"),
            Event::Paused => println!("Paused"),
            Event::Stopped => println!("Stopped"),
            Event::EndOfTrack => println!("Track ended"),
            Event::VolumeChanged(v) => println!("Volume {}%", volume_to_percent(v)),
            Event::Error(msg) => eprintln!("Error: {msg}"),
        }
    }

    engine_handle.await.context("engine task failed")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_line() {
        assert_eq!(parse_line(""), Ok(None));
        assert_eq!(parse_line("  "), Ok(None));
        assert_eq!(parse_line("p"), Ok(Some(Command::Pause)));
        assert_eq!(parse_line("P"), Ok(Some(Command::Pause)));
        assert_eq!(parse_line("r"), Ok(Some(Command::Resume)));
        assert_eq!(parse_line("s"), Ok(Some(Command::Stop)));
        assert_eq!(parse_line("q"), Ok(Some(Command::Quit)));

        assert_eq!(parse_line("v 50"), Ok(Some(Command::SetVolume(32767))));
        assert_eq!(parse_line("v 0"), Ok(Some(Command::SetVolume(0))));
        assert_eq!(parse_line("v 100"), Ok(Some(Command::SetVolume(65535))));

        assert_eq!(
            parse_line("v"),
            Err("volume must be a number from 0 to 100".to_string())
        );
        assert_eq!(
            parse_line("v abc"),
            Err("volume must be a number from 0 to 100".to_string())
        );
        assert_eq!(
            parse_line("v 101"),
            Err("volume must be a number from 0 to 100".to_string())
        );
        assert_eq!(
            parse_line("v -1"),
            Err("volume must be a number from 0 to 100".to_string())
        );

        assert_eq!(
            parse_line("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"),
            Ok(Some(Command::Play(
                "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT".to_string()
            )))
        );
        // A track ID that happens to start with "v" is a track, not a volume command.
        assert_eq!(
            parse_line("vXyz123abcDEF456ghiJKL"),
            Ok(Some(Command::Play("vXyz123abcDEF456ghiJKL".to_string())))
        );
        assert_eq!(parse_line("V 30"), Ok(Some(Command::SetVolume(19660))));
        assert_eq!(
            parse_line(" spotify:track:x "),
            Ok(Some(Command::Play("spotify:track:x".to_string())))
        );
    }

    #[test]
    fn test_volume_conversions() {
        assert_eq!(percent_to_volume(0), 0);
        assert_eq!(percent_to_volume(50), 32767);
        assert_eq!(percent_to_volume(100), 65535);

        assert_eq!(volume_to_percent(0), 0);
        assert_eq!(volume_to_percent(32767), 50);
        assert_eq!(volume_to_percent(65535), 100);
    }

    #[test]
    fn test_parse_args() {
        assert_eq!(parse_args(&[]).unwrap(), None);
        assert_eq!(
            parse_args(&["--to-file".to_string(), "a.pcm".to_string()]).unwrap(),
            Some(PathBuf::from("a.pcm"))
        );
        assert!(parse_args(&["--to-file".to_string()]).is_err());
        assert!(parse_args(&["x".to_string()]).is_err());
    }
}
