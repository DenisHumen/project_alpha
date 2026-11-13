// SPDX-License-Identifier: MPL-2.0
//! Candidate recall of the local title index (META-007).
//!
//! For every scorable labelled unit (a file whose work carries external ids)
//! the product parser (`pa_recognition::parse`) builds a
//! [`TitleQuery`] from the path, the [`TitleIndex`] returns ranked
//! candidates, and the unit counts as found at `k` when a candidate in the
//! top `k` is the labelled work (same rule as the identification metrics:
//! a shared id namespace and no disagreeing one). No provider is called.
//!
//! The index is built from local data only: the bundled anime-offline-database
//! ([`load_anime`]) and a catalogue of cached provider records
//! ([`load_catalog`], JSONL of [`TitleRecord`]).

use std::collections::BTreeMap;

use pa_recognition::parse::parse;
use pa_recognition::title::{TitleIndex, TitleQuery, TitleRecord, anime};
use serde::{Deserialize, Serialize};

use crate::corpus::{Corpus, Ids};
use crate::metrics::ids_match;

/// Default cut-offs.
pub const DEFAULT_KS: [usize; 4] = [1, 3, 5, 10];

/// Misses kept in the report.
const MAX_MISSES: usize = 200;

/// Reads a JSONL catalogue of title records (blank lines and `#` comments
/// are skipped).
pub fn load_catalog(text: &str) -> Result<Vec<TitleRecord>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1)))
        .collect()
}

/// Reads anime-offline-database data: the derived bundle
/// (`anime-offline-ids.jsonl`) or the upstream JSONL.
pub fn load_anime(bytes: &[u8]) -> Result<Vec<TitleRecord>, String> {
    use pa_anime_mappings::offline_db;
    let first = bytes.split(|&b| b == b'\n').next().unwrap_or_default();
    let entries = if first.windows(8).any(|w| w == b"\"format\"") {
        offline_db::read_bundle(bytes).map_err(|e| e.to_string())?.1
    } else {
        offline_db::parse_upstream_jsonl(bytes)
            .map_err(|e| e.to_string())?
            .1
    };
    Ok(entries.iter().map(anime::record).collect())
}

/// Record ids in the corpus [`Ids`] shape.
pub fn record_ids(record: &TitleRecord) -> Ids {
    let mut ids = Ids::default();
    for (ns, v) in &record.ids {
        let num = || v.parse::<u64>().ok();
        match ns.as_str() {
            "tmdb" => ids.tmdb = Some(v.clone()),
            "tvdb" => ids.tvdb = Some(v.clone()),
            "imdb" => ids.imdb = Some(v.clone()),
            "anidb" => ids.anidb = num(),
            "anilist" => ids.anilist = num(),
            "mal" => ids.mal = num(),
            "kitsu" => ids.kitsu = num(),
            "shikimori" => ids.shikimori = num(),
            "kinopoisk" => ids.kinopoisk = num(),
            "tvmaze" => ids.tvmaze = num(),
            _ => {}
        }
    }
    ids
}

/// Recall at each cut-off for a set of units.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Recall {
    /// Scorable units.
    pub items: usize,
    /// Units whose work is in the top `k`, per cut-off (same order as `ks`).
    pub found: Vec<usize>,
    /// `found / items`, per cut-off.
    pub recall: Vec<f64>,
    /// Mean reciprocal rank (0 when not in the candidate list).
    pub mrr: f64,
}

impl Recall {
    fn from_ranks(ranks: &[Option<usize>], ks: &[usize]) -> Self {
        let items = ranks.len();
        let found: Vec<usize> = ks
            .iter()
            .map(|&k| ranks.iter().filter(|r| r.is_some_and(|r| r < k)).count())
            .collect();
        let div = |n: f64| if items == 0 { 0.0 } else { n / items as f64 };
        Self {
            items,
            recall: found.iter().map(|&f| div(f as f64)).collect(),
            found,
            mrr: div(ranks.iter().flatten().map(|&r| 1.0 / (r + 1) as f64).sum()),
        }
    }
}

/// A unit whose work is not in the top `k_max`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Miss {
    pub case_id: String,
    pub path: String,
    /// Labelled work title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Query title variants.
    pub query: Vec<String>,
    /// Main title of the top candidate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<String>,
    /// Rank (0-based) beyond the cut-off, if the work was retrieved at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<usize>,
}

/// Candidate-recall report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateReport {
    pub parser_version: String,
    /// Records and distinct titles in the index.
    pub index_records: usize,
    pub index_titles: usize,
    /// Corpus files the report was computed on.
    pub sources: Vec<String>,
    pub ks: Vec<usize>,
    /// How many candidates were retrieved per unit.
    pub depth: usize,
    pub overall: Recall,
    pub per_split: BTreeMap<String, Recall>,
    pub per_category: BTreeMap<String, Recall>,
    pub per_kind: BTreeMap<String, Recall>,
    pub per_role: BTreeMap<String, Recall>,
    /// Misses at the largest cut-off (first [`MAX_MISSES`]).
    pub misses: Vec<Miss>,
    pub misses_total: usize,
}

struct Unit<'a> {
    split: &'a str,
    categories: &'a [String],
    kind: &'a str,
    role: &'a str,
    rank: Option<usize>,
}

/// Runs the index over every scorable unit of `corpus`.
pub fn evaluate(corpus: &Corpus, index: &TitleIndex, ks: &[usize]) -> CandidateReport {
    let k_max = ks.iter().copied().max().unwrap_or(10);
    // Look deeper than the largest cut-off to report near misses.
    let depth = k_max.max(50);
    let record_ids: Vec<Ids> = index.records().iter().map(record_ids).collect();
    let mut units = Vec::new();
    let mut misses = Vec::new();
    for case in &corpus.cases {
        for label in &case.labels.files {
            let Some(work) = label
                .work
                .as_ref()
                .and_then(|r| case.labels.works.iter().find(|w| &w.reference == r))
            else {
                continue;
            };
            if work.ids.is_empty() {
                continue;
            }
            let query = TitleQuery::from_parse(&parse(&label.path));
            let hits = index.search(&query, depth);
            let rank = hits
                .iter()
                .position(|c| ids_match(&work.ids, &record_ids[c.record]));
            if rank.is_none_or(|r| r >= k_max) {
                misses.push(Miss {
                    case_id: case.id.clone(),
                    path: label.path.clone(),
                    expected: work.title.clone(),
                    query: query.titles.iter().map(|(t, _)| t.clone()).collect(),
                    top: hits
                        .first()
                        .and_then(|c| index.record(c.record).titles.first().cloned()),
                    rank,
                });
            }
            units.push(Unit {
                split: &case.split,
                categories: &case.categories,
                kind: &work.kind,
                role: &label.role,
                rank,
            });
        }
    }

    let group = |key: &dyn Fn(&Unit) -> Vec<String>| {
        let mut m: BTreeMap<String, Vec<Option<usize>>> = BTreeMap::new();
        for u in &units {
            for k in key(u) {
                m.entry(k).or_default().push(u.rank);
            }
        }
        m.into_iter()
            .map(|(k, v)| (k, Recall::from_ranks(&v, ks)))
            .collect::<BTreeMap<_, _>>()
    };
    let all: Vec<Option<usize>> = units.iter().map(|u| u.rank).collect();
    let misses_total = misses.len();
    misses.truncate(MAX_MISSES);
    CandidateReport {
        parser_version: pa_recognition::parse::PARSER_VERSION.to_owned(),
        index_records: index.len(),
        index_titles: index.title_count(),
        sources: corpus
            .sources
            .iter()
            .map(|s| s.rsplit(['/', '\\']).next().unwrap_or(s).to_owned())
            .collect(),
        ks: ks.to_vec(),
        depth,
        overall: Recall::from_ranks(&all, ks),
        per_split: group(&|u| vec![u.split.to_owned()]),
        per_category: group(&|u| u.categories.to_vec()),
        per_kind: group(&|u| vec![u.kind.to_owned()]),
        per_role: group(&|u| vec![u.role.to_owned()]),
        misses,
        misses_total,
    }
}

impl CandidateReport {
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("report serializes");
        s.push('\n');
        s
    }

    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        let head = |md: &mut String, first: &str, ks: &[usize]| {
            md.push_str(&format!("| {first} | Элементов |"));
            for k in ks {
                md.push_str(&format!(" R@{k} |"));
            }
            md.push_str(" MRR |\n|---|---:|");
            for _ in ks {
                md.push_str("---:|");
            }
            md.push_str("---:|\n");
        };
        let row = |md: &mut String, name: &str, r: &Recall| {
            md.push_str(&format!("| {name} | {} |", r.items));
            for v in &r.recall {
                md.push_str(&format!(" {:.2} % |", v * 100.0));
            }
            md.push_str(&format!(" {:.4} |\n", r.mrr));
        };
        md.push_str("# Кандидаты из локального индекса названий (META-007)\n\n");
        md.push_str(&format!(
            "Корпус: {}. Парсер {}. Индекс: {} записей, {} названий. Без внешних запросов.\n\n",
            self.sources.join(", "),
            self.parser_version,
            self.index_records,
            self.index_titles
        ));
        md.push_str("## Итог\n\n");
        head(&mut md, "Срез", &self.ks);
        row(&mut md, "**все**", &self.overall);
        for (k, r) in &self.per_split {
            row(&mut md, &format!("split `{k}`"), r);
        }
        for (k, r) in &self.per_kind {
            row(&mut md, &format!("kind `{k}`"), r);
        }
        for (k, r) in &self.per_role {
            row(&mut md, &format!("role `{k}`"), r);
        }
        md.push_str("\n## По категориям\n\n");
        head(&mut md, "Категория", &self.ks);
        let mut cats: Vec<&String> = self.per_category.keys().collect();
        cats.sort_by_key(|c| {
            (
                c.strip_prefix('A')
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(u32::MAX),
                (*c).clone(),
            )
        });
        for c in cats {
            row(&mut md, c, &self.per_category[c]);
        }
        let k_max = self.ks.iter().max().copied().unwrap_or(10);
        md.push_str(&format!(
            "\n## Промахи (нет в топ-{k_max}): {}\n\n",
            self.misses_total
        ));
        if !self.misses.is_empty() {
            md.push_str("| Кейс | Ожидается | Запрос | Топ-1 | Ранг |\n|---|---|---|---|---:|\n");
            for m in self.misses.iter().take(40) {
                md.push_str(&format!(
                    "| `{}` | {} | {} | {} | {} |\n",
                    m.case_id,
                    m.expected.as_deref().unwrap_or("—"),
                    m.query.first().map_or("—", String::as_str).replace('|', "\\|"),
                    m.top.as_deref().unwrap_or("—").replace('|', "\\|"),
                    m.rank.map_or("—".to_owned(), |r| (r + 1).to_string()),
                ));
            }
        }
        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_from_ranks() {
        let r = Recall::from_ranks(&[Some(0), Some(2), None, Some(9)], &[1, 3, 10]);
        assert_eq!(r.found, [1, 2, 3]);
        assert_eq!(r.recall, [0.25, 0.5, 0.75]);
        assert!((r.mrr - (1.0 + 1.0 / 3.0 + 0.1) / 4.0).abs() < 1e-9);
    }

    #[test]
    fn catalog_and_ids() {
        let recs = load_catalog(
            "# comment\n{\"ids\":{\"tmdb\":\"movie:603\",\"kinopoisk\":\"301\"},\"kind\":\"movie\",\"titles\":[\"The Matrix\"]}\n\n",
        )
        .unwrap();
        let ids = record_ids(&recs[0]);
        assert_eq!(ids.tmdb.as_deref(), Some("movie:603"));
        assert_eq!(ids.kinopoisk, Some(301));
        assert!(load_catalog("{oops").is_err());
    }
}
