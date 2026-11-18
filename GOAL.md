# Goal

Build a lightweight Spotify desktop player for Windows in Rust, from scratch on top of librespot.

- **What:** a small, fast native app (a single `.exe`) that logs in to Spotify, plays music, and later shows my
  playlists, search, and stats. It should use much less RAM than the official Spotify app.
- **Who:** me, for my own use and to learn Rust (async, channels, UI, HTTP/OAuth).
- **Most important:** the UI never freezes. It never waits on the network.
- **Look:** egui. Plain is fine at first; polish comes last.
- **Done (first version):** Phase 4: a "now playing" window with track info, album cover, progress bar and controls.
- **Private:** yes. Credentials live in `cache/` and are never committed.
- **Not:** a librespot fork with changes to the library itself. The player is a separate app that uses librespot.

---

## The plan

### Architecture

```
┌──────────────────────────┐        commands         ┌───────────────────────────┐
│   UI thread (egui)       │  ───────────────────▶   │  Background (Tokio async) │
│  buttons, lists, labels  │   Play, Pause, Next...  │  librespot Session+Player │
│                          │  ◀───────────────────   │  Spotify Web API calls    │
└──────────────────────────┘     events / state      └───────────────────────────┘
                                 Playing, Paused,
                                 TrackChanged, ...
```

- **UI:** egui via the `eframe` crate.
- **Engine:** librespot handles login, streaming and audio output, and runs async on Tokio.
- **Bridge:** two channels. The UI sends `Command`s to the engine, and the engine sends `Event`s back.
- **Data (later):** the Spotify Web API (playlists, search, recently played), using my own client ID.

### Phase 0: set up the project
- New binary crate `rusty-player`. `.gitignore` covers `target/` and `cache/`.
- `DEVLOG.md` with the first entry: librespot built, discovery failed, OAuth fixed it.

### Phase 1: play one track from my own code (CLI, no UI)
- Dependencies: `librespot` 0.8, `tokio` (full), `env_logger`, `log`, `anyhow`.
- Start from `examples/play.rs`, but load cached credentials with librespot's `Cache` instead of a token argument.
- Play a track by its Spotify ID.
- **Done when:** a song plays and the program exits cleanly at the end.

### Phase 2: the engine as a module, with commands and events
- `src/engine.rs`:
  ```rust
  pub enum Command { Play(String), Pause, Resume, Stop, SetVolume(u16) }
  pub enum Event  { Playing { track_id: String }, Paused, Stopped, EndOfTrack, Error(String) }
  ```
- `async fn run_engine(cmd_rx, evt_tx)`: log in, create the player, loop with `tokio::select!` over commands and
  librespot's player events, forwarding them as `Event`s.
- Terminal test loop: `p` pause, `r` resume, paste an ID to play it.
- **Done when:** I can pause, resume and switch tracks from the terminal.

### Phase 3: first window ("ugly dashboard v0")
- Tokio runtime on a background thread running `run_engine`; eframe on the main thread (required on Windows).
- UI: track-ID field + Play, Pause / Resume / Stop, volume slider, status label (Playing, Paused, last error).
- `update()` drains events with `try_recv()`. The engine calls `ctx.request_repaint()` on changes.
- **Done when:** clicking Play in my own window starts the music.

### Phase 4: real track info
- Track metadata through librespot's `metadata` module (no Web API needed).
- Album cover via `reqwest`, shown with `egui_extras` image support.
- Progress bar: position + timestamp from the playing event, computed every frame.
- **Done when:** it looks like a basic "now playing" screen.

### Phase 5: the real dashboard (Web API)
- Own app in the Spotify developer dashboard (Premium, max 5 users).
- `rspotify`, or my own `reqwest` calls (to learn HTTP/OAuth).
- Layout: playlists (left), tracks of the selected playlist (center, double-click plays), search (top, 10 results per
  request), now playing + controls + volume + progress (bottom), recently played and top artists/tracks (right).
- Web API calls go through the engine too (`LoadPlaylists` → `PlaylistsLoaded(Vec<…>)`).
- Handle 429 (quota) and cache results in memory.

### Phase 6: polish (pick what I like)
- Media keys and the Windows media overlay (`souvlaki`).
- Queue and next/previous in the engine.
- Settings (bitrate, normalization, device name) in a config file (`serde` + `toml`).
- Custom egui theme and dark mode.
- Release build: one small `.exe`. Compare its RAM use with the official app for the README.
