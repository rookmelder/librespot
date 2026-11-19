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

/// One playlist in the user's list.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub track_count: u32,
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
                track_count: item.tracks.map_or(0, |t| t.total),
            }));
            // Only follow "next" links on the API host: never send the token anywhere else.
            url = match page.next {
                Some(next_url) if next_url.starts_with(&format!("{API}/")) => Some(next_url),
                Some(other) => bail!("unexpected playlist page link: {other}"),
                None => None,
            };
        }
        Ok(playlists)
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
