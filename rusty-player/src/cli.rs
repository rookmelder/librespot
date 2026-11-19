//! Terminal mode (`--cli`): type commands, see events. Same engine and channels as the window.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::sync::mpsc::unbounded_channel;

use crate::engine::{self, Command, EngineConfig, Event, EventSender};

/// Volume on librespot's scale (0..=65535) from a percentage. Requires `percent <= 100`.
pub fn percent_to_volume(percent: u8) -> u16 {
    (percent as u32 * 65535 / 100) as u16
}

/// Percentage (rounded) from librespot's volume scale.
pub fn volume_to_percent(volume: u16) -> u16 {
    ((volume as u32 * 100 + 32767) / 65535) as u16
}

/// One typed line -> a command. `Ok(None)` for an empty line, `Err` with a message for a bad volume.
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
        match rest.trim().parse::<u8>() {
            Ok(n) if n <= 100 => Ok(Some(Command::SetVolume(percent_to_volume(n)))),
            _ => Err("volume must be a number from 0 to 100".to_string()),
        }
    } else {
        Ok(Some(Command::Play(trimmed.to_string())))
    }
}

/// Run the terminal controller until the engine stops.
pub async fn run(to_file: Option<PathBuf>) -> Result<()> {
    let (cmd_tx, cmd_rx) = unbounded_channel::<Command>();
    let (evt_tx, mut evt_rx) = unbounded_channel::<Event>();

    let engine_handle = tokio::spawn(engine::run_engine(
        EngineConfig { to_file },
        cmd_rx,
        EventSender::new(evt_tx, || {}),
    ));

    println!(
        "Commands: <track id or link> = play, p = pause, r = resume, s = stop, v <0-100> = volume, q = quit"
    );

    // Reading stdin blocks, so it gets its own OS thread instead of a Tokio task.
    std::thread::spawn(move || {
        for line_result in std::io::stdin().lines() {
            match line_result {
                Ok(line) => match parse_line(&line) {
                    Ok(Some(cmd)) => {
                        let is_quit = cmd == Command::Quit;
                        if cmd_tx.send(cmd).is_err() || is_quit {
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

    const VOLUME_ERROR: &str = "volume must be a number from 0 to 100";

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
        assert_eq!(parse_line("V 30"), Ok(Some(Command::SetVolume(19660))));

        for bad in ["v", "v abc", "v 101", "v -1"] {
            assert_eq!(parse_line(bad), Err(VOLUME_ERROR.to_string()), "{bad}");
        }

        let link = "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT";
        assert_eq!(parse_line(link), Ok(Some(Command::Play(link.to_string()))));
        // A track ID that happens to start with "v" is a track, not a volume command.
        assert_eq!(
            parse_line("vXyz123abcDEF456ghiJKL"),
            Ok(Some(Command::Play("vXyz123abcDEF456ghiJKL".to_string())))
        );
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
}
