// SPDX-License-Identifier: MPL-2.0
//! Disk cache of artwork for Kinolune (LIB-013).
//!
//! Posters, backdrops and episode stills from providers (TMDB, AniList,
//! Shikimori; `artwork` table, META-027) are downloaded once over the core's
//! HTTPS stack (`ureq` + rustls, as `pa-metadata`) into the platform cache
//! directory (`PlatformDirs::cache`, `pa-runtime`) and handed to the UI as
//! local file paths. The UI decodes them at display size off its UI thread
//! (Flutter `ResizeImage`), so this crate does no image decoding itself.
//!
//! - **Size variants** — provider size URLs ([`size`]): a grid of TMDB
//!   posters downloads `w185`/`w342`, not the stored `w500`.
//! - **LRU with a size limit** — [`CacheOptions::max_bytes`]; when it is
//!   exceeded, least recently used files go first, down to 90 % of the
//!   limit. The order survives restarts: the last access is the file's
//!   modification time.
//! - **Offline** — [`ImageCache::get`] serves a cached copy (of any size
//!   variant) when the network fails; [`ImageCache::cached`] never touches
//!   the network.
//! - **Provider terms** — files stay in the user's own cache, are never
//!   served to other devices, and expire after [`CacheOptions::max_age`]
//!   (six months by default, the limit of TMDB's API terms; see also the
//!   `ProviderCache` of `pa-metadata`).
//! - **Local art** (`poster.jpg` next to the media, LIB-007) is not copied:
//!   local paths and `file://` URLs come back as paths.
//!
//! On tvOS the cache directory is `Library/Caches`, which the system may
//! purge; the cache then simply starts empty.
//!
//! ```
//! use std::sync::Arc;
//! use pa_imagecache::{CacheOptions, Fetched, FetchError, ImageCache, ImageFetcher};
//!
//! struct OneJpeg;
//! impl ImageFetcher for OneJpeg {
//!     fn fetch(&self, _url: &str) -> Result<Fetched, FetchError> {
//!         Ok(Fetched { status: 200, body: vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0] })
//!     }
//! }
//!
//! let dir = tempfile::tempdir().unwrap();
//! let cache = ImageCache::open(dir.path(), CacheOptions::default(), Arc::new(OneJpeg)).unwrap();
//! let path = cache.get("https://image.tmdb.org/t/p/w500/x.jpg", Some(300)).unwrap();
//! assert!(path.extension().is_some_and(|e| e == "jpg"));
//! assert_eq!(cache.cached("https://image.tmdb.org/t/p/w500/x.jpg", Some(300)), Some(path));
//! ```

pub mod fetch;
pub mod size;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub use fetch::{FetchError, Fetched, ImageFetcher, MAX_IMAGE_BYTES};

#[cfg(feature = "http")]
pub use fetch::UreqFetcher;

/// Default size limit: 256 MiB, about 10 000 grid posters.
pub const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Default age limit: 180 days (TMDB API terms: no cached content older
/// than six months).
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(180 * 24 * 60 * 60);

/// A hit refreshes the file's modification time at most this often, so
/// scrolling a grid does not write to the disk for every frame.
const TOUCH_INTERVAL_MS: u64 = 10 * 60 * 1000;

/// Error of a cache call.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reading or writing the cache directory failed.
    #[error("image cache I/O: {0}")]
    Io(#[from] io::Error),
    /// Neither an `http(s)` URL nor a local path.
    #[error("unsupported image URL scheme")]
    Unsupported,
    /// The server answered with a non-success status.
    #[error("image request failed with HTTP {0}")]
    Status(u16),
    /// The response is not an image we know (JPEG, PNG, WebP, GIF, AVIF).
    #[error("response is not an image")]
    NotImage,
    /// No network and no cached copy.
    #[error("image not cached and the download failed: {0}")]
    Offline(String),
}

/// Result of a cache call.
pub type Result<T> = std::result::Result<T, Error>;

/// Limits of an [`ImageCache`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheOptions {
    /// Total size of the cached files, in bytes.
    pub max_bytes: u64,
    /// Files older than this (since download) are fetched again, or dropped
    /// if that fails.
    pub max_age: Duration,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_age: DEFAULT_MAX_AGE,
        }
    }
}

/// Size of the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Bytes of all cached files.
    pub bytes: u64,
    /// Number of cached files.
    pub files: u64,
    /// The current limit.
    pub max_bytes: u64,
}

#[derive(Debug, Clone)]
struct Entry {
    file: String,
    size: u64,
    /// Download time, seconds since the epoch (part of the file name).
    fetched_s: u64,
    /// Last access, ms since the epoch.
    access_ms: u64,
    /// Last time the file's mtime was set, ms since the epoch.
    touched_ms: u64,
}

#[derive(Debug)]
struct State {
    entries: HashMap<String, Entry>,
    total: u64,
    options: CacheOptions,
}

/// The disk image cache. Cheap to share behind an `Arc`; every method may
/// be called from several threads.
pub struct ImageCache {
    dir: PathBuf,
    fetcher: Arc<dyn ImageFetcher>,
    state: Mutex<State>,
    /// One download per key at a time.
    downloads: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl std::fmt::Debug for ImageCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageCache")
            .field("dir", &self.dir)
            .field("usage", &self.usage())
            .finish_non_exhaustive()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn ms_of(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Cache key of a (variant) URL: 32 hex digits of its SHA-256.
fn key_of(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    let mut s = String::with_capacity(32);
    for b in &digest[..16] {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Parses `<key>-<fetched_s>.<ext>`.
fn parse_name(name: &str) -> Option<(String, u64)> {
    let (stem, _ext) = name.rsplit_once('.')?;
    let (key, fetched) = stem.split_once('-')?;
    if key.len() != 32 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((key.to_owned(), fetched.parse().ok()?))
}

/// File extension of a known image format, by its magic bytes.
pub fn sniff_image(body: &[u8]) -> Option<&'static str> {
    if body.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if body.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if body.len() >= 12 && &body[..4] == b"RIFF" && &body[8..12] == b"WEBP" {
        Some("webp")
    } else if body.starts_with(b"GIF87a") || body.starts_with(b"GIF89a") {
        Some("gif")
    } else if body.len() >= 12
        && &body[4..8] == b"ftyp"
        && (&body[8..12] == b"avif" || &body[8..12] == b"avis")
    {
        Some("avif")
    } else {
        None
    }
}

/// Local path of a `file://` URL or a plain absolute path; `None` for
/// anything else.
pub fn local_path(url: &str) -> Option<PathBuf> {
    if let Some(rest) = url.strip_prefix("file://") {
        // `file:///x` and `file://localhost/x`.
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        if !rest.starts_with('/') {
            return None;
        }
        let decoded = percent_decode(rest)?;
        // `file:///C:/x` on Windows.
        let b = decoded.as_bytes();
        if b.len() > 3 && b[2] == b':' && b[1].is_ascii_alphabetic() {
            return Some(PathBuf::from(&decoded[1..]));
        }
        return Some(PathBuf::from(decoded));
    }
    let b = url.as_bytes();
    let windows_drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    if url.starts_with('/') || windows_drive {
        return Some(PathBuf::from(url));
    }
    None
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn is_remote(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ImageCache {
    /// Opens (creating) the cache in `dir`: indexes the files already there,
    /// drops leftovers of interrupted downloads and expired files, and
    /// enforces the size limit.
    pub fn open(
        dir: impl Into<PathBuf>,
        options: CacheOptions,
        fetcher: Arc<dyn ImageFetcher>,
    ) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let now = now_ms();
        let max_age_s = options.max_age.as_secs();
        let mut entries: HashMap<String, Entry> = HashMap::new();
        let mut total = 0;
        for item in fs::read_dir(&dir)? {
            let item = item?;
            let name = item.file_name().to_string_lossy().into_owned();
            let meta = match item.metadata() {
                Ok(m) if m.is_file() => m,
                _ => continue,
            };
            if name.contains(".tmp") {
                let _ = fs::remove_file(item.path());
                continue;
            }
            let Some((key, fetched_s)) = parse_name(&name) else {
                continue;
            };
            if (now / 1000).saturating_sub(fetched_s) > max_age_s {
                let _ = fs::remove_file(item.path());
                continue;
            }
            let access_ms = meta.modified().map_or(fetched_s * 1000, ms_of);
            let entry = Entry {
                file: name,
                size: meta.len(),
                fetched_s,
                access_ms,
                touched_ms: access_ms,
            };
            // Two files of one key (a crash between rename and delete):
            // keep the newer.
            if let Some(old) = entries.get(&key)
                && old.fetched_s >= entry.fetched_s
            {
                let _ = fs::remove_file(dir.join(&entry.file));
                continue;
            }
            total += entry.size;
            if let Some(old) = entries.insert(key, entry) {
                total -= old.size;
                let _ = fs::remove_file(dir.join(&old.file));
            }
        }
        let cache = Self {
            dir,
            fetcher,
            state: Mutex::new(State {
                entries,
                total,
                options,
            }),
            downloads: Mutex::new(HashMap::new()),
        };
        cache.evict(&mut lock(&cache.state), None);
        Ok(cache)
    }

    /// The cache directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Local path of the image at `url`, shown `width` physical pixels wide
    /// (`None`: the stored size).
    ///
    /// Local paths and `file://` URLs come back as paths. Remote images are
    /// served from the cache or downloaded (the provider size variant for
    /// `width`, see [`size::variant_url`]). When the download fails without
    /// an HTTP response (offline), any cached size variant is served.
    pub fn get(&self, url: &str, width: Option<u32>) -> Result<PathBuf> {
        if let Some(p) = local_path(url) {
            return Ok(p);
        }
        if !is_remote(url) {
            return Err(Error::Unsupported);
        }
        let variant = size::variant_url(url, width);
        let key = key_of(&variant);
        if let Some(p) = self.hit(&key) {
            return Ok(p);
        }
        let slot = Arc::clone(lock(&self.downloads).entry(key.clone()).or_default());
        let result = {
            let _download = lock(&slot);
            match self.hit(&key) {
                Some(p) => Ok(p),
                None => self.download(&variant, &key),
            }
        };
        {
            let mut downloads = lock(&self.downloads);
            if Arc::strong_count(&slot) <= 2 {
                downloads.remove(&key);
            }
        }
        match result {
            Ok(p) => Ok(p),
            Err(e @ (Error::Offline(_) | Error::Status(500..))) => self.any_variant(url).ok_or(e),
            Err(e) => Err(e),
        }
    }

    /// Cached path for `url` at `width` without touching the network: the
    /// exact variant, else any cached size of the same image. Local paths
    /// come back as they are.
    pub fn cached(&self, url: &str, width: Option<u32>) -> Option<PathBuf> {
        if let Some(p) = local_path(url) {
            return Some(p);
        }
        if !is_remote(url) {
            return None;
        }
        self.hit(&key_of(&size::variant_url(url, width)))
            .or_else(|| self.any_variant(url))
    }

    /// Current size and limit.
    pub fn usage(&self) -> Usage {
        let s = lock(&self.state);
        Usage {
            bytes: s.total,
            files: s.entries.len() as u64,
            max_bytes: s.options.max_bytes,
        }
    }

    /// Changes the size limit and evicts down to it.
    pub fn set_max_bytes(&self, max_bytes: u64) {
        let mut s = lock(&self.state);
        s.options.max_bytes = max_bytes;
        self.evict(&mut s, None);
    }

    /// Deletes every cached file.
    pub fn clear(&self) -> Result<()> {
        let mut s = lock(&self.state);
        for (_, e) in s.entries.drain() {
            match fs::remove_file(self.dir.join(&e.file)) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
        s.total = 0;
        Ok(())
    }

    /// Path of a fresh cached file of `key`, refreshing its LRU position.
    fn hit(&self, key: &str) -> Option<PathBuf> {
        let mut s = lock(&self.state);
        let max_age_s = s.options.max_age.as_secs();
        let now = now_ms();
        let entry = s.entries.get_mut(key)?;
        let path = self.dir.join(&entry.file);
        if (now / 1000).saturating_sub(entry.fetched_s) > max_age_s || !path.is_file() {
            // Expired, or deleted behind our back (OS cache purge).
            let size = entry.size;
            s.entries.remove(key);
            s.total -= size;
            let _ = fs::remove_file(&path);
            return None;
        }
        entry.access_ms = now.max(entry.access_ms);
        if now.saturating_sub(entry.touched_ms) >= TOUCH_INTERVAL_MS {
            entry.touched_ms = now;
            let _ = fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|f| f.set_modified(SystemTime::now()));
        }
        Some(path)
    }

    fn any_variant(&self, url: &str) -> Option<PathBuf> {
        size::variants(url)
            .iter()
            .find_map(|v| self.hit(&key_of(v)))
    }

    fn download(&self, url: &str, key: &str) -> Result<PathBuf> {
        let fetched = self.fetcher.fetch(url).map_err(|e| Error::Offline(e.0))?;
        if !(200..300).contains(&fetched.status) {
            return Err(Error::Status(fetched.status));
        }
        let ext = sniff_image(&fetched.body).ok_or(Error::NotImage)?;
        self.store(key, ext, &fetched.body)
    }

    fn store(&self, key: &str, ext: &str, body: &[u8]) -> Result<PathBuf> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let now = now_ms();
        let fetched_s = now / 1000;
        let name = format!("{key}-{fetched_s}.{ext}");
        let tmp = self.dir.join(format!(
            "{key}.tmp{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let path = self.dir.join(&name);
        // The directory may have been purged by the OS since `open`.
        fs::create_dir_all(&self.dir)?;
        fs::write(&tmp, body)
            .and_then(|()| fs::rename(&tmp, &path))
            .inspect_err(|_| {
                let _ = fs::remove_file(&tmp);
            })?;
        let mut s = lock(&self.state);
        let entry = Entry {
            file: name,
            size: body.len() as u64,
            fetched_s,
            access_ms: now,
            touched_ms: now,
        };
        s.total += entry.size;
        if let Some(old) = s.entries.insert(key.to_owned(), entry) {
            s.total -= old.size;
            if self.dir.join(&old.file) != path {
                let _ = fs::remove_file(self.dir.join(&old.file));
            }
        }
        self.evict(&mut s, Some(key));
        Ok(path)
    }

    /// Drops least recently used files until the total is at most 90 % of
    /// the limit. `keep` (the file just stored) is never dropped.
    fn evict(&self, s: &mut State, keep: Option<&str>) {
        if s.total <= s.options.max_bytes {
            return;
        }
        let target = s.options.max_bytes / 10 * 9;
        let mut order: Vec<(u64, String)> = s
            .entries
            .iter()
            .filter(|(k, _)| Some(k.as_str()) != keep)
            .map(|(k, e)| (e.access_ms, k.clone()))
            .collect();
        order.sort_unstable();
        for (_, key) in order {
            if s.total <= target {
                break;
            }
            if let Some(e) = s.entries.remove(&key) {
                s.total -= e.size;
                let _ = fs::remove_file(self.dir.join(&e.file));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_formats() {
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xDB]), Some("jpg"));
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_image(b"GIF89a.."), Some("gif"));
        assert_eq!(sniff_image(b"\0\0\0\x1cftypavif\0\0"), Some("avif"));
        assert_eq!(sniff_image(b"<!doctype html>"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[test]
    fn local_paths() {
        assert_eq!(
            local_path("file:///media/My%20Movie/poster.jpg"),
            Some(PathBuf::from("/media/My Movie/poster.jpg"))
        );
        assert_eq!(
            local_path("file://localhost/a.png"),
            Some(PathBuf::from("/a.png"))
        );
        assert_eq!(
            local_path("file:///C:/m/p.jpg"),
            Some(PathBuf::from("C:/m/p.jpg"))
        );
        assert_eq!(local_path("/m/p.jpg"), Some(PathBuf::from("/m/p.jpg")));
        assert_eq!(
            local_path("C:\\m\\p.jpg"),
            Some(PathBuf::from("C:\\m\\p.jpg"))
        );
        assert_eq!(local_path("https://x/p.jpg"), None);
        assert_eq!(local_path("file://host/p.jpg"), None);
        assert_eq!(local_path("file:///bad%zz"), None);
    }

    #[test]
    fn names_round_trip() {
        let key = key_of("https://example.org/a.jpg");
        assert_eq!(key.len(), 32);
        assert_eq!(
            parse_name(&format!("{key}-1700000000.jpg")),
            Some((key.clone(), 1_700_000_000))
        );
        assert_eq!(parse_name(&format!("{key}.tmp1-2")), None);
        assert_eq!(parse_name("README"), None);
    }
}
