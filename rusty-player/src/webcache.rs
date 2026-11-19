//! Disk cache for Web API answers and session-local freshness information.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::webapi::{PlaylistSummary, TrackSummary};

const SEARCH_TTL: Duration = Duration::from_secs(10 * 60);
const SEARCH_LIMIT: usize = 50;

#[derive(Default)]
struct State {
    snapshots: HashMap<String, String>,
    liked_checked: bool,
    searches: HashMap<String, (Instant, Vec<TrackSummary>)>,
}

struct Inner {
    dir: Option<PathBuf>,
    state: Mutex<State>,
    disk: Mutex<()>,
}

#[derive(Clone)]
pub struct WebCache {
    inner: Arc<Inner>,
}

#[derive(Serialize, Deserialize)]
struct Saved<T> {
    saved_unix: u64,
    value: T,
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

pub fn key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 100
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

impl WebCache {
    pub fn new() -> Self {
        match std::env::var_os("APPDATA") {
            Some(appdata) => Self::with_dir(PathBuf::from(appdata).join("rusty-player/cache/web")),
            None => Self::from_dir(None),
        }
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self::from_dir(Some(dir))
    }

    fn from_dir(dir: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                dir,
                state: Mutex::new(State::default()),
                disk: Mutex::new(()),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn load<T: DeserializeOwned>(&self, key: &str) -> Option<(u64, T)> {
        if !key_ok(key) {
            return None;
        }
        let dir = self.inner.dir.as_ref()?;
        let path = dir.join(format!("{key}.json"));
        let result = (|| -> anyhow::Result<Saved<T>> {
            let bytes = std::fs::read(&path)?;
            Ok(serde_json::from_slice(&bytes)?)
        })();
        match result {
            Ok(saved) => Some((saved.saved_unix, saved.value)),
            Err(error) => {
                log::debug!("could not load Web API cache {}: {error:#}", path.display());
                None
            }
        }
    }

    pub fn save<T: Serialize>(&self, key: &str, value: &T) {
        if !key_ok(key) {
            return;
        }
        let Some(dir) = &self.inner.dir else { return };
        // Serialize writers sharing this cache so their temporary files cannot collide.
        let _disk = self
            .inner
            .disk
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = dir.join(format!("{key}.json"));
        let tmp = path.with_extension("json.tmp");
        let result = (|| -> anyhow::Result<()> {
            let bytes = serde_json::to_vec(&Saved {
                saved_unix: now_unix(),
                value,
            })?;
            std::fs::create_dir_all(dir)?;
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if let Err(error) = result {
            log::warn!("could not save Web API cache {}: {error:#}", path.display());
            if let Err(error) = std::fs::remove_file(&tmp) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!(
                        "could not remove cache temporary file {}: {error}",
                        tmp.display()
                    );
                }
            }
        }
    }

    pub fn remove_prefix(&self, prefix: &str) {
        if !key_ok(prefix) {
            return;
        }
        let Some(dir) = &self.inner.dir else { return };
        let _disk = self
            .inner
            .disk
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = (|| -> std::io::Result<()> {
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
            };
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                let Some(key) = path.file_stem().and_then(|name| name.to_str()) else {
                    continue;
                };
                if path.extension().is_some_and(|ext| ext == "json")
                    && key_ok(key)
                    && key.starts_with(prefix)
                    && entry.file_type()?.is_file()
                {
                    if let Err(error) = std::fs::remove_file(&path) {
                        log::warn!("could not remove Web API cache {}: {error}", path.display());
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            log::warn!("could not clear Web API cache prefix {prefix}: {error}");
        }
    }

    pub fn set_snapshots(&self, list: &[PlaylistSummary]) {
        self.state().snapshots = list
            .iter()
            .filter_map(|playlist| {
                playlist
                    .snapshot_id
                    .as_ref()
                    .map(|snapshot| (playlist.id.clone(), snapshot.clone()))
            })
            .collect();
    }

    pub fn snapshot(&self, id: &str) -> Option<String> {
        self.state().snapshots.get(id).cloned()
    }

    pub fn liked_checked(&self) -> bool {
        self.state().liked_checked
    }

    pub fn set_liked_checked(&self) {
        self.state().liked_checked = true;
    }

    pub fn search_get(&self, query: &str) -> Option<Vec<TrackSummary>> {
        let mut state = self.state();
        state
            .searches
            .retain(|_, (saved, _)| saved.elapsed() < SEARCH_TTL);
        state.searches.get(query).map(|(_, tracks)| tracks.clone())
    }

    pub fn search_put(&self, query: &str, tracks: Vec<TrackSummary>) {
        let mut state = self.state();
        state
            .searches
            .retain(|_, (saved, _)| saved.elapsed() < SEARCH_TTL);
        if !state.searches.contains_key(query) && state.searches.len() >= SEARCH_LIMIT {
            let oldest = state
                .searches
                .iter()
                .min_by_key(|(_, (saved, _))| *saved)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                state.searches.remove(&oldest);
            }
        }
        state
            .searches
            .insert(query.to_owned(), (Instant::now(), tracks));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> anyhow::Result<Self> {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rusty-player-webcache-{}-{nonce}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path)?;
            Ok(Self(path))
        }

        fn cache(&self) -> WebCache {
            WebCache::with_dir(self.0.clone())
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tracks(name: &str) -> Vec<TrackSummary> {
        vec![TrackSummary {
            uri: "spotify:track:example".into(),
            name: name.into(),
            artists: "Example artist".into(),
            album: "Example album".into(),
            duration_ms: 1234,
            image_url: None,
        }]
    }

    #[test]
    fn webcache_round_trip_and_overwrite_leave_no_tmp() -> anyhow::Result<()> {
        let dir = TestDir::new()?;
        let cache = dir.cache();
        let before = now_unix();
        cache.save("tracks", &tracks("First"));
        let (saved, value) = cache
            .load::<Vec<TrackSummary>>("tracks")
            .context("missing cache")?;
        assert_eq!(value, tracks("First"));
        assert!((before..=now_unix()).contains(&saved));
        assert!(now_unix().saturating_sub(saved) < 5);
        cache.save("tracks", &tracks("Second"));
        assert_eq!(
            cache
                .load::<Vec<TrackSummary>>("tracks")
                .map(|(_, value)| value),
            Some(tracks("Second"))
        );
        assert!(!dir.0.join("tracks.json.tmp").exists());
        assert_eq!(std::fs::read_dir(&dir.0)?.count(), 1);
        Ok(())
    }

    #[test]
    fn webcache_missing_and_corrupt_are_misses() -> anyhow::Result<()> {
        let dir = TestDir::new()?;
        let cache = dir.cache();
        assert!(cache.load::<Vec<TrackSummary>>("missing").is_none());
        std::fs::write(dir.0.join("corrupt.json"), b"{broken")?;
        assert!(cache.load::<Vec<TrackSummary>>("corrupt").is_none());
        Ok(())
    }

    #[test]
    fn webcache_rejects_bad_keys() -> anyhow::Result<()> {
        let dir = TestDir::new()?;
        let cache = dir.cache();
        for key in ["", "../x", "a/b", "a b", &"a".repeat(101)] {
            assert!(!key_ok(key));
            cache.save(key, &tracks("Example"));
            assert!(cache.load::<Vec<TrackSummary>>(key).is_none());
            cache.remove_prefix(key);
        }
        assert_eq!(std::fs::read_dir(&dir.0)?.count(), 0);
        cache.save("playlists", &tracks("Keep"));
        cache.remove_prefix("");
        assert!(cache.load::<Vec<TrackSummary>>("playlists").is_some());
        Ok(())
    }

    #[test]
    fn webcache_remove_prefix_keeps_other_keys() -> anyhow::Result<()> {
        let dir = TestDir::new()?;
        let cache = dir.cache();
        for key in ["liked_0", "liked_50", "playlists"] {
            cache.save(key, &tracks("Example"));
        }
        cache.remove_prefix("liked_");
        assert!(cache.load::<Vec<TrackSummary>>("liked_0").is_none());
        assert!(cache.load::<Vec<TrackSummary>>("liked_50").is_none());
        assert!(cache.load::<Vec<TrackSummary>>("playlists").is_some());
        Ok(())
    }

    #[test]
    fn webcache_search_memo_expires_and_keeps_newest_fifty() -> anyhow::Result<()> {
        let dir = TestDir::new()?;
        let cache = dir.cache();
        assert!(cache.search_get("unknown").is_none());
        cache.search_put("first", tracks("First"));
        assert_eq!(cache.search_get("first"), Some(tracks("First")));
        // Set the old entry's timestamp explicitly; no timing-dependent sleeps.
        cache
            .state()
            .searches
            .get_mut("first")
            .context("missing search")?
            .0 = Instant::now() - Duration::from_secs(1);
        for index in 0..50 {
            cache.search_put(&format!("query{index}"), tracks("New"));
        }
        assert!(cache.search_get("first").is_none());
        assert_eq!(cache.state().searches.len(), 50);
        for index in 0..50 {
            assert!(cache.search_get(&format!("query{index}")).is_some());
        }
        cache
            .state()
            .searches
            .get_mut("query49")
            .context("missing search")?
            .0 = Instant::now() - SEARCH_TTL;
        assert!(cache.search_get("query49").is_none());
        Ok(())
    }

    #[test]
    fn webcache_snapshots_and_liked_state_are_shared() -> anyhow::Result<()> {
        let dir = TestDir::new()?;
        let cache = dir.cache();
        let clone = cache.clone();
        assert!(cache.snapshot("example").is_none());
        cache.set_snapshots(&[PlaylistSummary {
            id: "example".into(),
            name: "Example".into(),
            track_count: 0,
            image_url: None,
            snapshot_id: Some("revision".into()),
        }]);
        assert_eq!(clone.snapshot("example").as_deref(), Some("revision"));
        assert!(!clone.liked_checked());
        cache.set_liked_checked();
        assert!(clone.liked_checked());
        cache.set_snapshots(&[]);
        assert!(clone.snapshot("example").is_none());
        Ok(())
    }
}
