// SPDX-License-Identifier: MPL-2.0
//! Bridge to the progress API of pa-library (LIB-011).
//!
//! [`Replica`] is a [`ProgressSink`]: `pa_library::progress` calls it inside
//! the transaction that writes `progress`, and every [`ProgressChange`]
//! becomes a `progress.upsert`, `watched` or `unwatch` op in the outbox of
//! the same database. [`Replica::receive_into`] goes the other way: it
//! merges a received op and writes the progress change into the library.
//!
//! ```
//! use pa_library::progress::{self, PositionReport, Thresholds};
//! use pa_model::MediaKind;
//! use pa_sync::Replica;
//!
//! let mut phone_db = pa_library::open_in_memory().unwrap();
//! let tv_db = pa_library::open_in_memory().unwrap();
//! let mut phone = Replica::new("phone").unwrap();
//! let mut tv = Replica::new("tv").unwrap();
//! let report = PositionReport {
//!     profile_id: 1,
//!     key: "tmdb:movie:603".parse().unwrap(),
//!     kind: MediaKind::Movie,
//!     position_ms: 7_900_000,
//!     duration_ms: Some(8_160_000),
//! };
//! let th = Thresholds::default();
//! progress::report_position(&mut phone_db, &mut phone, &th, &report, 1_763_000_000_000).unwrap();
//! // 96.8 %: a position op and a "watched" op are queued.
//! let queued = pa_sync::outbox::due(&phone_db, i64::MAX, 10).unwrap();
//! assert_eq!(queued.len(), 2);
//! for p in &queued {
//!     tv.receive_into(&tv_db, &p.op, 1_763_000_000_100).unwrap();
//! }
//! let item = progress::get(&tv_db, 1, &report.key).unwrap().unwrap();
//! assert!(item.watched);
//! assert_eq!(item.device_id, "phone");
//! ```

use pa_library::progress::{self, ProgressChange, ProgressSink, SinkError};
use pa_library::rusqlite::Connection;

use crate::op::{Op, OpBody};
use crate::{Error, Replica};

/// The sync op for a local progress change.
pub fn op_body(change: ProgressChange) -> OpBody {
    match change {
        ProgressChange::Position {
            profile,
            key,
            kind,
            position_ms,
            duration_ms,
        } => OpBody::ProgressUpsert {
            profile,
            key,
            kind,
            position_ms,
            duration_ms,
        },
        ProgressChange::Watched { profile, key } => OpBody::Watched { profile, key },
        ProgressChange::Unwatched { profile, key } => OpBody::Unwatch { profile, key },
    }
}

/// The progress change an op carries, if it is a progress op.
pub fn progress_change(body: &OpBody) -> Option<ProgressChange> {
    Some(match body.clone() {
        OpBody::ProgressUpsert {
            profile,
            key,
            kind,
            position_ms,
            duration_ms,
        } => ProgressChange::Position {
            profile,
            key,
            kind,
            position_ms,
            duration_ms,
        },
        OpBody::Watched { profile, key } => ProgressChange::Watched { profile, key },
        OpBody::Unwatch { profile, key } => ProgressChange::Unwatched { profile, key },
        _ => return None,
    })
}

impl ProgressSink for Replica {
    fn device_id(&self) -> &str {
        Replica::device_id(self)
    }

    fn record_change(
        &mut self,
        conn: &Connection,
        change: ProgressChange,
        now_ms: i64,
    ) -> Result<(), SinkError> {
        self.record(conn, op_body(change), now_ms)?;
        Ok(())
    }
}

impl Replica {
    /// [`Replica::receive`], then, if the op changed the merged state and is
    /// a progress op, writes it into the `progress` table of `conn`
    /// ([`progress::apply_remote`]). Returns whether the state changed.
    pub fn receive_into(&mut self, conn: &Connection, op: &Op, now_ms: i64) -> Result<bool, Error> {
        let changed = self.receive(op, now_ms)?;
        if changed && let Some(change) = progress_change(&op.body) {
            let at_ms = i64::try_from(op.hlc.wall_ms).map_err(|_| Error::Invalid("hlc"))?;
            progress::apply_remote(conn, &change, at_ms, &op.hlc.device)?;
        }
        Ok(changed)
    }
}
