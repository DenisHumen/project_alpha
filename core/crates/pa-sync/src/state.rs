// SPDX-License-Identifier: MPL-2.0
//! Merged state: one last-writer-wins register per field.
//!
//! A register keeps the value of the op with the greatest `(hlc, op_id)`.
//! That order is total, so applying the same set of ops in any order, any
//! number of times, gives the same state (commutative, idempotent). No CRDT
//! library is needed (docs/research/04 §1.8).
//!
//! "Watched" lives in its own register that only [`OpBody::Watched`] and
//! [`OpBody::Unwatch`] write: a progress update from a device that has not
//! seen the mark yet never clears it, and an unwatch wins only with a newer
//! HLC than the mark (monotonic "watched").

use std::collections::BTreeMap;

use pa_model::{MediaKey, MediaKind};

use crate::hlc::Hlc;
use crate::op::{Op, OpBody, OpId};

/// Order key of a register write.
pub type Stamp = (Hlc, OpId);

/// A last-writer-wins register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reg<T> {
    pub value: T,
    pub stamp: Stamp,
}

/// Position part of the playback state of one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub kind: MediaKind,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
}

/// Name and type of a playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistMeta {
    pub name: String,
    pub video: bool,
}

/// `(profile sync_id, id)`.
pub type ProfileKey<K> = (String, K);

/// The merged sync state of one household. Deleted values stay as `None`
/// tombstones so a late, older write cannot bring them back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncState {
    pub progress: BTreeMap<ProfileKey<MediaKey>, Reg<Position>>,
    pub watched: BTreeMap<ProfileKey<MediaKey>, Reg<bool>>,
    pub favorites: BTreeMap<ProfileKey<String>, Reg<bool>>,
    pub ratings: BTreeMap<ProfileKey<String>, Reg<Option<u8>>>,
    pub playlists: BTreeMap<ProfileKey<String>, Reg<Option<PlaylistMeta>>>,
    pub playlist_items: BTreeMap<ProfileKey<String>, Reg<Vec<MediaKey>>>,
    pub folder_rules: BTreeMap<String, Reg<Option<String>>>,
    pub settings: BTreeMap<String, Reg<Option<String>>>,
    pub twitch_follows: BTreeMap<ProfileKey<String>, Reg<bool>>,
}

/// Writes `value` if `stamp` is newer than the register. Returns true if the
/// register changed.
fn lww<K: Ord, V>(map: &mut BTreeMap<K, Reg<V>>, key: K, stamp: &Stamp, value: V) -> bool {
    match map.get(&key) {
        Some(reg) if reg.stamp >= *stamp => false,
        _ => {
            map.insert(
                key,
                Reg {
                    value,
                    stamp: stamp.clone(),
                },
            );
            true
        }
    }
}

impl SyncState {
    /// Applies one op. Returns true if any register changed; false for a
    /// duplicate or an op older than what is already known.
    pub fn apply(&mut self, op: &Op) -> bool {
        let stamp: Stamp = (op.hlc.clone(), op.op_id);
        let s = &stamp;
        match op.body.clone() {
            OpBody::ProgressUpsert {
                profile,
                key,
                kind,
                position_ms,
                duration_ms,
            } => {
                let pos = Position {
                    kind,
                    position_ms,
                    duration_ms,
                };
                lww(&mut self.progress, (profile, key), s, pos)
            }
            OpBody::Watched { profile, key } => lww(&mut self.watched, (profile, key), s, true),
            OpBody::Unwatch { profile, key } => lww(&mut self.watched, (profile, key), s, false),
            OpBody::Favorite {
                profile,
                target,
                favorite,
            } => lww(&mut self.favorites, (profile, target), s, favorite),
            OpBody::Rating {
                profile,
                target,
                rating,
            } => lww(&mut self.ratings, (profile, target), s, rating),
            OpBody::PlaylistUpsert {
                profile,
                playlist,
                name,
                video,
            } => {
                let meta = PlaylistMeta { name, video };
                lww(&mut self.playlists, (profile, playlist), s, Some(meta))
            }
            OpBody::PlaylistDelete { profile, playlist } => {
                lww(&mut self.playlists, (profile, playlist), s, None)
            }
            OpBody::PlaylistItems {
                profile,
                playlist,
                items,
            } => lww(&mut self.playlist_items, (profile, playlist), s, items),
            OpBody::FolderRule { rule_key, rule } => lww(&mut self.folder_rules, rule_key, s, rule),
            OpBody::Setting { key, value } => lww(&mut self.settings, key, s, value),
            OpBody::TwitchFollow {
                profile,
                channel_id,
                followed,
            } => lww(&mut self.twitch_follows, (profile, channel_id), s, followed),
        }
    }

    /// Whether `key` is watched on `profile`.
    pub fn is_watched(&self, profile: &str, key: &MediaKey) -> bool {
        self.watched
            .get(&(profile.to_owned(), key.clone()))
            .is_some_and(|r| r.value)
    }

    /// Items of a live (not deleted) playlist.
    pub fn playlist(&self, profile: &str, playlist: &str) -> Option<(&PlaylistMeta, &[MediaKey])> {
        let id = (profile.to_owned(), playlist.to_owned());
        let meta = self.playlists.get(&id)?.value.as_ref()?;
        let items = self
            .playlist_items
            .get(&id)
            .map_or(&[][..], |r| r.value.as_slice());
        Some((meta, items))
    }
}
