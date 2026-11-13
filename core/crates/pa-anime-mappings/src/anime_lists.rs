// SPDX-License-Identifier: MPL-2.0
//! Anime-Lists `anime-list-master.xml`: AniDB → TVDB/TMDB IDs and episode
//! numbering (`defaulttvdbseason`, `episodeoffset`, `<mapping-list>`).
//!
//! **No licence upstream.** This file is never bundled or mirrored by the
//! project; the device downloads it from upstream ([`crate::update`]).

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::{Error, Result};

/// A season reference: a numbered season or TVDB/TMDB absolute order (`a`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SeasonRef {
    Season(u32),
    Absolute,
}

/// An episode in TVDB or TMDB numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// `season` 0 is specials.
    Season { season: u32, episode: u32 },
    /// Absolute episode number.
    Absolute { episode: u32 },
}

/// One `<mapping>` element.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeasonMapping {
    /// 0 = AniDB specials (S), 1 = regular episodes.
    pub anidb_season: u32,
    pub tvdb_season: u32,
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub offset: i32,
    /// `;a-b;a-b+c;` pairs: AniDB episode → TVDB episodes (empty = none).
    pub explicit: Vec<(u32, Vec<u32>)>,
}

/// One `<anime>` element.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnimeListEntry {
    pub anidb: u32,
    pub name: String,
    pub tvdb: Option<u32>,
    /// Non-numeric `tvdbid` (`movie`, `OVA`, `hentai`, `unknown`, …).
    pub tvdb_kind: Option<String>,
    pub default_tvdb_season: Option<SeasonRef>,
    pub episode_offset: i32,
    pub tmdb_tv: Option<u32>,
    pub tmdb_season: Option<SeasonRef>,
    pub tmdb_offset: i32,
    pub tmdb_movie: Vec<u32>,
    pub imdb: Vec<String>,
    pub mappings: Vec<SeasonMapping>,
}

fn parse_season(s: &str) -> Option<SeasonRef> {
    match s.trim() {
        "a" | "A" => Some(SeasonRef::Absolute),
        t => t.parse().ok().map(SeasonRef::Season),
    }
}

fn parse_list<T: std::str::FromStr>(s: &str) -> Vec<T> {
    s.split(',').filter_map(|p| p.trim().parse().ok()).collect()
}

fn attrs(e: &BytesStart<'_>) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| Error::Parse(e.to_string()))?;
        let key = a.key.as_ref().to_owned();
        let val = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| Error::Parse(e.to_string()))?
            .into_owned();
        out.push((key, val));
    }
    Ok(out)
}

fn anime_from_attrs(a: &[(String, String)]) -> Result<AnimeListEntry> {
    let mut e = AnimeListEntry::default();
    let mut anidb = None;
    for (k, v) in a {
        let v = v.trim();
        match k.as_str() {
            "anidbid" => anidb = v.parse().ok(),
            "tvdbid" => match v.parse() {
                Ok(id) => e.tvdb = Some(id),
                Err(_) if !v.is_empty() => e.tvdb_kind = Some(v.to_owned()),
                Err(_) => {}
            },
            "defaulttvdbseason" => e.default_tvdb_season = parse_season(v),
            "episodeoffset" => e.episode_offset = v.parse().unwrap_or(0),
            "tmdbtv" => e.tmdb_tv = v.parse().ok(),
            "tmdbseason" => e.tmdb_season = parse_season(v),
            "tmdboffset" => e.tmdb_offset = v.parse().unwrap_or(0),
            "tmdbid" => e.tmdb_movie = parse_list(v),
            "imdbid" => {
                e.imdb = v
                    .split(',')
                    .map(str::trim)
                    .filter(|s| s.starts_with("tt"))
                    .map(str::to_owned)
                    .collect()
            }
            _ => {}
        }
    }
    e.anidb = anidb.ok_or_else(|| Error::Parse("<anime> without numeric anidbid".into()))?;
    Ok(e)
}

fn mapping_from_attrs(a: &[(String, String)]) -> SeasonMapping {
    let mut m = SeasonMapping::default();
    for (k, v) in a {
        let v = v.trim();
        match k.as_str() {
            "anidbseason" => m.anidb_season = v.parse().unwrap_or(1),
            "tvdbseason" => m.tvdb_season = v.parse().unwrap_or(0),
            "start" => m.start = v.parse().ok(),
            "end" => m.end = v.parse().ok(),
            "offset" => m.offset = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    m
}

/// Parse `;1-3;2-0;5-6+7;` into pairs. Malformed items are skipped.
pub fn parse_explicit(text: &str) -> Vec<(u32, Vec<u32>)> {
    text.split(';')
        .filter_map(|item| {
            let (a, b) = item.trim().split_once('-')?;
            let a = a.trim().parse().ok()?;
            let targets: Vec<u32> = b
                .split('+')
                .filter_map(|x| x.trim().parse().ok())
                .filter(|&x| x != 0)
                .collect();
            Some((a, targets))
        })
        .collect()
}

#[derive(PartialEq)]
enum TextTarget {
    None,
    Name,
    Mapping,
}

/// Parse the whole `anime-list-master.xml`.
pub fn parse(xml: &str) -> Result<Vec<AnimeListEntry>> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut cur: Option<AnimeListEntry> = None;
    let mut target = TextTarget::None;
    let mut text = String::new();
    loop {
        let ev = reader
            .read_event()
            .map_err(|e| Error::Parse(format!("at byte {}: {e}", reader.buffer_position())))?;
        match ev {
            Event::Start(e) if e.name().as_ref() == "anime" => {
                cur = Some(anime_from_attrs(&attrs(&e)?)?);
            }
            Event::Empty(e) if e.name().as_ref() == "anime" => {
                out.push(anime_from_attrs(&attrs(&e)?)?);
            }
            Event::Start(e) if e.name().as_ref() == "name" && cur.is_some() => {
                target = TextTarget::Name;
                text.clear();
            }
            Event::Start(e) if e.name().as_ref() == "mapping" => {
                if let Some(c) = cur.as_mut() {
                    c.mappings.push(mapping_from_attrs(&attrs(&e)?));
                    target = TextTarget::Mapping;
                    text.clear();
                }
            }
            Event::Empty(e) if e.name().as_ref() == "mapping" => {
                if let Some(c) = cur.as_mut() {
                    c.mappings.push(mapping_from_attrs(&attrs(&e)?));
                }
            }
            Event::Text(t) if target != TextTarget::None => text.push_str(&t.xml10_content()),
            Event::CData(t) if target != TextTarget::None => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if target != TextTarget::None => {
                if let Ok(Some(ch)) = r.resolve_char_ref() {
                    text.push(ch);
                } else {
                    text.push_str(match r.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(e) => match e.name().as_ref() {
                "name" if target == TextTarget::Name => {
                    if let Some(c) = cur.as_mut() {
                        c.name = text.trim().to_owned();
                    }
                    target = TextTarget::None;
                }
                "mapping" if target == TextTarget::Mapping => {
                    if let Some(m) = cur.as_mut().and_then(|c| c.mappings.last_mut()) {
                        m.explicit = parse_explicit(&text);
                    }
                    target = TextTarget::None;
                }
                "anime" => {
                    if let Some(c) = cur.take() {
                        out.push(c);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// Count entries (validation for runtime updates).
pub fn validate(bytes: &[u8]) -> Result<usize> {
    let s = std::str::from_utf8(bytes).map_err(|e| Error::Parse(e.to_string()))?;
    parse(s).map(|v| v.len())
}

impl AnimeListEntry {
    /// TVDB episodes for AniDB episode `episode` of `anidb_season`
    /// (1 = regular, 0 = specials). Empty: no TVDB counterpart known.
    ///
    /// Precedence as in HAMA: explicit pairs, then ranges, then the default
    /// season with `episodeoffset` (regular episodes only).
    pub fn to_tvdb(&self, anidb_season: u32, episode: u32) -> Vec<Target> {
        for m in self
            .mappings
            .iter()
            .filter(|m| m.anidb_season == anidb_season)
        {
            if let Some((_, t)) = m.explicit.iter().find(|(a, _)| *a == episode) {
                return t
                    .iter()
                    .map(|&e| Target::Season {
                        season: m.tvdb_season,
                        episode: e,
                    })
                    .collect();
            }
        }
        for m in self
            .mappings
            .iter()
            .filter(|m| m.anidb_season == anidb_season)
        {
            if let (Some(s), Some(e)) = (m.start, m.end)
                && (s..=e).contains(&episode)
            {
                return shift(episode, m.offset)
                    .map(|ep| Target::Season {
                        season: m.tvdb_season,
                        episode: ep,
                    })
                    .into_iter()
                    .collect();
            }
        }
        if anidb_season != 1 || self.tvdb.is_none() {
            return Vec::new();
        }
        project(self.default_tvdb_season, episode, self.episode_offset)
    }

    /// TMDB TV episode for a regular AniDB episode (`tmdbseason`/`tmdboffset`;
    /// the mapping list applies to TVDB only).
    pub fn to_tmdb(&self, episode: u32) -> Vec<Target> {
        if self.tmdb_tv.is_none() {
            return Vec::new();
        }
        project(self.tmdb_season, episode, self.tmdb_offset)
    }

    /// AniDB `(season, episode)` pairs that map onto `target` in TVDB.
    pub fn from_tvdb(&self, target: Target) -> Vec<(u32, u32)> {
        let mut cands: Vec<(u32, u32)> = Vec::new();
        for m in &self.mappings {
            let Target::Season { season, episode } = target else {
                continue;
            };
            if m.tvdb_season != season {
                continue;
            }
            for (a, t) in &m.explicit {
                if t.contains(&episode) {
                    cands.push((m.anidb_season, *a));
                }
            }
            if let Some(a) = shift(episode, -m.offset) {
                cands.push((m.anidb_season, a));
            }
        }
        let default_matches = match (self.default_tvdb_season, target) {
            (Some(SeasonRef::Season(s)), Target::Season { season, .. }) => s == season,
            (Some(SeasonRef::Absolute), Target::Absolute { .. }) => true,
            _ => false,
        };
        if default_matches {
            let ep = match target {
                Target::Season { episode, .. } | Target::Absolute { episode } => episode,
            };
            if let Some(a) = shift(ep, -self.episode_offset) {
                cands.push((1, a));
            }
        }
        // Keep only candidates whose forward mapping really yields `target`.
        let mut out: Vec<(u32, u32)> = cands
            .into_iter()
            .filter(|&(s, e)| self.to_tvdb(s, e).contains(&target))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

fn shift(episode: u32, offset: i32) -> Option<u32> {
    let v = i64::from(episode) + i64::from(offset);
    u32::try_from(v).ok().filter(|&v| v > 0)
}

fn project(season: Option<SeasonRef>, episode: u32, offset: i32) -> Vec<Target> {
    let Some(ep) = shift(episode, offset) else {
        return Vec::new();
    };
    match season {
        Some(SeasonRef::Season(s)) => vec![Target::Season {
            season: s,
            episode: ep,
        }],
        Some(SeasonRef::Absolute) => vec![Target::Absolute { episode: ep }],
        None => Vec::new(),
    }
}
