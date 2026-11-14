// SPDX-License-Identifier: MPL-2.0
//! Types of versions, collections and playlists (LIB-010) for Dart. The
//! calls are methods of [`CoreLibrary`](crate::api::library::CoreLibrary).

use pa_catalog as c;

/// One version of a movie or episode: a file (or a disc, a split file) that
/// plays the whole item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreVersion {
    /// Id to pass to `set_default_version`.
    pub file_id: i64,
    /// `4K · HDR10 · Director's Cut`, else the release source or the file
    /// name (not translated: written in the release names).
    pub label: String,
    pub source_id: i64,
    /// Path relative to the source root.
    pub rel_path: String,
    /// Absolute path for local sources.
    pub local_path: Option<String>,
    pub size_bytes: i64,
    pub duration_ms: Option<i64>,
    /// `4K`, `1080p`, `720p`, `SD`, or as written in the name.
    pub resolution: Option<String>,
    pub hdr: Option<String>,
    pub edition: Option<String>,
    /// The version the item plays.
    pub is_default: bool,
}

impl From<c::Version> for CoreVersion {
    fn from(v: c::Version) -> Self {
        Self {
            label: v.label(),
            file_id: v.file_id,
            source_id: v.source_id,
            rel_path: v.rel_path,
            local_path: v.local_path,
            size_bytes: v.size_bytes,
            duration_ms: v.duration_ms,
            resolution: v.resolution,
            hdr: v.hdr,
            edition: v.edition,
            is_default: v.is_default,
        }
    }
}

/// A collection: the user's own or a TMDB one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCollection {
    pub id: i64,
    pub name: String,
    /// From provider metadata (`tmdb:collection:<id>`); not editable.
    pub external: bool,
    pub item_count: i64,
}

impl From<c::CollectionInfo> for CoreCollection {
    fn from(col: c::CollectionInfo) -> Self {
        Self {
            id: col.id,
            name: col.name,
            external: col.kind == c::CollectionKind::External,
            item_count: col.item_count,
        }
    }
}

/// A video playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorePlaylist {
    pub id: i64,
    pub name: String,
    pub item_count: i64,
    pub updated_at_ms: i64,
}

impl From<c::PlaylistInfo> for CorePlaylist {
    fn from(p: c::PlaylistInfo) -> Self {
        Self {
            id: p.id,
            name: p.name,
            item_count: p.item_count,
            updated_at_ms: p.updated_at_ms,
        }
    }
}
