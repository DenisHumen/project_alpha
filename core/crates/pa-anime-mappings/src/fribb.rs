// SPDX-License-Identifier: MPL-2.0
//! Fribb `anime-list-full.json`: cross-ID table with TVDB/TMDB seasons.
//!
//! **No licence upstream.** Never bundled or mirrored by the project; the
//! device downloads it from upstream ([`crate::update`]). Optional: the index
//! works without it (`docs/legal/anime-mappings.md` §4).

use serde_json::Value;

use crate::{AnimeIds, Error, Result};

/// One entry of `anime-list-full.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FribbEntry {
    pub ids: AnimeIds,
    pub kind: String,
    pub tvdb_season: Option<u32>,
    pub tmdb_season: Option<u32>,
}

fn id(v: &Value, key: &str) -> Option<u32> {
    match v.get(key)? {
        Value::Number(n) => n.as_u64().and_then(|x| u32::try_from(x).ok()),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Parse the whole file (a JSON array). Entries that are not objects are
/// rejected; unknown fields are ignored.
pub fn parse(bytes: &[u8]) -> Result<Vec<FribbEntry>> {
    let v: Value = serde_json::from_slice(bytes)?;
    let arr = v
        .as_array()
        .ok_or_else(|| Error::Parse("expected a JSON array".into()))?;
    arr.iter()
        .map(|o| {
            if !o.is_object() {
                return Err(Error::Parse("array item is not an object".into()));
            }
            let mut ids = AnimeIds {
                anidb: id(o, "anidb_id"),
                anilist: id(o, "anilist_id"),
                mal: id(o, "mal_id"),
                kitsu: id(o, "kitsu_id"),
                anisearch: id(o, "anisearch_id"),
                livechart: id(o, "livechart_id"),
                simkl: id(o, "simkl_id"),
                ann: id(o, "animenewsnetwork_id"),
                anime_planet: o
                    .get("anime-planet_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                tvdb: id(o, "tvdb_id"),
                ..Default::default()
            };
            match o.get("themoviedb_id") {
                Some(t @ Value::Object(_)) => {
                    ids.tmdb_tv = id(t, "tv");
                    ids.tmdb_movie = id(t, "movie");
                }
                Some(Value::Number(n)) => ids.tmdb_tv = n.as_u64().and_then(|x| x.try_into().ok()),
                _ => {}
            }
            match o.get("imdb_id") {
                Some(Value::Array(a)) => {
                    ids.imdb = a
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                }
                Some(Value::String(s)) => ids.imdb = vec![s.clone()],
                _ => {}
            }
            let season = o.get("season");
            Ok(FribbEntry {
                ids,
                kind: o
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                tvdb_season: season.and_then(|s| id(s, "tvdb")),
                tmdb_season: season.and_then(|s| id(s, "tmdb")),
            })
        })
        .collect()
}

/// Count entries (validation for runtime updates).
pub fn validate(bytes: &[u8]) -> Result<usize> {
    parse(bytes).map(|v| v.len())
}
