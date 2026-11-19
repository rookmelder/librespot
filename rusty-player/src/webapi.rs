//! The Spotify Web API (playlists, search, stats), with our own app's client ID.
//!
//! Login is OAuth with PKCE (no client secret) through librespot's oauth crate. The first time, a browser opens;
//! after that the refresh token in `%APPDATA%\rusty-player\webapi_token.json` keeps us logged in.

use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use librespot::oauth::{OAuthClientBuilder, OAuthToken};
use log::{info, warn};
use reqwest::{StatusCode, header::RETRY_AFTER};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Our app in the Spotify developer dashboard. Not a secret: PKCE login needs no client secret.
const CLIENT_ID: &str = "35d8ddaa5a804111887fdbb1e7fc1491";
/// Must match the redirect URI registered in the dashboard exactly.
const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const SCOPES: &[&str] = &[
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-library-read",
    "user-read-recently-played",
    "user-top-read",
];
const API: &str = "https://api.spotify.com/v1";
/// Refresh the access token this long before it expires.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);
/// Longest wait we accept from a 429 "Retry-After" before giving up.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(30);
/// Search pages to fetch (10 results each: Spotify's maximum per request).
const SEARCH_PAGES: usize = 2;

/// One playlist in the user's list.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub track_count: u32,
}

/// One playable track in a playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSummary {
    /// `spotify:track:<id>`: what `Command::Play` takes.
    pub uri: String,
    pub name: String,
    /// Artist names joined with ", ".
    pub artists: String,
    pub duration_ms: u32,
}

/// The token as saved on disk. Expiry is wall-clock time, because `Instant` cannot be saved.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedToken {
    access_token: String,
    refresh_token: String,
    /// Seconds since 1970 when the access token stops working.
    expires_at_unix: u64,
}

impl SavedToken {
    fn from_oauth(token: &OAuthToken, now_unix: u64) -> Self {
        let valid_for = token.expires_at.saturating_duration_since(Instant::now());
        Self {
            access_token: token.access_token.clone(),
            refresh_token: token.refresh_token.clone(),
            expires_at_unix: now_unix + valid_for.as_secs(),
        }
    }

    fn is_fresh(&self, now_unix: u64) -> bool {
        now_unix + EXPIRY_MARGIN.as_secs() < self.expires_at_unix
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn token_path() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata)
        .join("rusty-player")
        .join("webapi_token.json"))
}

/// A logged-in Web API client. Keeps its token fresh and saves it after every change.
pub struct WebApi {
    http: reqwest::Client,
    token: SavedToken,
}

impl WebApi {
    /// Use the saved token (refreshing it if needed), or log in through the browser the first time.
    pub async fn login() -> Result<Self> {
        let path = token_path()?;
        let saved = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<SavedToken>(&text).ok());

        let token = match saved {
            Some(token) if token.is_fresh(now_unix()) => token,
            Some(token) => match refresh(&token).await {
                Ok(fresh) => fresh,
                Err(error) => {
                    warn!("Web API token refresh failed ({error:#}), logging in again");
                    browser_login().await?
                }
            },
            None => browser_login().await?,
        };
        save_token(&token)?;
        Ok(Self {
            http: reqwest::Client::new(),
            token,
        })
    }

    /// All of the user's playlists (follows the paging until the end).
    pub async fn playlists(&mut self) -> Result<Vec<PlaylistSummary>> {
        #[derive(Deserialize)]
        struct Page {
            items: Vec<Item>,
            next: Option<String>,
        }
        #[derive(Deserialize)]
        struct Item {
            id: String,
            name: String,
            // Newer answers call the count `items`, older ones `tracks`.
            items: Option<Tracks>,
            tracks: Option<Tracks>,
        }
        #[derive(Deserialize)]
        struct Tracks {
            total: u32,
        }

        let mut playlists = Vec::new();
        let mut url = Some(format!("{API}/me/playlists?limit=50"));
        while let Some(next) = url {
            let page: Page = self.get(&next).await.context("could not load playlists")?;
            playlists.extend(page.items.into_iter().map(|item| PlaylistSummary {
                id: item.id,
                name: item.name,
                track_count: item.items.or(item.tracks).map_or(0, |t| t.total),
            }));
            url = next_page(page.next)?;
        }
        Ok(playlists)
    }

    /// The playable tracks of one playlist, in playlist order (follows the paging until the end). Episodes,
    /// local files and removed tracks are skipped.
    pub async fn playlist_tracks(&mut self, playlist_id: &str) -> Result<Vec<TrackSummary>> {
        if playlist_id.is_empty() || !playlist_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            bail!("not a playlist ID: {playlist_id}");
        }
        let mut tracks = Vec::new();
        let mut url = Some(format!(
            "{API}/playlists/{playlist_id}/items?limit=100&additional_types=track"
        ));
        while let Some(next) = url {
            let page: TrackPage = match self.get(&next).await {
                Ok(page) => page,
                Err(error) if is_forbidden(&error) => {
                    bail!("Spotify only shows the songs of playlists you own or collaborate on")
                }
                Err(error) => return Err(error.context("could not load the playlist's tracks")),
            };
            tracks.extend(
                page.items
                    .into_iter()
                    .filter_map(PlaylistEntry::into_summary),
            );
            url = next_page(page.next)?;
        }
        Ok(tracks)
    }

    /// Search tracks. Spotify returns at most 10 per request, so this fetches up to `SEARCH_PAGES` pages.
    pub async fn search_tracks(&mut self, query: &str) -> Result<Vec<TrackSummary>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        // Url::parse_with_params escapes the query text (spaces, &, #, ...).
        let first = reqwest::Url::parse_with_params(
            &format!("{API}/search"),
            &[("q", query), ("type", "track"), ("limit", "10")],
        )
        .context("could not build the search URL")?;

        let mut tracks = Vec::new();
        let mut url = Some(first.to_string());
        for _ in 0..SEARCH_PAGES {
            let Some(next) = url else { break };
            let answer: SearchAnswer = self.get(&next).await.context("search failed")?;
            tracks.extend(
                answer
                    .tracks
                    .items
                    .into_iter()
                    .flatten()
                    .filter_map(ApiTrack::into_summary),
            );
            url = next_page(answer.tracks.next)?;
        }
        Ok(tracks)
    }

    /// GET a JSON endpoint with the access token. Refreshes the token first if it is about to expire, and
    /// waits once on a 429 (rate limit) when Spotify asks for a short wait.
    async fn get<T: DeserializeOwned>(&mut self, url: &str) -> Result<T> {
        if !self.token.is_fresh(now_unix()) {
            self.token = refresh(&self.token).await?;
            save_token(&self.token)?;
        }
        for attempt in 0..2 {
            let response = self
                .http
                .get(url)
                .bearer_auth(&self.token.access_token)
                .send()
                .await
                .context("Web API request failed")?;
            if response.status() == StatusCode::TOO_MANY_REQUESTS && attempt == 0 {
                let wait = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map_or(Duration::from_secs(1), Duration::from_secs);
                if wait > MAX_RETRY_WAIT {
                    bail!("Spotify rate limit: try again in {} s", wait.as_secs());
                }
                warn!("Spotify rate limit, waiting {} s", wait.as_secs());
                tokio::time::sleep(wait).await;
                continue;
            }
            let response = response.error_for_status().context("Web API error")?;
            return response
                .json::<T>()
                .await
                .context("unexpected Web API answer");
        }
        bail!("Spotify rate limit: try again later")
    }
}

#[derive(Deserialize)]
struct TrackPage {
    items: Vec<PlaylistEntry>,
    next: Option<String>,
}

/// One entry of a playlist page. The track is under `item` (current) or `track` (deprecated).
#[derive(Deserialize)]
struct PlaylistEntry {
    item: Option<ApiTrack>,
    track: Option<ApiTrack>,
}

#[derive(Deserialize)]
struct ApiTrack {
    uri: Option<String>,
    name: Option<String>,
    #[serde(default)]
    artists: Vec<ApiArtist>,
    #[serde(default)]
    duration_ms: u32,
    #[serde(default)]
    is_local: bool,
}

#[derive(Deserialize)]
struct ApiArtist {
    name: String,
}

impl PlaylistEntry {
    fn into_summary(self) -> Option<TrackSummary> {
        self.item.or(self.track)?.into_summary()
    }
}

impl ApiTrack {
    /// `None` for local files, episodes and entries without a URI.
    fn into_summary(self) -> Option<TrackSummary> {
        let uri = self.uri?;
        if self.is_local || !uri.starts_with("spotify:track:") {
            return None;
        }
        Some(TrackSummary {
            uri,
            name: self.name.unwrap_or_default(),
            artists: self
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            duration_ms: self.duration_ms,
        })
    }
}

#[derive(Deserialize)]
struct SearchAnswer {
    tracks: SearchTracks,
}

#[derive(Deserialize)]
struct SearchTracks {
    items: Vec<Option<ApiTrack>>,
    next: Option<String>,
}

fn is_forbidden(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<reqwest::Error>())
        .any(|e| e.status() == Some(StatusCode::FORBIDDEN))
}

/// The next page to fetch. Only links on the API host are followed: never send the token anywhere else.
fn next_page(next: Option<String>) -> Result<Option<String>> {
    match next {
        Some(url) if url.starts_with(&format!("{API}/")) => Ok(Some(url)),
        Some(other) => bail!("unexpected page link: {other}"),
        None => Ok(None),
    }
}

/// First login: opens the browser and waits for Spotify to redirect back to 127.0.0.1:8898.
async fn browser_login() -> Result<SavedToken> {
    info!("Opening the browser for the Spotify Web API login");
    // librespot's listener for the redirect is blocking I/O, so it runs on a blocking thread, not on a
    // Tokio worker. The sync `get_access_token` uses a blocking HTTP client, which is fine there too.
    let token = tokio::task::spawn_blocking(|| {
        OAuthClientBuilder::new(CLIENT_ID, REDIRECT_URI, SCOPES.to_vec())
            .open_in_browser()
            .build()
            .and_then(|client| client.get_access_token())
    })
    .await
    .context("the login thread failed")?
    .context("Web API login failed")?;
    Ok(SavedToken::from_oauth(&token, now_unix()))
}

async fn refresh(token: &SavedToken) -> Result<SavedToken> {
    let client = OAuthClientBuilder::new(CLIENT_ID, REDIRECT_URI, SCOPES.to_vec())
        .build()
        .context("could not set up the Web API login")?;
    let fresh = client
        .refresh_token_async(&token.refresh_token)
        .await
        .context("could not refresh the Web API token")?;
    let mut saved = SavedToken::from_oauth(&fresh, now_unix());
    // Spotify may leave the refresh token out of a refresh answer: then the old one stays valid.
    if saved.refresh_token.is_empty() {
        saved.refresh_token = token.refresh_token.clone();
    }
    Ok(saved)
}

fn save_token(token: &SavedToken) -> Result<()> {
    let path = token_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    let text = serde_json::to_string(token).context("could not encode the token")?;
    // Write a temp file first and rename it over the old one, so a crash mid-write never leaves a
    // half-written token behind.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("could not write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("could not save {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_entries_keep_only_spotify_tracks() {
        let page: TrackPage = serde_json::from_str(
            r#"{"next": null, "items": [
                {"track": {"uri": "spotify:track:abc", "name": "Song", "duration_ms": 61000,
                           "artists": [{"name": "A"}, {"name": "B"}]}},
                {"item": {"uri": "spotify:track:def", "name": "New shape", "artists": []}},
                {"track": {"uri": "spotify:local:x:y:z:1", "name": "Local", "is_local": true}},
                {"track": {"uri": "spotify:episode:e", "name": "Podcast"}},
                {"track": null}
            ]}"#,
        )
        .unwrap();
        let tracks: Vec<TrackSummary> = page
            .items
            .into_iter()
            .filter_map(PlaylistEntry::into_summary)
            .collect();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].uri, "spotify:track:abc");
        assert_eq!(tracks[0].artists, "A, B");
        assert_eq!(tracks[0].duration_ms, 61000);
        assert_eq!(tracks[1].name, "New shape");
    }

    #[test]
    fn search_answer_keeps_tracks_and_skips_nulls() {
        let answer: SearchAnswer = serde_json::from_str(
            r#"{"tracks": {"next": null, "items": [
                {"uri": "spotify:track:abc", "name": "Song", "duration_ms": 1000, "artists": [{"name": "A"}]},
                null
            ]}}"#,
        )
        .unwrap();
        let tracks: Vec<TrackSummary> = answer
            .tracks
            .items
            .into_iter()
            .flatten()
            .filter_map(ApiTrack::into_summary)
            .collect();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name, "Song");
    }

    #[test]
    fn next_page_only_follows_api_links() {
        assert_eq!(next_page(None).unwrap(), None);
        let ok = format!("{API}/me/playlists?offset=50");
        assert_eq!(next_page(Some(ok.clone())).unwrap(), Some(ok));
        assert!(next_page(Some("https://evil.example/v1/x".into())).is_err());
        assert!(next_page(Some("https://api.spotify.com.evil.example/v1/x".into())).is_err());
    }

    #[test]
    fn token_freshness_uses_the_margin() {
        let token = SavedToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at_unix: 1_000,
        };
        assert!(token.is_fresh(900));
        assert!(!token.is_fresh(940)); // within 60 s of expiry
        assert!(!token.is_fresh(2_000));
    }

    #[test]
    fn saved_token_round_trips_through_json() {
        let token = SavedToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at_unix: 123,
        };
        let text = serde_json::to_string(&token).unwrap();
        let back: SavedToken = serde_json::from_str(&text).unwrap();
        assert_eq!(back.access_token, "a");
        assert_eq!(back.refresh_token, "r");
        assert_eq!(back.expires_at_unix, 123);
    }
}
