// SPDX-License-Identifier: MPL-2.0
//! In-memory title index: ranked work candidates without network requests
//! (stage \[4\] of research 03 §4.2).
//!
//! Records come from any local source — the bundled anime-offline-database
//! ([`super::anime`]), cached provider responses (TMDB titles, translations
//! and alternative titles; later AniDB titles and Shikimori), the user's own
//! library. Every title of a record is indexed under its [`TitleKey`]s:
//! exact maps for the plain, loose, compact and base keys, an inverted index
//! of loose tokens (weighted by IDF, BM25-style) and a character-trigram
//! index of the compact key for fuzzy matches (typos, missing words,
//! transliterations the loose space does not merge).
//!
//! A [`TitleQuery`] carries every title variant of the parser's N-best
//! hypotheses plus year, season and media hints. [`TitleIndex::search`]
//! returns candidates ranked by a hand-set score in `[0, 1]`; it is a
//! *retrieval* score for building the candidate list, not a calibrated
//! probability — the scorer (META-014/018) decides.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::normalize::TitleKey;
use crate::parse::{MediaHint, ParseResult};

/// What kind of work a record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    /// Feature film.
    Movie,
    /// TV or web series.
    Series,
    /// OVA, special, music video and other short anime entries.
    Special,
    /// Unknown.
    #[default]
    Unknown,
}

/// One work in the index.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleRecord {
    /// External ids, namespace → value, in the corpus notation: `anidb` →
    /// `9980`, `tmdb` → `tv:46261`, `imdb` → `tt0111161`.
    #[serde(default)]
    pub ids: BTreeMap<String, String>,
    /// Kind of work.
    #[serde(default)]
    pub kind: WorkKind,
    /// The work is anime (record from an anime source).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub anime: bool,
    /// Premiere year.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// Number of regular episodes, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episodes: Option<u32>,
    /// Titles; the first one is the main title.
    pub titles: Vec<String>,
}

/// How the best title of a candidate matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchTier {
    /// Same plain key.
    Exact,
    /// Same loose key (transliteration, romaji variants).
    Loose,
    /// Same loose key without spaces.
    Compact,
    /// Same key without season/part/year qualifiers.
    Base,
    /// Token or trigram similarity.
    Fuzzy,
}

/// A ranked candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Position of the record in the index ([`TitleIndex::record`]).
    pub record: usize,
    /// Retrieval score in `[0, 1]`, higher is better.
    pub score: f32,
    /// Position of the best-matching title in the record's `titles`.
    pub title: usize,
    /// How that title matched.
    pub tier: MatchTier,
}

/// What to look for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TitleQuery {
    /// Title variants with weights in `(0, 1]`.
    pub titles: Vec<(String, f32)>,
    /// Year from the name, if any.
    pub year: Option<u16>,
    /// Season from the name or folders, if any.
    pub season: Option<u32>,
    /// What the path most likely is.
    pub media: MediaHint,
    /// The name looks like an anime release.
    pub anime_hint: bool,
}

/// Most title variants taken from one parse result.
const MAX_VARIANTS: usize = 8;

impl TitleQuery {
    /// A query for one title without hints.
    pub fn new(title: &str) -> Self {
        Self {
            titles: vec![(title.to_owned(), 1.0)],
            ..Default::default()
        }
    }

    /// Builds a query from the parser's N-best hypotheses: every distinct
    /// title and alternative title, weighted by its hypothesis; year,
    /// season and hints from the best hypothesis.
    pub fn from_parse(result: &ParseResult) -> Self {
        let mut titles: Vec<(String, f32)> = Vec::new();
        let mut push = |t: &str, w: f32| {
            let t = t.trim();
            if t.is_empty() {
                return;
            }
            match titles.iter_mut().find(|(x, _)| x == t) {
                Some((_, old)) => *old = old.max(w),
                None => titles.push((t.to_owned(), w)),
            }
        };
        for h in &result.hypotheses {
            if let Some(t) = &h.parsed.title {
                push(t, h.weight);
            }
            for a in &h.parsed.alt_titles {
                push(a, h.weight * 0.9);
            }
        }
        let max = titles.iter().map(|(_, w)| *w).fold(0.0f32, f32::max);
        if max > 0.0 {
            for (_, w) in &mut titles {
                *w /= max;
            }
        }
        titles.sort_by(|a, b| b.1.total_cmp(&a.1));
        titles.truncate(MAX_VARIANTS);
        let best = &result.best().parsed;
        Self {
            titles,
            year: best.year,
            season: best.season,
            media: best.media,
            anime_hint: best.anime_hint,
        }
    }
}

struct Entry {
    record: u32,
    title: u16,
    key: TitleKey,
    compact: String,
    tokens: Vec<u32>,
}

/// The title index. Build once with [`insert`](Self::insert), query many
/// times; queries take `&self`, so one index can serve several threads.
#[derive(Default)]
pub struct TitleIndex {
    records: Vec<TitleRecord>,
    entries: Vec<Entry>,
    plain: HashMap<String, Vec<u32>>,
    loose: HashMap<String, Vec<u32>>,
    compact: HashMap<String, Vec<u32>>,
    base: HashMap<String, Vec<u32>>,
    token_ids: HashMap<String, u32>,
    postings: Vec<Vec<u32>>,
    trigrams: HashMap<u64, Vec<u32>>,
}

impl std::fmt::Debug for TitleIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TitleIndex")
            .field("records", &self.records.len())
            .field("titles", &self.entries.len())
            .finish()
    }
}

/// Character trigrams of `^compact$`, each packed into a `u64`.
fn trigrams(compact: &str) -> Vec<u64> {
    let chars: Vec<char> = std::iter::once('^')
        .chain(compact.chars())
        .chain(std::iter::once('$'))
        .collect();
    let mut out: Vec<u64> = chars
        .windows(3)
        .map(|w| (u64::from(w[0]) << 42) | (u64::from(w[1]) << 21) | u64::from(w[2]))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Dice coefficient of two sorted, deduplicated trigram lists.
fn dice(a: &[u64], b: &[u64]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut shared) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    2.0 * shared as f32 / (a.len() + b.len()) as f32
}

/// One title of the query, prepared for matching.
struct QueryTitle {
    key: TitleKey,
    compact: String,
    tokens: Vec<Option<u32>>,
    trigrams: Vec<u64>,
    weight: f32,
}

impl TitleIndex {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds an index from records.
    pub fn from_records(records: impl IntoIterator<Item = TitleRecord>) -> Self {
        let mut index = Self::new();
        for r in records {
            index.insert(r);
        }
        index
    }

    /// Number of records.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// `true` when there are no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Number of indexed (distinct per record) titles.
    pub fn title_count(&self) -> usize {
        self.entries.len()
    }

    /// The record at `position` (from [`Candidate::record`]).
    pub fn record(&self, position: usize) -> &TitleRecord {
        &self.records[position]
    }

    /// All records in insertion order.
    pub fn records(&self) -> &[TitleRecord] {
        &self.records
    }

    /// Adds a record and returns its position. Titles that normalize to the
    /// same loose key as an earlier title of the record are indexed once.
    pub fn insert(&mut self, record: TitleRecord) -> usize {
        let rid = u32::try_from(self.records.len()).expect("fewer than 2^32 records");
        let mut seen = HashSet::new();
        for (ti, text) in record.titles.iter().enumerate().take(usize::from(u16::MAX)) {
            let key = TitleKey::new(text);
            if key.plain.is_empty() || !seen.insert(key.loose.clone()) {
                continue;
            }
            let eid = u32::try_from(self.entries.len()).expect("fewer than 2^32 titles");
            let compact = key.compact();
            for (map, k) in [
                (&mut self.plain, key.plain.clone()),
                (&mut self.loose, key.loose.clone()),
                (&mut self.compact, compact.clone()),
                (&mut self.base, key.base.clone()),
            ] {
                let list = map.entry(k).or_default();
                if list.last() != Some(&eid) {
                    list.push(eid);
                }
            }
            let mut tokens = Vec::new();
            for tok in key.loose.split(' ') {
                let next = u32::try_from(self.postings.len()).expect("token count");
                let id = *self.token_ids.entry(tok.to_owned()).or_insert(next);
                if id == next {
                    self.postings.push(Vec::new());
                }
                if !tokens.contains(&id) {
                    tokens.push(id);
                    self.postings[id as usize].push(eid);
                }
            }
            for t in trigrams(&compact) {
                self.trigrams.entry(t).or_default().push(eid);
            }
            self.entries.push(Entry {
                record: rid,
                title: ti as u16,
                key,
                compact,
                tokens,
            });
        }
        self.records.push(record);
        rid as usize
    }

    fn idf(&self, df: usize) -> f32 {
        let n = self.entries.len() as f32;
        ((n + 1.0) / (df as f32 + 0.5)).ln().max(0.05)
    }

    /// Ranked candidates for `query`, best first, at most `limit`.
    ///
    /// ```
    /// use pa_recognition::title::{TitleIndex, TitleQuery, TitleRecord, WorkKind};
    ///
    /// let mut index = TitleIndex::new();
    /// index.insert(TitleRecord {
    ///     kind: WorkKind::Movie,
    ///     year: Some(1975),
    ///     titles: vec!["The Irony of Fate".into(), "Ирония судьбы, или С лёгким паром!".into()],
    ///     ..Default::default()
    /// });
    /// let hits = index.search(&TitleQuery::new("Ironiya sudby ili s lyogkim parom"), 10);
    /// assert_eq!(hits[0].record, 0);
    /// ```
    pub fn search(&self, query: &TitleQuery, limit: usize) -> Vec<Candidate> {
        if self.entries.is_empty() || limit == 0 {
            return Vec::new();
        }
        let n = self.entries.len();
        // Very frequent tokens and trigrams do not generate candidates on
        // their own (they still count when scoring).
        let max_token_df = (n / 50).max(2_000);
        let max_trigram_df = (n / 20).max(5_000);

        // Best (score, title, tier) per record.
        let mut best: HashMap<u32, (f32, u16, MatchTier)> = HashMap::new();
        for (text, weight) in &query.titles {
            let key = TitleKey::new(text);
            if key.plain.is_empty() {
                continue;
            }
            let compact = key.compact();
            let mut seen_tok = HashSet::new();
            let tokens = key
                .loose
                .split(' ')
                .filter(|t| seen_tok.insert(*t))
                .map(|t| self.token_ids.get(t).copied())
                .collect();
            let q = QueryTitle {
                trigrams: trigrams(&compact),
                key,
                compact,
                tokens,
                weight: weight.clamp(0.0, 1.0),
            };

            let mut pool: HashSet<u32> = HashSet::new();
            for (map, k) in [
                (&self.plain, &q.key.plain),
                (&self.loose, &q.key.loose),
                (&self.compact, &q.compact),
                (&self.base, &q.key.base),
            ] {
                if let Some(list) = map.get(k) {
                    pool.extend(list.iter().copied());
                }
            }
            for id in q.tokens.iter().flatten() {
                let list = &self.postings[*id as usize];
                if list.len() <= max_token_df {
                    pool.extend(list.iter().copied());
                }
            }
            let mut counts: HashMap<u32, u16> = HashMap::new();
            let mut considered = 0u16;
            for t in &q.trigrams {
                if let Some(list) = self.trigrams.get(t)
                    && list.len() <= max_trigram_df
                {
                    considered += 1;
                    for &e in list {
                        *counts.entry(e).or_default() += 1;
                    }
                }
            }
            let min_shared = (f32::from(considered) * 0.4).ceil().max(1.0) as u16;
            pool.extend(
                counts
                    .into_iter()
                    .filter(|&(_, c)| c >= min_shared)
                    .map(|(e, _)| e),
            );

            for eid in pool {
                let entry = &self.entries[eid as usize];
                let (score, tier) = self.title_score(&q, entry, query.season);
                let score = score * (0.9 + 0.1 * q.weight);
                if score <= 0.0 {
                    continue;
                }
                let slot = best
                    .entry(entry.record)
                    .or_insert((f32::MIN, entry.title, tier));
                if score > slot.0 || (score == slot.0 && entry.title < slot.1) {
                    *slot = (score, entry.title, tier);
                }
            }
        }

        let query_year = query.year.or_else(|| {
            query
                .titles
                .first()
                .and_then(|(t, _)| TitleKey::new(t).year)
        });
        let mut out: Vec<Candidate> = best
            .into_iter()
            .map(|(rid, (score, title, tier))| {
                let record = &self.records[rid as usize];
                let score = (score + record_boost(query, query_year, record)).clamp(0.0, 1.0);
                Candidate {
                    record: rid as usize,
                    score,
                    title: usize::from(title),
                    tier,
                }
            })
            .collect();
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.record.cmp(&b.record)));
        out.truncate(limit);
        out
    }

    fn title_score(&self, q: &QueryTitle, e: &Entry, season_hint: Option<u32>) -> (f32, MatchTier) {
        let (mut score, tier) = if q.key.plain == e.key.plain {
            (1.0, MatchTier::Exact)
        } else if q.key.loose == e.key.loose {
            (0.97, MatchTier::Loose)
        } else if q.compact == e.compact {
            (0.95, MatchTier::Compact)
        } else if q.key.base == e.key.base {
            (0.90, MatchTier::Base)
        } else {
            let tri = dice(&q.trigrams, &trigrams(&e.compact));
            (0.88 * tri.max(self.token_f1(q, e)), MatchTier::Fuzzy)
        };
        let season = q.key.season.or(season_hint);
        score += match (season, e.key.season) {
            (Some(a), Some(b)) if a == b => 0.05,
            (Some(_), Some(_)) => -0.06,
            (Some(a), None) if a >= 2 => -0.03,
            (None, Some(b)) if b >= 2 => -0.03,
            _ => 0.0,
        };
        score += match (q.key.part, e.key.part) {
            (Some(a), Some(b)) if a == b => 0.02,
            (Some(_), Some(_)) => -0.03,
            _ => 0.0,
        };
        (score, tier)
    }

    /// IDF-weighted F1 of shared loose tokens.
    fn token_f1(&self, q: &QueryTitle, e: &Entry) -> f32 {
        let df = |id: u32| self.postings[id as usize].len();
        let mut q_sum = 0.0;
        let mut shared = 0.0;
        for t in &q.tokens {
            match t {
                Some(id) => {
                    let w = self.idf(df(*id));
                    q_sum += w;
                    if e.tokens.contains(id) {
                        shared += w;
                    }
                }
                // Unknown token: as rare as it gets.
                None => q_sum += self.idf(0),
            }
        }
        let e_sum: f32 = e.tokens.iter().map(|&id| self.idf(df(id))).sum();
        if shared == 0.0 {
            0.0
        } else {
            2.0 * shared / (q_sum + e_sum)
        }
    }
}

/// Year, media and anime adjustments of a record's score.
fn record_boost(query: &TitleQuery, year: Option<u16>, record: &TitleRecord) -> f32 {
    let mut boost = match (year, record.year) {
        (Some(a), Some(b)) => match a.abs_diff(b) {
            0 => 0.04,
            1 => 0.02,
            _ => -0.08,
        },
        _ => 0.0,
    };
    boost += match (query.media, record.kind) {
        (MediaHint::Movie, WorkKind::Movie) | (MediaHint::Episode, WorkKind::Series) => 0.02,
        (MediaHint::Movie, WorkKind::Series) | (MediaHint::Episode, WorkKind::Movie) => -0.02,
        _ => 0.0,
    };
    if query.anime_hint && record.anime {
        boost += 0.01;
    }
    boost
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: WorkKind, year: u16, titles: &[&str]) -> TitleRecord {
        TitleRecord {
            kind,
            year: Some(year),
            titles: titles.iter().map(|t| (*t).to_owned()).collect(),
            ..Default::default()
        }
    }

    fn index() -> TitleIndex {
        TitleIndex::from_records([
            rec(WorkKind::Series, 2016, &["Mob Psycho 100", "Моб Психо 100"]),
            rec(
                WorkKind::Series,
                2022,
                &["Mob Psycho 100 III", "Mob Psycho 100 Season 3"],
            ),
            rec(
                WorkKind::Movie,
                1975,
                &[
                    "The Irony of Fate",
                    "Ирония судьбы, или С лёгким паром!",
                    "Ирония судьбы",
                ],
            ),
            rec(
                WorkKind::Movie,
                2007,
                &["The Irony of Fate 2", "Ирония судьбы. Продолжение"],
            ),
            rec(
                WorkKind::Series,
                2009,
                &[
                    "Fullmetal Alchemist: Brotherhood",
                    "Hagane no Renkinjutsushi: Fullmetal Alchemist",
                ],
            ),
            rec(
                WorkKind::Series,
                2003,
                &["Fullmetal Alchemist", "Hagane no Renkinjutsushi"],
            ),
            rec(WorkKind::Movie, 1987, &["RoboCop"]),
            rec(WorkKind::Movie, 2014, &["RoboCop"]),
            rec(
                WorkKind::Series,
                2013,
                &["Shingeki no Kyojin", "Attack on Titan", "Атака титанов"],
            ),
        ])
    }

    fn top(index: &TitleIndex, q: &TitleQuery) -> Vec<usize> {
        index.search(q, 10).into_iter().map(|c| c.record).collect()
    }

    #[test]
    fn exact_and_transliterated() {
        let ix = index();
        assert_eq!(top(&ix, &TitleQuery::new("Ironiya sudby"))[0], 2);
        assert_eq!(top(&ix, &TitleQuery::new("Ирония судьбы"))[0], 2);
        assert_eq!(top(&ix, &TitleQuery::new("Ataka titanov"))[0], 8);
        assert_eq!(top(&ix, &TitleQuery::new("Attack on Titan"))[0], 8);
        assert_eq!(top(&ix, &TitleQuery::new("Hagane no Renkinjutsushi"))[0], 5);
    }

    #[test]
    fn season_hint_prefers_the_sequel_entry() {
        let ix = index();
        let mut q = TitleQuery::new("Mob Psycho 100");
        assert_eq!(top(&ix, &q)[0], 0);
        q.season = Some(3);
        let r = top(&ix, &q);
        assert!(r[..2].contains(&1), "{r:?}");
        assert_eq!(top(&ix, &TitleQuery::new("Mob Psycho 100 S3"))[0], 1);
    }

    #[test]
    fn year_breaks_ties_between_remakes() {
        let ix = index();
        let mut q = TitleQuery::new("RoboCop");
        q.year = Some(2014);
        assert_eq!(top(&ix, &q)[..2], [7, 6]);
        q.year = Some(1987);
        assert_eq!(top(&ix, &q)[..2], [6, 7]);
        assert_eq!(top(&ix, &TitleQuery::new("Robocop (1987)"))[0], 6);
    }

    #[test]
    fn fuzzy_finds_typos_and_partial_titles() {
        let ix = index();
        assert_eq!(
            top(&ix, &TitleQuery::new("Fulmetal Alchemist Brotherhod"))[0],
            4
        );
        assert!(top(&ix, &TitleQuery::new("Shingeki no Kyoujin")).contains(&8));
        let hits = ix.search(&TitleQuery::new("Completely Unrelated Words"), 10);
        assert!(hits.iter().all(|c| c.score < 0.5), "{hits:?}");
    }

    #[test]
    fn query_from_parse_uses_all_hypotheses() {
        let ix = index();
        let parsed = crate::parse::parse("Кино/Ирония судьбы | Ironiya sudby (1975).mkv");
        let q = TitleQuery::from_parse(&parsed);
        assert!(!q.titles.is_empty());
        let hits = ix.search(&q, 3);
        assert_eq!(hits[0].record, 2);
        assert_eq!(hits[0].tier, MatchTier::Exact);
    }

    #[test]
    fn deterministic_and_bounded() {
        let ix = index();
        let q = TitleQuery::new("Fullmetal");
        let a = ix.search(&q, 2);
        assert_eq!(a, ix.search(&q, 2));
        assert!(a.len() <= 2);
        assert!(ix.search(&TitleQuery::new("!!!"), 10).is_empty());
        assert!(TitleIndex::new().search(&q, 10).is_empty());
    }

    #[test]
    fn records_roundtrip_as_json() {
        let r = TitleRecord {
            ids: BTreeMap::from([("tmdb".into(), "movie:603".into())]),
            kind: WorkKind::Movie,
            year: Some(1999),
            titles: vec!["The Matrix".into(), "Матрица".into()],
            ..Default::default()
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"ids":{"tmdb":"movie:603"},"kind":"movie","year":1999,"titles":["The Matrix","Матрица"]}"#
        );
        assert_eq!(serde_json::from_str::<TitleRecord>(&json).unwrap(), r);
    }
}
