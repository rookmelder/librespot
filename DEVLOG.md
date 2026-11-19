# Devlog

What happened on rusty-player, newest first. Short entries: what was done, what went wrong, what was learned.

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
