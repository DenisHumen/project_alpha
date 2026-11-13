// SPDX-License-Identifier: MPL-2.0
//! Hub journal in SQLite: the persistent twin of `pa_sync::proto::Journal`.
//!
//! Same semantics as the reference: ops in arrival order, `seq` from 1,
//! deduplicated by `op_id` (a duplicate keeps its first `seq`), payloads
//! stored opaque. The journal lives in its own file (`sync.sqlite3`), not in
//! the library database: it is hub state, not library state, and the library
//! schema and migrations are shared with the apps.

use std::path::Path;

use pa_library::rusqlite::{self, Connection, OptionalExtension, params};
use pa_sync::OpId;
use pa_sync::proto::{Cursor, MAX_PULL_LIMIT, OpFrame, Payload, Pull, PullEnd};

/// Schema version in `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS hub_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS journal (
    seq         INTEGER PRIMARY KEY,
    op_id       TEXT NOT NULL UNIQUE,
    enc         TEXT NOT NULL,
    body        TEXT NOT NULL,
    received_ms INTEGER NOT NULL
) STRICT;
";

/// Error of the journal database.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// The file was written by a newer server.
    #[error("sync journal schema {found} is newer than {SCHEMA_VERSION}")]
    TooNew { found: i64 },
    /// A stored id is not valid.
    #[error("corrupt sync journal: {0}")]
    Corrupt(&'static str),
}

/// The journal of this hub and its identity.
#[derive(Debug)]
pub struct SqliteJournal {
    conn: Connection,
    log: String,
    hub_id: String,
    head: u64,
}

impl SqliteJournal {
    /// Opens (creating if needed) the journal file.
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // A crash may lose the last ops but never corrupts the file; the
        // devices resend everything they did not see acked.
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Self::init(conn)
    }

    /// An in-memory journal (tests).
    pub fn open_in_memory() -> Result<Self, JournalError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, JournalError> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(JournalError::TooNew { found: version });
        }
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        let log = meta_or_insert(&conn, "log_id")?;
        let hub_id = meta_or_insert(&conn, "hub_id")?;
        for id in [&log, &hub_id] {
            if !pa_sync::hlc::is_valid_device_id(id) {
                return Err(JournalError::Corrupt("id"));
            }
        }
        let head: i64 = conn.query_row("SELECT coalesce(max(seq), 0) FROM journal", [], |r| {
            r.get(0)
        })?;
        Ok(Self {
            conn,
            log,
            hub_id,
            head: u64::try_from(head).map_err(|_| JournalError::Corrupt("seq"))?,
        })
    }

    /// Stable `device_id` of this hub (sent in its `hello`).
    pub fn hub_id(&self) -> &str {
        &self.hub_id
    }

    /// The cursor after the last op.
    pub fn head(&self) -> Cursor {
        Cursor {
            log: self.log.clone(),
            seq: self.head,
        }
    }

    /// Stores an op frame. Returns its `seq` and whether it is new; a
    /// duplicate keeps its first `seq`, so the ack is the same.
    pub fn append(&mut self, frame: &OpFrame, now_ms: i64) -> Result<(u64, bool), JournalError> {
        let op_id = frame.op_id.to_string();
        if let Some(seq) = self.seq_of(&op_id)? {
            return Ok((seq, false));
        }
        let seq = self.head + 1;
        self.conn.execute(
            "INSERT INTO journal (seq, op_id, enc, body, received_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                i64::try_from(seq).map_err(|_| JournalError::Corrupt("seq"))?,
                op_id,
                frame.payload.enc,
                frame.payload.body,
                now_ms
            ],
        )?;
        self.head = seq;
        Ok((seq, true))
    }

    fn seq_of(&self, op_id: &str) -> Result<Option<u64>, JournalError> {
        let seq: Option<i64> = self
            .conn
            .query_row(
                "SELECT seq FROM journal WHERE op_id = ?1",
                params![op_id],
                |r| r.get(0),
            )
            .optional()?;
        seq.map(|s| u64::try_from(s).map_err(|_| JournalError::Corrupt("seq")))
            .transpose()
    }

    /// Answers a [`Pull`] like `pa_sync::proto::Journal::pull`: up to `limit`
    /// frames after the cursor (from the start for another journal) and the
    /// closing [`PullEnd`].
    pub fn pull(&self, pull: &Pull) -> Result<(Vec<OpFrame>, PullEnd), JournalError> {
        let after = match &pull.cursor {
            Some(c) if c.log == self.log => c.seq.min(self.head),
            _ => 0,
        };
        let limit = pull.limit.clamp(1, MAX_PULL_LIMIT);
        let mut stmt = self.conn.prepare_cached(
            "SELECT seq, op_id, enc, body FROM journal WHERE seq > ?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            params![i64::try_from(after).unwrap_or(i64::MAX), limit],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )?;
        let mut frames = Vec::new();
        for row in rows {
            let (seq, op_id, enc, body) = row?;
            frames.push(OpFrame {
                op_id: op_id
                    .parse::<OpId>()
                    .map_err(|_| JournalError::Corrupt("op id"))?,
                seq: Some(u64::try_from(seq).map_err(|_| JournalError::Corrupt("seq"))?),
                payload: Payload { enc, body },
            });
        }
        let seq = frames.last().and_then(|f| f.seq).unwrap_or(after);
        let end = PullEnd {
            cursor: Cursor {
                log: self.log.clone(),
                seq,
            },
            more: seq < self.head,
        };
        Ok((frames, end))
    }

    /// Moves the WAL into the main file (on shutdown).
    pub fn checkpoint(&self) -> Result<(), JournalError> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }
}

/// Reads a meta value or stores a fresh UUID under `key`.
fn meta_or_insert(conn: &Connection, key: &str) -> Result<String, JournalError> {
    let fresh = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT OR IGNORE INTO hub_meta (key, value) VALUES (?1, ?2)",
        params![key, fresh],
    )?;
    Ok(conn.query_row(
        "SELECT value FROM hub_meta WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use pa_sync::proto::Journal;

    use super::*;

    fn frame(n: u8) -> OpFrame {
        OpFrame {
            op_id: OpId(uuid::Uuid::from_bytes([n; 16])),
            seq: None,
            payload: Payload {
                enc: "secretbox.v1".to_owned(),
                body: format!("opaque-{n}"),
            },
        }
    }

    /// The SQLite journal answers exactly like the reference journal.
    #[test]
    fn matches_reference_journal() {
        let mut db = SqliteJournal::open_in_memory().unwrap();
        let mut reference = Journal::new(db.head().log).unwrap();
        for n in [1, 2, 3, 2, 4, 1, 5] {
            let f = frame(n);
            assert_eq!(db.append(&f, 0).unwrap(), reference.append(&f));
        }
        assert_eq!(db.head(), reference.head());
        let other = Cursor {
            log: "other-log".to_owned(),
            seq: 2,
        };
        let mine = |seq| Cursor {
            log: db.head().log,
            seq,
        };
        for cursor in [None, Some(other), Some(mine(0)), Some(mine(2)), Some(mine(5)), Some(mine(99))] {
            for limit in [1, 2, 10] {
                let pull = Pull {
                    cursor: cursor.clone(),
                    limit,
                };
                assert_eq!(db.pull(&pull).unwrap(), reference.pull(&pull), "{pull:?}");
            }
        }
    }

    #[test]
    fn survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sync.sqlite3");
        let (head, hub) = {
            let mut db = SqliteJournal::open(&path).unwrap();
            db.append(&frame(1), 0).unwrap();
            db.append(&frame(2), 0).unwrap();
            db.checkpoint().unwrap();
            (db.head(), db.hub_id().to_owned())
        };
        let mut db = SqliteJournal::open(&path).unwrap();
        assert_eq!(db.head(), head);
        assert_eq!(db.hub_id(), hub);
        assert_eq!(db.append(&frame(1), 0).unwrap(), (1, false));
        assert_eq!(db.append(&frame(3), 0).unwrap(), (3, true));
    }
}
