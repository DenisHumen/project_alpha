// SPDX-License-Identifier: MPL-2.0
//! Shikimori REST API v1 response types (only the fields Kinolune uses).

use serde::{Deserialize, Serialize};

use crate::fetch::year_of;

/// Image paths (relative to the site root).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Image {
    /// Full size.
    pub original: Option<String>,
    /// Small preview.
    pub preview: Option<String>,
}

/// An anime as listed by `/animes` and inside `/related`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimeShort {
    /// Shikimori id; for anime it equals the MyAnimeList id.
    pub id: u64,
    /// Romanised title (`Shingeki no Kyojin`).
    pub name: String,
    /// Russian title (`Атака титанов`).
    pub russian: Option<String>,
    /// Images.
    pub image: Option<Image>,
    /// Page path on the site.
    pub url: Option<String>,
    /// `tv`, `movie`, `ova`, `ona`, `special`, `tv_special`, `music`, `pv`, `cm`.
    pub kind: Option<String>,
    /// Score as a decimal string (`"8.5"`).
    pub score: Option<String>,
    /// `anons`, `ongoing`, `released`.
    pub status: Option<String>,
    /// Planned or final episode count (0 when unknown).
    pub episodes: Option<u32>,
    /// Episodes aired so far.
    pub episodes_aired: Option<u32>,
    /// First air date, `YYYY-MM-DD`.
    pub aired_on: Option<String>,
    /// Last air date, `YYYY-MM-DD`.
    pub released_on: Option<String>,
}

impl AnimeShort {
    /// Year of the first air date.
    pub fn year(&self) -> Option<u32> {
        self.aired_on.as_deref().and_then(year_of)
    }

    /// True for movies.
    pub fn is_movie(&self) -> bool {
        self.kind.as_deref() == Some("movie")
    }

    /// Still airing or announced: its data changes often.
    pub fn is_ongoing(&self) -> bool {
        !matches!(self.status.as_deref(), Some("released"))
    }

    /// The Russian title, if Shikimori has a non-empty one.
    pub fn russian_title(&self) -> Option<&str> {
        self.russian.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
}

/// A genre.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Genre {
    /// Genre id.
    pub id: u64,
    /// English name.
    pub name: String,
    /// Russian name.
    pub russian: Option<String>,
}

/// `/animes/{id}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Anime {
    /// Fields shared with list entries.
    #[serde(flatten)]
    pub base: AnimeShort,
    /// Age rating: `g`, `pg`, `pg_13`, `r`, `r_plus`, `rx` (hentai).
    pub rating: Option<String>,
    /// English titles.
    pub english: Vec<Option<String>>,
    /// Japanese titles.
    pub japanese: Vec<Option<String>>,
    /// Other titles.
    pub synonyms: Vec<String>,
    /// Russian title of the official Russian licence, if any.
    pub license_name_ru: Option<String>,
    /// Episode length, minutes.
    pub duration: Option<u32>,
    /// Russian synopsis with Shikimori markup (`[character=1]Имя[/character]`);
    /// see [`Anime::plain_description`].
    pub description: Option<String>,
    /// Where the synopsis comes from.
    pub description_source: Option<String>,
    /// Franchise slug (groups seasons).
    pub franchise: Option<String>,
    /// MyAnimeList id.
    pub myanimelist_id: Option<u64>,
    /// Genres.
    pub genres: Vec<Genre>,
    /// Next episode time (ISO 8601), for ongoing series.
    pub next_episode_at: Option<String>,
}

impl Anime {
    /// 18+ (hentai).
    pub fn adult(&self) -> bool {
        self.rating.as_deref() == Some("rx")
    }

    /// The synopsis without Shikimori markup.
    pub fn plain_description(&self) -> Option<String> {
        self.description
            .as_deref()
            .map(strip_markup)
            .filter(|s| !s.is_empty())
    }
}

/// One entry of `/animes/{id}/related`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Related {
    /// Relation in English (`Sequel`, `Prequel`, `Side story`, …).
    pub relation: Option<String>,
    /// Relation in Russian (`Продолжение`, …).
    pub relation_russian: Option<String>,
    /// The related anime; `None` when the entry is a manga.
    pub anime: Option<AnimeShort>,
}

/// Removes BBCode-like tags (`[b]`, `[/i]`, `[character=12]`, `[url=…]`)
/// and trims; text between tags is kept.
pub fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        match after.find(']') {
            Some(end) if is_tag(&after[1..end]) => rest = &after[end + 1..],
            _ => {
                out.push('[');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out.replace("\r\n", "\n").trim().to_owned()
}

fn is_tag(inner: &str) -> bool {
    let name = inner.strip_prefix('/').unwrap_or(inner);
    let name = name.split_once('=').map_or(name, |(n, _)| n);
    !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_shikimori_markup() {
        assert_eq!(
            strip_markup(
                "[character=101]Мика[/character] ищет [b]ответ[/b] [1/2] [url=https://x]тут[/url]. "
            ),
            "Мика ищет ответ [1/2] тут."
        );
        assert_eq!(strip_markup("без [тегов"), "без [тегов");
    }
}
