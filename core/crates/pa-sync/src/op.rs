// SPDX-License-Identifier: MPL-2.0
//! Sync operations: what one device tells the others.
//!
//! Every user change becomes an [`Op`]: a unique [`OpId`] (UUIDv7), the
//! [`Hlc`] timestamp of the change and an [`OpBody`]. Ops carry only stable
//! ids (`profiles.sync_id`, [`MediaKey`], external refs), never local row ids,
//! paths of files or secrets (ADR-0004, docs/research/04 §1.8).

use std::fmt;
use std::str::FromStr;

use pa_model::{MediaKey, MediaKind};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::Error;
use crate::hlc::Hlc;

/// Maximum length of an id or key inside an op (profile, ref, setting key).
pub const MAX_REF_LEN: usize = 256;

/// Maximum rating value (ratings are `0..=10`).
pub const MAX_RATING: u8 = 10;

/// Unique id of an operation: a UUIDv7, so ids also sort by creation time.
/// Receivers deduplicate by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpId(pub Uuid);

impl OpId {
    /// A fresh UUIDv7 from the system clock and a random tail.
    pub fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for OpId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl FromStr for OpId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Uuid::try_parse(s)
            .map(Self)
            .map_err(|_| Error::Invalid("op id"))
    }
}

impl Serialize for OpId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for OpId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// One operation. Wire form (JSON):
/// `{"op_id":"…","hlc":{…},"op":"progress.upsert","profile":"default",…}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    pub op_id: OpId,
    pub hlc: Hlc,
    #[serde(flatten)]
    pub body: OpBody,
}

/// What changed. Each variant writes one last-writer-wins register (see
/// [`crate::SyncState`]); `profile` is `profiles.sync_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum OpBody {
    /// Playback position of an item. Never changes "watched".
    #[serde(rename = "progress.upsert")]
    ProgressUpsert {
        profile: String,
        key: MediaKey,
        kind: MediaKind,
        position_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    /// Mark an item watched.
    #[serde(rename = "watched")]
    Watched { profile: String, key: MediaKey },
    /// Explicitly clear "watched"; wins only if newer than the last mark.
    #[serde(rename = "unwatch")]
    Unwatch { profile: String, key: MediaKey },
    /// Add (`true`) or remove a favorite. `target` is a media key or a stable
    /// work ref such as `tmdb:tv:1399`.
    #[serde(rename = "favorite.set")]
    Favorite {
        profile: String,
        target: String,
        favorite: bool,
    },
    /// Set (`0..=10`) or clear a rating of a media key or work ref.
    #[serde(rename = "rating.set")]
    Rating {
        profile: String,
        target: String,
        rating: Option<u8>,
    },
    /// Create or rename a playlist; `playlist` is a UUID made by its creator.
    #[serde(rename = "playlist.upsert")]
    PlaylistUpsert {
        profile: String,
        playlist: String,
        name: String,
        video: bool,
    },
    /// Delete a playlist (a later upsert with a newer HLC restores it).
    #[serde(rename = "playlist.delete")]
    PlaylistDelete { profile: String, playlist: String },
    /// Replace the item list of a playlist (the list is one register).
    #[serde(rename = "playlist.items")]
    PlaylistItems {
        profile: String,
        playlist: String,
        items: Vec<MediaKey>,
    },
    /// Set or delete (`rule: None`) a folder rule of META-017. `rule_key`
    /// identifies the folder by a stable ref, `rule` is opaque JSON.
    #[serde(rename = "folder_rule.set")]
    FolderRule {
        rule_key: String,
        rule: Option<String>,
    },
    /// Set or reset (`value: None`) a setting; `value` is JSON text.
    /// Secrets are rejected by [`OpBody::validate`].
    #[serde(rename = "setting.set")]
    Setting { key: String, value: Option<String> },
    /// Follow or unfollow a Twitch channel locally (Twitch favorites).
    #[serde(rename = "twitch_follow.set")]
    TwitchFollow {
        profile: String,
        channel_id: String,
        followed: bool,
    },
}

/// Words that mark a setting key as a secret; such settings never sync in
/// clear text (SYNC-010 adds an E2E container for them).
const SECRET_PARTS: [&str; 5] = ["password", "passwd", "secret", "token", "credential"];
/// Whole words (between non-alphanumerics) that mark a secret.
const SECRET_WORDS: [&str; 3] = ["pin", "key", "apikey"];

/// True if a setting with this key must not go into a sync op.
pub fn is_secret_setting(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SECRET_PARTS.iter().any(|p| key.contains(p))
        || key
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| SECRET_WORDS.contains(&w))
}

pub(crate) fn check_ref(s: &str, what: &'static str) -> Result<(), Error> {
    if s.is_empty() || s.len() > MAX_REF_LEN || s.chars().any(char::is_control) {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

fn check_json(s: &str, what: &'static str) -> Result<(), Error> {
    serde_json::from_str::<serde_json::Value>(s)
        .map(|_| ())
        .map_err(|_| Error::Invalid(what))
}

impl OpBody {
    /// Checks ids, ranges and the no-secrets rule. Ops are validated before
    /// they are recorded and when they are received.
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::ProgressUpsert { profile, key, .. }
            | Self::Watched { profile, key }
            | Self::Unwatch { profile, key } => {
                check_ref(profile, "profile")?;
                key.is_valid()
                    .then_some(())
                    .ok_or(Error::Invalid("media key"))
            }
            Self::Favorite {
                profile, target, ..
            } => {
                check_ref(profile, "profile")?;
                check_ref(target, "target")
            }
            Self::Rating {
                profile,
                target,
                rating,
            } => {
                check_ref(profile, "profile")?;
                check_ref(target, "target")?;
                match rating {
                    Some(r) if *r > MAX_RATING => Err(Error::Invalid("rating")),
                    _ => Ok(()),
                }
            }
            Self::PlaylistUpsert {
                profile,
                playlist,
                name,
                ..
            } => {
                check_ref(profile, "profile")?;
                check_ref(playlist, "playlist")?;
                check_ref(name, "playlist name")
            }
            Self::PlaylistDelete { profile, playlist } => {
                check_ref(profile, "profile")?;
                check_ref(playlist, "playlist")
            }
            Self::PlaylistItems {
                profile,
                playlist,
                items,
            } => {
                check_ref(profile, "profile")?;
                check_ref(playlist, "playlist")?;
                items
                    .iter()
                    .all(MediaKey::is_valid)
                    .then_some(())
                    .ok_or(Error::Invalid("media key"))
            }
            Self::FolderRule { rule_key, rule } => {
                check_ref(rule_key, "folder rule key")?;
                rule.as_deref()
                    .map_or(Ok(()), |r| check_json(r, "folder rule"))
            }
            Self::Setting { key, value } => {
                check_ref(key, "setting key")?;
                if is_secret_setting(key) {
                    return Err(Error::Invalid("secret setting"));
                }
                value
                    .as_deref()
                    .map_or(Ok(()), |v| check_json(v, "setting value"))
            }
            Self::TwitchFollow {
                profile,
                channel_id,
                ..
            } => {
                check_ref(profile, "profile")?;
                check_ref(channel_id, "channel id")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_settings_are_detected() {
        assert!(is_secret_setting("source.nas.password"));
        assert!(is_secret_setting("twitch/AccessToken"));
        assert!(is_secret_setting("tmdb.api_key"));
        assert!(is_secret_setting("parental.pin"));
        assert!(!is_secret_setting("player.subtitle_size"));
        assert!(!is_secret_setting("home.pinned_rows"));
        assert!(
            OpBody::Setting {
                key: "smb.password".into(),
                value: Some("\"x\"".into())
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn op_id_round_trips() {
        let id = OpId::new_v7();
        assert_eq!(id.0.get_version_num(), 7);
        assert_eq!(id.to_string().parse::<OpId>().unwrap(), id);
        assert!("nope".parse::<OpId>().is_err());
    }
}
