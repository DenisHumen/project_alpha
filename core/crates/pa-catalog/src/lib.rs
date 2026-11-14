// SPDX-License-Identifier: MPL-2.0
//! The client media library on the Rust core (UI-024).
//!
//! [`Catalog`] is what the Kinolune app (through `pa-frb` and the Dart
//! package `pa_library`) needs from the core, on one `pa-library` database
//! (schema v1):
//!
//! - sources ([`Catalog::sources`], [`Catalog::add_source`]);
//! - the scan pipeline ([`Catalog::scan`]): `pa-scanner` listing, music tags
//!   ([`pa_scanner::index_music`]), file fingerprints ([`fingerprint`]),
//!   local recognition of video files with the `pa-recognition` parser
//!   ([`recognize`]) and the search index;
//! - item views shaped like the release-0.1 Dart `Library` ([`Item`]):
//!   movies, shows, episodes, tracks, recently added, FTS5 search;
//! - progress, "Continue watching" and Up Next on `pa_library::progress`;
//! - the import of a `pa_library` v0 database ([`Catalog::import_v0`],
//!   the part of LIB-015 the app needs);
//! - provider metadata ([`Catalog::fetch_metadata`], [`metadata`],
//!   META-013): TMDB / AniList / Shikimori ids, titles, overviews, artwork,
//!   genres and episode data for the recognized works;
//! - library queries ([`Catalog::query`], [`query`], LIB-009): combinable
//!   filters, sorts and paging, the alphabet index, facets, folder browsing
//!   with "folders first", per-view preferences and smart shelves;
//! - the change watcher ([`watch::LibraryWatcher`], LIB-005): OS change
//!   notifications for local folders, a timer for network shares, and
//!   incremental rescans of the changed folders
//!   ([`Catalog::rescan_folders`]).
//!
//! Recognition in the scan is local: works are created from parsed titles,
//! without providers. The metadata run then matches them against TMDB (and
//! anime against AniList and Shikimori) and attaches what it finds to the
//! same works.
//!
//! Item ids are strings, stable while the catalog rows live: `m<work>` (a
//! movie), `s<work>` (a show or anime), `e<episode>`, `t<track>`.

mod error;
pub mod fingerprint;
mod fix;
mod import_v0;
mod index;
mod items;
pub mod metadata;
mod progress;
pub mod query;
pub mod recognize;
mod scan;
mod sources;
pub mod uri;
pub mod watch;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use pa_library::rusqlite::Connection;

pub use error::Error;
pub use import_v0::ImportReport;
pub use items::{Item, ItemKind, ItemRef};
pub use metadata::{MetadataOptions, MetadataProviders, MetadataReport};
pub use pa_metadata;
pub use pa_scanner::CancelToken;
pub use progress::{ContinueItem, ItemProgress};
pub use query::{
    AlphabetEntry, FacetValue, Facets, FolderEntry, HdrFilter, ItemPage, LibraryFilter,
    LibraryQuery, LibrarySort, Resolution, SmartShelf, SortField, ViewPrefs, WatchState,
};
pub use scan::{RescanReport, ScanPhase, ScanUpdate};
pub use sources::Source;

/// Device id written with local progress changes until profiles and
/// device identity reach the app (SYNC-*).
pub const LOCAL_DEVICE_ID: &str = "local";

/// The media library of one database file.
#[derive(Debug)]
pub struct Catalog {
    path: PathBuf,
    conn: Mutex<Connection>,
    /// Scans are serialized; queries keep running on `conn` meanwhile.
    scan_lock: Mutex<()>,
    /// In-memory index of library queries (LIB-009) and the database state
    /// it was built from.
    index: Mutex<Option<(index::Stamp, Arc<index::Index>)>>,
}

impl Catalog {
    /// Opens (creating and migrating) the library database at `path`.
    ///
    /// A `pa_library` v0 file is refused with
    /// `Error::Library(ForeignDatabase { legacy_v0: true })`: open a new file
    /// and [`import_v0`](Self::import_v0) the old one.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).map_err(|e| Error::Io(e.to_string()))?;
        }
        let conn = pa_library::open(&path)?;
        Ok(Self {
            path,
            conn: Mutex::new(conn),
            scan_lock: Mutex::new(()),
            index: Mutex::new(None),
        })
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The query index, rebuilt first if the database changed since it was
    /// built. `conn` is the locked catalog connection.
    fn index(&self, conn: &Connection) -> Result<Arc<index::Index>, Error> {
        let stamp = index::Stamp::of(conn)?;
        let mut cached = self.index.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((built, index)) = cached.as_ref()
            && *built == stamp
        {
            return Ok(Arc::clone(index));
        }
        let index = Arc::new(index::Index::build(conn)?);
        *cached = Some((stamp, Arc::clone(&index)));
        Ok(index)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent
        // (transactions roll back on drop), so a poisoned lock is usable.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `true` if the file at `path` is a `pa_library` v0 database.
pub fn is_legacy_v0(path: impl AsRef<Path>) -> Result<bool, Error> {
    let path = path.as_ref();
    if !path.is_file() {
        return Ok(false);
    }
    let conn = Connection::open_with_flags(
        path,
        pa_library::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | pa_library::rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(pa_library::is_legacy_v0(&conn)?)
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
