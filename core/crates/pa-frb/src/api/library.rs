// SPDX-License-Identifier: MPL-2.0
//! The media library on the Rust core (`pa-catalog`, UI-024) for Dart.
//!
//! [`CoreLibrary`] wraps one library database: sources, the scan pipeline
//! (listing, music tags, fingerprints, recognition, search index), item
//! views, progress with "Continue watching" and Up Next, provider metadata
//! (TMDB, AniList, Shikimori: titles, overviews, artwork; META-013), library
//! queries (filters, sorts, alphabet index, facets, folder browsing, view
//! preferences, smart shelves; LIB-009, types in
//! [`library_query`](crate::api::library_query)) and the import of a
//! `pa_library` v0 database. The Dart package `pa_library` adapts it to
//! the app's `Library` interface.
//!
//! Network sources (SMB, WebDAV) are scanned and played through `pa-vfs`
//! (LIB-018): [`CoreLibrary::connect_sources`] lets the catalog reach them
//! with the passwords of the source manager's secret store, and
//! [`CoreLibrary::stream_base_url`] starts the loopback HTTP proxy
//! (`pa-proxy`) that serves their files to the player.
//!
//! Every method blocks on SQLite or the file system, so none is
//! `#[frb(sync)]`: Dart awaits them off the UI thread. Queries run while a
//! scan is in progress (the scan has its own connection).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use flutter_rust_bridge::frb;
use pa_catalog as c;
use pa_catalog::pa_sync::lan::service::SyncService;

use crate::api::library_lists::{CoreCollection, CorePlaylist, CoreVersion};
use crate::api::music::CoreMusicTrack;
use crate::api::library_query::{
    CoreAlphabetEntry, CoreFacets, CoreFolderEntry, CoreItemPage, CoreLibraryQuery,
    CoreLibrarySort, CoreSmartShelf, CoreViewPrefs,
};
use crate::api::subtitles::CoreSidecarSubtitle;
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
    /// `local`, `smb` or `webdav`.
    pub kind: String,
    pub name: String,
    /// `file:///…`, `smb://host/share/dir` or `https://…`, without credentials.
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
    /// Local poster or album cover next to the files, relative to the
    /// source root like `rel_path` (LIB-007 groups; episodes: the show's).
    /// Network sources: fetch it through [`CoreLibrary::stream_base_url`].
    pub local_poster: Option<String>,
    /// Versions of a movie or episode (LIB-010): 2+ shows a version picker
    /// on play ([`CoreLibrary::versions`]). 1 for shows and tracks.
    pub version_count: u32,
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
            local_poster: i.local_poster,
            version_count: i.version_count,
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

/// One group of [`CoreLibrary::search_grouped`].
#[derive(Debug, Clone)]
pub struct CoreSearchGroup {
    pub kind: CoreItemKind,
    /// Best first.
    pub items: Vec<CoreItem>,
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
    /// The sync service over this library (UI-019): told to send new
    /// progress at once.
    sync: Mutex<Option<Weak<SyncService>>>,
    proxy: Mutex<Option<pa_proxy::Proxy>>,
    opener: Arc<c::SourceOpener>,
}

impl CoreLibrary {
    /// Opens (creating and migrating) the database file at `path`. A
    /// `pa_library` v0 file is refused: open a new file and import it.
    pub fn open(path: String) -> Result<CoreLibrary> {
        let catalog = Arc::new(c::Catalog::open(path)?);
        Ok(Self {
            opener: Arc::new(c::SourceOpener::new(Arc::clone(&catalog))),
            catalog,
            cancel: Mutex::new(None),
            watcher: Mutex::new(None),
            sync: Mutex::new(None),
            proxy: Mutex::new(None),
        })
    }

    /// Reaches SMB and WebDAV sources through the source manager
    /// (`pa-sources`) with the passwords of the secret store whose
    /// fallback file is in `data_dir` (the same as for `CoreSources.open`).
    /// Until this is called only local folders are scanned. Fails if the
    /// secret store cannot be opened; local folders keep working.
    pub fn connect_sources(&self, data_dir: String) -> Result<()> {
        let opened =
            crate::shared::secret_store(Path::new(&data_dir)).map_err(|e| LibraryError {
                message: format!("secret store: {e}"),
            })?;
        let manager =
            pa_sources::SourceManager::open(self.catalog.path(), opened.store).map_err(|e| {
                LibraryError {
                    message: e.to_string(),
                }
            })?;
        self.catalog
            .set_connector(Arc::new(c::SourcesConnector::new(Arc::new(manager))));
        self.opener.reset();
        Ok(())
    }

    /// Base URL of the loopback HTTP proxy that streams files of any source
    /// (`http://127.0.0.1:<port>/<token>`), starting it on first use. The
    /// URL of a file is the base, `/`, the source id and the path inside
    /// the source, every segment percent-encoded. The token is a secret of
    /// this process: do not log the URL.
    pub fn stream_base_url(&self) -> Result<String> {
        let mut proxy = self.proxy.lock().unwrap_or_else(|e| e.into_inner());
        if proxy.is_none() {
            let opener: Arc<dyn pa_proxy::Opener> = self.opener.clone();
            *proxy = Some(
                pa_proxy::Proxy::start(opener, pa_proxy::ProxyOptions::default()).map_err(|e| {
                    LibraryError {
                        message: format!("stream proxy: {e}"),
                    }
                })?,
            );
        }
        Ok(proxy
            .as_ref()
            .map(pa_proxy::Proxy::base_url)
            .unwrap_or_default())
    }

    /// The catalog, for the sync service.
    #[frb(ignore)]
    pub(crate) fn catalog(&self) -> Arc<c::Catalog> {
        Arc::clone(&self.catalog)
    }

    /// Connects the sync service ([`crate::api::sync::CoreSync`]).
    #[frb(ignore)]
    pub(crate) fn set_sync_hook(&self, service: Weak<SyncService>) {
        *self.sync.lock().unwrap_or_else(|e| e.into_inner()) = Some(service);
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

    /// Scans one source or all of them (network sources once
    /// [`connect_sources`](Self::connect_sources) was called). The stream
    /// ends with a `Done` update; errors come as updates with `error` set.
    pub fn scan(&self, source_id: Option<i64>, sink: StreamSink<CoreScanUpdate>) {
        let token = c::CancelToken::new();
        *self.cancel.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.clone());
        let mut done = false;
        let result = self.catalog.scan(source_id, &token, &mut |u| {
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
    /// incrementally, network sources on a timer. Every rescan comes
    /// as an event; the stream stays open until
    /// [`stop_watcher`](Self::stop_watcher).
    pub fn start_watcher(
        &self,
        settings: CoreWatchSettings,
        sink: StreamSink<CoreWatchEvent>,
    ) -> Result<()> {
        // The old watcher (and its stream) goes first.
        self.stop_watcher();
        // Sources may have changed: connect again on the next stream.
        self.opener.reset();
        let options = c::watch::WatchOptions {
            network_interval: (settings.network_interval_secs > 0)
                .then(|| std::time::Duration::from_secs(u64::from(settings.network_interval_secs))),
            debounce: std::time::Duration::from_millis(u64::from(settings.debounce_ms)),
            scan_on_start: settings.scan_on_start,
            ..c::watch::WatchOptions::default()
        };
        let watcher = c::watch::LibraryWatcher::start(
            Arc::clone(&self.catalog),
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

    /// Search results grouped for the Search screen (UI-011): movies,
    /// shows, episodes and tracks, in this order, best first, at most
    /// `per_group` each; empty groups are left out.
    pub fn search_grouped(&self, query: String, per_group: u32) -> Result<Vec<CoreSearchGroup>> {
        Ok(self
            .catalog
            .search_grouped(&query, usize::try_from(per_group).unwrap_or(usize::MAX))?
            .into_iter()
            .map(|g| CoreSearchGroup {
                kind: g.kind.into(),
                items: items(g.items),
            })
            .collect())
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
        let saved = self.catalog.save_progress(
            item_ref(&item_id)?,
            position_ms,
            duration_ms,
            updated_at_ms,
            watched,
        )?;
        let sync = self.sync.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if saved && let Some(service) = sync.and_then(|w| w.upgrade()) {
            service.flush();
        }
        Ok(saved)
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

    // -----------------------------------------------------------------------
    // library queries (LIB-009)

    /// One page of a query (`limit` items from `offset`) and the total.
    pub fn query(&self, query: CoreLibraryQuery, offset: u32, limit: u32) -> Result<CoreItemPage> {
        Ok(self
            .catalog
            .query(&query.into(), offset as usize, limit as usize)?
            .into())
    }

    /// Alphabet index of a query in title order.
    pub fn alphabet_index(&self, query: CoreLibraryQuery) -> Result<Vec<CoreAlphabetEntry>> {
        Ok(self
            .catalog
            .alphabet_index(&query.into())?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Filter values present among movies or shows, for filter chips.
    pub fn facets(&self, kind: CoreItemKind) -> Result<CoreFacets> {
        Ok(self.catalog.facets(kind.into())?.into())
    }

    /// Entries of a folder of a source (`""` = root), sorted, folders first
    /// if asked.
    pub fn browse(
        &self,
        source_id: i64,
        dir: String,
        sort: CoreLibrarySort,
        folders_first: bool,
    ) -> Result<Vec<CoreFolderEntry>> {
        Ok(self
            .catalog
            .browse(source_id, &dir, sort.into(), folders_first)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Saved preferences of a view, or its defaults.
    pub fn view_prefs(&self, view: String) -> Result<CoreViewPrefs> {
        Ok(self.catalog.view_prefs(&view)?.into())
    }

    pub fn set_view_prefs(&self, prefs: CoreViewPrefs) -> Result<()> {
        Ok(self.catalog.set_view_prefs(&prefs.into())?)
    }

    /// Smart shelves by position; `pinned_only`: those on Home.
    pub fn shelves(&self, pinned_only: bool) -> Result<Vec<CoreSmartShelf>> {
        Ok(self
            .catalog
            .shelves(pinned_only)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn create_shelf(
        &self,
        name: String,
        query: CoreLibraryQuery,
        pinned_home: bool,
    ) -> Result<CoreSmartShelf> {
        Ok(self
            .catalog
            .create_shelf(&name, &query.into(), pinned_home)?
            .into())
    }

    pub fn update_shelf(
        &self,
        id: i64,
        name: String,
        query: CoreLibraryQuery,
        pinned_home: bool,
    ) -> Result<()> {
        Ok(self
            .catalog
            .update_shelf(id, &name, &query.into(), pinned_home)?)
    }

    pub fn delete_shelf(&self, id: i64) -> Result<()> {
        Ok(self.catalog.delete_shelf(id)?)
    }

    /// Orders the shelves as `ids`; unlisted ones follow.
    pub fn reorder_shelves(&self, ids: Vec<i64>) -> Result<()> {
        Ok(self.catalog.reorder_shelves(&ids)?)
    }

    /// One page of a shelf's query.
    pub fn shelf_items(&self, id: i64, offset: u32, limit: u32) -> Result<CoreItemPage> {
        Ok(self
            .catalog
            .shelf_items(id, offset as usize, limit as usize)?
            .into())
    }

    /// Genres of a movie or show item.
    pub fn genres(&self, item_id: String) -> Result<Vec<String>> {
        Ok(self.catalog.genres(item_ref(&item_id)?)?)
    }

    // -----------------------------------------------------------------------
    // sidecar subtitles (PLAY-007)

    /// Subtitle files of the file `rel_path` of source `source_id` (as in
    /// `CoreItem` and `CoreVersion`): those named after it first, then those
    /// of its movie folder. Empty for an unknown file.
    pub fn sidecar_subtitles(
        &self,
        source_id: i64,
        rel_path: String,
    ) -> Result<Vec<CoreSidecarSubtitle>> {
        Ok(self
            .catalog
            .sidecar_subtitles(source_id, &rel_path)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    // -----------------------------------------------------------------------
    // versions, favorites, collections, playlists (LIB-010)

    /// Versions of a movie or episode, best resolution first; the one the
    /// item plays has `is_default`.
    pub fn versions(&self, item_id: String) -> Result<Vec<CoreVersion>> {
        Ok(self
            .catalog
            .versions(item_ref(&item_id)?)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Makes a version (`CoreVersion::file_id`) the default of a movie, or
    /// goes back to its first file (`None`).
    pub fn set_default_version(&self, item_id: String, file_id: Option<i64>) -> Result<()> {
        Ok(self
            .catalog
            .set_default_version(item_ref(&item_id)?, file_id)?)
    }

    /// Favorite items, newest first; `pinned_only`: those on Home.
    pub fn favorites(&self, pinned_only: bool) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.favorites(pinned_only)?))
    }

    pub fn is_favorite(&self, item_id: String) -> Result<bool> {
        let Ok(id) = item_id.parse::<c::ItemRef>() else {
            return Ok(false);
        };
        Ok(self.catalog.is_favorite(id)?)
    }

    /// Adds an item to the favorites or removes it; whether anything
    /// changed.
    pub fn set_favorite(&self, item_id: String, favorite: bool) -> Result<bool> {
        Ok(self.catalog.set_favorite(item_ref(&item_id)?, favorite)?)
    }

    /// Shows a favorite in the Favorites row of Home or hides it there.
    pub fn set_favorite_pinned(&self, item_id: String, pinned: bool) -> Result<bool> {
        Ok(self
            .catalog
            .set_favorite_pinned(item_ref(&item_id)?, pinned)?)
    }

    /// The user's collections and the TMDB ones, by name.
    pub fn collections(&self) -> Result<Vec<CoreCollection>> {
        Ok(self
            .catalog
            .collections()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Collections that hold an item.
    pub fn item_collections(&self, item_id: String) -> Result<Vec<CoreCollection>> {
        Ok(self
            .catalog
            .item_collections(item_ref(&item_id)?)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn collection_items(&self, id: i64) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.collection_items(id)?))
    }

    pub fn create_collection(&self, name: String) -> Result<CoreCollection> {
        Ok(self.catalog.create_collection(&name)?.into())
    }

    pub fn rename_collection(&self, id: i64, name: String) -> Result<()> {
        Ok(self.catalog.rename_collection(id, &name)?)
    }

    pub fn delete_collection(&self, id: i64) -> Result<()> {
        Ok(self.catalog.delete_collection(id)?)
    }

    pub fn add_to_collection(&self, id: i64, item_id: String) -> Result<bool> {
        Ok(self.catalog.add_to_collection(id, item_ref(&item_id)?)?)
    }

    pub fn remove_from_collection(&self, id: i64, item_id: String) -> Result<bool> {
        Ok(self
            .catalog
            .remove_from_collection(id, item_ref(&item_id)?)?)
    }

    /// Video playlists, most recently changed first.
    pub fn playlists(&self) -> Result<Vec<CorePlaylist>> {
        Ok(self
            .catalog
            .playlists()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn create_playlist(&self, name: String) -> Result<CorePlaylist> {
        Ok(self.catalog.create_playlist(&name)?.into())
    }

    pub fn rename_playlist(&self, id: i64, name: String) -> Result<()> {
        Ok(self.catalog.rename_playlist(id, &name)?)
    }

    pub fn delete_playlist(&self, id: i64) -> Result<()> {
        Ok(self.catalog.delete_playlist(id)?)
    }

    /// Items of a playlist in order (missing files left out).
    pub fn playlist_items(&self, id: i64) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.playlist_items(id)?))
    }

    /// Appends a movie, an episode or every episode of a show; returns how
    /// many entries were added (items already there are skipped).
    pub fn add_to_playlist(&self, id: i64, item_id: String) -> Result<u32> {
        Ok(count(
            self.catalog.add_to_playlist(id, item_ref(&item_id)?)?,
        ))
    }

    pub fn remove_from_playlist(&self, id: i64, item_id: String) -> Result<u32> {
        Ok(count(
            self.catalog.remove_from_playlist(id, item_ref(&item_id)?)?,
        ))
    }

    /// Orders the playlist as `item_ids`; others follow.
    pub fn reorder_playlist(&self, id: i64, item_ids: Vec<String>) -> Result<()> {
        let refs: Vec<c::ItemRef> = item_ids
            .iter()
            .filter_map(|i| i.parse::<c::ItemRef>().ok())
            .collect();
        Ok(self.catalog.reorder_playlist(id, &refs)?)
    }

    // -- music (MUS-004) -------------------------------------------------------

    /// Music tracks with album, credits, format and play counts, by artist,
    /// album, disc and track number.
    pub fn music_tracks(&self) -> Result<Vec<CoreMusicTrack>> {
        Ok(self
            .catalog
            .music_tracks()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Counts a play of a track (it started in the player); returns the new
    /// count, 0 if the track is not hashed yet.
    pub fn record_track_play(&self, item_id: String) -> Result<i64> {
        Ok(self.catalog.record_track_play(item_ref(&item_id)?)?)
    }

    /// Audio playlists, most recently changed first.
    pub fn music_playlists(&self) -> Result<Vec<CorePlaylist>> {
        Ok(self
            .catalog
            .music_playlists()?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn create_music_playlist(&self, name: String) -> Result<CorePlaylist> {
        Ok(self.catalog.create_music_playlist(&name)?.into())
    }

    pub fn rename_music_playlist(&self, id: i64, name: String) -> Result<()> {
        Ok(self.catalog.rename_music_playlist(id, &name)?)
    }

    pub fn delete_music_playlist(&self, id: i64) -> Result<()> {
        Ok(self.catalog.delete_music_playlist(id)?)
    }

    /// Tracks of an audio playlist in order (missing files left out).
    pub fn music_playlist_items(&self, id: i64) -> Result<Vec<CoreItem>> {
        Ok(items(self.catalog.music_playlist_items(id)?))
    }

    /// `false` if the track already was in the playlist.
    pub fn add_to_music_playlist(&self, id: i64, item_id: String) -> Result<bool> {
        Ok(self
            .catalog
            .add_to_music_playlist(id, item_ref(&item_id)?)?)
    }

    pub fn remove_from_music_playlist(&self, id: i64, item_id: String) -> Result<bool> {
        Ok(self
            .catalog
            .remove_from_music_playlist(id, item_ref(&item_id)?)?)
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

    #[test]
    fn streams_files_of_a_source_through_the_proxy() {
        use std::io::{Read, Write};

        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("media");
        std::fs::create_dir_all(media.join("Dune (2021)")).unwrap();
        std::fs::write(media.join("Dune (2021)/Dune.mkv"), b"0123456789").unwrap();
        let lib = CoreLibrary::open(dir.path().join("l.db").to_string_lossy().into()).unwrap();
        let src = lib
            .add_source(
                None,
                "local".into(),
                "M".into(),
                media.to_string_lossy().into(),
                None,
            )
            .unwrap();
        let base = lib.stream_base_url().unwrap();
        assert_eq!(lib.stream_base_url().unwrap(), base);
        let rest = base.strip_prefix("http://").unwrap();
        let (addr, token) = rest.split_once('/').unwrap();
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(
            s,
            "GET /{token}/{}/Dune%20%282021%29/Dune.mkv HTTP/1.1\r\nRange: bytes=3-5\r\nConnection: close\r\n\r\n",
            src.id
        )
        .unwrap();
        let mut reply = String::new();
        s.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 206"), "{reply}");
        assert!(reply.ends_with("\r\n\r\n345"), "{reply}");
    }
}
