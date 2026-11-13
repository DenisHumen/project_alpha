// SPDX-License-Identifier: MPL-2.0
//! Per-item scoring and metric aggregation (research 03 §6.2).
//!
//! Definitions (all identification metrics are over *scorable* items: labelled
//! files that reference a work whose label carries at least one external id):
//!
//! - decision: `auto` when a work is predicted with `confidence ≥ threshold`,
//!   `review` when predicted below it, `manual` when the recognizer abstains;
//! - work is correct when prediction and label share at least one id
//!   namespace, and every shared namespace has the same value;
//! - Precision@auto = correct auto / auto; Coverage = auto / scorable;
//!   Recall = correct auto / scorable;
//! - silent errors = wrong auto / scorable (main KPI, target ≤ 0.5 %);
//! - user load = review / scorable and manual / scorable;
//! - ECE over predicted items, 10 equal-width confidence bins;
//! - episode accuracy per scheme: a scheme is labelled for an item when every
//!   episode of the file has it; correct needs a correct auto match and the
//!   same multiset of positions;
//! - parser accuracy per labelled `parse` field (title compared after
//!   [`normalize`](crate::recognizer::normalize)).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::corpus::{FileLabel, Ids, Numbering, ParsedName, SCHEMES};
use crate::recognizer::{FilePrediction, normalize};

/// Number of reliability bins for ECE.
pub const BINS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Auto,
    Review,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkOutcome {
    Correct,
    Wrong,
    /// No work predicted.
    None,
    /// The file label has no work, or the work has no ids.
    Unscorable,
}

/// Outcome for one labelled unit; kept in the JSON report for diffs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemOutcome {
    pub case_id: String,
    pub path: String,
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub role: String,
    pub decision: Decision,
    pub work: WorkOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// Per labelled scheme: episode position correct.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub episodes: BTreeMap<String, bool>,
    /// Per labelled parse field: value correct.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parse: BTreeMap<String, bool>,
}

impl ItemOutcome {
    pub fn scorable(&self) -> bool {
        self.work != WorkOutcome::Unscorable
    }

    /// Confident and wrong.
    pub fn silent_error(&self) -> bool {
        self.decision == Decision::Auto && self.work == WorkOutcome::Wrong
    }

    pub fn auto_correct(&self) -> bool {
        self.decision == Decision::Auto && self.work == WorkOutcome::Correct
    }
}

/// Scores one labelled file against the recognizer output for its case.
pub fn score_item(
    case_id: &str,
    categories: &[String],
    label: &FileLabel,
    work_label: Option<(&str, &Ids)>,
    predictions: &[FilePrediction],
    threshold: f64,
) -> ItemOutcome {
    let prediction = find_prediction(&label.path, predictions);
    let confidence = prediction.and_then(|p| p.work.as_ref().map(|_| p.confidence));
    let decision = match confidence {
        Some(c) if c >= threshold => Decision::Auto,
        Some(_) => Decision::Review,
        None => Decision::Manual,
    };
    let work = match (work_label, prediction.and_then(|p| p.work.as_ref())) {
        (None, _) => WorkOutcome::Unscorable,
        (Some((_, ids)), _) if ids.is_empty() => WorkOutcome::Unscorable,
        (Some(_), None) => WorkOutcome::None,
        (Some((_, truth)), Some(guess)) if ids_match(truth, &guess.ids) => WorkOutcome::Correct,
        (Some(_), Some(_)) => WorkOutcome::Wrong,
    };

    let mut episodes = BTreeMap::new();
    if work != WorkOutcome::Unscorable && !label.episodes.is_empty() {
        let truth: Vec<Option<&Numbering>> = label
            .episodes
            .iter()
            .map(|e| e.numbering.as_ref())
            .collect();
        for scheme in SCHEMES {
            let Some(want) = keys(truth.iter().copied(), scheme) else {
                continue;
            };
            let got = prediction.and_then(|p| keys(p.episodes.iter().map(Some), scheme));
            let ok = decision == Decision::Auto
                && work == WorkOutcome::Correct
                && got.as_ref() == Some(&want);
            episodes.insert(scheme.to_owned(), ok);
        }
    }

    let parse = match &label.parse {
        Some(want) => score_parse(want, prediction.and_then(|p| p.parse.as_ref())),
        None => BTreeMap::new(),
    };

    ItemOutcome {
        case_id: case_id.to_owned(),
        path: label.path.clone(),
        categories: categories.to_vec(),
        kind: work_label.map(|(k, _)| k.to_owned()),
        role: label.role.clone(),
        decision,
        work,
        confidence,
        episodes,
        parse,
    }
}

/// Exact path, or the most confident prediction inside a disc directory.
fn find_prediction<'p>(
    path: &str,
    predictions: &'p [FilePrediction],
) -> Option<&'p FilePrediction> {
    if let Some(p) = predictions.iter().find(|p| p.path == path) {
        return Some(p);
    }
    let prefix = format!("{path}/");
    predictions
        .iter()
        .filter(|p| p.path.starts_with(&prefix) && p.work.is_some())
        .max_by(|a, b| a.confidence.total_cmp(&b.confidence))
}

fn ids_match(truth: &Ids, guess: &Ids) -> bool {
    let guess: BTreeMap<_, _> = guess.pairs().into_iter().collect();
    let mut shared = 0;
    for (ns, value) in truth.pairs() {
        if let Some(g) = guess.get(ns) {
            if *g != value {
                return false;
            }
            shared += 1;
        }
    }
    shared > 0
}

/// Sorted positions in `scheme`, or `None` if any episode lacks it.
fn keys<'a>(eps: impl Iterator<Item = Option<&'a Numbering>>, scheme: &str) -> Option<Vec<String>> {
    let mut out = eps
        .map(|n| n.and_then(|n| n.key(scheme)))
        .collect::<Option<Vec<_>>>()?;
    if out.is_empty() {
        return None;
    }
    out.sort();
    Some(out)
}

fn score_parse(want: &ParsedName, got: Option<&ParsedName>) -> BTreeMap<String, bool> {
    let empty = ParsedName::default();
    let got = got.unwrap_or(&empty);
    let mut out = BTreeMap::new();
    let mut check = |field: &str, labelled: bool, ok: bool| {
        if labelled {
            out.insert(field.to_owned(), ok);
        }
    };
    let title_ok = match (&want.title, &got.title) {
        (Some(a), Some(b)) => normalize(a) == normalize(b),
        _ => false,
    };
    check("title", want.title.is_some(), title_ok);
    check("year", want.year.is_some(), want.year == got.year);
    check("season", want.season.is_some(), want.season == got.season);
    check(
        "episodes",
        want.episodes.is_some(),
        same_numbers(&want.episodes, &got.episodes),
    );
    check(
        "absolute",
        want.absolute.is_some(),
        same_numbers(&want.absolute, &got.absolute),
    );
    check("part", want.part.is_some(), want.part == got.part);
    let group_ok = match (&want.release_group, &got.release_group) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    };
    check("release_group", want.release_group.is_some(), group_ok);
    check("crc32", want.crc32.is_some(), want.crc32 == got.crc32);
    check(
        "version",
        want.version.is_some(),
        want.version == got.version,
    );
    out
}

fn same_numbers(a: &Option<Vec<f64>>, b: &Option<Vec<f64>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            let mut a = a.clone();
            let mut b = b.clone();
            a.sort_by(f64::total_cmp);
            b.sort_by(f64::total_cmp);
            a == b
        }
        _ => false,
    }
}

/// Ratio rounded to 6 decimals; `None` when the denominator is zero.
pub fn ratio(num: usize, den: usize) -> Option<f64> {
    (den > 0).then(|| round6(num as f64 / den as f64))
}

pub fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub labelled: usize,
    pub correct: usize,
    pub accuracy: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EpisodeCounts {
    pub labelled: usize,
    pub auto: usize,
    pub correct: usize,
    /// correct / auto.
    pub precision: Option<f64>,
    /// correct / labelled.
    pub recall: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bin {
    pub lower: f64,
    pub upper: f64,
    pub count: usize,
    pub mean_confidence: f64,
    pub accuracy: f64,
}

/// Aggregated metrics for a group of items.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub items: usize,
    pub scorable: usize,
    pub auto: usize,
    pub review: usize,
    pub manual: usize,
    pub auto_correct: usize,
    pub silent_errors: usize,
    pub precision_at_auto: Option<f64>,
    pub coverage: Option<f64>,
    pub recall: Option<f64>,
    pub silent_error_rate: Option<f64>,
    pub review_rate: Option<f64>,
    pub manual_rate: Option<f64>,
    pub ece: Option<f64>,
    /// Non-empty reliability bins.
    pub reliability: Vec<Bin>,
    pub episodes: BTreeMap<String, EpisodeCounts>,
    pub parse: BTreeMap<String, Counts>,
}

impl Metrics {
    pub fn from_items<'a>(items: impl IntoIterator<Item = &'a ItemOutcome>) -> Self {
        let mut m = Metrics::default();
        let mut bins = [(0usize, 0f64, 0usize); BINS];
        for item in items {
            m.items += 1;
            for (field, ok) in &item.parse {
                let c = m.parse.entry(field.clone()).or_default();
                c.labelled += 1;
                c.correct += usize::from(*ok);
            }
            if !item.scorable() {
                continue;
            }
            m.scorable += 1;
            match item.decision {
                Decision::Auto => m.auto += 1,
                Decision::Review => m.review += 1,
                Decision::Manual => m.manual += 1,
            }
            m.auto_correct += usize::from(item.auto_correct());
            m.silent_errors += usize::from(item.silent_error());
            if let Some(conf) = item.confidence {
                let conf = conf.clamp(0.0, 1.0);
                let b = ((conf * BINS as f64) as usize).min(BINS - 1);
                bins[b].0 += 1;
                bins[b].1 += conf;
                bins[b].2 += usize::from(item.work == WorkOutcome::Correct);
            }
            for (scheme, ok) in &item.episodes {
                let e = m.episodes.entry(scheme.clone()).or_default();
                e.labelled += 1;
                e.auto += usize::from(item.decision == Decision::Auto);
                e.correct += usize::from(*ok);
            }
        }
        m.precision_at_auto = ratio(m.auto_correct, m.auto);
        m.coverage = ratio(m.auto, m.scorable);
        m.recall = ratio(m.auto_correct, m.scorable);
        m.silent_error_rate = ratio(m.silent_errors, m.scorable);
        m.review_rate = ratio(m.review, m.scorable);
        m.manual_rate = ratio(m.manual, m.scorable);

        let predicted: usize = bins.iter().map(|b| b.0).sum();
        let mut ece = 0.0;
        for (i, &(count, conf_sum, correct)) in bins.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let mean_confidence = conf_sum / count as f64;
            let accuracy = correct as f64 / count as f64;
            ece += count as f64 / predicted as f64 * (accuracy - mean_confidence).abs();
            m.reliability.push(Bin {
                lower: i as f64 / BINS as f64,
                upper: (i + 1) as f64 / BINS as f64,
                count,
                mean_confidence: round6(mean_confidence),
                accuracy: round6(accuracy),
            });
        }
        m.ece = (predicted > 0).then(|| round6(ece));
        for e in m.episodes.values_mut() {
            e.precision = ratio(e.correct, e.auto);
            e.recall = ratio(e.correct, e.labelled);
        }
        for c in m.parse.values_mut() {
            c.accuracy = ratio(c.correct, c.labelled);
        }
        m
    }
}
