# Devlog

What happened on rusty-player, newest first. Short entries: what was done, what went wrong, what was learned.

## 2026-11-19: Phase 3, the first window

- `rusty-player` now opens an egui window (eframe 0.32.3, the newest that fits Rust 1.85); `--cli` keeps the
  terminal mode. Window: track field + Play (or Enter), Pause / Resume / Stop, volume slider, status and errors.
- The window runs on the main thread, the engine on its own thread with its own Tokio runtime. The engine wakes
  the window after every event (`EventSender` calls `ctx.request_repaint()`), so it never polls.
- Fixed from Phase 2: closing (or `q`) during login no longer waits for the login; commands sent during login
  are kept and run after it, in order.
- Who built what: the Lead wrote the threading (`ui::run`), the engine changes and `cli.rs`; builder (Gemini 3.8
  Flash Low) wrote the window layout (1 fix round). rust-reviewer (GPT-6 Astra): no real problems.
- Tested: window opens, logs in, shows the controls (screenshot); closing it exits in 0.1 s with code 0; terminal
  mode still works and runs commands typed during login. Play in the window not clicked yet, no sound heard yet.
- Lesson: clippy rejects `while !flag { ... }` when the loop never changes `flag` (`while_immutable_condition`).

## 2026-11-19: Phase 2, the engine with commands and events

- Split the app into parts: `login.rs` (cached login), `track.rs` (reading a track ID or link), `engine.rs`
  (owns the Spotify session, player and volume mixer) and `main.rs` (the terminal controller).
- The engine runs as a Tokio task. The terminal sends it `Command`s (Play, Pause, Resume, Stop, SetVolume, Quit)
  over one channel and gets `Event`s back (LoggedIn, Playing, Paused, Stopped, EndOfTrack, VolumeChanged, Error)
  over another. Phase 3's window will use the same two channels.
- Typing in the terminal: paste a track = play, `p` pause, `r` resume, `s` stop, `v 0-100` volume, `q` quit.
  Nothing plays until you paste a track.
- Who built what: the Lead wrote `login.rs`, `track.rs` and the engine's types; builderplus (GPT-6.1 Sol on Codex)
  wrote the engine (1 fix round); builder (Gemini 3.8 Flash Low on agy) wrote the terminal loop.
- Lead fixes after review: a pasted track ID starting with "v" was read as a volume command; a stdin read error
  now says why it quits (found by rust-reviewer, GPT-6 Astra).
- Tested silently (`--to-file`): bad ID and `v 150` rejected with clear messages, volume 40%, play, pause,
  resume, stop, quit, all in order; quitting by closing the input also exits cleanly.
- For Phase 3: quitting while the login is still running waits until the login ends. Fine in a terminal
  (login takes under a second), but closing the window during login must not hang.

## 2026-11-19: Phase 1, log in and play one track (CLI)

- `rusty-player` with no arguments only logs in with the cached credentials and prints the account. It never plays.
- `rusty-player play <track>` plays one track on the speakers and exits when it ends (or on Ctrl+C).
  `<track>` can be an ID, a `spotify:track:` URI, or a share link (`open.spotify.com/track/...?si=...`).
- `rusty-player play <track> --to-file <file>` decodes the track into a raw audio file instead: silent.
  Used to test without sound: a 3:33 track gave 37,671,984 bytes (44.1 kHz, stereo, 16-bit = exactly 3:33) in 10 s.
- Not yet heard on the speakers (sound was off on purpose).
- Review (rust-reviewer, GPT-6 Astra): 4 small findings, all fixed: `std::env::args()` panics on non-Unicode
  arguments (now `args_os()`), a Ctrl+C listener error was ignored, the session was not shut down when playing
  failed, and a share link with `/track/` only in its `?si=` part was accepted.
- Lesson: `OsString::display()` needs Rust 1.87; the workspace promises 1.85, so clippy refused it.

## 2026-11-19: Phase 0, project set up

- Created `rusty-player/`, a new crate in this workspace. It uses the librespot code in this repo directly
  (a path dependency), so we can read and, if needed, change librespot next to the app.
- librespot features: only `native-tls` (Windows' own TLS, no OpenSSL) and `rodio-backend` (audio out).
  Discovery (mDNS) is off: we log in with OAuth, not by being picked from the Spotify app on the LAN.
- The login cache moved out of the repo to `%APPDATA%\rusty-player\cache`.
- `cargo run -p rusty-player` builds and prints the version.

## 2026-11-18: librespot builds, OAuth login works

- Forked librespot and built it on Windows.
- Logging in through discovery (picking the device in the Spotify app) failed.
- OAuth login worked and saved the credentials to the cache, so next runs don't need to log in again.
