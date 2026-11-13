// SPDX-License-Identifier: MPL-2.0
//! Scorer v0: picks the work (and, for anime, the franchise entry and the
//! episode inside it) for every file of a group from the local title index —
//! stage \[5\] of research 03 §4.2, hand-set weights (META-026; the trained
//! and calibrated scorer is META-018).
//!
//! For each file the parser's N-best hypotheses build a [`TitleQuery`], the
//! [`TitleIndex`] returns the top candidates, and every candidate is turned
//! into one or more *interpretations*:
//!
//! - movies and western series: the candidate itself, adjusted for kind
//!   (episode markers vs a movie record) and year;
//! - anime series with an episode number: the franchise of the candidate
//!   (TV/ONA records sharing the title base key, ordered by year) lets the
//!   written number be read directly (`Clannad - After Story - 05`), as an
//!   absolute number across the franchise (`Boku no Hero Academia - 133` →
//!   6th season, ep. 20), through an explicit season (`Haikyuu!! S04E15` →
//!   the 4th entry, overflowing into the next cour) or as a number that runs
//!   on from the matched entry.
//!
//! Interpretations are scored, merged per record, and the confidence of a
//! file comes from the best score and its margin over the best *different*
//! record — two equally good works (remakes without a year, same-name
//! series) give a low confidence instead of a silent error. Files without a
//! title of their own (samples, trailers in `Extras/`) take the decision of
//! their folder when every confident file there agrees.
//!
//! Everything is deterministic and offline: no provider calls, no clocks.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::parse::{EpisodeKind, MediaHint, ParsedName, parse};
use crate::title::normalize::TitleKey;
use crate::title::{Candidate, MatchTier, TitleIndex, TitleQuery, TitleRecord, WorkKind};

/// Candidates taken from the index per file.
pub const TOP_K: usize = 10;

/// One input file of a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupFile<'a> {
    /// Path relative to the library root, `/`-separated.
    pub path: &'a str,
    /// Container duration, if probed.
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

/// How the episode number of the decision was read.
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
    /// Explicit season → n-th franchise entry.
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
    /// Score of the chosen interpretation.
    pub top: f32,
    /// Margin over the best interpretation pointing at a different record
    /// (`top` when there is none).
    pub margin: f32,
    /// How the best title matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<MatchTier>,
    /// How the episode number was read.
    pub reading: Reading,
    /// The name carries a year.
    pub year: bool,
    /// The name carries episode markers.
    pub episodic: bool,
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
    /// mapping of the signals, not calibrated yet — META-018).
    pub confidence: f32,
    /// Episodes contained in the file, inside `record`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<EpisodeRef>,
    /// Why.
    pub signals: Signals,
}

/// One reading of a file as a record.
#[derive(Debug, Clone)]
struct Hyp {
    record: usize,
    score: f32,
    episodes: Vec<EpisodeRef>,
    reading: Reading,
    tier: MatchTier,
}

/// What the file name says about numbers.
struct Numbers {
    /// Whole episode numbers as written (absolute or within a season).
    values: Vec<u32>,
    season: Option<u32>,
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
        Self {
            episodic: !source.is_empty() || p.date.is_some() || p.episode_kind.is_some(),
            values,
            season: p.season,
        }
    }
}

/// Scores a whole group (one folder tree, one case) at once.
pub fn score_group(index: &TitleIndex, files: &[GroupFile<'_>]) -> Vec<FileMatch> {
    let mut out: Vec<FileMatch> = files.iter().map(|f| score_file(index, f.path)).collect();
    inherit_from_folder(files, &mut out);
    out
}

/// Scores one file on its own (no group context).
pub fn score_file(index: &TitleIndex, path: &str) -> FileMatch {
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
    let candidates = index.search(&query, TOP_K);
    signals.candidates = candidates.len();

    let mut hyps = Vec::new();
    for c in &candidates {
        interpret(index, c, best, &numbers, &mut hyps);
    }
    // Best interpretation per record.
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
    let runner_up = ranked.get(1).map_or(0.0, |h| h.score);
    signals.top = top.score;
    signals.margin = top.score - runner_up;
    signals.tier = Some(top.tier);
    signals.reading = top.reading;
    let confidence = confidence(&signals, best);
    FileMatch {
        record: Some(top.record),
        confidence,
        episodes: top.episodes.clone(),
        signals,
    }
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
    c.clamp(0.0, 1.0)
}

/// Score adjustment for kind and year agreement between the name and a record.
fn agreement(best: &ParsedName, numbers: &Numbers, rec: &TitleRecord) -> f32 {
    let mut adj = 0.0;
    match rec.kind {
        WorkKind::Movie if numbers.episodic => adj -= 0.12,
        WorkKind::Series if !numbers.episodic && best.media == MediaHint::Movie => adj -= 0.06,
        WorkKind::Special
            if matches!(
                best.episode_kind,
                Some(EpisodeKind::Ova | EpisodeKind::Ona | EpisodeKind::Special | EpisodeKind::Sp)
            ) =>
        {
            adj += 0.03
        }
        WorkKind::Series
            if matches!(
                best.episode_kind,
                Some(EpisodeKind::Ova | EpisodeKind::Ona)
            ) =>
        {
            adj -= 0.03
        }
        _ => {}
    }
    if let (Some(a), Some(b)) = (best.year, rec.year)
        && a.abs_diff(b) > 1
    {
        adj -= 0.10;
    }
    adj
}

/// Turns one candidate into interpretations.
fn interpret(
    index: &TitleIndex,
    c: &Candidate,
    best: &ParsedName,
    numbers: &Numbers,
    out: &mut Vec<Hyp>,
) {
    let rec = index.record(c.record);
    let base = c.score + agreement(best, numbers, rec);
    let hyp = |record, score, episodes, reading| Hyp {
        record,
        score,
        episodes,
        reading,
        tier: c.tier,
    };
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
        out.push(hyp(c.record, base, episodes, Reading::Title));
        return;
    }

    let chain = franchise(index, c);
    let pos = chain.iter().position(|&r| r == c.record);
    let first = numbers.values[0];
    let last = *numbers.values.iter().max().unwrap_or(&first);
    let place = |from: usize| -> Option<(usize, Vec<EpisodeRef>)> {
        let mut target = None;
        let mut eps = Vec::new();
        for &n in &numbers.values {
            let (p, e) = locate(index, &chain, from, n)?;
            if *target.get_or_insert(p) != p {
                // A multi-episode file across two entries: keep the first.
                continue;
            }
            eps.push(EpisodeRef {
                season: None,
                episode: e,
                absolute: offset(index, &chain, p).map(|o| o + e),
            });
        }
        target.map(|p| (chain[p], eps))
    };

    // Direct: the number is inside the matched entry.
    let own = rec.episodes.filter(|&n| n > 0);
    if own.is_none_or(|n| last <= n) {
        let mut prior = 0.0;
        // `Title - 05` matched through a qualified title (`Title (2017)`,
        // `Title 2`) of a later entry: the bare title means the first one.
        let qualified = TitleKey::new(&rec.titles[c.title]);
        if c.tier != MatchTier::Exact && (qualified.season.is_some() || qualified.year.is_some()) {
            prior -= 0.04;
        }
        if let (Some(s), Some(p)) = (numbers.season, pos)
            && chain.len() > 1
            && s as usize != p + 1
        {
            prior -= 0.06;
        }
        let eps = numbers
            .values
            .iter()
            .map(|&e| EpisodeRef {
                season: None,
                episode: e,
                absolute: pos.and_then(|p| offset(index, &chain, p)).map(|o| o + e),
            })
            .collect();
        out.push(hyp(c.record, base + prior, eps, Reading::Direct));
    } else if pos.is_none() || chain.len() < 2 {
        // Number past the end and no franchise to run into.
        let eps = Vec::new();
        out.push(hyp(c.record, base - 0.15, eps, Reading::Direct));
    }
    let Some(pos) = pos else { return };
    if chain.len() < 2 {
        return;
    }
    // Absolute across the franchise.
    let head_eps = index.record(chain[0]).episodes.unwrap_or(0);
    if numbers.season.is_none_or(|s| s <= 1)
        && first > head_eps
        && let Some((r, eps)) = place(0)
    {
        let prior = if pos == 0 { 0.0 } else { -0.02 };
        out.push(hyp(r, base + prior, eps, Reading::Absolute));
    }
    // Explicit season → n-th entry.
    if let Some(s) = numbers.season
        && s >= 1
        && (s as usize) <= chain.len()
        && let Some((r, eps)) = place(s as usize - 1)
    {
        let prior = if s as usize == pos + 1 { 0.01 } else { 0.0 };
        out.push(hyp(r, base + prior, eps, Reading::Season));
    }
    // Number running on from a later entry (`Gintama. - 14`).
    if pos > 0
        && own.is_some_and(|n| last > n)
        && let Some((r, eps)) = place(pos)
    {
        out.push(hyp(r, base - 0.04, eps, Reading::Overflow));
    }
}

/// Franchise of an anime candidate: TV/ONA records sharing the base key of
/// the matched title or of the main title, by year, then AniDB id.
fn franchise(index: &TitleIndex, c: &Candidate) -> Vec<usize> {
    let rec = index.record(c.record);
    let mut members = index.records_with_base(&TitleKey::new(&rec.titles[c.title]).base);
    if c.title != 0 {
        members.extend(index.records_with_base(&TitleKey::new(&rec.titles[0]).base));
    }
    members.retain(|&r| {
        let x = index.record(r);
        x.anime && x.kind == WorkKind::Series
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
    members
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

/// Files without a decision of their own (no title: `sample.mkv`,
/// `Extras/Trailer 2.mkv`) take the record of the nearest enclosing folder
/// whose confident files all agree on one record.
fn inherit_from_folder(files: &[GroupFile<'_>], out: &mut [FileMatch]) {
    const CONFIDENT: f32 = 0.6;
    let own: Vec<Option<(usize, f32)>> = out
        .iter()
        .map(|m| m.record.map(|r| (r, m.confidence)))
        .collect();
    for (i, file) in files.iter().enumerate() {
        if out[i].record.is_some() {
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
                if let Some((r, c)) = own[j]
                    && c >= CONFIDENT
                {
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
            movie(8587, "The Lion King", 1994),
            movie(420818, "The Lion King", 2019),
            movie(603, "The Matrix", 1999),
        ])
    }

    fn one(index: &TitleIndex, path: &str) -> FileMatch {
        score_group(index, &[GroupFile { path, duration_ms: None }]).remove(0)
    }

    #[test]
    fn absolute_number_runs_into_later_season() {
        let ix = index();
        let m = one(&ix, "Anime/Boku no Hero Academia/[Commie] Boku no Hero Academia - 35 (1080p).mkv");
        assert_eq!(ix.record(m.record.unwrap()).ids["anidb"], "12233");
        assert_eq!(m.episodes, vec![EpisodeRef { season: None, episode: 22, absolute: Some(35) }]);
        assert_eq!(m.signals.reading, Reading::Absolute);
        assert!(m.confidence >= 0.9, "{m:?}");
    }

    #[test]
    fn small_number_stays_in_first_season() {
        let ix = index();
        let m = one(&ix, "Anime/Boku no Hero Academia/[Commie] Boku no Hero Academia - 05 (1080p).mkv");
        assert_eq!(ix.record(m.record.unwrap()).ids["anidb"], "11739");
        assert_eq!(m.episodes[0].episode, 5);
    }

    #[test]
    fn explicit_season_picks_entry() {
        let ix = index();
        let m = one(&ix, "TV/Boku no Hero Academia/Season 03/Boku no Hero Academia S03E04.mkv");
        assert_eq!(ix.record(m.record.unwrap()).ids["anidb"], "13485");
        assert_eq!(m.episodes[0].episode, 4);
    }

    #[test]
    fn sequel_title_is_its_own_entry() {
        let ix = index();
        let m = one(&ix, "Anime/Clannad - After Story/[MTBB] Clannad - After Story - 03 [1080p].mkv");
        assert_eq!(ix.record(m.record.unwrap()).ids["anidb"], "5841");
    }

    #[test]
    fn remake_without_year_is_not_auto() {
        let ix = index();
        let m = one(&ix, "Movies/The Lion King/The Lion King.mkv");
        assert!(m.confidence < 0.5, "{m:?}");
        let m = one(&ix, "Movies/The Lion King (1994)/The Lion King (1994).mkv");
        assert_eq!(ix.record(m.record.unwrap()).ids["tmdb"], "movie:8587");
        assert!(m.confidence >= 0.9, "{m:?}");
    }

    #[test]
    fn sample_inherits_folder_decision() {
        let ix = index();
        let files = [
            GroupFile { path: "Movies/The Matrix (1999)/The.Matrix.1999.1080p.BluRay.mkv", duration_ms: None },
            GroupFile { path: "Movies/The Matrix (1999)/Extras/sample.mkv", duration_ms: None },
        ];
        let out = score_group(&ix, &files);
        assert_eq!(out[0].record, out[1].record);
        assert!(out[0].record.is_some());
    }
}
