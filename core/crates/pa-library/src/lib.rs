// SPDX-License-Identifier: MPL-2.0
//! Media library storage of the project_alpha core.
//!
//! One SQLite schema for clients and the media server (docs/dev/db.md,
//! ADR-0013). This crate owns the schema, its migrations and the connection
//! setup; queries are added by the LIB tasks that need them.
//!
//! ```
//! let conn = pa_library::open_in_memory().unwrap();
//! let profiles: i64 = conn
//!     .query_row("SELECT count(*) FROM profiles", [], |r| r.get(0))
//!     .unwrap();
//! assert_eq!(profiles, 1); // the default profile
//! ```

mod migrations;

use std::fmt;
use std::path::Path;
use std::time::Duration;

pub use migrations::{MIGRATIONS, MigrationFile};
pub use rusqlite;

use rusqlite::{Connection, OpenFlags};

/// Name of the refinery history table.
pub const HISTORY_TABLE: &str = "refinery_schema_history";

/// How long a writer waits for a lock held by another connection.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Tables of the pa_library v0 database (Dart, release 0.1). See
/// [`is_legacy_v0`] and docs/dev/db.md §"Переход с pa_library v0".
pub const LEGACY_V0_TABLES: [&str; 5] = ["items", "progress", "settings", "sources", "tmdb_cache"];

/// Error opening or migrating a library database.
#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    Migration(refinery::Error),
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
            Self::Migration(e) => write!(f, "migration: {e}"),
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
            Self::Migration(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<refinery::Error> for Error {
    fn from(e: refinery::Error) -> Self {
        Self::Migration(e)
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

    let known = migrations::runner_migrations()?;
    let known_latest = known.iter().map(|m| m.version()).max().unwrap_or(0);
    let runner = refinery::Runner::new(&known)
        .set_grouped(true)
        .set_abort_divergent(true)
        // Unknown newer migrations are allowed (checked below); refinery
        // would otherwise refuse a database written by a newer build.
        .set_abort_missing(false);

    let applied: Vec<i64> = if tables.is_empty() {
        Vec::new()
    } else {
        runner
            .get_applied_migrations(conn)?
            .iter()
            .map(|m| m.version())
            .collect()
    };
    let db_latest = applied.iter().copied().max();
    for v in &applied {
        if *v < known_latest && !known.iter().any(|m| m.version() == *v) {
            return Err(Error::UnknownMigration { version: *v });
        }
    }
    if let Some(db_latest) = db_latest {
        for m in &known {
            if m.version() < db_latest && !applied.contains(&m.version()) {
                return Err(Error::OutOfOrder {
                    version: m.version(),
                });
            }
        }
    }

    let report = runner.run(conn)?;
    let applied_now: Vec<i64> = report
        .applied_migrations()
        .iter()
        .map(|m| m.version())
        .collect();
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
