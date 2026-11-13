// SPDX-License-Identifier: MPL-2.0
//! AniList GraphQL response types (only the fields Kinolune asks for).
//!
//! Every field is optional or defaulted: AniList returns `null` freely
//! (no English title, unknown episode count, no MAL id).

use serde::{Deserialize, Serialize};

/// `{"data": ..., "errors": [...]}`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphqlResponse<T> {
    pub data: Option<T>,
    #[serde(default)]
    pub errors: Vec<GraphqlError>,
}

/// One entry of `errors`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GraphqlError {
    #[serde(default)]
    pub message: String,
    pub status: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PageData {
    #[serde(rename = "Page")]
    pub page: Option<PageMedia>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PageMedia {
    #[serde(default)]
    pub media: Vec<Media>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MediaData {
    #[serde(rename = "Media")]
    pub media: Option<Media>,
}

/// Titles in the three scripts AniList keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaTitle {
    /// Romanised Japanese (`Shingeki no Kyojin`).
    pub romaji: Option<String>,
    /// Official English title.
    pub english: Option<String>,
    /// Native script (`進撃の巨人`).
    pub native: Option<String>,
}

/// A date where any part may be unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FuzzyDate {
    /// Year.
    pub year: Option<u32>,
    /// Month 1–12.
    pub month: Option<u32>,
    /// Day 1–31.
    pub day: Option<u32>,
}

/// Cover image URLs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverImage {
    /// ~230 px wide.
    pub large: Option<String>,
}

/// Next episode of an airing series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiringEpisode {
    /// Episode number.
    pub episode: u32,
    /// Unix seconds.
    pub airing_at: u64,
}

/// One anime (`Media` with `type: ANIME`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Media {
    /// AniList id.
    pub id: u64,
    /// MyAnimeList id (also the Shikimori id).
    pub id_mal: Option<u64>,
    /// `TV`, `TV_SHORT`, `MOVIE`, `SPECIAL`, `OVA`, `ONA`, `MUSIC`.
    pub format: Option<String>,
    /// `FINISHED`, `RELEASING`, `NOT_YET_RELEASED`, `CANCELLED`, `HIATUS`.
    pub status: Option<String>,
    /// Planned or final episode count.
    pub episodes: Option<u32>,
    /// Episode length, minutes.
    pub duration: Option<u32>,
    /// `WINTER`, `SPRING`, `SUMMER`, `FALL`.
    pub season: Option<String>,
    /// Year of the airing season.
    pub season_year: Option<u32>,
    /// 18+ content; filtered out unless the client allows it.
    pub is_adult: Option<bool>,
    /// Titles.
    pub title: MediaTitle,
    /// Alternative titles in any language.
    pub synonyms: Vec<String>,
    /// First release.
    pub start_date: FuzzyDate,
    /// Last release.
    pub end_date: FuzzyDate,
    /// Synopsis (plain text with `<br>` line breaks, English).
    pub description: Option<String>,
    /// Genres.
    pub genres: Vec<String>,
    /// Score 0–100.
    pub average_score: Option<u32>,
    /// Number of users with the title on their list.
    pub popularity: Option<u64>,
    /// Page on anilist.co.
    pub site_url: Option<String>,
    /// Cover image.
    pub cover_image: Option<CoverImage>,
    /// Next episode, for airing series.
    pub next_airing_episode: Option<AiringEpisode>,
    /// Related works (only in [`AniListClient::media`](super::AniListClient::media)).
    pub relations: Option<Relations>,
}

impl Media {
    /// Year of the first release (start date, else season year).
    pub fn year(&self) -> Option<u32> {
        self.start_date.year.or(self.season_year)
    }

    /// True for movies (one-off works), false for series.
    pub fn is_movie(&self) -> bool {
        self.format.as_deref() == Some("MOVIE")
    }

    /// Still airing or announced: its data changes often.
    pub fn is_ongoing(&self) -> bool {
        matches!(
            self.status.as_deref(),
            None | Some("RELEASING" | "NOT_YET_RELEASED" | "HIATUS")
        )
    }

    /// 18+ content.
    pub fn adult(&self) -> bool {
        self.is_adult.unwrap_or(false)
    }

    /// Every distinct title and synonym, for the title index.
    pub fn all_titles(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        let t = &self.title;
        for s in [&t.romaji, &t.english, &t.native]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .chain(self.synonyms.iter().map(String::as_str))
        {
            if !s.trim().is_empty() && !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }
}

/// `relations { edges { relationType node } }`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Relations {
    /// Edges of the relation graph.
    pub edges: Vec<RelationEdge>,
}

/// One related work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationEdge {
    /// `SEQUEL`, `PREQUEL`, `PARENT`, `SIDE_STORY`, `SPIN_OFF`,
    /// `ALTERNATIVE`, `SUMMARY`, `ADAPTATION`, `SOURCE`, `CHARACTER`,
    /// `COMPILATION`, `CONTAINS`, `OTHER`.
    pub relation_type: Option<String>,
    /// The related work (anime or manga).
    pub node: RelatedMedia,
}

/// A related work, with the fields needed to walk seasons.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RelatedMedia {
    /// AniList id.
    pub id: u64,
    /// MyAnimeList id.
    pub id_mal: Option<u64>,
    /// `ANIME` or `MANGA`.
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    /// Format (see [`Media::format`]).
    pub format: Option<String>,
    /// Status (see [`Media::status`]).
    pub status: Option<String>,
    /// Season year.
    pub season_year: Option<u32>,
    /// 18+ content.
    pub is_adult: Option<bool>,
    /// Titles.
    pub title: MediaTitle,
    /// First release.
    pub start_date: FuzzyDate,
}

impl RelatedMedia {
    /// True for anime (relations also list manga and novels).
    pub fn is_anime(&self) -> bool {
        self.media_type.as_deref() == Some("ANIME")
    }

    /// Year of the first release.
    pub fn year(&self) -> Option<u32> {
        self.start_date.year.or(self.season_year)
    }
}
