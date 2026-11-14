// SPDX-License-Identifier: MPL-2.0
//! Sync of progress and "watched" between paired devices (UI-019).
//!
//! The catalog does not talk to the network: a transport (the LAN node of
//! `pa-sync`) takes [`Catalog::pending_ops`], hands back acknowledgements
//! ([`Catalog::ack_op`]) and the ops of other devices
//! ([`Catalog::apply_remote`]).

use std::sync::MutexGuard;

use pa_library::progress as lib_progress;
use pa_library::rusqlite::OptionalExtension;
use pa_model::MediaKey;
use pa_sync::handoff::ResumePoint;
use pa_sync::{Op, OpId, Replica, outbox};

use crate::items::ItemRef;
use crate::progress::{PROFILE, item_of_key, key_of};
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

    /// Sync id of the profile progress is stored for (`profiles.sync_id`,
    /// the `profile` of ops and `pb.*` messages).
    pub fn sync_profile(&self) -> Result<String, Error> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT sync_id FROM profiles WHERE id = ?1",
                [PROFILE],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| "default".to_owned()))
    }

    /// The media key progress of `item` is synced under (SYNC-003: the key
    /// of `pb.update`); `None` until its file is fingerprinted.
    pub fn sync_key(&self, item: ItemRef) -> Result<Option<MediaKey>, Error> {
        let conn = self.lock();
        Ok(key_of(&conn, item)?.map(|(key, _)| key))
    }

    /// The item of this library a synced key belongs to, if any.
    pub fn item_for_key(&self, key: &MediaKey) -> Result<Option<ItemRef>, Error> {
        let conn = self.lock();
        item_of_key(&conn, key)
    }

    /// Stored progress of `item` as a resume point (SYNC-003): the last
    /// change on any device, stamped with its time and device.
    pub fn resume_point(&self, item: ItemRef) -> Result<Option<ResumePoint>, Error> {
        let conn = self.lock();
        let Some((key, _)) = key_of(&conn, item)? else {
            return Ok(None);
        };
        Ok(lib_progress::get(&conn, PROFILE, &key)?
            .filter(|p| !p.watched)
            .map(|p| ResumePoint::from_progress(&p)))
    }
}
