// SPDX-License-Identifier: MPL-2.0
//! Offline outbox: the `sync_outbox` table of the library database.
//!
//! Every local op is written here in the same transaction as the local state
//! change and stays until a peer or hub acknowledges it. Sending is
//! idempotent: receivers deduplicate by `op_id`, and [`enqueue`] ignores an
//! op that is already queued. Failed sends retry with exponential backoff
//! ([`backoff_ms`]); ops are never dropped. Ops commute, so the send order is
//! not important; [`due`] returns them oldest first anyway.

use pa_library::rusqlite::{Connection, OptionalExtension, params};
use pa_model::Versioned;

use crate::Error;
use crate::op::{Op, OpId};

/// Delay before the first retry.
pub const BACKOFF_BASE_MS: i64 = 1_000;
/// Upper bound of the retry delay.
pub const BACKOFF_MAX_MS: i64 = 5 * 60 * 1_000;

/// A queued op and how many sends of it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub op: Op,
    pub attempts: u32,
}

/// Retry delay after `attempts` failed sends: 1 s, 2 s, 4 s… up to 5 min.
pub fn backoff_ms(attempts: u32) -> i64 {
    let shift = attempts.saturating_sub(1).min(20);
    (BACKOFF_BASE_MS << shift).min(BACKOFF_MAX_MS)
}

/// Encodes an op as the versioned JSON payload stored in the outbox and
/// sent to peers.
pub fn encode(op: &Op) -> Result<String, Error> {
    Ok(serde_json::to_string(&Versioned::new(op))?)
}

/// Decodes and validates a payload made by [`encode`] on this or another
/// device. Fails on a payload from a newer, incompatible model.
pub fn decode(payload: &str) -> Result<Op, Error> {
    let versioned: Versioned<Op> = serde_json::from_str(payload)?;
    let op = versioned
        .into_supported()
        .map_err(|_| Error::Invalid("schema version"))?;
    op.body.validate()?;
    Ok(op)
}

/// Queues `op` for sending now. Returns false if it was already queued.
pub fn enqueue(conn: &Connection, op: &Op, now_ms: i64) -> Result<bool, Error> {
    let payload = encode(op)?;
    let kind = serde_json::to_value(&op.body)?["op"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let n = conn.execute(
        "INSERT OR IGNORE INTO sync_outbox (op_id, op, payload, hlc, next_attempt_at_ms, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![op.op_id.to_string(), kind, payload, op.hlc.to_string(), now_ms],
    )?;
    Ok(n == 1)
}

/// Up to `limit` ops whose next attempt is due at `now_ms`, oldest first.
pub fn due(conn: &Connection, now_ms: i64, limit: u32) -> Result<Vec<Pending>, Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT payload, attempts FROM sync_outbox
         WHERE next_attempt_at_ms <= ?1 ORDER BY seq LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![now_ms, limit], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
    })?;
    rows.map(|row| {
        let (payload, attempts) = row?;
        Ok(Pending {
            op: decode(&payload)?,
            attempts,
        })
    })
    .collect()
}

/// Removes an acknowledged op. Returns false if it was not queued (a
/// duplicate ack).
pub fn ack(conn: &Connection, op_id: OpId) -> Result<bool, Error> {
    let n = conn.execute(
        "DELETE FROM sync_outbox WHERE op_id = ?1",
        params![op_id.to_string()],
    )?;
    Ok(n == 1)
}

/// Records a failed send and schedules the next attempt. Returns the new
/// attempt count, or `None` if the op is not queued.
pub fn fail(
    conn: &Connection,
    op_id: OpId,
    now_ms: i64,
    error: &str,
) -> Result<Option<u32>, Error> {
    let id = op_id.to_string();
    let attempts: Option<u32> = conn
        .query_row(
            "SELECT attempts FROM sync_outbox WHERE op_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(attempts) = attempts.map(|a| a.saturating_add(1)) else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE sync_outbox SET attempts = ?2, next_attempt_at_ms = ?3, last_error = ?4
         WHERE op_id = ?1",
        params![
            id,
            attempts,
            now_ms.saturating_add(backoff_ms(attempts)),
            error
        ],
    )?;
    Ok(Some(attempts))
}

/// Number of queued ops.
pub fn len(conn: &Connection) -> Result<u64, Error> {
    let n: i64 = conn.query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get(0))?;
    Ok(n.unsigned_abs())
}

/// Time of the earliest pending attempt, to schedule the next send.
pub fn next_attempt_at(conn: &Connection) -> Result<Option<i64>, Error> {
    Ok(
        conn.query_row("SELECT min(next_attempt_at_ms) FROM sync_outbox", [], |r| {
            r.get(0)
        })?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_is_capped() {
        assert_eq!(backoff_ms(0), 1_000);
        assert_eq!(backoff_ms(1), 1_000);
        assert_eq!(backoff_ms(2), 2_000);
        assert_eq!(backoff_ms(4), 8_000);
        assert_eq!(backoff_ms(9), 256_000);
        assert_eq!(backoff_ms(10), BACKOFF_MAX_MS);
        assert_eq!(backoff_ms(u32::MAX), BACKOFF_MAX_MS);
    }
}
