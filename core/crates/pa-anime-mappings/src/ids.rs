// SPDX-License-Identifier: MPL-2.0
//! Provider identifiers of one anime entry.

use serde::{Deserialize, Serialize};

/// IDs of one anime entry across providers. `None`/empty means unknown.
///
/// AniDB is the canonical space for anime (research 03 §4.3); TVDB and TMDB
/// only come from the runtime-downloaded lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimeIds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anidb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anilist: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kitsu: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anisearch: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub livechart: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simkl: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ann: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anime_planet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvdb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb_tv: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb_movie: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imdb: Vec<String>,
}

/// A single provider ID used as a lookup key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdRef {
    AniDb(u32),
    AniList(u32),
    Mal(u32),
    Kitsu(u32),
    Tvdb(u32),
    TmdbTv(u32),
    TmdbMovie(u32),
}

impl AnimeIds {
    /// All numeric keys of this entry usable with [`IdRef`].
    pub fn keys(&self) -> Vec<IdRef> {
        [
            self.anidb.map(IdRef::AniDb),
            self.anilist.map(IdRef::AniList),
            self.mal.map(IdRef::Mal),
            self.kitsu.map(IdRef::Kitsu),
            self.tvdb.map(IdRef::Tvdb),
            self.tmdb_tv.map(IdRef::TmdbTv),
            self.tmdb_movie.map(IdRef::TmdbMovie),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Fill unknown fields from `other`. Known fields are kept: the layer
    /// added first wins, so callers add the most trusted source first.
    pub fn fill_from(&mut self, other: &AnimeIds) {
        fn fill<T: Clone>(a: &mut Option<T>, b: &Option<T>) {
            if a.is_none() {
                a.clone_from(b);
            }
        }
        fill(&mut self.anidb, &other.anidb);
        fill(&mut self.anilist, &other.anilist);
        fill(&mut self.mal, &other.mal);
        fill(&mut self.kitsu, &other.kitsu);
        fill(&mut self.anisearch, &other.anisearch);
        fill(&mut self.livechart, &other.livechart);
        fill(&mut self.simkl, &other.simkl);
        fill(&mut self.ann, &other.ann);
        fill(&mut self.anime_planet, &other.anime_planet);
        fill(&mut self.tvdb, &other.tvdb);
        fill(&mut self.tmdb_tv, &other.tmdb_tv);
        fill(&mut self.tmdb_movie, &other.tmdb_movie);
        for id in &other.imdb {
            if !self.imdb.contains(id) {
                self.imdb.push(id.clone());
            }
        }
    }

    /// Set the ID that a provider page URL points to (anime-offline-database
    /// `sources`). Unknown hosts are ignored. Returns `true` if recognised.
    pub fn set_from_source_url(&mut self, url: &str) -> bool {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .unwrap_or(url);
        let rest = rest.strip_prefix("www.").unwrap_or(rest);
        let num = |prefix: &str| -> Option<u32> {
            let tail = rest.strip_prefix(prefix)?;
            let digits: &str = tail.split(['/', '?', '#']).next()?;
            digits.parse().ok()
        };
        if let Some(v) = num("anidb.net/anime/") {
            self.anidb = Some(v);
        } else if let Some(v) = num("anilist.co/anime/") {
            self.anilist = Some(v);
        } else if let Some(v) = num("myanimelist.net/anime/") {
            self.mal = Some(v);
        } else if let Some(v) = num("kitsu.app/anime/").or_else(|| num("kitsu.io/anime/")) {
            self.kitsu = Some(v);
        } else if let Some(v) = num("anisearch.com/anime/") {
            self.anisearch = Some(v);
        } else if let Some(v) = num("livechart.me/anime/") {
            self.livechart = Some(v);
        } else if let Some(v) = num("simkl.com/anime/") {
            self.simkl = Some(v);
        } else if let Some(v) = num("animenewsnetwork.com/encyclopedia/anime.php?id=") {
            self.ann = Some(v);
        } else if let Some(slug) = rest.strip_prefix("anime-planet.com/anime/") {
            self.anime_planet = Some(slug.trim_end_matches('/').to_owned());
        } else {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_urls() {
        let mut ids = AnimeIds::default();
        for u in [
            "https://anidb.net/anime/10143",
            "https://anilist.co/anime/102416",
            "https://myanimelist.net/anime/20707",
            "https://kitsu.app/anime/8925",
            "https://animenewsnetwork.com/encyclopedia/anime.php?id=32920",
            "https://anime-planet.com/anime/some-slug",
        ] {
            assert!(ids.set_from_source_url(u), "{u}");
        }
        assert!(!ids.set_from_source_url("https://example.org/anime/1"));
        assert_eq!(ids.anidb, Some(10143));
        assert_eq!(ids.anilist, Some(102416));
        assert_eq!(ids.mal, Some(20707));
        assert_eq!(ids.kitsu, Some(8925));
        assert_eq!(ids.ann, Some(32920));
        assert_eq!(ids.anime_planet.as_deref(), Some("some-slug"));
    }

    #[test]
    fn fill_keeps_first() {
        let mut a = AnimeIds {
            anidb: Some(1),
            tvdb: Some(10),
            ..Default::default()
        };
        let b = AnimeIds {
            anidb: Some(2),
            mal: Some(3),
            imdb: vec!["tt1".into()],
            ..Default::default()
        };
        a.fill_from(&b);
        assert_eq!(a.anidb, Some(1));
        assert_eq!(a.mal, Some(3));
        assert_eq!(a.imdb, vec!["tt1".to_string()]);
    }
}
