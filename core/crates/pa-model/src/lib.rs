// SPDX-License-Identifier: MPL-2.0
//! Domain types of the project_alpha core.
//!
//! Plain data shared by every other `pa-*` crate and exported through
//! `pa-ffi`. Design rules:
//!
//! - FFI-friendly: records and enums of strings, integers, `bool`, `Option`
//!   and `Vec`; no generics, lifetimes or `u128` in entity types, so they map
//!   to UniFFI records and enums as they are.
//! - Times are Unix milliseconds (`i64`), durations and positions are
//!   milliseconds (`u64`).
//! - Serde-serializable; unknown fields are ignored on read, so a payload from
//!   a newer client still loads. Sync payloads are wrapped in [`Versioned`].
//! - Row ids ([`RowId`]) are local; sync uses the stable [`MediaKey`].

mod item;
mod key;
mod music;
mod twitch;
mod video;

pub use item::{IN_PROGRESS_MAX, IN_PROGRESS_MIN, MediaItem, MediaKind, PlaybackItem};
pub use key::{MAX_KEY_LEN, MediaKey, ParseKeyError};
pub use music::{Album, Artist, Track};
pub use twitch::{BroadcasterType, Streamer, Video, VideoAvailability, VideoType};
pub use video::{
    AnimeType, Episode, EpisodeType, ExternalIds, MediaFile, NumberingScheme, RowId, Season, Work,
    WorkKind,
};

use serde::{Deserialize, Serialize};

/// Version of the serialized model. Bump on an incompatible change
/// (removed or retyped field, changed key format).
pub const SCHEMA_VERSION: u32 = 1;

/// A serialized payload tagged with the model version that wrote it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Versioned<T> {
    pub schema_version: u32,
    pub data: T,
}

/// The payload was written by a newer, incompatible model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedSchema {
    pub found: u32,
    pub supported: u32,
}

impl std::fmt::Display for UnsupportedSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "schema version {} is newer than supported {}",
            self.found, self.supported
        )
    }
}

impl std::error::Error for UnsupportedSchema {}

impl<T> Versioned<T> {
    /// Wraps `data` with the current [`SCHEMA_VERSION`].
    pub fn new(data: T) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            data,
        }
    }

    /// Returns the data if this build can read its version.
    pub fn into_supported(self) -> Result<T, UnsupportedSchema> {
        if self.schema_version == 0 || self.schema_version > SCHEMA_VERSION {
            Err(UnsupportedSchema {
                found: self.schema_version,
                supported: SCHEMA_VERSION,
            })
        } else {
            Ok(self.data)
        }
    }
}
