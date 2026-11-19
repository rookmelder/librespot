//! Disk cache for album and playlist images.

use std::{
    collections::HashMap,
    fs::{self},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    task::Poll,
};

use eframe::egui::{
    self,
    load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError},
};

#[derive(Clone)]
struct CachedFile {
    bytes: Arc<[u8]>,
    mime: Option<String>,
}

type Entry = Poll<Result<CachedFile, String>>;

/// Loader that caches remote images on disk and in memory.
pub struct DiskImageLoader {
    dir: PathBuf,
    cache: Arc<Mutex<HashMap<String, Entry>>>,
}

impl DiskImageLoader {
    pub const ID: &'static str = egui::generate_loader_id!(DiskImageLoader);

    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            cache: Arc::new(Mutex::new(HashMap::default())),
        }
    }
}

/// Computes a deterministic 64-bit FNV-1a hash of uri bytes as 16 lowercase hex characters + `.img`.
pub fn file_name(uri: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    for &b in uri.as_bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}.img")
}

/// Guesses mime type from file header magic bytes.
pub fn guess_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        Some("image/png")
    } else {
        None
    }
}

/// Trims `dir` to `target_bytes` if `*.img` files exceed `max_bytes`, and removes `*.tmp` files.
pub fn trim(dir: &Path, max_bytes: u64, target_bytes: u64) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            log::debug!("failed to read image cache dir {dir:?}: {err}");
            return;
        }
    };

    struct ImgEntry {
        path: PathBuf,
        size: u64,
        modified: std::time::SystemTime,
    }

    let mut img_files = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                log::debug!("error reading entry in {dir:?}: {err}");
                continue;
            }
        };

        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if name.ends_with(".tmp") {
            if let Err(err) = fs::remove_file(&path) {
                log::debug!("failed to remove tmp file {path:?}: {err}");
            }
            continue;
        }

        if name.ends_with(".img") {
            match entry.metadata() {
                Ok(meta) if meta.is_file() => {
                    let size = meta.len();
                    let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    img_files.push(ImgEntry {
                        path,
                        size,
                        modified,
                    });
                }
                Ok(_) => {}
                Err(err) => {
                    log::debug!("metadata error on {path:?}: {err}");
                }
            }
        }
    }

    let mut total_bytes: u64 = img_files.iter().map(|f| f.size).sum();
    if total_bytes > max_bytes {
        // Sort oldest modified first
        img_files.sort_by_key(|f| f.modified);

        for file in img_files {
            if total_bytes <= target_bytes {
                break;
            }
            if let Err(err) = fs::remove_file(&file.path) {
                log::debug!("failed to remove cache file {:?}: {err}", file.path);
            } else {
                total_bytes = total_bytes.saturating_sub(file.size);
            }
        }
    }
}

/// Installs the disk image loader into the egui context.
pub fn install(ctx: &egui::Context) {
    let appdata = match std::env::var_os("APPDATA") {
        Some(val) => PathBuf::from(val),
        None => return,
    };

    let dir = appdata.join("rusty-player").join("cache").join("images");
    if let Err(err) = fs::create_dir_all(&dir) {
        log::debug!("failed to create cache dir {dir:?}: {err}");
        return;
    }

    let trim_dir = dir.clone();
    std::thread::Builder::new()
        .name("imagecache-trim".into())
        .spawn(move || {
            trim(&trim_dir, 200 * 1024 * 1024, 150 * 1024 * 1024);
        })
        .ok();

    ctx.add_bytes_loader(Arc::new(DiskImageLoader::new(dir)));
}

impl BytesLoader for DiskImageLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !uri.starts_with("https://") {
            return Err(LoadError::NotSupported);
        }

        let mut cache = match self.cache.lock() {
            Ok(c) => c,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(entry) = cache.get(uri).cloned() {
            return match entry {
                Poll::Ready(Ok(file)) => Ok(BytesPoll::Ready {
                    size: None,
                    bytes: Bytes::Shared(file.bytes),
                    mime: file.mime,
                }),
                Poll::Ready(Err(err)) => Err(LoadError::Loading(err)),
                Poll::Pending => Ok(BytesPoll::Pending { size: None }),
            };
        }

        let disk_path = self.dir.join(file_name(uri));
        if disk_path.is_file() {
            match fs::read(&disk_path) {
                Ok(data) => {
                    let mime = guess_mime(&data).map(str::to_owned);
                    let bytes: Arc<[u8]> = Arc::from(data);
                    let file = CachedFile {
                        bytes: Arc::clone(&bytes),
                        mime: mime.clone(),
                    };
                    cache.insert(uri.to_owned(), Poll::Ready(Ok(file)));
                    return Ok(BytesPoll::Ready {
                        size: None,
                        bytes: Bytes::Shared(bytes),
                        mime,
                    });
                }
                Err(err) => {
                    log::debug!("failed to read disk cache file {disk_path:?}: {err}");
                }
            }
        }

        // Cache miss: mark Pending and fetch via ehttp
        log::trace!("started loading {uri:?}");
        let uri_str = uri.to_owned();
        cache.insert(uri_str.clone(), Poll::Pending);
        drop(cache);

        let disk_dir = self.dir.clone();
        ehttp::fetch(ehttp::Request::get(uri_str.clone()), {
            let ctx = ctx.clone();
            let cache = Arc::clone(&self.cache);
            let uri = uri_str;
            move |response| {
                let result = match response {
                    Ok(resp) => {
                        if !resp.ok {
                            let text = resp.text().unwrap_or_default();
                            Err(format!(
                                "failed to load {uri:?}: {} {} {text}",
                                resp.status, resp.status_text
                            ))
                        } else {
                            let mime = resp.content_type().map(str::to_owned);
                            let data = resp.bytes;
                            let fname = file_name(&uri);
                            let target_path = disk_dir.join(&fname);
                            let tmp_path = disk_dir.join(format!("{fname}.tmp"));

                            if let Err(err) = fs::write(&tmp_path, &data) {
                                log::debug!("failed to write tmp cache file {tmp_path:?}: {err}");
                            } else if let Err(err) = fs::rename(&tmp_path, &target_path) {
                                log::debug!(
                                    "failed to rename {tmp_path:?} to {target_path:?}: {err}"
                                );
                                let _ = fs::remove_file(&tmp_path);
                            }

                            Ok(CachedFile {
                                bytes: data.into(),
                                mime,
                            })
                        }
                    }
                    Err(err) => {
                        log::error!("Failed to load {uri:?}: {err}");
                        Err(format!("Failed to load {uri:?}"))
                    }
                };

                let repaint = {
                    let mut cache = match cache.lock() {
                        Ok(c) => c,
                        Err(p) => p.into_inner(),
                    };
                    if let std::collections::hash_map::Entry::Occupied(mut entry) =
                        cache.entry(uri.clone())
                    {
                        *entry.get_mut() = Poll::Ready(result);
                        log::trace!("Finished loading {uri:?}");
                        true
                    } else {
                        log::trace!("Canceled loading {uri:?}");
                        false
                    }
                };

                if repaint {
                    ctx.request_repaint();
                }
            }
        });

        Ok(BytesPoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        let mut cache = match self.cache.lock() {
            Ok(c) => c,
            Err(p) => p.into_inner(),
        };
        let _ = cache.remove(uri);
    }

    fn forget_all(&self) {
        let mut cache = match self.cache.lock() {
            Ok(c) => c,
            Err(p) => p.into_inner(),
        };
        cache.clear();
    }

    fn byte_size(&self) -> usize {
        let cache = match self.cache.lock() {
            Ok(c) => c,
            Err(p) => p.into_inner(),
        };
        cache
            .values()
            .map(|entry| match entry {
                Poll::Ready(Ok(file)) => {
                    file.bytes.len() + file.mime.as_ref().map_or(0, std::string::String::len)
                }
                Poll::Ready(Err(err)) => err.len(),
                _ => 0,
            })
            .sum()
    }

    fn has_pending(&self) -> bool {
        let cache = match self.cache.lock() {
            Ok(c) => c,
            Err(p) => p.into_inner(),
        };
        cache.values().any(std::task::Poll::is_pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("rusty_player_test_{name}_{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_file_name_stability() {
        let uri = "https://i.scdn.co/image/abc";
        let mut expected_hash: u64 = 0xcbf29ce484222325;
        const PRIME: u64 = 0x100000001b3;
        for &b in uri.as_bytes() {
            expected_hash ^= u64::from(b);
            expected_hash = expected_hash.wrapping_mul(PRIME);
        }
        let expected_name = format!("{expected_hash:016x}.img");

        let name = file_name(uri);
        assert_eq!(name, expected_name);
        assert_eq!(name.len(), 20); // 16 hex chars + ".img" (4)
        assert!(name.ends_with(".img"));

        let diff_uri = "https://i.scdn.co/image/xyz";
        assert_ne!(file_name(diff_uri), name);
    }

    #[test]
    fn test_mime_sniffing() {
        assert_eq!(guess_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(
            guess_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A]),
            Some("image/png")
        );
        assert_eq!(guess_mime(&[0x00, 0x01, 0x02, 0x03]), None);
        assert_eq!(guess_mime(&[]), None);
    }

    #[test]
    fn test_trim_over_limit_and_tmp_removal() {
        let temp = TempDir::new("trim_over");
        let dir = &temp.0;

        let f1 = dir.join("1.img");
        let f2 = dir.join("2.img");
        let f3 = dir.join("3.img");
        let tmp = dir.join("leftover.tmp");

        fs::write(&f1, [1u8; 100]).unwrap();
        fs::write(&f2, [2u8; 100]).unwrap();
        fs::write(&f3, [3u8; 100]).unwrap();
        fs::write(&tmp, [4u8; 50]).unwrap();

        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let file1 = fs::File::options().write(true).open(&f1).unwrap();
        file1
            .set_modified(base_time + Duration::from_secs(10))
            .unwrap();
        drop(file1);

        let file2 = fs::File::options().write(true).open(&f2).unwrap();
        file2
            .set_modified(base_time + Duration::from_secs(20))
            .unwrap();
        drop(file2);

        let file3 = fs::File::options().write(true).open(&f3).unwrap();
        file3
            .set_modified(base_time + Duration::from_secs(30))
            .unwrap();
        drop(file3);

        // 3 files * 100 bytes = 300 bytes. max 250, target 150.
        // Oldest two (f1, f2) should be deleted, leaving f3 (100 bytes <= 150).
        // leftover.tmp should also be removed.
        trim(dir, 250, 150);

        assert!(!f1.exists(), "f1 (oldest) should be removed");
        assert!(!f2.exists(), "f2 should be removed");
        assert!(f3.exists(), "f3 (newest) should be kept");
        assert!(!tmp.exists(), "tmp file should be removed");
    }

    #[test]
    fn test_trim_under_limit() {
        let temp = TempDir::new("trim_under");
        let dir = &temp.0;

        let f1 = dir.join("1.img");
        let f2 = dir.join("2.img");
        fs::write(&f1, [1u8; 100]).unwrap();
        fs::write(&f2, [2u8; 100]).unwrap();

        // 200 bytes <= 250 bytes max -> nothing removed
        trim(dir, 250, 150);

        assert!(f1.exists());
        assert!(f2.exists());
    }

    #[test]
    fn test_loader_unsupported_protocols() {
        let temp = TempDir::new("loader_unsupported");
        let loader = DiskImageLoader::new(&temp.0);
        let ctx = egui::Context::default();

        match loader.load(&ctx, "file://x") {
            Err(LoadError::NotSupported) => {}
            Err(err) => panic!("expected NotSupported, got error: {err:?}"),
            Ok(_) => panic!("expected NotSupported, got Ok"),
        }

        match loader.load(&ctx, "bytes://x") {
            Err(LoadError::NotSupported) => {}
            Err(err) => panic!("expected NotSupported, got error: {err:?}"),
            Ok(_) => panic!("expected NotSupported, got Ok"),
        }
    }

    #[test]
    fn test_loader_reads_prewritten_disk_file() {
        let temp = TempDir::new("loader_prewritten");
        let uri = "https://i.scdn.co/image/prewritten";
        let target_name = file_name(uri);
        let file_path = temp.0.join(target_name);

        // Write a PNG file
        let png_bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        fs::write(&file_path, png_bytes).unwrap();

        let loader = DiskImageLoader::new(&temp.0);
        let ctx = egui::Context::default();

        let result = loader.load(&ctx, uri);
        match result {
            Ok(BytesPoll::Ready { size, bytes, mime }) => {
                assert_eq!(size, None);
                assert_eq!(&*bytes, &png_bytes);
                assert_eq!(mime.as_deref(), Some("image/png"));
            }
            Ok(BytesPoll::Pending { .. }) => panic!("expected Ready, got Pending"),
            Err(err) => panic!("expected Ready, got Err: {err:?}"),
        }
    }
}
