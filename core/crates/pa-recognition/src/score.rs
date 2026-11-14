// SPDX-License-Identifier: MPL-2.0
//! Scorer v0: picks the work (and, for anime, the franchise entry and the
//! episode inside it) for every file of a group from the local title index —
//! stage \[5\] of research 03 §4.2 with hand-set weights (META-026; the
//! trained and calibrated scorer is META-018).
//!
//! For each file the parser's N-best hypotheses build a [`TitleQuery`], the
//! [`TitleIndex`] returns the top candidates, and every candidate is turned
//! into one or more *readings*:
//!
//! - movies and western series: the candidate itself, adjusted for kind
//!   (episode markers vs a movie record, a date vs an anime record) and year;
//! - anime series with an episode number: the *franchise* of the candidate —
//!   AniDB-backed TV/ONA records sharing its title base key, ordered by year
//!   and cut at long gaps (remakes) — lets the written number be read inside
//!   the matched entry (`Clannad - After Story - 05`), as an absolute number
//!   across the franchise (`Boku no Hero Academia - 133` → 6th season,
//!   ep. 20; entries titled `(Episodes 202-252)` say it directly), through an
//!   explicit season (`Haikyuu!! S04E15` → the entry titled `Season 4`,
//!   overflowing into its second cour) or as a number running on from the
//!   matched entry (`Mugen Ressha-hen - 12` → `Yuukaku-hen` ep. 5).
//!
//! Readings are scored, merged per record, and the confidence of a file comes
//! from the best score and its margin over the best *different* record: two
//! equally good works (remakes without a year, same-name series) give a low
//! confidence instead of a silent error. Extras and files without a title of
//! their own (`sample.mkv`, `Extras/Trailer 2.mkv`) take the decision of
//! their folder when every confident file there agrees.
//!
//! Everything is deterministic and offline: no provider calls, no clocks.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::parse::{EpisodeKind, MediaHint, ParsedName, parse};
use crate::title::normalize::TitleKey;
use crate::title::{Candidate, MatchTier, TitleIndex, TitleQuery, TitleRecord, WorkKind};

/// Candidates taken from the index per file.
pub const TOP_K: usize = 10;

/// Candidates searched for franchise entries named after an arc
/// (`Kimetsu no Yaiba: Hashira Geiko-hen` ranks low for `Kimetsu no Yaiba`).
const POOL_K: usize = 30;

/// Consecutive franchise entries further apart than this many years are
/// different series (remake, reboot).
const REMAKE_GAP_YEARS: u16 = 8;

/// One input file of a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupFile<'a> {
    /// Path relative to the library root, `/`-separated.
    pub path: &'a str,
    /// Container duration, if probed (not used by v0 yet).
    pub duration_ms: Option<u64>,
}

/// An episode position inside the chosen record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EpisodeRef {
    /// Season as written (western series, `SxxEyy`); `None` for anime, whose
    /// records are single entries (AniDB-style).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<u32>,
    /// Regular episode number inside the record (anime) or season (series).
    pub episode: u32,
    /// Absolute number across the franchise, when every earlier entry has a
    /// known episode count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute: Option<u32>,
}

/// How the decision was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reading {
    /// No episode number involved (movies, series without numbers).
    #[default]
    Title,
    /// Number inside the matched entry.
    Direct,
    /// Absolute number across the franchise.
    Absolute,
    /// Explicit season → franchise entry.
    Season,
    /// Number running on from the matched entry into the next ones.
    Overflow,
    /// Taken from the folder's other files.
    Inherited,
}

/// Signals behind a decision (explanation, and features for the trained
/// scorer of META-018).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Signals {
    /// Score of the chosen reading.
    pub top: f32,
    /// Margin over the best reading of a different record (`top` when there
    /// is none).
    pub margin: f32,
    /// Best different record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_up: Option<usize>,
    /// How the best title matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<MatchTier>,
    /// How the decision was reached.
    pub reading: Reading,
    /// The name carries a year.
    pub year: bool,
    /// The name carries episode markers.
    pub episodic: bool,
    /// The franchise of the chosen record has a remake with the same title.
    pub remake: bool,
    /// Candidates returned by the index.
    pub candidates: usize,
}

/// Decision for one file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileMatch {
    /// Chosen record ([`TitleIndex::record`]); `None` — abstain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<usize>,
    /// Confidence in `[0, 1]` that `record` is the right work (hand-set
    /// mapping of the signals, not calibrated — META-018).
    pub confidence: f32,
    /// Episodes contained in the file, inside `record`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<EpisodeRef>,
    /// Why.
    pub signals: Signals,
    /// Best readings per record, best first (the decision included), for
    /// explanations and the review UI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
}

/// A scored reading of the file as one record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alternative {
    /// Record ([`TitleIndex::record`]).
    pub record: usize,
    /// Score of the reading.
    pub score: f32,
    /// How the number was read.
    pub reading: Reading,
    /// Episodes inside the record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<EpisodeRef>,
}

/// Alternatives kept per file.
const ALTERNATIVES: usize = 5;

/// Per-record facts derived from its titles, computed once.
#[derive(Debug, Default)]
struct RecordInfo {
    /// Base keys of all titles.
    bases: Vec<String>,
    /// `(base, season, part)` of every title claiming a season (`Season 2`,
    /// `2nd Season`, `S2 Part 2`).
    seasons: Vec<(String, u32, u32)>,
    /// Absolute ranges written in titles: `Gintama (Episodes 202-252)`.
    ranges: Vec<(u32, u32)>,
    /// Lower-cased titles, for literal matches.
    lower: Vec<String>,
}

impl RecordInfo {
    fn of(rec: &TitleRecord) -> Self {
        let mut info = Self::default();
        for t in &rec.titles {
            let key = TitleKey::new(t);
            if !info.bases.contains(&key.base) {
                info.bases.push(key.base.clone());
            }
            if let Some(s) = key.season {
                info.seasons.push((key.base.clone(), s, key.part.unwrap_or(1)));
            }
            if let Some(r) = episode_range(t) {
                info.ranges.push(r);
            }
            info.lower.push(t.trim().to_lowercase());
        }
        info
    }

    /// Titles claiming season `s` (any part when `part` is `None`) under
    /// one of `bases` (any base when empty): `sxf s2` or `Vigilante: …
    /// 2nd Season` do not speak for `Spy x Family` or `My Hero Academia`.
    fn claims(&self, s: u32, part: Option<u32>, bases: &[String]) -> u32 {
        self.seasons
            .iter()
            .filter(|(b, ss, pp)| {
                *ss == s
                    && part.is_none_or(|p| p == *pp)
                    && (bases.is_empty() || bases.contains(b))
            })
            .count() as u32
    }
}

/// `… (Episodes 202-252)` (also `Episoden`, `Episodi`, `Episodios`).
fn episode_range(title: &str) -> Option<(u32, u32)> {
    let lower = title.to_lowercase();
    let start = lower.find("(episod")?;
    let rest = &lower[start..];
    let close = rest.find(')')?;
    let inner = &rest[..close];
    let digits = inner.find(|c: char| c.is_ascii_digit())?;
    let (a, b) = inner[digits..].split_once('-')?;
    let a: u32 = a.trim().parse().ok()?;
    let b: u32 = b.trim().parse().ok()?;
    (a >= 1 && b >= a).then_some((a, b))
}

/// The scorer with its per-record cache. One instance can serve many groups
/// of the same index.
#[derive(Debug, Default)]
pub struct Scorer {
    info: HashMap<usize, RecordInfo>,
}

/// One reading of a file as a record.
#[derive(Debug, Clone)]
struct Hyp {
    record: usize,
    score: f32,
    episodes: Vec<EpisodeRef>,
    reading: Reading,
    tier: MatchTier,
    remake: bool,
}

/// What the file name says about numbers.
struct Numbers {
    /// Whole episode numbers as written (absolute or within a season).
    values: Vec<u32>,
    season: Option<u32>,
    part: Option<u32>,
    /// The name has episode markers of any kind (numbers, `.5`, dates, kinds).
    episodic: bool,
}

impl Numbers {
    fn of(p: &ParsedName) -> Self {
        let source = if p.absolute.is_empty() {
            &p.episodes
        } else {
            &p.absolute
        };
        let values: Vec<u32> = source
            .iter()
            .filter(|n| n.fraction.is_none())
            .map(|n| n.number)
            .collect();
        let key = p.title.as_deref().map(TitleKey::new);
        Self {
            episodic: !source.is_empty() || p.date.is_some() || p.episode_kind.is_some(),
            values,
            season: p.season.or(key.as_ref().and_then(|k| k.season)),
            part: key.and_then(|k| k.part),
        }
    }
}

/// A franchise: positions of records in the index, in broadcast order.
struct Franchise {
    chain: Vec<usize>,
    /// Another series with the same title exists outside this chain.
    remake: bool,
    /// Episode count of a much later entry of the chain that also carries
    /// the written title (`Kino no Tabi (2017)`): a bare title with a number
    /// up to it may mean either series.
    soft_remake: Option<u32>,
}

/// Scores a whole group (one folder tree) with a fresh cache.
pub fn score_group(index: &TitleIndex, files: &[GroupFile<'_>]) -> Vec<FileMatch> {
    Scorer::default().score_group(index, files)
}

impl Scorer {
    /// A scorer with an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scores a whole group (one folder tree, one case) at once.
    pub fn score_group(&mut self, index: &TitleIndex, files: &[GroupFile<'_>]) -> Vec<FileMatch> {
        let parsed: Vec<ParsedName> = files
            .iter()
            .map(|f| parse(f.path).best().parsed.clone())
            .collect();
        let mut out: Vec<FileMatch> = files
            .iter()
            .map(|f| self.score_file(index, f.path))
            .collect();
        inherit_from_folder(files, &parsed, &mut out);
        out
    }

    fn info(&mut self, index: &TitleIndex, r: usize) -> &RecordInfo {
        self.info
            .entry(r)
            .or_insert_with(|| RecordInfo::of(index.record(r)))
    }

    /// Scores one file on its own (no group context).
    pub fn score_file(&mut self, index: &TitleIndex, path: &str) -> FileMatch {
        let parsed = parse(path);
        let best = &parsed.best().parsed;
        let numbers = Numbers::of(best);
        let mut signals = Signals {
            year: best.year.is_some(),
            episodic: numbers.episodic,
            ..Default::default()
        };
        if best.title.is_none() {
            return FileMatch {
                signals,
                ..Default::default()
            };
        }
        let query = TitleQuery::from_parse(&parsed);
        let pool = index.search(&query, POOL_K);
        let candidates = &pool[..pool.len().min(TOP_K)];
        signals.candidates = candidates.len();

        let mut hyps = Vec::new();
        for c in candidates {
            self.interpret(index, c, &pool, best, &numbers, &mut hyps);
        }
        let mut per_record: BTreeMap<usize, Hyp> = BTreeMap::new();
        for h in hyps {
            match per_record.get(&h.record) {
                Some(old) if old.score >= h.score => {}
                _ => {
                    per_record.insert(h.record, h);
                }
            }
        }
        let mut ranked: Vec<Hyp> = per_record.into_values().collect();
        ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.record.cmp(&b.record)));
        let Some(top) = ranked.first() else {
            return FileMatch {
                signals,
                ..Default::default()
            };
        };
        signals.top = top.score;
        signals.margin = top.score - ranked.get(1).map_or(0.0, |h| h.score);
        signals.runner_up = ranked.get(1).map(|h| h.record);
        signals.tier = Some(top.tier);
        signals.reading = top.reading;
        signals.remake = top.remake;
        let alternatives = ranked
            .iter()
            .take(ALTERNATIVES)
            .map(|h| Alternative {
                record: h.record,
                score: h.score,
                reading: h.reading,
                episodes: h.episodes.clone(),
            })
            .collect();
        FileMatch {
            record: Some(top.record),
            confidence: confidence(&signals, best),
            episodes: top.episodes.clone(),
            signals,
            alternatives,
        }
    }

    /// Turns one candidate into readings.
    fn interpret(
        &mut self,
        index: &TitleIndex,
        c: &Candidate,
        all: &[Candidate],
        best: &ParsedName,
        numbers: &Numbers,
        out: &mut Vec<Hyp>,
    ) {
        let rec = index.record(c.record);
        let mut base = c.score + agreement(best, numbers, rec);
        let anime_series = rec.anime && rec.kind == WorkKind::Series;
        if !anime_series || numbers.values.is_empty() {
            let episodes = match (rec.anime, rec.kind, numbers.season) {
                (false, WorkKind::Series, Some(season)) => numbers
                    .values
                    .iter()
                    .map(|&e| EpisodeRef {
                        season: Some(season),
                        episode: e,
                        absolute: None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            base += self.literal(index, c, best);
            out.push(Hyp {
                record: c.record,
                score: base,
                episodes,
                reading: Reading::Title,
                tier: c.tier,
                remake: false,
            });
            return;
        }

        let fr = self.franchise(index, c, all, best);
        if !fr.remake {
            base += self.literal(index, c, best);
        }
        let chain = &fr.chain;
        let pos = chain.iter().position(|&r| r == c.record);
        let first = numbers.values[0];
        let last = *numbers.values.iter().max().unwrap_or(&first);
        let mut push = |record, score, episodes, reading| {
            out.push(Hyp {
                record,
                score,
                episodes,
                reading,
                tier: c.tier,
                remake: fr.remake
                    || (reading == Reading::Direct && fr.soft_remake.is_some_and(|n| last <= n)),
            })
        };

        // Direct: the number is inside the matched entry.
        let own = rec.episodes.filter(|&n| n > 0);
        if own.is_none_or(|n| last <= n) {
            let mut prior = 0.0;
            // `Title - 05` matched through a qualified title (`Title (2017)`,
            // `Title 2`) of a later entry: the bare title means the first one.
            let matched = TitleKey::new(&rec.titles[c.title]);
            if c.tier != MatchTier::Exact && (matched.season.is_some() || matched.year.is_some()) {
                prior -= 0.04;
            }
            // A later entry reached through a bare synonym (`Dorohedoro` for
            // `Dorohedoro Season 2`): a bare title means the first entry.
            if pos.is_some_and(|p| p > 0) && c.title != 0 && matched.season.is_none() {
                prior -= 0.05;
            }
            if let Some(s) = numbers.season
                && chain.len() > 1
                && self.info(index, c.record).claims(s, None, &[]) == 0
                && pos.is_none_or(|p| p + 1 != s as usize)
            {
                prior -= 0.12;
            }
            let eps = numbers
                .values
                .iter()
                .map(|&e| EpisodeRef {
                    season: None,
                    episode: e,
                    absolute: pos.and_then(|p| offset(index, chain, p)).map(|o| o + e),
                })
                .collect();
            push(c.record, base + prior, eps, Reading::Direct);
        } else if pos.is_none() || chain.len() < 2 {
            // Number past the end and no franchise to run into.
            push(c.record, base - 0.15, Vec::new(), Reading::Direct);
        }
        let Some(pos) = pos else { return };
        if chain.len() < 2 {
            return;
        }
        let place = |from: usize| place(index, chain, from, &numbers.values);

        // Absolute across the franchise.
        if numbers.season.is_none_or(|s| s <= 1) {
            // A title that states the range (`Gintama (Episodes 202-252)`).
            let mut ranged = false;
            for &r in chain {
                let hit = self
                    .info(index, r)
                    .ranges
                    .iter()
                    .find(|(a, b)| *a <= first && first <= *b)
                    .copied();
                if let Some((a, _)) = hit
                    && a > 1
                {
                    ranged = true;
                    let eps = numbers
                        .values
                        .iter()
                        .map(|&n| EpisodeRef {
                            season: None,
                            episode: n + 1 - a.min(n),
                            absolute: Some(n),
                        })
                        .collect();
                    push(r, base + 0.02, eps, Reading::Absolute);
                }
            }
            let head_eps = index.record(chain[0]).episodes.unwrap_or(0);
            if !ranged
                && first > head_eps
                && let Some((r, eps)) = place(0)
            {
                let prior = if pos == 0 { 0.0 } else { -0.02 };
                push(r, base + prior, eps, Reading::Absolute);
            }
        }
        // Explicit season → the first entry whose titles claim it most
        // (`Season 2`, `2nd Season`; a later cour continues the season), or
        // the n-th entry when none does. Season 1 starts the franchise.
        if let Some(s) = numbers.season
            && s >= 1
        {
            let part = numbers.part.unwrap_or(1);
            let mut bases = self.info(index, chain[0]).bases.clone();
            if let Some(q) = best.title.as_deref() {
                bases.push(TitleKey::new(q).base);
            }
            let claims: Vec<u32> = chain
                .iter()
                .map(|&r| self.info(index, r).claims(s, Some(part), &bases))
                .collect();
            let max = claims.iter().copied().max().unwrap_or(0);
            if max > 0 && (s > 1 || part > 1) {
                let start = claims.iter().position(|&n| n == max).unwrap_or(0);
                // A single title claiming the season is weak evidence
                // (synonym lists carry stray `Season 4`s).
                let strength = if max == 1 { -0.06 } else { 0.0 };
                for (p, &n) in claims.iter().enumerate() {
                    if n > 0
                        && let Some((r, eps)) = place(p)
                    {
                        let prior = if p == start {
                            0.0
                        } else {
                            0.10 * (n as f32 / max as f32 - 1.0) - 0.03
                        };
                        push(r, base + 0.01 + strength + prior, eps, Reading::Season);
                    }
                }
            } else if (s as usize) <= chain.len()
                && let Some((r, eps)) = place(s as usize - 1)
            {
                push(r, base - 0.02, eps, Reading::Season);
            }
        }
        // Number running on from a later entry named after itself
        // (`Mugen Ressha-hen - 12`), not reached through the franchise title.
        let specific = !self
            .info(index, chain[0])
            .bases
            .contains(&TitleKey::new(&rec.titles[c.title]).base);
        if pos > 0
            && specific
            && own.is_some_and(|n| last > n)
            && let Some((r, eps)) = place(pos)
        {
            push(r, base - 0.04, eps, Reading::Overflow);
        }
    }

    /// Bonus for a literal (case-insensitive, unfolded) match of the written
    /// title: `Gintama'` is the record titled `Gintama'`, not `Gintama`,
    /// although both fold to the same key.
    fn literal(&mut self, index: &TitleIndex, c: &Candidate, best: &ParsedName) -> f32 {
        let Some(title) = best.title.as_deref() else {
            return 0.0;
        };
        let want = title.trim().to_lowercase();
        let info = self.info(index, c.record);
        if info.lower.first() == Some(&want) {
            0.08
        } else if info.lower.contains(&want) {
            0.01
        } else {
            0.0
        }
    }

    /// Franchise of an anime candidate.
    fn franchise(
        &mut self,
        index: &TitleIndex,
        c: &Candidate,
        all: &[Candidate],
        best: &ParsedName,
    ) -> Franchise {
        let rec = index.record(c.record);
        let anidb = rec.ids.contains_key("anidb");
        let query_base = best.title.as_deref().map(|t| TitleKey::new(t).base);
        let mut bases: Vec<String> = vec![
            TitleKey::new(&rec.titles[c.title]).base,
            TitleKey::new(&rec.titles[0]).base,
        ];
        bases.extend(query_base.clone());
        bases.sort();
        bases.dedup();
        let mut members: Vec<usize> = bases
            .iter()
            .flat_map(|b| index.records_with_base(b))
            .collect();
        // Entries named after an arc (`Kimetsu no Yaiba: Yuukaku-hen`) share
        // only a prefix with the franchise title.
        if let Some(q) = &query_base {
            for o in all {
                let main = &self.info(index, o.record).bases;
                if main.iter().any(|b| shares_prefix(q, b)) {
                    members.push(o.record);
                }
            }
        }
        members.push(c.record);
        members.retain(|&r| {
            let x = index.record(r);
            x.anime && x.kind == WorkKind::Series && (!anidb || x.ids.contains_key("anidb"))
        });
        members.sort_by_key(|&r| {
            let x = index.record(r);
            let aid = x
                .ids
                .get("anidb")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(u64::MAX);
            (x.year.unwrap_or(u16::MAX), aid, r)
        });
        members.dedup();
        // Cut at a restart: after a long gap, an entry whose main title is
        // the segment head's title again (`Fruits Basket 1st Season` (2019)
        // after `Fruits Basket` (2001), `Hunter x Hunter (2011)`) is a
        // remake; `Bleach: Sennen Kessen-hen` (2022) continues `Bleach`.
        let mut segments: Vec<Vec<usize>> = Vec::new();
        let mut last_year: Option<u16> = None;
        // Entries that follow a long gap without a season claim and carry
        // the written title (`Kino no Tabi (2017)`): a small number under
        // the bare title may belong to either series.
        let mut remake_like: Vec<usize> = Vec::new();
        for r in members {
            let year = index.record(r).year;
            let gap = matches!((last_year, year), (Some(a), Some(b)) if b.saturating_sub(a) >= REMAKE_GAP_YEARS);
            let restart = match segments.last() {
                Some(seg) if gap => {
                    let head = TitleKey::new(&index.record(seg[0]).titles[0]).base;
                    TitleKey::new(&index.record(r).titles[0]).base == head
                }
                _ => false,
            };
            if gap && !restart {
                let info = self.info(index, r);
                if let Some(q) = &query_base
                    && info.bases.contains(q)
                    && !info.seasons.iter().any(|(b, s, _)| *s >= 2 && b == q)
                {
                    remake_like.push(r);
                }
            }
            if restart || segments.is_empty() {
                segments.push(Vec::new());
            }
            segments.last_mut().expect("pushed").push(r);
            if year.is_some() {
                last_year = year;
            }
        }
        // A remake competes only if it carries the written title too.
        let remake = segments.len() > 1
            && query_base.as_ref().is_some_and(|q| {
                segments
                    .iter()
                    .filter(|seg| !seg.contains(&c.record))
                    .flatten()
                    .any(|&r| self.info(index, r).bases.contains(q))
            });
        let chain = segments
            .into_iter()
            .find(|s| s.contains(&c.record))
            .unwrap_or_else(|| vec![c.record]);
        let soft_remake = remake_like
            .iter()
            .filter(|r| chain.contains(r))
            .filter_map(|&r| index.record(r).episodes)
            .max();
        Franchise {
            chain,
            remake,
            soft_remake,
        }
    }
}

/// `entry` continues the written title `query`: it starts with all words of
/// `query` (`gintama` → `gintama porori hen`), or both share at least three
/// leading words (`kimetsu no yaiba mugen ressha hen` → `kimetsu no yaiba
/// yuukaku hen`). A shorter parent title (`dragon ball` for `dragon ball
/// super`) does not.
fn shares_prefix(query: &str, entry: &str) -> bool {
    let (wq, we): (Vec<&str>, Vec<&str>) = (query.split(' ').collect(), entry.split(' ').collect());
    let common = wq.iter().zip(&we).take_while(|(x, y)| x == y).count();
    common >= 3 || (common == wq.len() && common > 0)
}

/// Places written numbers counted from the start of entry `from`.
fn place(
    index: &TitleIndex,
    chain: &[usize],
    from: usize,
    values: &[u32],
) -> Option<(usize, Vec<EpisodeRef>)> {
    let mut target = None;
    let mut eps = Vec::new();
    for &n in values {
        let (p, e) = locate(index, chain, from, n)?;
        if *target.get_or_insert(p) != p {
            // A multi-episode file across two entries: keep the first.
            continue;
        }
        eps.push(EpisodeRef {
            season: None,
            episode: e,
            absolute: offset(index, chain, p).map(|o| o + e),
        });
    }
    target.map(|p| (chain[p], eps))
}

/// Hand-set mapping of the signals to `[0, 1]`.
fn confidence(s: &Signals, best: &ParsedName) -> f32 {
    let quality = ((s.top - 0.70) / 0.30).clamp(0.0, 1.0);
    let separation = (s.margin / 0.10).clamp(0.0, 1.0);
    let mut c = 0.25 + 0.75 * quality * separation;
    if matches!(s.tier, Some(MatchTier::Fuzzy)) {
        c *= 0.85;
    }
    if best.extra.is_some() {
        c *= 0.95;
    }
    if s.remake && !s.year {
        c = c.min(0.6);
    }
    c.clamp(0.0, 1.0)
}

/// Score adjustment for kind and year agreement between the name and a record.
fn agreement(best: &ParsedName, numbers: &Numbers, rec: &TitleRecord) -> f32 {
    let mut adj = 0.0;
    let last = numbers.values.iter().copied().max();
    match rec.kind {
        WorkKind::Movie if numbers.episodic => adj -= 0.12,
        WorkKind::Series if !numbers.episodic && best.media == MediaHint::Movie => adj -= 0.06,
        WorkKind::Special => {
            if matches!(
                best.episode_kind,
                Some(EpisodeKind::Ova | EpisodeKind::Ona | EpisodeKind::Special | EpisodeKind::Sp)
            ) {
                adj += 0.03;
            } else if numbers.episodic {
                adj -= 0.03;
            }
        }
        WorkKind::Series
            if matches!(best.episode_kind, Some(EpisodeKind::Ova | EpisodeKind::Ona)) =>
        {
            adj -= 0.03
        }
        _ => {}
    }
    // Episode number past the end of a record that cannot run on.
    if rec.kind != WorkKind::Series
        && let (Some(n), Some(count)) = (last, rec.episodes.filter(|&c| c > 0))
        && n > count
    {
        adj -= 0.15;
    }
    // Dated episodes are western daily shows, never anime.
    if let Some(date) = best.date {
        if rec.anime {
            adj -= 0.08;
        }
        if rec.year.is_some_and(|y| date.year + 1 < y) {
            adj -= 0.10;
        }
    }
    if rec.anime && !best.anime_hint && numbers.season.is_some() && !rec.ids.is_empty() {
        // `Show.S01E02` without fansub markers leans western.
        adj -= 0.01;
    }
    if let (Some(a), Some(b)) = (best.year, rec.year)
        && a.abs_diff(b) > 1
    {
        adj -= 0.10;
    }
    // AniDB is the canonical anime space; anime-offline-database entries
    // without an AniDB id are mostly split duplicates of one that has it.
    if rec.anime && !rec.ids.contains_key("anidb") {
        adj -= 0.03;
    }
    adj
}

/// Entry (position in `chain`) and episode for number `n` counted from the
/// start of entry `from`. An entry with an unknown count takes everything.
fn locate(index: &TitleIndex, chain: &[usize], from: usize, n: u32) -> Option<(usize, u32)> {
    if n == 0 {
        return None;
    }
    let mut e = n;
    let mut i = from;
    loop {
        match index.record(*chain.get(i)?).episodes.filter(|&c| c > 0) {
            Some(count) if e > count => {
                e -= count;
                i += 1;
            }
            _ => return Some((i, e)),
        }
    }
}

/// Regular episodes of the entries before `pos`, if all are known.
fn offset(index: &TitleIndex, chain: &[usize], pos: usize) -> Option<u32> {
    if chain.len() < 2 {
        return None;
    }
    chain[..pos]
        .iter()
        .map(|&r| index.record(r).episodes.filter(|&c| c > 0))
        .sum()
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Extras (`Extras/Trailer 2.mkv`, `sample.mkv`) and files without a title
/// take the record of the nearest enclosing folder whose confident main
/// files all agree. A main file never inherits: `The Lion King.mkv` next to
/// `The Lion King (1994).mkv` may well be the remake.
fn inherit_from_folder(files: &[GroupFile<'_>], parsed: &[ParsedName], out: &mut [FileMatch]) {
    const CONFIDENT: f32 = 0.6;
    let own: Vec<Option<(usize, f32)>> = out
        .iter()
        .zip(parsed)
        .map(|(m, p)| {
            m.record
                .filter(|_| p.extra.is_none() && m.confidence >= CONFIDENT)
                .map(|r| (r, m.confidence))
        })
        .collect();
    for (i, file) in files.iter().enumerate() {
        let extra = parsed[i].extra.is_some();
        if !(extra || out[i].record.is_none()) {
            continue;
        }
        let mut dir = parent(file.path);
        loop {
            let prefix = format!("{dir}/");
            let mut found: Option<(usize, f32)> = None;
            let mut conflict = false;
            for (j, other) in files.iter().enumerate() {
                if j == i || !(dir.is_empty() || other.path.starts_with(&prefix)) {
                    continue;
                }
                if let Some((r, c)) = own[j] {
                    match found {
                        Some((fr, fc)) if fr == r => found = Some((r, fc.min(c))),
                        Some(_) => conflict = true,
                        None => found = Some((r, c)),
                    }
                }
            }
            if conflict {
                break;
            }
            if let Some((r, c)) = found {
                let m = &mut out[i];
                m.record = Some(r);
                m.confidence = c * 0.95;
                m.episodes.clear();
                m.signals.reading = Reading::Inherited;
                break;
            }
            if dir.is_empty() {
                break;
            }
            dir = parent(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anime(aid: u32, title: &str, year: u16, episodes: u32, synonyms: &[&str]) -> TitleRecord {
        let mut titles = vec![title.to_owned()];
        titles.extend(synonyms.iter().map(|s| (*s).to_owned()));
        TitleRecord {
            ids: [("anidb".to_owned(), aid.to_string())].into(),
            kind: WorkKind::Series,
            anime: true,
            year: Some(year),
            episodes: Some(episodes),
            titles,
        }
    }

    fn movie(tmdb: u32, title: &str, year: u16) -> TitleRecord {
        TitleRecord {
            ids: [("tmdb".to_owned(), format!("movie:{tmdb}"))].into(),
            kind: WorkKind::Movie,
            year: Some(year),
            titles: vec![title.to_owned()],
            ..Default::default()
        }
    }

    fn index() -> TitleIndex {
        TitleIndex::from_records([
            anime(11739, "Boku no Hero Academia", 2016, 13, &["My Hero Academia"]),
            anime(12233, "Boku no Hero Academia 2nd Season", 2017, 25, &["Boku no Hero Academia (2017)"]),
            anime(13485, "Boku no Hero Academia 3rd Season", 2018, 25, &["Boku no Hero Academia (2018)"]),
            anime(5841, "Clannad: After Story", 2008, 24, &[]),
            anime(5000, "Clannad", 2007, 23, &[]),
            anime(3468, "Gintama", 2006, 201, &["Gintama: Season 1"]),
            anime(8126, "Gintama'", 2011, 51, &["Gintama (Episodes 202-252)", "Gintama Season 2"]),
            anime(16803, "Kimetsu no Yaiba: Mugen Ressha-hen", 2021, 7, &["Demon Slayer: Kimetsu no Yaiba Season 2"]),
            anime(14107, "Kimetsu no Yaiba", 2019, 26, &["Demon Slayer", "Demon Slayer: Kimetsu no Yaiba"]),
            anime(16054, "Kimetsu no Yaiba: Yuukaku-hen", 2022, 11, &["Demon Slayer Season 2"]),
            anime(34, "Fruits Basket", 2001, 26, &["Fruits Basket (2001)"]),
            anime(14490, "Fruits Basket 1st Season", 2019, 25, &["Fruits Basket (2019)"]),
            movie(8587, "The Lion King", 1994),
            movie(420818, "The Lion King", 2019),
            movie(603, "The Matrix", 1999),
        ])
    }

    fn one(index: &TitleIndex, path: &str) -> FileMatch {
        score_group(index, &[GroupFile { path, duration_ms: None }]).remove(0)
    }

    fn aid(index: &TitleIndex, m: &FileMatch) -> String {
        index.record(m.record.expect("decided")).ids["anidb"].clone()
    }

    #[test]
    fn absolute_number_runs_into_later_season() {
        let ix = index();
        let m = one(&ix, "Anime/Boku no Hero Academia/[Commie] Boku no Hero Academia - 35 (1080p).mkv");
        assert_eq!(aid(&ix, &m), "12233");
        assert_eq!(m.episodes, vec![EpisodeRef { season: None, episode: 22, absolute: Some(35) }]);
        assert_eq!(m.signals.reading, Reading::Absolute);
        assert!(m.confidence >= 0.9, "{m:?}");
    }

    #[test]
    fn small_number_stays_in_first_season() {
        let ix = index();
        let m = one(&ix, "Anime/Boku no Hero Academia/[Commie] Boku no Hero Academia - 05 (1080p).mkv");
        assert_eq!(aid(&ix, &m), "11739");
        assert_eq!(m.episodes[0].episode, 5);
    }

    #[test]
    fn explicit_season_picks_entry() {
        let ix = index();
        let m = one(&ix, "TV/Boku no Hero Academia/Season 03/Boku no Hero Academia S03E04.mkv");
        assert_eq!(aid(&ix, &m), "13485");
        assert_eq!(m.episodes[0].episode, 4);
    }

    #[test]
    fn season_overflows_into_next_entry() {
        let ix = index();
        let m = one(&ix, "TV/Demon Slayer/Season 02/Demon.Slayer.S02E12.1080p.WEB.mkv");
        assert_eq!(aid(&ix, &m), "16054");
        assert_eq!(m.episodes[0].episode, 5);
    }

    #[test]
    fn written_range_wins() {
        let ix = index();
        let m = one(&ix, "Anime/Gintama/[FFF] Gintama - 210 [720p].mkv");
        assert_eq!(aid(&ix, &m), "8126");
        assert_eq!(m.episodes[0].episode, 9);
    }

    #[test]
    fn literal_title_breaks_folded_tie() {
        let ix = index();
        let m = one(&ix, "Anime/Gintama'/Gintama' - 31 [1080p].mkv");
        assert_eq!(aid(&ix, &m), "8126");
        let m = one(&ix, "Anime/Gintama/Gintama - 31 [1080p].mkv");
        assert_eq!(aid(&ix, &m), "3468");
    }

    #[test]
    fn sequel_title_is_its_own_entry() {
        let ix = index();
        let m = one(&ix, "Anime/Clannad - After Story/[MTBB] Clannad - After Story - 03 [1080p].mkv");
        assert_eq!(aid(&ix, &m), "5841");
    }

    #[test]
    fn remakes_are_not_auto() {
        let ix = index();
        let m = one(&ix, "Movies/The Lion King/The Lion King.mkv");
        assert!(m.confidence < 0.5, "{m:?}");
        let m = one(&ix, "Movies/The Lion King (1994)/The Lion King (1994).mkv");
        assert_eq!(ix.record(m.record.unwrap()).ids["tmdb"], "movie:8587");
        assert!(m.confidence >= 0.9, "{m:?}");
        let m = one(&ix, "Anime/Fruits Basket/Fruits Basket - 05 [720p].mkv");
        assert!(m.signals.remake && m.confidence < 0.9, "{m:?}");
    }

    #[test]
    fn extra_inherits_folder_decision() {
        let ix = index();
        let files = [
            GroupFile { path: "Movies/The Lion King (2019)/The Lion King (2019).mkv", duration_ms: None },
            GroupFile { path: "Movies/The Lion King (2019)/Extras/Trailer 2.mkv", duration_ms: None },
        ];
        let out = score_group(&ix, &files);
        assert_eq!(ix.record(out[0].record.unwrap()).ids["tmdb"], "movie:420818");
        assert_eq!(out[1].record, out[0].record);
        assert_eq!(out[1].signals.reading, Reading::Inherited);
    }

    #[test]
    fn range_parsing() {
        assert_eq!(episode_range("Gintama (Episodes 202-252)"), Some((202, 252)));
        assert_eq!(episode_range("Gintama (Episoden 354-367)"), Some((354, 367)));
        assert_eq!(episode_range("Gintama (2011)"), None);
        assert!(shares_prefix("kimetsu no yaiba", "kimetsu no yaiba yuukaku hen"));
        assert!(!shares_prefix("boku no hero academia", "boku no pico"));
        assert!(shares_prefix("gintama", "gintama porori hen"));
        assert!(!shares_prefix("dragon ball super", "dragon ball"));
    }
}
