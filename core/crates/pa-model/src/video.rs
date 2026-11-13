// SPDX-License-Identifier: MPL-2.0
//! Video library entities: canonical works, seasons, episodes and files.
//!
//! Follows docs/research/03 §5. Enum values match the recognition corpus
//! schema (`tools/recognition-eval/schema/corpus-record.v1.schema.json`), so
//! ground truth and core results use one vocabulary.

use serde::{Deserialize, Serialize};

use crate::key::MediaKey;

/// Local database row id. Not stable across devices: use [`MediaKey`] for sync.
pub type RowId = i64;

/// Kind of a canonical work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Movie,
    Show,
    Anime,
}

/// AniDB-style anime type; set only for [`WorkKind::Anime`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimeType {
    Tv,
    Ova,
    Ona,
    Movie,
    Special,
    MusicVideo,
}

/// Episode numbering scheme (provider order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberingScheme {
    Tmdb,
    TvdbAired,
    TvdbDvd,
    TvdbAbsolute,
    Anidb,
    TmdbGroup,
}

/// Episode type in the canonical space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeType {
    #[default]
    Regular,
    Special,
    Credit,
    Trailer,
    Parody,
    Recap,
    Other,
}

/// External ids of a work. `tmdb` and `tvdb` carry the entity type as a
/// prefix (`movie:603`, `tv:1399`, `series:121361`), as in the corpus schema.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalIds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmdb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tvdb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imdb: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anidb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anilist: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mal: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kitsu: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shikimori: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kinopoisk: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tvmaze: Option<u64>,
}

impl ExternalIds {
    /// TMDB movie id from `tmdb = "movie:<id>"`.
    pub fn tmdb_movie(&self) -> Option<u64> {
        self.tmdb_typed("movie")
    }

    /// TMDB TV show id from `tmdb = "tv:<id>"`.
    pub fn tmdb_tv(&self) -> Option<u64> {
        self.tmdb_typed("tv")
    }

    fn tmdb_typed(&self, kind: &str) -> Option<u64> {
        let (k, id) = self.tmdb.as_deref()?.split_once(':')?;
        (k == kind).then(|| id.parse().ok()).flatten()
    }
}

/// Canonical work: a movie, a show or an anime entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Work {
    pub id: RowId,
    pub kind: WorkKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anime_type: Option<AnimeType>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    /// ISO 639 language code of the original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default)]
    pub ids: ExternalIds,
}

impl Work {
    /// Sync key of a movie work, if it is recognised on TMDB.
    pub fn media_key(&self) -> Option<MediaKey> {
        let movie = self.kind == WorkKind::Movie || self.anime_type == Some(AnimeType::Movie);
        movie
            .then(|| self.ids.tmdb_movie())
            .flatten()
            .map(|id| MediaKey::TmdbMovie { id })
    }
}

/// Season of a work in one numbering scheme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Season {
    pub id: RowId,
    pub work_id: RowId,
    pub scheme: NumberingScheme,
    /// 0 = specials.
    pub number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
}

/// Episode of a work. Numbers are in TMDB order when known; other orders live
/// in episode mappings (owned by pa-library / pa-recognition).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    pub id: RowId,
    pub work_id: RowId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season_id: Option<RowId>,
    #[serde(default)]
    pub episode_type: EpisodeType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_number: Option<u32>,
    /// `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_ms: Option<u64>,
}

impl Episode {
    /// Sync key, given the TMDB id of the parent show.
    pub fn media_key(&self, tmdb_show_id: u64) -> Option<MediaKey> {
        Some(MediaKey::TmdbEpisode {
            show_id: tmdb_show_id,
            season: self.season_number?,
            episode: self.episode_number?,
        })
    }
}

/// A file in a storage source (local disk, SMB, NFS, WebDAV, server).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: RowId,
    /// Source (storage target) the file belongs to.
    pub source_id: RowId,
    /// POSIX path relative to the source root. Never synced (04 §1.8).
    pub path: String,
    pub size_bytes: u64,
    /// Modification time, Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Content fingerprint, lowercase hex; basis of the `file:` key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// OpenSubtitles hash, 16 lowercase hex digits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oshash: Option<String>,
}

impl MediaFile {
    /// Fallback sync key for unrecognised content.
    pub fn fingerprint_key(&self) -> Option<MediaKey> {
        let key = MediaKey::File {
            fingerprint: self.fingerprint.clone()?,
        };
        key.is_valid().then_some(key)
    }
}
