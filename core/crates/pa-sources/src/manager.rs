// SPDX-License-Identifier: MPL-2.0
//! CRUD of sources and their status in the library database.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pa_library::rusqlite::{Connection, OptionalExtension, Row, params};
use pa_secrets::{SecretKey, SecretStore, SecretString};

use crate::model::SpeedSample;
use crate::{
    Error, NewSource, PasswordChange, Result, Source, SourceKind, SourceOptions, SourceStatus,
    SourceUpdate, address, now_ms, scrub,
};

/// Keeps sources in the library database and their passwords in a
/// [`SecretStore`].
///
/// Methods block on SQLite, the secret store (a keychain may prompt) and,
/// for [`connect`](Self::connect), [`check`](Self::check) and
/// [`speed_test`](Self::speed_test), on the network: call them off the UI
/// thread. Network I/O runs without holding the database lock.
pub struct SourceManager {
    conn: Mutex<Connection>,
    secrets: Arc<dyn SecretStore>,
}

impl std::fmt::Debug for SourceManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceManager")
            .field("secrets", &self.secrets.backend())
            .finish_non_exhaustive()
    }
}

const COLUMNS: &str = "id, kind, name, uri, username, credential_ref, options_json, \
     scan_into_library, auto_scan, enabled, position, created_at_ms, updated_at_ms, \
     online, checked_at_ms, last_scan_started_at_ms, last_scan_finished_at_ms, \
     last_error, last_error_at_ms, speed_bytes_per_sec, speed_latency_ms, speed_measured_at_ms";

/// A row plus the key of its password, which is not part of [`Source`].
pub(crate) struct Stored {
    pub(crate) source: Source,
    pub(crate) credential_ref: Option<String>,
}

fn from_row(r: &Row<'_>) -> pa_library::rusqlite::Result<Stored> {
    use pa_library::rusqlite::types::Type;
    let bad = |i: usize, e: Error| {
        pa_library::rusqlite::Error::FromSqlConversionFailure(i, Type::Text, Box::new(e))
    };
    let kind: String = r.get(1)?;
    let kind = kind.parse::<SourceKind>().map_err(|e| bad(1, e))?;
    let options: String = r.get(6)?;
    // Unknown or damaged options fall back to defaults rather than hiding
    // the source.
    let options: SourceOptions = serde_json::from_str(&options).unwrap_or_default();
    let credential_ref: Option<String> = r.get(5)?;
    let speed = match (r.get::<_, Option<i64>>(20)?, r.get::<_, Option<i64>>(21)?) {
        (Some(latency), Some(at)) => Some(SpeedSample {
            bytes_per_sec: r
                .get::<_, Option<i64>>(19)?
                .map(|b| u64::try_from(b).unwrap_or(0)),
            latency_ms: u64::try_from(latency).unwrap_or(0),
            measured_at_ms: at,
        }),
        _ => None,
    };
    Ok(Stored {
        source: Source {
            id: r.get(0)?,
            kind,
            name: r.get(2)?,
            uri: r.get(3)?,
            username: r.get(4)?,
            has_password: credential_ref.is_some(),
            options,
            scan_into_library: r.get(7)?,
            auto_scan: r.get(8)?,
            enabled: r.get(9)?,
            position: r.get(10)?,
            created_at_ms: r.get(11)?,
            updated_at_ms: r.get(12)?,
            status: SourceStatus {
                online: r.get(13)?,
                checked_at_ms: r.get(14)?,
                last_scan_started_at_ms: r.get(15)?,
                last_scan_finished_at_ms: r.get(16)?,
                last_error: r.get(17)?,
                last_error_at_ms: r.get(18)?,
                speed,
            },
        },
        credential_ref,
    })
}

fn options_json(options: &SourceOptions) -> Result<String> {
    if let Some(pin) = &options.tls_pin {
        pin.parse::<pa_vfs_webdav::Fingerprint>()
            .map_err(|e| Error::Invalid(e.to_string()))?;
    }
    serde_json::to_string(options).map_err(|e| Error::Invalid(format!("options: {e}")))
}

fn non_empty(text: &str, what: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Invalid(format!("empty {what}")));
    }
    Ok(text.to_owned())
}

fn username(kind: SourceKind, username: Option<&str>) -> Result<Option<String>> {
    let username = username.map(str::trim).filter(|u| !u.is_empty());
    if username.is_some() && !kind.is_network() {
        return Err(Error::Invalid(format!("a {kind} source has no login")));
    }
    Ok(username.map(str::to_owned))
}

/// Fails for a password on a local source; ignores an empty password.
fn password(kind: SourceKind, password: Option<&SecretString>) -> Result<Option<&SecretString>> {
    let password = password.filter(|p| !p.is_empty());
    if password.is_some() && !kind.is_network() {
        return Err(Error::Invalid(format!("a {kind} source has no login")));
    }
    Ok(password)
}

/// A fresh secret key: random, so a reused row id never meets an old secret.
fn new_secret_key() -> Result<SecretKey> {
    Ok(SecretKey::source_password(
        &uuid::Uuid::now_v7().simple().to_string(),
    )?)
}

impl SourceManager {
    /// Manages the sources of an open, migrated library connection.
    pub fn new(conn: Connection, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            conn: Mutex::new(conn),
            secrets,
        }
    }

    /// Opens (creating and migrating if needed) the library database at
    /// `path`.
    pub fn open(path: impl AsRef<Path>, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        Ok(Self::new(pa_library::open(path)?, secrets))
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn stored(&self, id: i64) -> Result<Stored> {
        self.lock()
            .query_row(
                &format!("SELECT {COLUMNS} FROM sources WHERE id = ?1"),
                [id],
                from_row,
            )
            .optional()?
            .ok_or(Error::NotFound(id))
    }

    /// The password of a stored source, from the secret store.
    pub(crate) fn password_of(&self, stored: &Stored) -> Result<Option<SecretString>> {
        match &stored.credential_ref {
            Some(key) => Ok(self.secrets.get(&SecretKey::new(key.clone())?)?),
            None => Ok(None),
        }
    }

    /// Adds a source. The password goes to the secret store; if that fails
    /// nothing is added.
    pub fn create(&self, new: NewSource) -> Result<Source> {
        let name = non_empty(&new.name, "name")?;
        let uri = address::normalize(new.kind, &new.address)?;
        let username = username(new.kind, new.username.as_deref())?;
        let password = password(new.kind, new.password.as_ref())?;
        let options = options_json(&new.options)?;
        let key = password.map(|_| new_secret_key()).transpose()?;

        let id = {
            let mut conn = self.lock();
            let tx = conn.transaction()?;
            let now = now_ms();
            tx.execute(
                "INSERT INTO sources (kind, name, uri, username, credential_ref, options_json,
                     scan_into_library, auto_scan, enabled, position, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                     (SELECT ifnull(max(position), 0) + 1 FROM sources), ?10, ?10)",
                params![
                    new.kind.as_str(),
                    name,
                    uri,
                    username,
                    key.as_ref().map(SecretKey::as_str),
                    options,
                    new.scan_into_library,
                    new.auto_scan,
                    new.enabled,
                    now,
                ],
            )?;
            let id = tx.last_insert_rowid();
            if let (Some(key), Some(password)) = (&key, password) {
                self.secrets.set(key, password)?;
            }
            tx.commit()?;
            id
        };
        self.get(id)
    }

    /// The source with this id.
    pub fn get(&self, id: i64) -> Result<Source> {
        Ok(self.stored(id)?.source)
    }

    /// All sources in display order.
    pub fn list(&self) -> Result<Vec<Source>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM sources ORDER BY position, name, id"
        ))?;
        let rows = stmt.query_map([], |r| from_row(r).map(|s| s.source))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Sources an automatic scan should include: enabled, scanned into the
    /// library and with auto-scan on.
    pub fn auto_scan_sources(&self) -> Result<Vec<Source>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(Source::wants_auto_scan)
            .collect())
    }

    /// Changes the settings of a source.
    pub fn update(&self, id: i64, update: SourceUpdate) -> Result<Source> {
        let stored = self.stored(id)?;
        let mut s = stored.source;
        let kind = s.kind;
        if let Some(name) = &update.name {
            s.name = non_empty(name, "name")?;
        }
        if let Some(address) = &update.address {
            s.uri = address::normalize(kind, address)?;
        }
        if let Some(name) = &update.username {
            s.username = username(kind, name.as_deref())?;
        }
        let options = options_json(update.options.as_ref().unwrap_or(&s.options))?;
        s.scan_into_library = update.scan_into_library.unwrap_or(s.scan_into_library);
        s.auto_scan = update.auto_scan.unwrap_or(s.auto_scan);
        s.enabled = update.enabled.unwrap_or(s.enabled);

        let old_key = stored.credential_ref;
        let (credential_ref, set) = match &update.password {
            PasswordChange::Keep => (old_key.clone(), None),
            PasswordChange::Clear => (None, None),
            PasswordChange::Set(p) => match password(kind, Some(p))? {
                Some(p) => (Some(new_secret_key()?.as_str().to_owned()), Some(p)),
                None => (None, None),
            },
        };

        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE sources SET name = ?2, uri = ?3, username = ?4, credential_ref = ?5,
                 options_json = ?6, scan_into_library = ?7, auto_scan = ?8, enabled = ?9,
                 updated_at_ms = ?10
             WHERE id = ?1",
            params![
                id,
                s.name,
                s.uri,
                s.username,
                credential_ref,
                options,
                s.scan_into_library,
                s.auto_scan,
                s.enabled,
                now_ms(),
            ],
        )?;
        if let (Some(key), Some(p)) = (&credential_ref, set) {
            self.secrets.set(&SecretKey::new(key.clone())?, p)?;
        }
        tx.commit()?;
        drop(conn);
        // The row no longer points at the old secret; a failure here only
        // leaves an orphaned keychain item.
        if let Some(old) = old_key.filter(|k| credential_ref.as_ref() != Some(k)) {
            let _ = self.secrets.delete(&SecretKey::new(old)?);
        }
        self.get(id)
    }

    /// Deletes a source and its password. Files of the source go with it
    /// (`ON DELETE CASCADE`); works left without files are removed by the
    /// catalog (`pa-catalog`).
    pub fn delete(&self, id: i64) -> Result<()> {
        let stored = self.stored(id)?;
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM sources WHERE id = ?1", [id])?;
        if let Some(key) = &stored.credential_ref {
            self.secrets.delete(&SecretKey::new(key.clone())?)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Records the start of a scan.
    pub fn record_scan_started(&self, id: i64) -> Result<()> {
        self.update_status(
            id,
            "last_scan_started_at_ms = ?2, last_scan_finished_at_ms = NULL",
            now_ms(),
        )
    }

    /// Records the end of a scan; `error` (masked) becomes the last error.
    pub fn record_scan_finished(&self, id: i64, error: Option<&str>) -> Result<()> {
        let now = now_ms();
        self.update_status(id, "last_scan_finished_at_ms = ?2", now)?;
        if let Some(error) = error {
            let stored = self.stored(id)?;
            let password = self.password_of(&stored).ok().flatten();
            self.record_error(id, &scrub(error, password.as_ref()), now)?;
        }
        Ok(())
    }

    /// Records the result of a reachability check.
    pub(crate) fn record_check(&self, id: i64, error: Option<&str>) -> Result<()> {
        let now = now_ms();
        let conn = self.lock();
        conn.execute(
            "UPDATE sources SET online = ?2, checked_at_ms = ?3 WHERE id = ?1",
            params![id, error.is_none(), now],
        )?;
        drop(conn);
        if let Some(error) = error {
            self.record_error(id, error, now)?;
        }
        Ok(())
    }

    pub(crate) fn record_speed(&self, id: i64, sample: &SpeedSample) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE sources SET online = 1, checked_at_ms = ?4, speed_bytes_per_sec = ?2,
                 speed_latency_ms = ?3, speed_measured_at_ms = ?4
             WHERE id = ?1",
            params![
                id,
                sample
                    .bytes_per_sec
                    .map(|b| i64::try_from(b).unwrap_or(i64::MAX)),
                i64::try_from(sample.latency_ms).unwrap_or(i64::MAX),
                sample.measured_at_ms,
            ],
        )?;
        Ok(())
    }

    /// `error` must already be masked.
    fn record_error(&self, id: i64, error: &str, at: i64) -> Result<()> {
        self.lock().execute(
            "UPDATE sources SET last_error = ?2, last_error_at_ms = ?3 WHERE id = ?1",
            params![id, error, at],
        )?;
        Ok(())
    }

    fn update_status(&self, id: i64, set: &str, at: i64) -> Result<()> {
        let n = self.lock().execute(
            &format!("UPDATE sources SET {set} WHERE id = ?1"),
            params![id, at],
        )?;
        if n == 0 {
            return Err(Error::NotFound(id));
        }
        Ok(())
    }
}
