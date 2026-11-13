// SPDX-License-Identifier: MPL-2.0
//! Serverless sync of user state between devices (ADR-0004).
//!
//! - [`hlc`]: hybrid logical clock; timestamps order all ops without a server.
//! - [`op`]: sync operations with a UUIDv7 [`OpId`] (progress, watched,
//!   favorites, ratings, playlists, folder rules, settings without secrets,
//!   Twitch follows).
//! - [`SyncState`]: last-writer-wins register per field; any order of the
//!   same ops gives the same state.
//! - [`outbox`]: the SQLite outbox with idempotent enqueue and retries.
//! - [`Replica`]: ties them together for one device.
//! - [`proto`]: the wire protocol (`docs/api/sync-protocol.md`): handshake
//!   with version negotiation, `pb.update`/`pb.takeover` for instant resume,
//!   op exchange with opaque versioned payloads, acks and journal cursors.
//! - [`library`]: `Replica` as the sync sink of `pa_library::progress`
//!   (LIB-011) and writing received progress ops into the library.
//!
//! Transports (LAN P2P, journal in a shared folder, the user's own server,
//! iroh) are later tasks; they move [`proto`] frames and call
//! [`Replica::receive`].
//!
//! ```
//! use pa_model::{MediaKey, MediaKind};
//! use pa_sync::{OpBody, Replica};
//!
//! let conn = pa_library::open_in_memory().unwrap();
//! let mut phone = Replica::new("phone").unwrap();
//! let mut tv = Replica::new("tv").unwrap();
//! let key: MediaKey = "tmdb:movie:603".parse().unwrap();
//! let op = phone
//!     .record(&conn, OpBody::ProgressUpsert {
//!         profile: "default".into(),
//!         key: key.clone(),
//!         kind: MediaKind::Movie,
//!         position_ms: 2_471_200,
//!         duration_ms: Some(8_160_000),
//!     }, 1_763_000_000_000)
//!     .unwrap();
//! tv.receive(&op, 1_763_000_000_100).unwrap();
//! let pos = &tv.state().progress[&("default".to_owned(), key)];
//! assert_eq!(pos.value.position_ms, 2_471_200);
//! assert_eq!(pa_sync::outbox::len(&conn).unwrap(), 1);
//! ```

pub mod hlc;
pub mod library;
pub mod op;
pub mod outbox;
pub mod proto;
mod state;

pub use hlc::{Hlc, HlcClock};
pub use op::{Op, OpBody, OpId};
pub use state::{PlaylistMeta, Position, ProfileKey, Reg, Stamp, SyncState};

use pa_library::rusqlite::{self, Connection};

/// Error of pa-sync.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A field of an op or an id is not acceptable.
    #[error("invalid {0}")]
    Invalid(&'static str),
    /// A peer's clock is too far ahead ([`hlc::MAX_DRIFT_MS`]).
    #[error("remote clock {remote_ms} ms is too far ahead of local {local_ms} ms")]
    ClockDrift { remote_ms: u64, local_ms: u64 },
    /// No protocol version in common (`(min, max)` of each side).
    #[error("no common protocol version: local {local:?}, remote {remote:?}")]
    Incompatible {
        local: (u16, u16),
        remote: (u16, u16),
    },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Writing a received change into the library failed.
    #[error(transparent)]
    Library(#[from] pa_library::Error),
}

/// Sync state of one device: its clock and the merged state. The state is
/// in memory; persisting it into library tables is up to the caller.
#[derive(Debug, Clone)]
pub struct Replica {
    clock: HlcClock,
    state: SyncState,
}

impl Replica {
    /// A replica for `device_id` with an empty state.
    pub fn new(device_id: impl Into<String>) -> Result<Self, Error> {
        Ok(Self {
            clock: HlcClock::new(device_id)?,
            state: SyncState::default(),
        })
    }

    /// Device id of this replica.
    pub fn device_id(&self) -> &str {
        self.clock.device()
    }

    /// The merged state.
    pub fn state(&self) -> &SyncState {
        &self.state
    }

    /// The clock (e.g. to restore it after restart with
    /// [`HlcClock::observe`] of the last saved stamp).
    pub fn clock_mut(&mut self) -> &mut HlcClock {
        &mut self.clock
    }

    /// Records a local change at `now_ms` (Unix ms): stamps it, applies it
    /// and queues it in the outbox of `conn`. To keep a library table and the
    /// outbox consistent, call it inside the caller's transaction.
    pub fn record(&mut self, conn: &Connection, body: OpBody, now_ms: i64) -> Result<Op, Error> {
        body.validate()?;
        let hlc = self.clock.tick(u64::try_from(now_ms).unwrap_or(0));
        let op = Op {
            op_id: OpId::new_v7(),
            hlc,
            body,
        };
        outbox::enqueue(conn, &op, now_ms)?;
        self.state.apply(&op);
        Ok(op)
    }

    /// Applies an op from another device. Returns true if the state changed;
    /// duplicates and stale ops return false.
    pub fn receive(&mut self, op: &Op, now_ms: i64) -> Result<bool, Error> {
        op.body.validate()?;
        if !hlc::is_valid_device_id(&op.hlc.device) {
            return Err(Error::Invalid("device id"));
        }
        self.clock
            .observe(&op.hlc, u64::try_from(now_ms).unwrap_or(0))?;
        Ok(self.state.apply(op))
    }
}
