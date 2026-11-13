// SPDX-License-Identifier: MPL-2.0
//! Typed view of corpus records (`corpus-record.v1`).
//!
//! Only the fields the runner needs are modelled; the structure is checked by
//! `pa-recognition-eval` before records reach this module. The split between
//! [`CaseInput`] (what a recognizer may see) and [`Labels`] (ground truth) is
//! deliberate: a recognizer never gets labels or categories.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One corpus line.
#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    pub id: String,
    pub categories: Vec<String>,
    #[serde(default)]
    pub split: String,
    pub input: CaseInput,
    pub labels: Labels,
}

/// Recognizer input: the directory tree with technical signals, no labels.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseInput {
    pub files: Vec<InputFile>,
}

/// One input file (`fileEntry` in the schema).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputFile {
    pub path: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub audio_langs: Vec<String>,
    #[serde(default)]
    pub sub_langs: Vec<String>,
    #[serde(default)]
    pub oshash: Option<String>,
    #[serde(default)]
    pub ed2k: Option<String>,
    #[serde(default)]
    pub crc32: Option<String>,
    #[serde(default)]
    pub embedded_tags: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Labels {
    #[serde(default)]
    pub works: Vec<WorkLabel>,
    pub files: Vec<FileLabel>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkLabel {
    #[serde(rename = "ref")]
    pub reference: String,
    pub kind: String,
    /// Canonical title (for reports only; recognizers never see labels).
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub ids: Ids,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileLabel {
    pub path: String,
    pub role: String,
    #[serde(default)]
    pub work: Option<String>,
    #[serde(default)]
    pub episodes: Vec<EpisodeLabel>,
    #[serde(default)]
    pub parse: Option<ParsedName>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EpisodeLabel {
    #[serde(default)]
    pub numbering: Option<Numbering>,
}

/// External ids; tmdb/tvdb carry the entity type (`tv:123`, `series:42`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ids {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvdb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imdb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anidb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anilist: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mal: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kitsu: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shikimori: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinopoisk: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvmaze: Option<u64>,
}

impl Ids {
    /// `(namespace, value)` pairs that are set, in a fixed order.
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let strs = [
            ("tmdb", &self.tmdb),
            ("tvdb", &self.tvdb),
            ("imdb", &self.imdb),
        ];
        for (ns, v) in strs {
            if let Some(v) = v {
                out.push((ns, v.clone()));
            }
        }
        let nums = [
            ("anidb", self.anidb),
            ("anilist", self.anilist),
            ("mal", self.mal),
            ("kitsu", self.kitsu),
            ("shikimori", self.shikimori),
            ("kinopoisk", self.kinopoisk),
            ("tvmaze", self.tvmaze),
        ];
        for (ns, v) in nums {
            if let Some(v) = v {
                out.push((ns, v.to_string()));
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.pairs().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SeasonEpisode {
    pub season: u32,
    pub episode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AnidbEpisode {
    #[serde(rename = "type")]
    pub kind: String,
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupEpisode {
    pub group_id: String,
    pub season: u32,
    pub episode: u32,
}

/// Episode position in each numbering scheme.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Numbering {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb: Option<SeasonEpisode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvdb_aired: Option<SeasonEpisode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvdb_dvd: Option<SeasonEpisode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvdb_absolute: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anidb: Option<AnidbEpisode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb_group: Option<GroupEpisode>,
}

/// Numbering schemes, in report order.
pub const SCHEMES: [&str; 6] = [
    "tmdb",
    "tvdb_aired",
    "tvdb_dvd",
    "tvdb_absolute",
    "anidb",
    "tmdb_group",
];

impl Numbering {
    /// Canonical string of the position in `scheme`, if set.
    pub fn key(&self, scheme: &str) -> Option<String> {
        let se = |v: &Option<SeasonEpisode>| v.map(|v| format!("S{}E{}", v.season, v.episode));
        match scheme {
            "tmdb" => se(&self.tmdb),
            "tvdb_aired" => se(&self.tvdb_aired),
            "tvdb_dvd" => se(&self.tvdb_dvd),
            "tvdb_absolute" => self.tvdb_absolute.map(|n| n.to_string()),
            "anidb" => self
                .anidb
                .as_ref()
                .map(|a| format!("{}:{}", a.kind, a.number)),
            "tmdb_group" => self
                .tmdb_group
                .as_ref()
                .map(|g| format!("{}:S{}E{}", g.group_id, g.season, g.episode)),
            _ => None,
        }
    }
}

/// Filename-level parse (`parseTarget`), used both as label and prediction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParsedName {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episodes: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crc32: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

/// A loaded corpus: cases in file order plus the names of the source files.
#[derive(Debug, Default)]
pub struct Corpus {
    pub sources: Vec<String>,
    pub cases: Vec<Case>,
}

impl Corpus {
    /// Validates and parses JSONL documents `(name, text)`.
    ///
    /// Validation errors (schema or cross-field rules from
    /// `pa-recognition-eval`) abort the load; warnings are tolerated, so
    /// `draft` records can be evaluated.
    pub fn load(docs: &[(String, String)], validate: bool) -> Result<Self, String> {
        if validate {
            let validator = pa_recognition_eval::CorpusValidator::new()?;
            let mut session = validator.session();
            for (name, text) in docs {
                session.add_jsonl(name, text);
            }
            let report = session.finish();
            let errors: Vec<String> = report
                .issues
                .iter()
                .filter(|i| i.severity == pa_recognition_eval::Severity::Error)
                .map(ToString::to_string)
                .collect();
            if !errors.is_empty() {
                return Err(format!("corpus is invalid:\n{}", errors.join("\n")));
            }
        }
        let mut corpus = Corpus::default();
        for (name, text) in docs {
            corpus.sources.push(name.clone());
            for (idx, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let case: Case =
                    serde_json::from_str(line).map_err(|e| format!("{name}:{}: {e}", idx + 1))?;
                corpus.cases.push(case);
            }
        }
        Ok(corpus)
    }
}
