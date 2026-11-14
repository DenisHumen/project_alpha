// SPDX-License-Identifier: MPL-2.0
//! The media library on the Rust core (`pa-catalog`, UI-024) for Dart.
//!
//! [`CoreLibrary`] wraps one library database: sources, the scan pipeline
//! (listing, music tags, fingerprints, recognition, search index), item
//! views, progress with "Continue watching" and Up Next, provider metadata
//! (TMDB, AniList, Shikimori: titles, overviews, artwork; META-013) and the
//! import of a `pa_library` v0 database. The Dart package `pa_library` adapts it to
//! the app's `Library` interface.
//!
//! Every method blocks on SQLite or the file system, so none is
//! `#[frb(sync)]`: Dart awaits them off the UI thread. Queries run while a
//! scan is in progress (the scan has its own connection).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use flutter_rust_bridge::frb;
use pa_catalog as c;

use crate::frb_generated::StreamSink;

/// Error of a library call; Dart receives it as an exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryError {
    /// Human-readable message (no secrets).
    pub message: String,
}

impl From<c::Error> for LibraryError {
    fn from(e: c::Error) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

type Result<T> = std::result::Result<T, LibraryError>;

/// A media source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSource {
    pub id: i64,
    /// `local` or `smb`.
    pub kind: String,
    pub name: String,
    /// `file:///…` or `smb://host/share/dir`, without credentials.
    pub uri: String,
    pub username: Option<String>,
    /// Local path of a `file://` root.
    pub local_root: Option<String>,
    /// Followed by the change watcher ([`CoreLibrary::start_watcher`]).
    pub auto_scan: bool,
}

impl From<c::Source> for CoreSource {
    fn from(s: c::Source) -> Self {
        Self {
            id: s.id,
            kind: s.kind,
            name: s.name,
            uri: s.uri,
            username: s.username,
            local_root: s.local_root.map(|p| p.to_string_lossy().into_owned()),
            auto_scan: s.auto_scan,
        }
    }
}

/// Kind of a library item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreItemKind {
    Movie,
    /// A show or an anime series (`show` is a Dart keyword).
    Series,
    Episode,
    Track,
}

impl From<c::ItemKind> for CoreItemKind {
    fn from(k: c::ItemKind) -> Self {
        match k {
            c::ItemKind::Movie => Self::Movie,
            c::ItemKind::Show => Self::Series,
            c::ItemKind::Episode => Self::Episode,
            c::ItemKind::Track => Self::Track,
        }
    }
}

impl From<CoreItemKind> for c::ItemKind {
    fn from(k: CoreItemKind) -> Self {
        match k {
            CoreItemKind::Movie => Self::Movie,
            CoreItemKind::Series => Self::Show,
            CoreItemKind::Episode => Self::Episode,
            CoreItemKind::Track => Self::Track,
        }
    }
}

/// A movie, show, episode or track.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreItem {
    /// `m<work>`, `s<work>`, `e<episode>` or `t<track>`.
    pub id: String,
    pub kind: CoreItemKind,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<i32>,
    pub overview: Option<String>,
    /// Episodes: id of the show item.
    pub show_id: Option<String>,
    pub season: Option<i32>,
    pub episode: Option<i32>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<i32>,
    pub source_id: i64,
    /// Path relative to the source root (shows: the show folder).
    pub rel_path: String,
    /// Absolute path for local sources.
    pub local_path: Option<String>,
    pub duration_ms: Option<i64>,
    pub match_confidence: Option<f64>,
    pub tmdb_id: Option<i64>,
    pub added_at_ms: Option<i64>,
    /// Poster URL (https or `file://`); episodes: the show's.
    pub poster_url: Option<String>,
    /// Backdrop URL; episodes: the episode still.
    pub backdrop_url: Option<String>,
}

fn small(v: Option<i64>) -> Option<i32> {
    v.and_then(|v| i32::try_from(v).ok())
}

impl From<c::Item> for CoreItem {
    fn from(i: c::Item) -> Self {
        Self {
            id: i.id.to_string(),
            kind: i.kind.into(),
            title: i.title,
            original_title: i.original_title,
            year: small(i.year),
            overview: i.overview,
            show_id: i.show_id.map(|s| s.to_string()),
            season: small(i.season),
            episode: small(i.episode),
            artist: i.artist,
            album: i.album,
            track_number: small(i.track_number),
            source_id: i.source_id,
            rel_path: i.rel_path,
            local_path: i.local_path,
            duration_ms: i.duration_ms,
            match_confidence: i.match_confidence,
            tmdb_id: i.tmdb_id,
            added_at_ms: i.added_at_ms,
            poster_url: i.poster_url,
            backdrop_url: i.backdrop_url,
        }
    }
}

/// Playback state of an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreProgress {
    pub item_id: String,
    pub position_ms: i64,
    /// 0 if unknown.
    pub duration_ms: i64,
    pub updated_at_ms: i64,
    pub watched: bool,
}

impl From<c::ItemProgress> for CoreProgress {
    fn from(p: c::ItemProgress) -> Self {
        Self {
            item_id: p.item.to_string(),
            position_ms: p.position_ms,
            duration_ms: p.duration_ms,
            updated_at_ms: p.updated_at_ms,
            watched: p.watched,
        }
    }
}

/// An entry of "Continue watching".
#[derive(Debug, Clone, PartialEq)]
pub struct CoreContinueItem {
    pub item: CoreItem,
    pub progress: CoreProgress,
    /// Up Next: the episode after a finished one.
    pub up_next: bool,
}

/// Stage of a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreScanPhase {
    Connecting,
    Scanning,
    /// Music tags, fingerprints, recognition.
    Analyzing,
    Done,
}

/// Progress of [`CoreLibrary::scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreScanUpdate {
    pub phase: CoreScanPhase,
    pub source_id: Option<i64>,
    pub files_found: i64,
    pub processed: i64,
    pub total: Option<i64>,
    pub current_path: Option<String>,
    pub error: Option<String>,
}

fn signed(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

impl From<c::ScanUpdate> for CoreScanUpdate {
    fn from(u: c::ScanUpdate) -> Self {
        Self {
            phase: match u.phase {
                c::ScanPhase::Connecting => CoreScanPhase::Connecting,
                c::ScanPhase::Scanning => CoreScanPhase::Scanning,
                c::ScanPhase::Analyzing => CoreScanPhase::Analyzing,
                c::ScanPhase::Done => CoreScanPhase::Done,
            },
            source_id: u.source_id,
            files_found: signed(u.files_found),
            processed: signed(u.processed),
            total: u.total.map(signed),
            current_path: u.current_path,
            error: u.error,
        }
    }
}

/// Local path of a source that is not `file://` (a mounted share).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSourceRoot {
    pub source_id: i64,
    pub path: String,
}

/// What [`CoreLibrary::import_v0`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreImportReport {
    pub sources: u32,
    pub files: u32,
    pub matched: u32,
    pub progress: u32,
    pub progress_skipped: u32,
    pub items_skipped: u32,
}

fn count(v: usize) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

impl From<c::ImportReport> for CoreImportReport {
    fn from(r: c::ImportReport) -> Self {
        Self {
            sources: count(r.sources),
            files: count(r.files),
            matched: count(r.matched),
            progress: count(r.progress),
            progress_skipped: count(r.progress_skipped),
            items_skipped: count(r.items_skipped),
        }
    }
}

fn item_ref(id: &str) -> Result<c::ItemRef> {
    Ok(id.parse::<c::ItemRef>()?)
}

fn items(v: Vec<c::Item>) -> Vec<CoreItem> {
    v.into_iter().map(Into::into).collect()
}

/// `true` if `path` is a database of the Dart `pa_library` v0.
pub fn is_legacy_v0_library(path: String) -> Result<bool> {
    Ok(c::is_legacy_v0(path)?)
}

/// Settings of a metadata run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMetadataSettings {
    /// The user's own TMDB key (v3 key or v4 token); `None` uses the key
    /// built into the app, if any. Without any key only anime is matched
    /// (AniList, Shikimori).
    pub tmdb_api_key: Option<String>,
    /// Metadata language, e.g. `ru-RU` or `en-US`.
    pub language: String,
    /// Directory of the provider response cache (`provider-cache.sqlite`);
    /// disposable, so the platform cache directory.
    pub cache_dir: String,
}

/// Progress of [`CoreLibrary::fetch_metadata`]; the last update has
/// `done` set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreMetadataUpdate {
    pub processed: u32,
    pub total: u32,
    pub done: bool,
    pub matched: u32,
    pub not_found: u32,
    pub failed: u32,
    /// Why the run stopped early (rejected key, offline), or a failure.
    pub error: Option<String>,
}

/// Settings of the change watcher ([`CoreLibrary::start_watcher`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreWatchSettings {
    /// Local paths of mounted network shares.
    pub roots: Vec<CoreSourceRoot>,
    /// Rescan interval of network shares, seconds; 0: no timer.
    pub network_interval_secs: u32,
    /// Quiet time after a change before the rescan, milliseconds.
    pub debounce_ms: u32,
    /// Scan every followed source when the watcher starts.
    pub scan_on_start: bool,
}

/// A rescan done by the change watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreWatchEvent {
    pub source_id: i64,
    /// The whole source was scanned (start, timer, many changes).
    pub full: bool,
    /// Rescanned folders, relative to the source root.
    pub folders: Vec<String>,
    pub added: i64,
    pub changed: i64,
    pub removed: i64,
    /// The library may have changed: refresh the views.
    pub may_have_changed: bool,
    pub error: Option<String>,
}

impl From<c::watch::WatchEvent> for CoreWatchEvent {
    fn from(e: c::watch::WatchEvent) -> Self {
        Self {
            source_id: e.source_id,
            full: e.full,
            may_have_changed: e.may_have_changed(),
            folders: e.folders,
            added: signed(e.report.added + e.report.restored),
            changed: signed(e.report.changed),
            removed: signed(e.report.removed),
            error: e.error,
        }
    }
}

/// One library database.
#[frb(opaque)]
pub struct CoreLibrary {
    catalog: Arc<c::Catalog>,
    cancel: Mutex<Option<c::CancelToken>>,
    watcher: Mutex<Option<c::watch::LibraryWatcher>>,
}

impl CoreLibrary {
    /// Opens (creating and migrating) the database file at `path`. A
    /// `pa_library` v0 file is refused: open a new file and import it.
    pub fn open(path: String) -> Result<CoreLibrary> {
        Ok(Self {
            catalog: Arc::new(c::Catalog::open(path)?),
            cancel: Mutex::new(None),
            watcher: Mutex::new(None),
        })
    }

    pub fn sources(&self) -> Result<Vec<CoreSource>> {
        Ok(self
            .catalog
            .sources()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Adds a source (`id` = None) or updates one. `kind` is `local` with an
    /// absolute folder path, or `smb` with an `smb://` URI.
    pub fn add_source(
        &self,
        id: Option<i64>,
        kind: String,
        name: String,
        location: String,
        username: Option<String>,
    ) -> Result<CoreSource> {
        Ok(self
            .catalog
            .add_source(id, &kind, &name, &location, username.as_deref())?
            .into())
    }

    /// Removes a source with everything found in it.
    pub fn remove_source(&self, id: i64) -> Result<()> {
        Ok(self.catalog.remove_source(id)?)
    }

    /// Scans one source or all of them. The stream ends with a `Done`
    /// update; errors come as updates with `error` set.
    pub fn scan(
        &self,
        source_id: Option<i64>,
        roots: Vec<CoreSourceRoot>,
        sink: StreamSink<CoreScanUpdate>,
    ) {
        let token = c::CancelToken::new();
        *self.cancel.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        let roots: HashMap<i64, PathBuf> = roots
            .into_iter()
            .map(|r| (r.source_id, PathBuf::from(r.path)))
            .collect();
        let mut done = false;
        let result = self.catalog.scan(source_id, &roots, &token, &mut |u| {
            done |= u.phase == c::ScanPhase::Done;
            // A closed stream (Dart cancelled) keeps the scan going: the
            // library still needs the result.
            let _ = sink.add(u.into());
        });
        if let Err(e) = result {
            let _ = sink.add(CoreScanUpdate {
                phase: CoreScanPhase::Done,
                source_id,
                files_found: 0,
                processed: 0,
                total: None,
                current_path: None,
                error: Some(e.to_string()),
            });
        } else if !done {
            let _ = sink.add(CoreScanUpdate {
                phase: CoreScanPhase::Done,
                source_id,
                files_found: 0,
                processed: 0,
                total: None,
                current_path: None,
                error: None,
            });
        }
    }

    /// Starts the change watcher (LIB-005), replacing a running one: local
    /// auto-scan sources are watched with OS notifications and rescanned
    /// incrementally, mounted network shares on a timer. Every rescan comes
    /// as an event; the stream stays open until
    /// [`stop_watcher`](Self::stop_watcher).
    pub fn start_watcher(
        &self,
        settings: CoreWatchSettings,
        sink: StreamSink<CoreWatchEvent>,
    ) -> Result<()> {
        // The old watcher (and its stream) goes first.
        self.stop_watcher();
        let roots: HashMap<i64, PathBuf> = settings
            .roots
            .into_iter()
            .map(|r| (r.source_id, PathBuf::from(r.path)))
            .collect();
        let options = c::watch::WatchOptions {
            network_interval: (settings.network_interval_secs > 0)
                .then(|| std::time::Duration::from_secs(u64::from(settings.network_interval_secs))),
            debounce: std::time::Duration::from_millis(u64::from(settings.debounce_ms)),
            scan_on_start: settings.scan_on_start,
            ..c::watch::WatchOptions::default()
        };
        let watcher = c::watch::LibraryWatcher::start(
            Arc::clone(&self.catalog),
            &roots,
            options,
            Box::new(move |e| {
                let _ = sink.add(e.into());
            }),
        )?;
        *self.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
        Ok(())
    }

    /// Stops the change watcher (a running rescan is cancelled) and closes
    /// its stream. Does nothing if none runs.
    pub fn stop_watcher(&self) {
        let old = self
            .watcher
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(w) = old {
            w.stop();
        }
    }

    /// Stops the running scan (it keeps what it wrote).
    pub fn cancel_scan(&self) {
        if let Some(t) = self
            .cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            t.cancel();
        }
    }

    pub fn movies(&self) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.movies()?))
    }

    pub fn shows(&self) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.shows()?))
    }

    /// Episodes of a show item.
    pub fn episodes(&self, show_id: String) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.episodes(item_ref(&show_id)?)?))
    }

    pub fn tracks(&self) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.tracks()?))
    }

    /// Items of `kind`, newest first.
    pub fn recently_added(&self, kind: CoreItemKind, limit: u32) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.recently_added(
            kind.into(),
            usize::try_from(limit).unwrap_or(usize::MAX),
        )?))
    }

    /// One item; `None` if unknown or its file is missing.
    pub fn item(&self, id: String) -> Result<Option<CoreItem>> {
        let Ok(id) = id.parse::<c::ItemRef>() else {
            return Ok(None);
        };
        Ok(self.catalog.item(id)?.map(Into::into))
    }

    /// FTS5 search over movies, shows, episodes and tracks.
    pub fn search(&self, query: String, limit: u32) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.search(
            &query,
            usize::try_from(limit).unwrap_or(usize::MAX),
        )?))
    }

    pub fn progress(&self, item_id: String) -> Result<Option<CoreProgress>> {
        let Ok(id) = item_id.parse::<c::ItemRef>() else {
            return Ok(None);
        };
        Ok(self.catalog.progress(id)?.map(Into::into))
    }

    /// Stores a player position report. `false`: the item has no stable key
    /// yet and nothing was stored.
    pub fn save_progress(
        &self,
        item_id: String,
        position_ms: i64,
        duration_ms: i64,
        updated_at_ms: i64,
        watched: bool,
    ) -> Result<bool> {
        Ok(self.catalog.save_progress(
            item_ref(&item_id)?,
            position_ms,
            duration_ms,
            updated_at_ms,
            watched,
        )?)
    }

    /// "Continue watching" with Up Next, latest first.
    pub fn continue_watching(&self, limit: u32) -> Result<Vec<CoreContinueItem>> {
        Ok(self
            .catalog
            .continue_watching(usize::try_from(limit).unwrap_or(usize::MAX))?
            .into_iter()
            .map(|e| CoreContinueItem {
                item: e.item.into(),
                progress: e.progress.into(),
                up_next: e.up_next,
            })
            .collect())
    }

    /// Manual fix of an item's work (an episode: its show).
    pub fn fix_match(
        &self,
        item_id: String,
        title: Option<String>,
        year: Option<i32>,
        tmdb_id: Option<i64>,
    ) -> Result<()> {
        Ok(self.catalog.fix_match(
            item_ref(&item_id)?,
            title.as_deref(),
            year.map(i64::from),
            tmdb_id,
        )?)
    }

    /// Fetches provider metadata for the works that need it (new, changed,
    /// stale or previously failed). Run it after [`scan`](Self::scan);
    /// [`cancel_scan`](Self::cancel_scan) stops it too. The stream ends
    /// with a `done` update.
    pub fn fetch_metadata(
        &self,
        settings: CoreMetadataSettings,
        sink: StreamSink<CoreMetadataUpdate>,
    ) {
        let token = c::CancelToken::new();
        *self.cancel.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        let mut last = CoreMetadataUpdate {
            processed: 0,
            total: 0,
            done: true,
            matched: 0,
            not_found: 0,
            failed: 0,
            error: None,
        };
        let cache_dir = PathBuf::from(&settings.cache_dir);
        let cache = std::fs::create_dir_all(&cache_dir)
            .map_err(|e| e.to_string())
            .and_then(|()| {
                c::pa_metadata::cache::SqliteCache::open(&cache_dir.join("provider-cache.sqlite"))
                    .map_err(|e| e.to_string())
            });
        let cache = match cache {
            Ok(cache) => cache,
            Err(e) => {
                last.error = Some(format!("provider cache: {e}"));
                let _ = sink.add(last);
                return;
            }
        };
        let key = settings
            .tmdb_api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty());
        let providers = c::MetadataProviders::live(Arc::new(cache), key, &settings.language);
        if let Some(tmdb) = &providers.tmdb {
            // TMDB terms: nothing cached for longer than six months.
            tmdb.purge_expired();
        }
        let result = self.catalog.fetch_metadata(
            &providers,
            &c::MetadataOptions::default(),
            &token,
            &mut |done, total| {
                let _ = sink.add(CoreMetadataUpdate {
                    processed: count(done),
                    total: count(total),
                    done: false,
                    matched: 0,
                    not_found: 0,
                    failed: 0,
                    error: None,
                });
            },
        );
        match result {
            Ok(r) => {
                last.processed = count(r.matched + r.not_found + r.failed);
                last.total = count(r.works);
                last.matched = count(r.matched);
                last.not_found = count(r.not_found);
                last.failed = count(r.failed);
                last.error = r.error;
            }
            Err(e) => last.error = Some(e.to_string()),
        }
        let _ = sink.add(last);
    }

    /// Makes the next metadata run ask again: for every work (`all`, e.g.
    /// after a language change) or only for works no provider matched
    /// (e.g. after a new TMDB key).
    pub fn reset_metadata(&self, all: bool) -> Result<()> {
        Ok(self.catalog.reset_metadata(all)?)
    }

    /// Imports a `pa_library` v0 database (read-only; renaming it is the
    /// caller's job). Safe to repeat.
    pub fn import_v0(&self, path: String) -> Result<CoreImportReport> {
        Ok(self.catalog.import_v0(Path::new(&path))?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_lists_an_empty_library() {
        let dir = tempfile::tempdir().unwrap();
        let lib = CoreLibrary::open(dir.path().join("l.db").to_string_lossy().into()).unwrap();
        assert!(lib.sources().unwrap().is_empty());
        assert!(lib.movies().unwrap().is_empty());
        assert!(lib.item("m1".into()).unwrap().is_none());
        assert!(lib.item("nonsense".into()).unwrap().is_none());
        let err = lib.episodes("m1".into()).unwrap_err();
        assert!(err.message.contains("not a show"), "{err:?}");
        let src = lib
            .add_source(None, "local".into(), "M".into(), "/srv/media".into(), None)
            .unwrap();
        assert_eq!(src.local_root.as_deref(), Some("/srv/media"));
        assert!(
            lib.add_source(None, "local".into(), "M".into(), "rel".into(), None)
                .is_err()
        );
    }
}
