// SPDX-License-Identifier: MPL-2.0
//! Disk image cache (`pa-imagecache`, LIB-013) for Dart.
//!
//! Posters, backdrops and stills are downloaded by the core over its HTTPS
//! stack and handed to Flutter as local file paths; Flutter decodes them at
//! display size. [`CoreImageCache::resolve`] may download, so Dart awaits it
//! off the UI thread; [`CoreImageCache::cached`] only looks up the index and
//! is synchronous, for images already on disk.

use std::sync::Arc;

use flutter_rust_bridge::frb;
use pa_imagecache as ic;

/// `User-Agent` of image downloads.
const USER_AGENT: &str = concat!(
    "Kinolune/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/DenisHumen/project_alpha)"
);

/// Error of an image cache call; Dart receives it as an exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageCacheError {
    /// Human-readable message (no URLs, no secrets).
    pub message: String,
    /// No network and no cached copy: the UI shows its placeholder and may
    /// try again later.
    pub offline: bool,
}

impl From<ic::Error> for ImageCacheError {
    fn from(e: ic::Error) -> Self {
        Self {
            offline: matches!(e, ic::Error::Offline(_)),
            message: e.to_string(),
        }
    }
}

/// Size of the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreImageCacheUsage {
    pub bytes: u64,
    pub files: u64,
    pub max_bytes: u64,
}

impl From<ic::Usage> for CoreImageCacheUsage {
    fn from(u: ic::Usage) -> Self {
        Self {
            bytes: u.bytes,
            files: u.files,
            max_bytes: u.max_bytes,
        }
    }
}

/// The disk image cache of the app.
#[frb(opaque)]
pub struct CoreImageCache {
    inner: Arc<ic::ImageCache>,
}

impl CoreImageCache {
    /// Opens the cache in `dir` (the platform cache directory, e.g.
    /// `<cache>/artwork`) with a size limit of `max_bytes`.
    pub fn open(dir: String, max_bytes: u64) -> Result<CoreImageCache, ImageCacheError> {
        let options = ic::CacheOptions {
            max_bytes,
            ..ic::CacheOptions::default()
        };
        let fetcher = Arc::new(ic::UreqFetcher::new(USER_AGENT));
        Ok(Self {
            inner: Arc::new(ic::ImageCache::open(dir, options, fetcher)?),
        })
    }

    /// Local path of the image at `url` (https, `file://` or a local path)
    /// for `width` physical pixels on screen; downloads it if needed.
    pub fn resolve(&self, url: String, width: Option<u32>) -> Result<String, ImageCacheError> {
        let path = self.inner.get(&url, width)?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Local path of `url` if it is already on disk (or local), without
    /// touching the network.
    #[frb(sync)]
    pub fn cached(&self, url: String, width: Option<u32>) -> Option<String> {
        self.inner
            .cached(&url, width)
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// Current size and limit.
    #[frb(sync)]
    pub fn usage(&self) -> CoreImageCacheUsage {
        self.inner.usage().into()
    }

    /// Changes the size limit (evicts at once).
    pub fn set_max_bytes(&self, max_bytes: u64) {
        self.inner.set_max_bytes(max_bytes);
    }

    /// Deletes every cached image.
    pub fn clear(&self) -> Result<(), ImageCacheError> {
        Ok(self.inner.clear()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_paths_pass_through_and_usage_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cache =
            CoreImageCache::open(dir.path().join("artwork").to_string_lossy().into(), 1 << 20)
                .unwrap();
        assert_eq!(
            cache.cached("/m/poster.jpg".into(), Some(100)),
            Some("/m/poster.jpg".into())
        );
        assert_eq!(cache.cached("https://example.org/x.jpg".into(), None), None);
        assert_eq!(cache.usage().files, 0);
        assert_eq!(cache.usage().max_bytes, 1 << 20);
        let err = cache
            .resolve("ftp://example.org/x.jpg".into(), None)
            .unwrap_err();
        assert!(!err.offline);
        cache.set_max_bytes(10);
        assert_eq!(cache.usage().max_bytes, 10);
        cache.clear().unwrap();
    }
}
