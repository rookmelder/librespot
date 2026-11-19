//! Logging in to Spotify with the credentials an earlier OAuth login saved in the cache.

use std::path::PathBuf;

use anyhow::{Context, Result};
use librespot::core::{
    authentication::Credentials, cache::Cache, config::SessionConfig, session::Session,
};

/// Where librespot keeps our login, volume and downloaded audio: `%APPDATA%\rusty-player\cache`.
pub fn cache_dir() -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join("rusty-player").join("cache"))
}

/// Log in with the credentials saved in the cache. Must run inside a Tokio runtime.
pub async fn connect() -> Result<Session> {
    let dir = cache_dir()?;
    let cache = Cache::new(Some(&dir), Some(&dir), Some(&dir.join("files")), None)
        .with_context(|| format!("could not open the cache in {}", dir.display()))?;

    let credentials: Credentials = cache.credentials().with_context(|| {
        format!(
            "no saved login in {}. Log in once with OAuth:\n  cargo run -- --enable-oauth --cache \"{}\"",
            dir.display(),
            dir.display()
        )
    })?;

    let session = Session::new(SessionConfig::default(), Some(cache));
    // `true`: save the refreshed credentials back to the cache, so the next run logs in too.
    session
        .connect(credentials, true)
        .await
        .context("could not log in to Spotify")?;
    Ok(session)
}
