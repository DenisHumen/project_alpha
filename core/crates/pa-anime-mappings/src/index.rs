// SPDX-License-Identifier: MPL-2.0
//! In-memory cross-ID index combining the layers on the user's device.
//!
//! Layers are added in trust order: bundled anime-offline-database first
//! (titles, episode counts, anime-site IDs), then the downloaded Anime-Lists
//! (TVDB/TMDB and episode numbering), then Fribb as a fallback. The merged
//! result lives only in memory (or in the user's local cache) and is never
//! redistributed.

use std::collections::HashMap;

use crate::anime_lists::{AnimeListEntry, SeasonRef, Target};
use crate::fribb::FribbEntry;
use crate::offline_db::OfflineEntry;
use crate::{AnimeIds, IdRef};

/// Everything known about one anime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    pub ids: AnimeIds,
    pub title: Option<String>,
    pub kind: Option<String>,
    /// Number of regular episodes (from anime-offline-database).
    pub episodes: Option<u32>,
    /// Fribb `season.tvdb` / `season.tmdb` (fallback when Anime-Lists lacks the entry).
    pub fribb_tvdb_season: Option<u32>,
    pub fribb_tmdb_season: Option<u32>,
    /// Anime-Lists entry with episode numbering rules.
    pub anime_list: Option<AnimeListEntry>,
}

/// An AniDB episode: `season` 1 = regular, 0 = specials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EpisodeRef {
    pub anidb: u32,
    pub season: u32,
    pub episode: u32,
}

/// Cross-ID and episode mapping index.
#[derive(Debug, Default)]
pub struct MappingIndex {
    records: Vec<Record>,
    keys: HashMap<IdRef, Vec<usize>>,
}

impl MappingIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Records carrying `id`. TVDB/TMDB IDs are shared by several AniDB
    /// entries (one per season or cour), so this may return many.
    pub fn get(&self, id: IdRef) -> impl Iterator<Item = &Record> {
        self.keys
            .get(&id)
            .into_iter()
            .flatten()
            .map(|&i| &self.records[i])
    }

    /// First record carrying `id`; meant for one-to-one keys (AniDB, MAL, AniList, Kitsu).
    pub fn find(&self, id: IdRef) -> Option<&Record> {
        self.get(id).next()
    }

    /// Merge `ids` into an existing record matched by a one-to-one key, or
    /// create a new one. Returns the record position.
    fn merge(&mut self, ids: &AnimeIds) -> usize {
        let unique = [
            ids.anidb.map(IdRef::AniDb),
            ids.mal.map(IdRef::Mal),
            ids.anilist.map(IdRef::AniList),
            ids.kitsu.map(IdRef::Kitsu),
        ];
        let found = unique
            .into_iter()
            .flatten()
            .find_map(|k| self.keys.get(&k).and_then(|v| v.first().copied()));
        let pos = match found {
            Some(p) => {
                self.records[p].ids.fill_from(ids);
                p
            }
            None => {
                self.records.push(Record {
                    ids: ids.clone(),
                    ..Default::default()
                });
                self.records.len() - 1
            }
        };
        for k in self.records[pos].ids.keys() {
            let v = self.keys.entry(k).or_default();
            if !v.contains(&pos) {
                v.push(pos);
            }
        }
        pos
    }

    /// Add the bundled anime-offline-database layer.
    pub fn add_offline(&mut self, entries: impl IntoIterator<Item = OfflineEntry>) {
        for e in entries {
            let p = self.merge(&e.ids);
            let r = &mut self.records[p];
            if r.title.is_none() && !e.title.is_empty() {
                r.title = Some(e.title);
            }
            if r.kind.is_none() && !e.kind.is_empty() {
                r.kind = Some(e.kind);
            }
            r.episodes = r.episodes.or(e.episodes);
        }
    }

    /// Add the downloaded Anime-Lists layer.
    pub fn add_anime_lists(&mut self, entries: impl IntoIterator<Item = AnimeListEntry>) {
        for e in entries {
            let ids = AnimeIds {
                anidb: Some(e.anidb),
                tvdb: e.tvdb,
                tmdb_tv: e.tmdb_tv,
                tmdb_movie: e.tmdb_movie.first().copied(),
                imdb: e.imdb.clone(),
                ..Default::default()
            };
            let p = self.merge(&ids);
            let r = &mut self.records[p];
            if r.title.is_none() && !e.name.is_empty() {
                r.title = Some(e.name.clone());
            }
            r.anime_list.get_or_insert(e);
        }
    }

    /// Add the downloaded Fribb layer.
    pub fn add_fribb(&mut self, entries: impl IntoIterator<Item = FribbEntry>) {
        for e in entries {
            let p = self.merge(&e.ids);
            let r = &mut self.records[p];
            r.fribb_tvdb_season = r.fribb_tvdb_season.or(e.tvdb_season);
            r.fribb_tmdb_season = r.fribb_tmdb_season.or(e.tmdb_season);
            if r.kind.is_none() && !e.kind.is_empty() {
                r.kind = Some(e.kind);
            }
        }
    }

    /// TVDB episode(s) for an AniDB episode. Uses Anime-Lists rules, falling
    /// back to Fribb `season.tvdb` for regular episodes.
    pub fn anidb_to_tvdb(&self, ep: EpisodeRef) -> Vec<Target> {
        let Some(r) = self.find(IdRef::AniDb(ep.anidb)) else {
            return Vec::new();
        };
        if let Some(al) = &r.anime_list {
            return al.to_tvdb(ep.season, ep.episode);
        }
        match (ep.season, r.ids.tvdb, r.fribb_tvdb_season) {
            (1, Some(_), Some(s)) => vec![Target::Season {
                season: s,
                episode: ep.episode,
            }],
            _ => Vec::new(),
        }
    }

    /// TMDB TV episode for a regular AniDB episode.
    pub fn anidb_to_tmdb(&self, anidb: u32, episode: u32) -> Vec<Target> {
        let Some(r) = self.find(IdRef::AniDb(anidb)) else {
            return Vec::new();
        };
        if let Some(al) = &r.anime_list {
            let t = al.to_tmdb(episode);
            if !t.is_empty() {
                return t;
            }
        }
        match (r.ids.tmdb_tv, r.fribb_tmdb_season) {
            (Some(_), Some(s)) => vec![Target::Season { season: s, episode }],
            _ => Vec::new(),
        }
    }

    /// AniDB episode for a TVDB episode of series `tvdb`.
    ///
    /// When several AniDB entries claim the episode (split cours share one
    /// TVDB season), candidates beyond the entry's known episode count are
    /// dropped, then the smallest AniDB episode number wins — i.e. the entry
    /// with the largest offset that still covers the episode.
    pub fn tvdb_to_anidb(&self, tvdb: u32, target: Target) -> Option<EpisodeRef> {
        self.pick(
            self.get(IdRef::Tvdb(tvdb))
                .flat_map(|r| {
                    let al = r.anime_list.as_ref()?;
                    Some(
                        al.from_tvdb(target)
                            .into_iter()
                            .map(move |(s, e)| (r, s, e)),
                    )
                })
                .flatten(),
        )
    }

    /// AniDB episode for a TMDB TV episode of series `tmdb_tv`.
    pub fn tmdb_to_anidb(&self, tmdb_tv: u32, target: Target) -> Option<EpisodeRef> {
        self.pick(self.get(IdRef::TmdbTv(tmdb_tv)).filter_map(|r| {
            let al = r.anime_list.as_ref()?;
            let (ep, matches) = match (al.tmdb_season, target) {
                (Some(SeasonRef::Season(s)), Target::Season { season, episode }) => {
                    (episode, s == season)
                }
                (Some(SeasonRef::Absolute), Target::Absolute { episode }) => (episode, true),
                _ => return None,
            };
            let a = i64::from(ep) - i64::from(al.tmdb_offset);
            let a = u32::try_from(a).ok().filter(|&a| a > 0 && matches)?;
            Some((r, 1, a))
        }))
    }

    fn pick<'a>(
        &'a self,
        cands: impl Iterator<Item = (&'a Record, u32, u32)>,
    ) -> Option<EpisodeRef> {
        cands
            .filter(|(r, s, e)| *s != 1 || r.episodes.is_none_or(|n| *e <= n))
            .filter_map(|(r, s, e)| {
                Some(EpisodeRef {
                    anidb: r.ids.anidb?,
                    season: s,
                    episode: e,
                })
            })
            .min_by_key(|c| (c.episode, c.anidb))
    }
}
