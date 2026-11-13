// SPDX-License-Identifier: MPL-2.0
//! Typed TMDB v3 responses: only the fields the recognition pipeline and
//! the library need. Unknown fields are ignored; movie and TV spellings
//! (`title`/`name`, `release_date`/`first_air_date`) are merged.

use serde::{Deserialize, Deserializer, Serialize};

/// Movie or TV series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    /// A film.
    Movie,
    /// A TV series (including anime series).
    Tv,
}

impl MediaKind {
    /// Path segment and `media_type` value: `movie` / `tv`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "tv",
        }
    }

    /// Parses `movie` / `tv` (also `show`, `series`).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "movie" | "film" => Some(Self::Movie),
            "tv" | "show" | "series" => Some(Self::Tv),
            _ => None,
        }
    }
}

/// Year from a TMDB date (`YYYY-MM-DD`; empty for unknown).
pub fn year_of(date: Option<&str>) -> Option<u32> {
    date?.get(..4)?.parse().ok()
}

fn empty_as_none<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let s: Option<String> = Option::deserialize(d)?;
    Ok(s.filter(|s| !s.is_empty()))
}

/// One page of search results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchPage {
    /// Page number (1-based).
    #[serde(default)]
    pub page: u32,
    /// Results on this page.
    #[serde(default)]
    pub results: Vec<SearchResult>,
    /// Total pages.
    #[serde(default)]
    pub total_pages: u32,
    /// Total results.
    #[serde(default)]
    pub total_results: u32,
}

/// A movie or series in search results, collection parts or `/find`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    /// TMDB id (unique per kind only).
    pub id: u64,
    /// `movie`, `tv`, or `person` in multi-search; filled by the client for
    /// single-kind searches.
    #[serde(default)]
    pub media_type: Option<String>,
    /// Localized title (`title` for movies, `name` for TV).
    #[serde(default, alias = "name")]
    pub title: Option<String>,
    /// Original title (`original_title` / `original_name`).
    #[serde(default, alias = "original_name")]
    pub original_title: Option<String>,
    /// ISO 639-1 of the original language.
    #[serde(default)]
    pub original_language: Option<String>,
    /// `release_date` / `first_air_date`.
    #[serde(default, alias = "first_air_date", deserialize_with = "empty_as_none")]
    pub release_date: Option<String>,
    /// Origin countries (TV).
    #[serde(default)]
    pub origin_country: Vec<String>,
    /// Genre ids.
    #[serde(default)]
    pub genre_ids: Vec<u32>,
    /// Overview in the requested language.
    #[serde(default)]
    pub overview: Option<String>,
    /// Poster path for [`image_url`](super::image_url).
    #[serde(default)]
    pub poster_path: Option<String>,
    /// Backdrop path.
    #[serde(default)]
    pub backdrop_path: Option<String>,
    /// TMDB popularity (relative, changes daily).
    #[serde(default)]
    pub popularity: f64,
    /// Adult flag.
    #[serde(default)]
    pub adult: bool,
}

impl SearchResult {
    /// Kind from `media_type`; `None` for people and unknown types.
    pub fn kind(&self) -> Option<MediaKind> {
        self.media_type.as_deref().and_then(MediaKind::parse)
    }

    /// Year of the first release or first air date.
    pub fn year(&self) -> Option<u32> {
        year_of(self.release_date.as_deref())
    }
}

/// Genre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genre {
    /// Genre id.
    pub id: u32,
    /// Localized name.
    pub name: String,
}

/// The collection a movie belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionRef {
    /// Collection id.
    pub id: u64,
    /// Localized name.
    #[serde(default)]
    pub name: Option<String>,
}

/// Season summary inside TV details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeasonSummary {
    /// Season number (0 = specials).
    pub season_number: u32,
    /// Episodes in the season.
    #[serde(default)]
    pub episode_count: u32,
    /// First air date.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub air_date: Option<String>,
    /// Localized name.
    #[serde(default)]
    pub name: Option<String>,
}

/// Movie or series details (`/movie/{id}`, `/tv/{id}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Details {
    /// TMDB id.
    pub id: u64,
    /// Localized title.
    #[serde(default, alias = "name")]
    pub title: Option<String>,
    /// Original title.
    #[serde(default, alias = "original_name")]
    pub original_title: Option<String>,
    /// ISO 639-1 of the original language.
    #[serde(default)]
    pub original_language: Option<String>,
    /// Overview in the requested language.
    #[serde(default)]
    pub overview: Option<String>,
    /// Tagline.
    #[serde(default)]
    pub tagline: Option<String>,
    /// `release_date` / `first_air_date`.
    #[serde(default, alias = "first_air_date", deserialize_with = "empty_as_none")]
    pub release_date: Option<String>,
    /// Last air date (TV).
    #[serde(default, deserialize_with = "empty_as_none")]
    pub last_air_date: Option<String>,
    /// `Released`, `Returning Series`, `Ended`, `In Production`, ...
    #[serde(default)]
    pub status: Option<String>,
    /// Movie runtime in minutes.
    #[serde(default)]
    pub runtime: Option<u32>,
    /// Typical episode runtimes in minutes (TV).
    #[serde(default)]
    pub episode_run_time: Vec<u32>,
    /// Genres.
    #[serde(default)]
    pub genres: Vec<Genre>,
    /// Poster path.
    #[serde(default)]
    pub poster_path: Option<String>,
    /// Backdrop path.
    #[serde(default)]
    pub backdrop_path: Option<String>,
    /// IMDb id (movies carry it inline).
    #[serde(default, deserialize_with = "empty_as_none")]
    pub imdb_id: Option<String>,
    /// Collection (movies).
    #[serde(default)]
    pub belongs_to_collection: Option<CollectionRef>,
    /// Number of seasons (TV).
    #[serde(default)]
    pub number_of_seasons: Option<u32>,
    /// Number of episodes (TV).
    #[serde(default)]
    pub number_of_episodes: Option<u32>,
    /// Seasons (TV).
    #[serde(default)]
    pub seasons: Vec<SeasonSummary>,
    /// Origin countries.
    #[serde(default)]
    pub origin_country: Vec<String>,
    /// Adult flag.
    #[serde(default)]
    pub adult: bool,
}

impl Details {
    /// Year of release / first air date.
    pub fn year(&self) -> Option<u32> {
        year_of(self.release_date.as_deref())
    }

    /// TMDB statuses of series that still get new episodes.
    pub fn is_ongoing(&self) -> bool {
        matches!(
            self.status.as_deref(),
            Some("Returning Series" | "In Production" | "Planned" | "Pilot")
        )
    }
}

/// `/{kind}/{id}/translations`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Translations {
    /// TMDB id.
    pub id: u64,
    /// One entry per language/country.
    #[serde(default)]
    pub translations: Vec<Translation>,
}

impl Translations {
    /// The translation for a `ll-CC` tag (`ru-RU`) or bare language (`ru`).
    pub fn find(&self, tag: &str) -> Option<&Translation> {
        let (lang, country) = tag.split_once('-').unwrap_or((tag, ""));
        self.translations
            .iter()
            .find(|t| t.iso_639_1 == lang && (country.is_empty() || t.iso_3166_1 == country))
            .or_else(|| self.translations.iter().find(|t| t.iso_639_1 == lang))
    }
}

/// One translation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Translation {
    /// Country (ISO 3166-1).
    #[serde(default)]
    pub iso_3166_1: String,
    /// Language (ISO 639-1).
    #[serde(default)]
    pub iso_639_1: String,
    /// Language name in English.
    #[serde(default)]
    pub english_name: Option<String>,
    /// Translated fields.
    #[serde(default)]
    pub data: TranslationData,
}

/// Translated fields; empty strings mean "not translated".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TranslationData {
    /// Title (`title` / `name`).
    #[serde(default, alias = "name", deserialize_with = "empty_as_none")]
    pub title: Option<String>,
    /// Overview.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub overview: Option<String>,
    /// Tagline.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub tagline: Option<String>,
}

/// `/{kind}/{id}/alternative_titles` (movies: `titles`, TV: `results`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternativeTitles {
    /// TMDB id.
    pub id: u64,
    /// Titles.
    #[serde(default, alias = "results")]
    pub titles: Vec<AlternativeTitle>,
}

/// One alternative title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternativeTitle {
    /// Country (ISO 3166-1).
    #[serde(default)]
    pub iso_3166_1: String,
    /// The title.
    pub title: String,
    /// Kind of title (`romaji`, `working title`, ...); often empty.
    #[serde(default, rename = "type", deserialize_with = "empty_as_none")]
    pub kind: Option<String>,
}

/// `/{kind}/{id}/external_ids`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalIds {
    /// TMDB id.
    #[serde(default)]
    pub id: u64,
    /// IMDb (`tt…`).
    #[serde(default, deserialize_with = "empty_as_none")]
    pub imdb_id: Option<String>,
    /// TheTVDB (TV).
    #[serde(default)]
    pub tvdb_id: Option<u64>,
    /// Wikidata (`Q…`).
    #[serde(default, deserialize_with = "empty_as_none")]
    pub wikidata_id: Option<String>,
}

/// `/{kind}/{id}/images`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Images {
    /// TMDB id.
    #[serde(default)]
    pub id: u64,
    /// Posters.
    #[serde(default)]
    pub posters: Vec<Image>,
    /// Backdrops.
    #[serde(default)]
    pub backdrops: Vec<Image>,
    /// Logos.
    #[serde(default)]
    pub logos: Vec<Image>,
}

/// One image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image {
    /// Path for [`image_url`](super::image_url).
    pub file_path: String,
    /// Width in pixels.
    #[serde(default)]
    pub width: u32,
    /// Height in pixels.
    #[serde(default)]
    pub height: u32,
    /// Language of text on the image; `None` for textless.
    #[serde(default)]
    pub iso_639_1: Option<String>,
    /// Average vote.
    #[serde(default)]
    pub vote_average: f64,
}

/// `/collection/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    /// Collection id.
    pub id: u64,
    /// Localized name.
    #[serde(default)]
    pub name: Option<String>,
    /// Overview.
    #[serde(default)]
    pub overview: Option<String>,
    /// Poster path.
    #[serde(default)]
    pub poster_path: Option<String>,
    /// Movies in the collection.
    #[serde(default)]
    pub parts: Vec<SearchResult>,
}

/// `/tv/{id}/episode_groups`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeGroupList {
    /// Series id.
    #[serde(default)]
    pub id: u64,
    /// Groups of the series.
    #[serde(default)]
    pub results: Vec<EpisodeGroupSummary>,
}

/// TMDB episode group types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum EpisodeGroupType {
    /// 1 — original air date.
    OriginalAirDate,
    /// 2 — absolute.
    Absolute,
    /// 3 — DVD.
    Dvd,
    /// 4 — digital.
    Digital,
    /// 5 — story arc.
    StoryArc,
    /// 6 — production.
    Production,
    /// 7 — TV.
    Tv,
    /// Unknown future value.
    Other(u8),
}

impl From<u8> for EpisodeGroupType {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::OriginalAirDate,
            2 => Self::Absolute,
            3 => Self::Dvd,
            4 => Self::Digital,
            5 => Self::StoryArc,
            6 => Self::Production,
            7 => Self::Tv,
            other => Self::Other(other),
        }
    }
}

impl From<EpisodeGroupType> for u8 {
    fn from(v: EpisodeGroupType) -> Self {
        match v {
            EpisodeGroupType::OriginalAirDate => 1,
            EpisodeGroupType::Absolute => 2,
            EpisodeGroupType::Dvd => 3,
            EpisodeGroupType::Digital => 4,
            EpisodeGroupType::StoryArc => 5,
            EpisodeGroupType::Production => 6,
            EpisodeGroupType::Tv => 7,
            EpisodeGroupType::Other(o) => o,
        }
    }
}

/// An episode group in the list of a series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeGroupSummary {
    /// Group id (a hex string).
    pub id: String,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Group type.
    #[serde(rename = "type")]
    pub group_type: EpisodeGroupType,
    /// Episodes in the group.
    #[serde(default)]
    pub episode_count: u32,
    /// Number of sub-groups (usually seasons).
    #[serde(default)]
    pub group_count: u32,
}

/// `/tv/episode_group/{id}`: an alternative ordering of a series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeGroup {
    /// Group id.
    pub id: String,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// Group type.
    #[serde(rename = "type")]
    pub group_type: EpisodeGroupType,
    /// Sub-groups ("seasons" of this ordering).
    #[serde(default)]
    pub groups: Vec<EpisodeSubgroup>,
}

/// A sub-group of an episode group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeSubgroup {
    /// Sub-group id.
    #[serde(default)]
    pub id: String,
    /// Name ("Season 1", "Arc 2", ...).
    #[serde(default)]
    pub name: String,
    /// Position of the sub-group (0-based or 1-based, as the group author set it).
    #[serde(default)]
    pub order: u32,
    /// Episodes in this ordering.
    #[serde(default)]
    pub episodes: Vec<GroupEpisode>,
}

/// An episode inside an episode group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupEpisode {
    /// Episode id.
    pub id: u64,
    /// Name.
    #[serde(default)]
    pub name: Option<String>,
    /// Position inside the sub-group (0-based).
    #[serde(default)]
    pub order: u32,
    /// Season in the default (aired) ordering.
    pub season_number: u32,
    /// Episode in the default (aired) ordering.
    pub episode_number: u32,
    /// Air date.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub air_date: Option<String>,
}

/// `/tv/{id}/season/{n}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Season {
    /// Season id.
    #[serde(default)]
    pub id: u64,
    /// Season number.
    pub season_number: u32,
    /// Name.
    #[serde(default)]
    pub name: Option<String>,
    /// Air date.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub air_date: Option<String>,
    /// Episodes.
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

/// One episode of a season.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    /// Episode id.
    pub id: u64,
    /// Season number.
    pub season_number: u32,
    /// Episode number in the season.
    pub episode_number: u32,
    /// Localized name.
    #[serde(default)]
    pub name: Option<String>,
    /// Air date.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub air_date: Option<String>,
    /// Runtime in minutes.
    #[serde(default)]
    pub runtime: Option<u32>,
    /// Overview.
    #[serde(default)]
    pub overview: Option<String>,
    /// Still path.
    #[serde(default)]
    pub still_path: Option<String>,
}

/// `/find/{external_id}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FindResults {
    /// Movies.
    #[serde(default)]
    pub movie_results: Vec<SearchResult>,
    /// Series.
    #[serde(default)]
    pub tv_results: Vec<SearchResult>,
}

/// External id namespaces accepted by `/find`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalSource {
    /// IMDb `tt…`.
    Imdb,
    /// TheTVDB numeric id.
    Tvdb,
    /// Wikidata `Q…`.
    Wikidata,
}

impl ExternalSource {
    /// Value of the `external_source` parameter.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Imdb => "imdb_id",
            Self::Tvdb => "tvdb_id",
            Self::Wikidata => "wikidata_id",
        }
    }

    /// Parses `imdb` / `tvdb` / `wikidata` (with or without `_id`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim_end_matches("_id") {
            "imdb" => Some(Self::Imdb),
            "tvdb" => Some(Self::Tvdb),
            "wikidata" => Some(Self::Wikidata),
            _ => None,
        }
    }
}
