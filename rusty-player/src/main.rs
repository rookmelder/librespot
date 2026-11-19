//! rusty-player: a lightweight Spotify desktop player for Windows, built on librespot.
//!
//! `rusty-player` opens the window; `rusty-player --cli` runs the terminal controller instead.
//! `--to-file <path>` (either mode) writes decoded audio to a file instead of the speakers: silent.

mod cli;
mod engine;
mod imagecache;
mod login;
mod queue;
mod settings;
mod track;
mod ui;
mod webapi;
mod webcache;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

const USAGE: &str = "usage: rusty-player [--cli] [--to-file <path>]
  --cli              terminal controller instead of the window
  --to-file <path>   write decoded audio to a file instead of the speakers (silent)";

/// Options from the command line.
#[derive(Debug, PartialEq, Default)]
struct Options {
    cli: bool,
    to_file: Option<PathBuf>,
}

/// Arguments without the program name -> [`Options`]. Each flag at most once.
fn parse_args(args: &[String]) -> Result<Options> {
    let mut options = Options::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--cli" if !options.cli => options.cli = true,
            "--to-file" if options.to_file.is_none() => match rest.next() {
                Some(path) => options.to_file = Some(PathBuf::from(path)),
                None => bail!("{USAGE}"),
            },
            _ => bail!("{USAGE}"),
        }
    }
    Ok(options)
}

fn main() -> Result<()> {
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
    let options = parse_args(&args)?;
    let settings = settings::load();

    if options.cli {
        let runtime =
            tokio::runtime::Runtime::new().context("could not start the Tokio runtime")?;
        let result = runtime.block_on(cli::run(options.to_file, settings));
        // Don't wait for a first-time Web API login still waiting on the browser.
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        result
    } else {
        ui::run(options.to_file, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn test_parse_args() {
        assert_eq!(parse_args(&[]).unwrap(), Options::default());
        assert_eq!(
            parse_args(&args(&["--cli"])).unwrap(),
            Options {
                cli: true,
                to_file: None
            }
        );
        let both = Options {
            cli: true,
            to_file: Some(PathBuf::from("a.pcm")),
        };
        assert_eq!(
            parse_args(&args(&["--cli", "--to-file", "a.pcm"])).unwrap(),
            both
        );
        assert_eq!(
            parse_args(&args(&["--to-file", "a.pcm", "--cli"])).unwrap(),
            both
        );
        for bad in [
            &["--to-file"][..],
            &["x"],
            &["--cli", "--cli"],
            &["--to-file", "a", "--to-file", "b"],
        ] {
            assert!(parse_args(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
