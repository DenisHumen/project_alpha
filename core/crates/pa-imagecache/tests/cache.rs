// SPDX-License-Identifier: MPL-2.0
//! Behaviour of the disk image cache with a scripted fetcher (no network).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pa_imagecache::{CacheOptions, Error, FetchError, Fetched, ImageCache, ImageFetcher};

const POSTER: &str = "https://image.tmdb.org/t/p/w500/poster.jpg";

fn jpeg(len: usize) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0];
    v.resize(len.max(4), 0x11);
    v
}

/// Serves `len`-byte JPEGs for every URL, counts requests per URL, can go
/// offline and can answer with custom bodies.
#[derive(Default)]
struct Fake {
    offline: AtomicBool,
    len: AtomicUsize,
    calls: Mutex<HashMap<String, usize>>,
    bodies: Mutex<HashMap<String, (u16, Vec<u8>)>>,
}

impl Fake {
    fn new(len: usize) -> Arc<Self> {
        let f = Arc::new(Self::default());
        f.len.store(len, Ordering::SeqCst);
        f
    }
    fn calls(&self, url: &str) -> usize {
        self.calls.lock().unwrap().get(url).copied().unwrap_or(0)
    }
    fn total_calls(&self) -> usize {
        self.calls.lock().unwrap().values().sum()
    }
}

impl ImageFetcher for Fake {
    fn fetch(&self, url: &str) -> Result<Fetched, FetchError> {
        *self.calls.lock().unwrap().entry(url.to_owned()).or_default() += 1;
        if self.offline.load(Ordering::SeqCst) {
            return Err(FetchError("host not found".into()));
        }
        if let Some((status, body)) = self.bodies.lock().unwrap().get(url) {
            return Ok(Fetched {
                status: *status,
                body: body.clone(),
            });
        }
        Ok(Fetched {
            status: 200,
            body: jpeg(self.len.load(Ordering::SeqCst)),
        })
    }
}

fn options(max_bytes: u64) -> CacheOptions {
    CacheOptions {
        max_bytes,
        ..CacheOptions::default()
    }
}

#[test]
fn downloads_once_and_serves_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(1000);
    let cache = ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap();
    let a = cache.get(POSTER, None).unwrap();
    let b = cache.get(POSTER, None).unwrap();
    assert_eq!(a, b);
    assert!(a.starts_with(dir.path()));
    assert_eq!(std::fs::read(&a).unwrap(), jpeg(1000));
    assert_eq!(fake.calls(POSTER), 1);
    assert_eq!(cache.usage().files, 1);
    assert_eq!(cache.usage().bytes, 1000);
}

#[test]
fn asks_tmdb_for_the_size_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(100);
    let cache = ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap();
    cache.get(POSTER, Some(240)).unwrap();
    assert_eq!(fake.calls("https://image.tmdb.org/t/p/w342/poster.jpg"), 1);
    assert_eq!(fake.calls(POSTER), 0);
}

#[test]
fn size_limit_evicts_least_recently_used() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(1000);
    let cache = ImageCache::open(dir.path(), options(5000), fake.clone()).unwrap();
    let url = |i: usize| format!("https://example.org/{i}.jpg");
    for i in 0..5 {
        cache.get(&url(i), None).unwrap();
        // Distinct access times.
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(cache.usage().bytes, 5000);
    // Use 0 again: 1 is now the least recently used.
    cache.get(&url(0), None).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    cache.get(&url(5), None).unwrap();
    let usage = cache.usage();
    assert!(usage.bytes <= 5000, "{usage:?}");
    // Down to 90 % of the limit: two files went (1 and 2).
    assert_eq!(usage.files, 4);
    assert!(cache.cached(&url(0), None).is_some());
    assert!(cache.cached(&url(1), None).is_none());
    assert!(cache.cached(&url(2), None).is_none());
    assert!(cache.cached(&url(5), None).is_some());
    let on_disk = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(on_disk, 4);
}

#[test]
fn limit_holds_for_many_images() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(700);
    let cache = ImageCache::open(dir.path(), options(50_000), fake.clone()).unwrap();
    for i in 0..1000 {
        cache
            .get(&format!("https://example.org/p{i}.jpg"), None)
            .unwrap();
        assert!(cache.usage().bytes <= 50_000);
    }
    let disk: u64 = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert_eq!(disk, cache.usage().bytes);
    // Lowering the limit evicts at once.
    cache.set_max_bytes(10_000);
    assert!(cache.usage().bytes <= 10_000);
}

#[test]
fn offline_serves_cached_copies_of_any_size() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(100);
    let cache = ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap();
    let full = cache.get(POSTER, None).unwrap();
    fake.offline.store(true, Ordering::SeqCst);
    // A smaller variant was never downloaded: the full size stands in.
    assert_eq!(cache.get(POSTER, Some(150)).unwrap(), full);
    assert_eq!(cache.cached(POSTER, Some(150)), Some(full));
    // Nothing cached at all: an offline error.
    let err = cache.get("https://example.org/none.jpg", None).unwrap_err();
    assert!(matches!(err, Error::Offline(_)), "{err:?}");
    assert_eq!(cache.cached("https://example.org/none.jpg", None), None);
}

#[test]
fn survives_restart_and_purge() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(100);
    let path = {
        let cache = ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap();
        cache.get(POSTER, None).unwrap()
    };
    // A leftover of an interrupted download is removed on open.
    std::fs::write(dir.path().join("0123.tmp1-1"), b"x").unwrap();
    let cache = ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap();
    assert_eq!(cache.cached(POSTER, None), Some(path.clone()));
    assert!(!dir.path().join("0123.tmp1-1").exists());
    assert_eq!(fake.calls(POSTER), 1);
    // The OS purged the cache directory (tvOS Caches): downloaded again.
    std::fs::remove_dir_all(dir.path()).unwrap();
    assert_eq!(cache.cached(POSTER, None), None);
    let again = cache.get(POSTER, None).unwrap();
    assert!(again.is_file());
    assert_eq!(fake.calls(POSTER), 2);
    assert_eq!(cache.usage().files, 1);
}

#[test]
fn expired_files_are_fetched_again() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(100);
    let opts = CacheOptions {
        max_bytes: 1 << 20,
        max_age: Duration::ZERO,
    };
    let cache = ImageCache::open(dir.path(), opts, fake.clone()).unwrap();
    cache.get(POSTER, None).unwrap();
    // Age is counted in whole seconds.
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(cache.cached(POSTER, None), None);
    cache.get(POSTER, None).unwrap();
    assert_eq!(fake.calls(POSTER), 2);
    // Expired and offline: nothing is served.
    std::thread::sleep(Duration::from_millis(1100));
    fake.offline.store(true, Ordering::SeqCst);
    assert!(cache.get(POSTER, None).is_err());
    drop(cache);
    // Expired files go on open.
    let cache = ImageCache::open(dir.path(), opts, fake.clone()).unwrap();
    assert_eq!(cache.usage().files, 0);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn rejects_errors_and_non_images() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(100);
    let html = "https://example.org/page.jpg";
    let gone = "https://example.org/gone.jpg";
    fake.bodies
        .lock()
        .unwrap()
        .insert(html.into(), (200, b"<html>blocked</html>".to_vec()));
    fake.bodies
        .lock()
        .unwrap()
        .insert(gone.into(), (404, b"not found".to_vec()));
    let cache = ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap();
    assert!(matches!(cache.get(html, None), Err(Error::NotImage)));
    assert!(matches!(cache.get(gone, None), Err(Error::Status(404))));
    assert!(matches!(
        cache.get("ftp://example.org/x.jpg", None),
        Err(Error::Unsupported)
    ));
    assert_eq!(cache.usage().files, 0);
}

#[test]
fn local_art_is_not_copied() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(100);
    let cache = ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap();
    assert_eq!(
        cache.get("/media/Movie (2020)/poster.jpg", Some(200)).unwrap(),
        std::path::PathBuf::from("/media/Movie (2020)/poster.jpg")
    );
    assert_eq!(
        cache.cached("file:///media/a%20b/fanart.jpg", None),
        Some(std::path::PathBuf::from("/media/a b/fanart.jpg"))
    );
    assert_eq!(fake.total_calls(), 0);
    assert_eq!(cache.usage().files, 0);
}

#[test]
fn concurrent_requests_download_once() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(100);
    let cache = Arc::new(ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || cache.get(POSTER, None).unwrap())
        })
        .collect();
    let paths: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(paths.windows(2).all(|w| w[0] == w[1]));
    assert_eq!(fake.calls(POSTER), 1);
}

#[test]
fn clear_removes_everything() {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::new(100);
    let cache = ImageCache::open(dir.path(), options(1 << 20), fake.clone()).unwrap();
    cache.get(POSTER, None).unwrap();
    cache.get("https://example.org/x.png", None).unwrap();
    cache.clear().unwrap();
    assert_eq!(cache.usage().bytes, 0);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
