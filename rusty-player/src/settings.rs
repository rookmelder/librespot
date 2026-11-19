//! Settings saved in `%APPDATA%\rusty-player\settings.toml`.

use std::{io::ErrorKind, path::PathBuf};

use anyhow::{Context, Result};
use librespot::playback::config::Bitrate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Audio bitrate: 96, 160 or 320 kbps.
    pub bitrate: u16,
    pub normalisation: bool,
    pub volume_percent: u8,
    pub shuffle: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bitrate: 320,
            normalisation: false,
            volume_percent: 100,
            shuffle: false,
        }
    }
}

pub fn path() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata)
        .join("rusty-player")
        .join("settings.toml"))
}

fn parse(text: &str) -> Settings {
    let mut settings: Settings = match toml::from_str(text) {
        Ok(settings) => settings,
        Err(error) => {
            log::warn!("could not parse settings ({error}), using defaults");
            return Settings::default();
        }
    };
    if !matches!(settings.bitrate, 96 | 160 | 320) {
        settings.bitrate = 320;
    }
    settings.volume_percent = settings.volume_percent.min(100);
    settings
}

pub fn load() -> Settings {
    let path = match path() {
        Ok(path) => path,
        Err(error) => {
            log::warn!("could not locate settings ({error:#}), using defaults");
            return Settings::default();
        }
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == ErrorKind::NotFound => Settings::default(),
        Err(error) => {
            log::warn!(
                "could not read {} ({error}), using defaults",
                path.display()
            );
            Settings::default()
        }
    }
}

pub fn save(settings: &Settings) -> Result<()> {
    let path = path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    let text = toml::to_string(settings).context("could not encode settings")?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("could not write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("could not save {}", path.display()))
}

pub fn bitrate(settings: &Settings) -> Bitrate {
    match settings.bitrate {
        96 => Bitrate::Bitrate96,
        160 => Bitrate::Bitrate160,
        _ => Bitrate::Bitrate320,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        assert_eq!(
            Settings::default(),
            Settings {
                bitrate: 320,
                normalisation: false,
                volume_percent: 100,
                shuffle: false,
            }
        );
        assert_eq!(parse(""), Settings::default());
    }

    #[test]
    fn toml_round_trip() {
        let settings = Settings {
            bitrate: 160,
            normalisation: true,
            volume_percent: 42,
            shuffle: true,
        };
        let text = toml::to_string(&settings).unwrap();
        assert_eq!(parse(&text), settings);
    }

    #[test]
    fn invalid_bitrate_uses_320() {
        assert_eq!(parse("bitrate = 123").bitrate, 320);
        for value in [96, 160, 320] {
            assert_eq!(parse(&format!("bitrate = {value}")).bitrate, value);
        }
    }

    #[test]
    fn old_toml_defaults_shuffle_to_false() {
        let settings = parse("bitrate = 160\nnormalisation = true\nvolume_percent = 42\n");
        assert_eq!(
            settings,
            Settings {
                bitrate: 160,
                normalisation: true,
                volume_percent: 42,
                shuffle: false,
            }
        );
    }

    #[test]
    fn volume_is_clamped_to_100() {
        assert_eq!(parse("volume_percent = 150").volume_percent, 100);
    }

    #[test]
    fn garbage_text_uses_defaults() {
        assert_eq!(parse("this is not TOML"), Settings::default());
    }
}
