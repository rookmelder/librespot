# Devlog

What happened on rusty-player, newest first. Short entries: what was done, what went wrong, what was learned.

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
