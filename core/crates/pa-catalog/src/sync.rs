// SPDX-License-Identifier: MPL-2.0
//! Sync of progress and "watched" between paired devices (UI-019).
//!
//! The catalog does not talk to the network: a transport (the LAN node of
//! `pa-sync`) takes [`Catalog::pending_ops`], hands back acknowledgements
//! ([`Catalog::ack_op`]) and the ops of other devices
//! ([`Catalog::apply_remote`]).

use std::sync::MutexGuard;

use pa_sync::{Op, OpId, Replica, outbox};

use crate::{Catalog, Error, now_ms};

impl Catalog {
    pub(crate) fn sync_lock(&self) -> MutexGuard<'_, Option<Replica>> {
        self.sync.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Turns sync on for this device: from now on progress changes are
    /// recorded as ops of `device_id` in the outbox. The clock continues
    /// after the newest queued op. Calling it again with the same id keeps
    /// the replica.
    pub fn enable_sync(&self, device_id: &str) -> Result<(), Error> {
        let conn = self.lock();
        let mut sync = self.sync_lock();
        if sync.as_ref().is_some_and(|r| r.device_id() == device_id) {
            return Ok(());
        }
        let mut replica = Replica::new(device_id)?;
        if let Some(hlc) = outbox::newest_hlc(&conn)? {
            let now = u64::try_from(now_ms()).unwrap_or(0);
            // A stamp from a clock far ahead is ignored, not fatal.
            let _ = replica.clock_mut().observe(&hlc, now);
        }
        outbox::compact(&conn)?;
        *sync = Some(replica);
        Ok(())
    }

    /// Turns sync off (the device left its family) and drops the outbox:
    /// nobody is left to send it to.
    pub fn disable_sync(&self) -> Result<(), Error> {
        let conn = self.lock();
        *self.sync_lock() = None;
        conn.execute("DELETE FROM sync_outbox", [])?;
        Ok(())
    }

    /// Device id of the sync replica, if sync is on.
    pub fn sync_device_id(&self) -> Option<String> {
        self.sync_lock().as_ref().map(|r| r.device_id().to_owned())
    }

    /// Queued local ops, oldest first, after dropping the ones a newer op of
    /// the same register replaced.
    pub fn pending_ops(&self, limit: u32) -> Result<Vec<Op>, Error> {
        let conn = self.lock();
        outbox::compact(&conn)?;
        Ok(outbox::due(&conn, i64::MAX, limit)?
            .into_iter()
            .map(|p| p.op)
            .collect())
    }

    /// Removes an op another device holds now. `false` if it was not queued.
    pub fn ack_op(&self, op_id: OpId) -> Result<bool, Error> {
        let conn = self.lock();
        Ok(outbox::ack(&conn, op_id)?)
    }

    /// Writes an op of another device into the library. Returns whether
    /// anything changed. Without sync on, nothing is applied.
    pub fn apply_remote(&self, op: &Op) -> Result<bool, Error> {
        let conn = self.lock();
        let mut sync = self.sync_lock();
        let Some(replica) = sync.as_mut() else {
            return Ok(false);
        };
        Ok(replica.receive_into(&conn, op, now_ms())?)
    }
}
