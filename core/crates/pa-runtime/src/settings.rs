// SPDX-License-Identifier: MPL-2.0
//! Key-value settings in SQLite with schema migrations, JSON export and import.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::SettingsError;

/// File name of the settings database inside
/// [`PlatformDirs::config`](crate::PlatformDirs::config).
pub const SETTINGS_FILE: &str = "settings.sqlite3";

/// Value of the `format` field of a settings export.
pub const EXPORT_FORMAT: &str = "kinolune.settings";

/// Version of the export document written by this build. Import accepts
/// versions `1..=EXPORT_VERSION`.
pub const EXPORT_VERSION: u32 = 1;

/// Longest accepted settings key, in bytes.
pub const MAX_SETTING_KEY_LEN: usize = 128;

/// Schema migrations, applied in order. The index + 1 is the schema version
/// stored in `PRAGMA user_version`. Only append; never edit a shipped entry.
const MIGRATIONS: &[&str] = &[
    // 1 (CORE-006): one row per key, value is a JSON document.
    "CREATE TABLE settings (
        key        TEXT    PRIMARY KEY NOT NULL,
        value      TEXT    NOT NULL,
        updated_at INTEGER NOT NULL
    ) STRICT, WITHOUT ROWID;",
];

/// Schema version this build creates and understands.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

/// How long a write waits for a lock held by another connection.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// How [`Settings::import`] treats keys that are not in the import document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// Keep them: imported keys overwrite, others stay.
    Merge,
    /// Delete them: afterwards the store holds exactly the imported keys.
    Replace,
}

/// Portable settings document: what export writes and import reads.
///
/// ```json
/// {"format": "kinolune.settings", "version": 1, "exported_at": 1763000000000,
///  "settings": {"player.subtitles.size": 42, "ui.theme": "dark"}}
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsExport {
    /// Always [`EXPORT_FORMAT`].
    pub format: String,
    /// Document version, see [`EXPORT_VERSION`].
    pub version: u32,
    /// Export time, Unix milliseconds.
    pub exported_at: i64,
    /// All settings, sorted by key.
    pub settings: BTreeMap<String, Value>,
}

/// The settings store: string keys, JSON values, one SQLite file.
///
/// Keys are dotted lower-case names such as `player.subtitles.size`
/// (`[a-z0-9._-]`, at most [`MAX_SETTING_KEY_LEN`] bytes). Values are any
/// JSON; [`get`](Self::get) and [`set`](Self::set) convert them with serde.
/// Every write is its own transaction and is on disk when the call returns.
///
/// Do not store secrets here: exports are plain JSON files.
pub struct Settings {
    conn: Mutex<Option<Connection>>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("open", &self.lock().is_some())
            .finish()
    }
}

impl Settings {
    /// Opens (or creates) the database at `path` and migrates it to
    /// [`SCHEMA_VERSION`].
    ///
    /// Fails with [`SettingsError::SchemaTooNew`] if a newer core wrote it.
    pub fn open(path: &Path) -> Result<Self, SettingsError> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        // WAL survives a process kill without losing committed writes and
        // lets readers run during writes.
        let _mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::from_connection(conn)
    }

    /// Opens a store that lives only in memory (tests, previews).
    pub fn open_in_memory() -> Result<Self, SettingsError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Self, SettingsError> {
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(Some(conn)),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Option<Connection>> {
        // A panic while holding the lock cannot leave SQLite inconsistent:
        // every write is a transaction that rolls back on drop.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_conn<R>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<R, SettingsError>,
    ) -> Result<R, SettingsError> {
        let mut guard = self.lock();
        let conn = guard.as_mut().ok_or(SettingsError::Closed)?;
        f(conn)
    }

    /// Schema version of the open database (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<i64, SettingsError> {
        self.with_conn(|conn| user_version(conn))
    }

    /// Raw JSON value of `key`, `None` if it is not set.
    pub fn get_value(&self, key: &str) -> Result<Option<Value>, SettingsError> {
        validate_key(key)?;
        self.with_conn(|conn| {
            let text: Option<String> = conn
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    [key],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(text.map(|t| serde_json::from_str(&t)).transpose()?)
        })
    }

    /// Value of `key` converted to `T`, `None` if it is not set.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, SettingsError> {
        Ok(self
            .get_value(key)?
            .map(serde_json::from_value)
            .transpose()?)
    }

    /// Value of `key` converted to `T`, or `default` if it is not set.
    pub fn get_or<T: DeserializeOwned>(&self, key: &str, default: T) -> Result<T, SettingsError> {
        Ok(self.get(key)?.unwrap_or(default))
    }

    /// Stores a raw JSON value under `key`.
    pub fn set_value(&self, key: &str, value: &Value) -> Result<(), SettingsError> {
        validate_key(key)?;
        let text = serde_json::to_string(value)?;
        self.with_conn(|conn| {
            upsert(conn, key, &text, now_ms())?;
            Ok(())
        })
    }

    /// Stores `value` (serialized to JSON) under `key`.
    pub fn set<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<(), SettingsError> {
        self.set_value(key, &serde_json::to_value(value)?)
    }

    /// Deletes `key`. Returns whether it existed.
    pub fn remove(&self, key: &str) -> Result<bool, SettingsError> {
        validate_key(key)?;
        self.with_conn(|conn| Ok(conn.execute("DELETE FROM settings WHERE key = ?1", [key])? > 0))
    }

    /// All keys, sorted.
    pub fn keys(&self) -> Result<Vec<String>, SettingsError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT key FROM settings ORDER BY key")?;
            let keys = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(keys)
        })
    }

    /// Snapshot of all settings as an export document.
    pub fn export(&self) -> Result<SettingsExport, SettingsError> {
        let settings = self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut map = BTreeMap::new();
            for (key, text) in rows {
                map.insert(key, serde_json::from_str(&text)?);
            }
            Ok(map)
        })?;
        Ok(SettingsExport {
            format: EXPORT_FORMAT.to_owned(),
            version: EXPORT_VERSION,
            exported_at: now_ms(),
            settings,
        })
    }

    /// [`export`](Self::export) as pretty-printed JSON, ready to save to a file.
    pub fn export_json(&self) -> Result<String, SettingsError> {
        Ok(serde_json::to_string_pretty(&self.export()?)?)
    }

    /// Writes an export document into the store in one transaction: either
    /// every key is imported or none. Returns the number of imported keys.
    pub fn import(&self, export: &SettingsExport, mode: ImportMode) -> Result<usize, SettingsError> {
        if export.format != EXPORT_FORMAT {
            return Err(SettingsError::InvalidExport(format!(
                "format is {:?}, expected {EXPORT_FORMAT:?}",
                export.format
            )));
        }
        if !(1..=EXPORT_VERSION).contains(&export.version) {
            return Err(SettingsError::InvalidExport(format!(
                "version {} is not supported (1..={EXPORT_VERSION})",
                export.version
            )));
        }
        let mut rows = Vec::with_capacity(export.settings.len());
        for (key, value) in &export.settings {
            validate_key(key)?;
            rows.push((key.as_str(), serde_json::to_string(value)?));
        }
        let now = now_ms();
        self.with_conn(|conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if mode == ImportMode::Replace {
                tx.execute("DELETE FROM settings", [])?;
            }
            for (key, text) in &rows {
                upsert(&tx, key, text, now)?;
            }
            tx.commit()?;
            Ok(rows.len())
        })
    }

    /// Parses `json` as an export document and [`import`](Self::import)s it.
    pub fn import_json(&self, json: &str, mode: ImportMode) -> Result<usize, SettingsError> {
        let export: SettingsExport = serde_json::from_str(json)
            .map_err(|e| SettingsError::InvalidExport(e.to_string()))?;
        self.import(&export, mode)
    }

    /// Moves the WAL into the main file. Called when the app goes to the
    /// background, so a kill by the OS leaves a compact database.
    pub fn checkpoint(&self) -> Result<(), SettingsError> {
        self.with_conn(|conn| {
            conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()))?;
            Ok(())
        })
    }

    /// Closes the database. Later calls fail with [`SettingsError::Closed`].
    /// Closing a closed store does nothing.
    pub fn close(&self) -> Result<(), SettingsError> {
        let conn = self.lock().take();
        if let Some(conn) = conn {
            conn.close().map_err(|(_, e)| SettingsError::from(e))?;
        }
        Ok(())
    }

    /// Whether [`close`](Self::close) was called.
    pub fn is_closed(&self) -> bool {
        self.lock().is_none()
    }
}

fn upsert(conn: &Connection, key: &str, text: &str, now: i64) -> Result<(), SettingsError> {
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, text, now],
    )?;
    Ok(())
}

fn user_version(conn: &Connection) -> Result<i64, SettingsError> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// Applies the missing migrations in one immediate transaction, so two
/// processes opening the same file cannot migrate it twice.
fn migrate(conn: &mut Connection) -> Result<(), SettingsError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = user_version(&tx)?;
    if current > SCHEMA_VERSION {
        return Err(SettingsError::SchemaTooNew {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    let start = usize::try_from(current).unwrap_or(0);
    for sql in &MIGRATIONS[start..] {
        tx.execute_batch(sql)?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}

fn validate_key(key: &str) -> Result<(), SettingsError> {
    let ok = !key.is_empty()
        && key.len() <= MAX_SETTING_KEY_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(SettingsError::InvalidKey(key.to_owned()))
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn typed_get_set_remove() {
        let s = Settings::open_in_memory().unwrap();
        assert_eq!(s.get::<u32>("player.volume").unwrap(), None);
        s.set("player.volume", &80u32).unwrap();
        s.set("ui.theme", "dark").unwrap();
        assert_eq!(s.get::<u32>("player.volume").unwrap(), Some(80));
        assert_eq!(s.get_or("ui.theme", String::new()).unwrap(), "dark");
        assert_eq!(s.keys().unwrap(), ["player.volume", "ui.theme"]);
        s.set("player.volume", &50u32).unwrap();
        assert_eq!(s.get::<u32>("player.volume").unwrap(), Some(50));
        assert!(s.remove("player.volume").unwrap());
        assert!(!s.remove("player.volume").unwrap());
        assert_eq!(s.get_or("player.volume", 100u32).unwrap(), 100);
    }

    #[test]
    fn wrong_type_is_a_json_error() {
        let s = Settings::open_in_memory().unwrap();
        s.set("ui.theme", "dark").unwrap();
        assert!(matches!(
            s.get::<u32>("ui.theme"),
            Err(SettingsError::Json(_))
        ));
    }

    #[test]
    fn keys_are_validated() {
        let s = Settings::open_in_memory().unwrap();
        for bad in ["", "UI.theme", "ui theme", "ключ", &"a".repeat(MAX_SETTING_KEY_LEN + 1)] {
            assert!(
                matches!(s.set(bad, &1), Err(SettingsError::InvalidKey(_))),
                "{bad:?} accepted"
            );
        }
        s.set("a-b_c.d9", &1).unwrap();
        s.set(&"a".repeat(MAX_SETTING_KEY_LEN), &1).unwrap();
    }

    #[test]
    fn fresh_database_is_at_latest_schema() {
        let s = Settings::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        let err = Settings::open(&path).expect_err("downgrade must fail");
        assert!(matches!(err, SettingsError::SchemaTooNew { found, .. } if found == SCHEMA_VERSION + 1));
    }

    #[test]
    fn reopen_keeps_values_and_does_not_remigrate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        let s = Settings::open(&path).unwrap();
        s.set("ui.theme", "light").unwrap();
        s.close().unwrap();
        let s = Settings::open(&path).unwrap();
        assert_eq!(s.get::<String>("ui.theme").unwrap().as_deref(), Some("light"));
        assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn export_import_round_trip() {
        let a = Settings::open_in_memory().unwrap();
        a.set("ui.theme", "dark").unwrap();
        a.set("player.subtitles", &json!({"size": 42, "langs": ["ru", "en"]}))
            .unwrap();
        a.set("player.hw-decoding", &true).unwrap();
        let json = a.export_json().unwrap();

        let b = Settings::open_in_memory().unwrap();
        assert_eq!(b.import_json(&json, ImportMode::Merge).unwrap(), 3);
        assert_eq!(b.export().unwrap().settings, a.export().unwrap().settings);
    }

    #[test]
    fn merge_keeps_and_replace_drops_other_keys() {
        let src = Settings::open_in_memory().unwrap();
        src.set("ui.theme", "dark").unwrap();
        let export = src.export().unwrap();

        let dst = Settings::open_in_memory().unwrap();
        dst.set("ui.theme", "light").unwrap();
        dst.set("player.volume", &10).unwrap();
        dst.import(&export, ImportMode::Merge).unwrap();
        assert_eq!(dst.keys().unwrap(), ["player.volume", "ui.theme"]);
        assert_eq!(dst.get::<String>("ui.theme").unwrap().as_deref(), Some("dark"));

        dst.import(&export, ImportMode::Replace).unwrap();
        assert_eq!(dst.keys().unwrap(), ["ui.theme"]);
    }

    #[test]
    fn bad_import_changes_nothing() {
        let s = Settings::open_in_memory().unwrap();
        s.set("ui.theme", "light").unwrap();
        for doc in [
            r#"{"format":"other","version":1,"exported_at":0,"settings":{}}"#,
            r#"{"format":"kinolune.settings","version":99,"exported_at":0,"settings":{}}"#,
            r#"{"format":"kinolune.settings","version":1,"exported_at":0,"settings":{"ui.theme":"x","BAD":1}}"#,
            "not json",
        ] {
            assert!(s.import_json(doc, ImportMode::Replace).is_err(), "{doc}");
        }
        assert_eq!(s.keys().unwrap(), ["ui.theme"]);
        assert_eq!(s.get::<String>("ui.theme").unwrap().as_deref(), Some("light"));
    }

    #[test]
    fn closed_store_refuses_calls() {
        let s = Settings::open_in_memory().unwrap();
        s.close().unwrap();
        s.close().unwrap();
        assert!(s.is_closed());
        assert!(matches!(s.get_value("ui.theme"), Err(SettingsError::Closed)));
        assert!(matches!(s.set("ui.theme", "x"), Err(SettingsError::Closed)));
    }
}
