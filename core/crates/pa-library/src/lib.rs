// SPDX-License-Identifier: MPL-2.0
//! Media library storage of the project_alpha core.
//!
//! One SQLite schema for clients and the media server (docs/dev/db.md,
//! ADR-0013). This crate owns the schema, its migrations and the connection
//! setup; queries are added by the tasks that need them: [`progress`]
//! (playback state, history, "Continue watching", Up Next; LIB-011),
//! [`music`] (artists, albums, tracks, genres; MUS-001), [`twitch`]
//! (streamers, favourites, VODs, clips, chapters, saved files, search;
//! TWI-007), [`search`] (FTS5 search over the whole library; LIB-008) and
//! [`groups`] (structural folder groups: discs, parts, extras, sidecars;
//! LIB-007).
//!
//! ```
//! let conn = pa_library::open_in_memory().unwrap();
//! let profiles: i64 = conn
//!     .query_row("SELECT count(*) FROM profiles", [], |r| r.get(0))
//!     .unwrap();
//! assert_eq!(profiles, 1); // the default profile
//! ```

pub mod groups;
mod migrations;
pub mod music;
pub mod progress;
pub mod search;
pub mod twitch;

use std::fmt;
use std::path::Path;
use std::time::Duration;

pub use migrations::{MIGRATIONS, MigrationFile};
pub use rusqlite;

use rusqlite::{Connection, OpenFlags};

/// Name of the migration history table (version, name, applied_on, checksum).
pub const HISTORY_TABLE: &str = "pa_schema_history";

/// How long a writer waits for a lock held by another connection.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Tables of the pa_library v0 database (Dart, release 0.1). See
/// [`is_legacy_v0`] and docs/dev/db.md §"Переход с pa_library v0".
pub const LEGACY_V0_TABLES: [&str; 5] = ["items", "progress", "settings", "sources", "tmdb_cache"];

/// Error of the library: opening, migrating or querying the database.
#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    /// An argument or a stored value is not acceptable (unknown profile,
    /// invalid media key, bad thresholds).
    Invalid(&'static str),
    /// The sync sink ([`progress::ProgressSink`]) failed; the local change
    /// was rolled back.
    Sync(Box<dyn std::error::Error + Send + Sync>),
    /// An applied migration was edited: its checksum no longer matches the
    /// history. Never edit an applied file; add a new migration.
    ChecksumMismatch {
        version: i64,
    },
    /// The file has tables but no migration history: it is not ours (for
    /// example a pa_library v0 database). Nothing was changed.
    ForeignDatabase {
        legacy_v0: bool,
    },
    /// The database has an applied migration this build does not know, older
    /// than the newest one it knows: histories diverged.
    UnknownMigration {
        version: i64,
    },
    /// A migration of this build is older than the newest applied one but was
    /// never applied (merged out of order). Rename it to a newer timestamp.
    OutOfOrder {
        version: i64,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "sqlite: {e}"),
            Self::Invalid(what) => write!(f, "invalid {what}"),
            Self::Sync(e) => write!(f, "sync: {e}"),
            Self::ChecksumMismatch { version } => {
                write!(
                    f,
                    "applied migration {version} was edited (checksum mismatch)"
                )
            }
            Self::ForeignDatabase { legacy_v0: true } => {
                f.write_str("database is a pa_library v0 file; import it instead of migrating")
            }
            Self::ForeignDatabase { legacy_v0: false } => {
                f.write_str("database has tables but no migration history")
            }
            Self::UnknownMigration { version } => {
                write!(f, "applied migration {version} is unknown to this build")
            }
            Self::OutOfOrder { version } => {
                write!(f, "migration {version} is older than the applied schema")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(e) => Some(e),
            Self::Sync(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

/// Result of [`migrate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Versions applied by this call, oldest first.
    pub applied: Vec<i64>,
    /// Newest migration version in the database after the call.
    pub db_version: i64,
    /// True if the database was migrated by a newer build. Expand→contract
    /// (LIB-002) keeps it readable by this build one version back.
    pub db_is_newer: bool,
}

/// Opens (creating if needed) a library database file, configures the
/// connection and migrates it to the latest schema.
pub fn open(path: impl AsRef<Path>) -> Result<Connection, Error> {
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )?;
    configure(&conn)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Opens a migrated in-memory database (tests, previews).
pub fn open_in_memory() -> Result<Connection, Error> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Per-connection settings. Every connection to a library database must
/// have them: foreign keys are off by default in SQLite.
pub fn configure(conn: &Connection) -> Result<(), Error> {
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    Ok(())
}

/// True if the connection holds a pa_library v0 (Dart) database.
pub fn is_legacy_v0(conn: &Connection) -> Result<bool, Error> {
    let tables = user_tables(conn)?;
    Ok(!tables.iter().any(|t| t == HISTORY_TABLE)
        && LEGACY_V0_TABLES
            .iter()
            .all(|t| tables.iter().any(|x| x == t)))
}

/// Applies pending migrations, all in one transaction.
///
/// Refuses a database that has tables but no history ([`Error::ForeignDatabase`])
/// and inconsistent histories. A database migrated by a newer build is left
/// as is and reported in [`MigrationReport::db_is_newer`].
pub fn migrate(conn: &mut Connection) -> Result<MigrationReport, Error> {
    let tables = user_tables(conn)?;
    if !tables.is_empty() && !tables.iter().any(|t| t == HISTORY_TABLE) {
        return Err(Error::ForeignDatabase {
            legacy_v0: is_legacy_v0(conn)?,
        });
    }

    let known = MIGRATIONS;
    let known_latest = latest_version();

    let applied: Vec<(i64, String)> = if tables.is_empty() {
        Vec::new()
    } else {
        let mut stmt = conn.prepare(&format!(
            "SELECT version, checksum FROM {HISTORY_TABLE} ORDER BY version"
        ))?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    let db_latest = applied.iter().map(|(v, _)| *v).max();
    for (v, checksum) in &applied {
        match known.iter().find(|m| m.version == *v) {
            Some(m) if m.checksum() != *checksum => {
                return Err(Error::ChecksumMismatch { version: *v });
            }
            Some(_) => {}
            None if *v < known_latest => return Err(Error::UnknownMigration { version: *v }),
            None => {}
        }
    }
    let is_applied = |v: i64| applied.iter().any(|(a, _)| *a == v);
    if let Some(db_latest) = db_latest {
        for m in known {
            if m.version < db_latest && !is_applied(m.version) {
                return Err(Error::OutOfOrder { version: m.version });
            }
        }
    }

    // All pending migrations and their history rows in one transaction.
    let tx = conn.transaction()?;
    tx.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {HISTORY_TABLE} (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_on TEXT NOT NULL,
            checksum TEXT NOT NULL
        ) STRICT"
    ))?;
    let mut applied_now = Vec::new();
    for m in known.iter().filter(|m| !is_applied(m.version)) {
        tx.execute_batch(m.sql)?;
        tx.execute(
            &format!(
                "INSERT INTO {HISTORY_TABLE} (version, name, applied_on, checksum) \
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?3)"
            ),
            rusqlite::params![m.version, m.name, m.checksum()],
        )?;
        applied_now.push(m.version);
    }
    tx.commit()?;

    let db_version = applied_now
        .iter()
        .copied()
        .chain(db_latest)
        .max()
        .unwrap_or(0);
    Ok(MigrationReport {
        applied: applied_now,
        db_version,
        db_is_newer: db_version > known_latest,
    })
}

/// Newest schema version this build knows.
pub fn latest_version() -> i64 {
    MIGRATIONS.iter().map(|m| m.version).max().unwrap_or(0)
}

fn user_tables(conn: &Connection) -> Result<Vec<String>, Error> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'",
    )?;
    let names = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(names)
}
