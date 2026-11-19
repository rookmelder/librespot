# Devlog

What happened on rusty-player, newest first. Short entries: what was done, what went wrong, what was learned.

## 2026-11-19: Phase 5c, search

- Search bar at the top; results (up to 20: Spotify allows 10 per request, so 2 pages) show in the middle and
  play on double-click. Older searches' answers are ignored. Terminal: `? <words>`.
- Who built what: Lead the Web API search (query escaped by `Url::parse_with_params`) and engine command; builder
  (Gemini 3.8 Flash Low) the window, first try, with one shared track list for playlists and search.
- No separate review: same pattern as 5b. Not tested with a real search yet.

## 2026-11-19: Phase 5b, playlist tracks, double-click to play

- Fix after the user's test: Spotify deprecated `/playlists/{id}/tracks` (403) for `/playlists/{id}/items`, the
  track is now under `item` and the count under `items.total` (all playlists showed "(0)"). Spotify only returns
  songs of playlists the user owns or collaborates on; others now give a clear message instead of "403 Forbidden".
- Seen: emoji in playlist names show as boxes (egui's default font has no emoji). For the styling pass.

- Click a playlist: its tracks load (Web API `/playlists/{id}/tracks`, 100 per page); double-click a track to play.
  Controls and now-playing moved to a bottom bar. Terminal: `t <playlist id>`.
- Episodes, local files and removed tracks are skipped. Answers for a playlist you already left are ignored.
- Who built what: Lead the Web API call and engine (one shared `spawn_web_call` helper for all Web API commands);
  builder (Gemini 3.8 Flash Low) the window (1 fix round). rust-reviewer (GPT-6 Astra): 2 fixes taken (retry after a
  failed reload, only visible rows are drawn for long playlists), 2 minor ones left.
- Not tested with real playlists yet (the Web API login lives in the user's AppData, not in the Claude sandbox).
- Still single-track: playback stops after the track. A queue (next/previous) is Phase 6.

## 2026-11-19: Phase 5a, Web API login + playlists

- Own Spotify app (client ID in `webapi.rs`, redirect `http://127.0.0.1:8898/login`). Login with PKCE through
  librespot's oauth crate, no extra library; token saved in `%APPDATA%\rusty-player\webapi_token.json` and refreshed
  automatically. HTTP with reqwest; 429 rate limits wait once (max 30 s).
- New command `LoadPlaylists` -> event `PlaylistsLoaded`. Web API calls run as their own Tokio task, so playback
  keeps reacting. Window: left panel with "Load playlists" and a selectable list. Terminal: `l`.
- Who built what: Lead `webapi.rs` and engine changes (login code stays with the Lead); builder (Gemini 3.8 Flash
  Low) the panel, first try. rust-reviewer (GPT-6 Astra) found 3 real issues, fixed: closing during the first
  browser login could hang (runtime shutdown now waits max 1 s), a crash mid-save could corrupt the token file
  (temp file + rename), a bad "next page" link could send the token to another host (only api.spotify.com).
- Not tested yet: the browser login and real playlists need the user.

## 2026-11-19: Phase 4, now playing (title, artists, album, cover, progress)

- Engine: librespot's `TrackChanged` event already carries name, artists, album, length and cover URLs, so no
  separate metadata lookup was needed. New events `TrackInfo`, `Position`; `Playing`/`Paused` carry the position.
- Window: cover (120x120, downloaded by egui_extras' http + image loaders), title, artists, album, progress bar
  with "m:ss / m:ss". The bar is a clock: last known position + time since; it repaints 4x/s only while playing.
- Who built what: Lead the engine events and image loader setup; builder (Gemini 3.8 Flash Low) the window,
  first try. rust-reviewer (GPT-6 Astra) found 2 real issues, fixed by the Lead: the clock could wrap after
  ~49 days, and the bar kept running if the engine died.
- Tested: terminal mode prints "Rick Astley - Never Gonna Give You Up ..." for a track (silent). The window could
  not be screenshotted this time (Windows denied input/screen access), so the look is not checked yet.
- Lesson: the Claude app's %APPDATA% is a private sandbox copy: a login saved there is invisible to the user's
  own terminal. The user logged in once from their own terminal.

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
