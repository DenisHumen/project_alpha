// SPDX-License-Identifier: MPL-2.0
//! The interface the runner evaluates, plus trivial baselines.
//!
//! The real pipeline (`pa-recognition`: parser META-005, candidates, scorer)
//! plugs in through an adapter implementing [`Recognizer`]. The baselines
//! here only exist so the runner and its metrics are testable now and to give
//! a floor to compare against.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::corpus::{CaseInput, Ids, Numbering, ParsedName};
use crate::provider::{Provider, ProviderRequest};

/// Recognizer output for one unit (a file, or a disc directory).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FilePrediction {
    /// Input file path, or a disc directory (`unit: bdmv|video_ts`).
    pub path: String,
    /// Matched work; `None` means the recognizer abstains (manual queue).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<WorkGuess>,
    /// One entry per episode contained in the file, in any scheme(s).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<Numbering>,
    /// Calibrated probability that `work` is correct, in `[0, 1]`.
    #[serde(default)]
    pub confidence: f64,
    /// Filename-level parse, for parser metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse: Option<ParsedName>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkGuess {
    /// `movie`, `show` or `anime`, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub ids: Ids,
}

/// A recognition pipeline under evaluation.
///
/// Implementations must be deterministic for a given input and provider
/// responses: no clocks, no randomness without a fixed seed, no network
/// except through `provider`.
pub trait Recognizer {
    /// Stable name written to the report.
    fn name(&self) -> &str;

    /// Recognizes one case. Units without a prediction count as abstentions.
    fn recognize(&mut self, case: &CaseInput, provider: &mut dyn Provider) -> Vec<FilePrediction>;
}

/// Abstains on everything: the "all manual" floor.
#[derive(Debug, Default)]
pub struct Abstain;

impl Recognizer for Abstain {
    fn name(&self) -> &str {
        "abstain"
    }

    fn recognize(
        &mut self,
        _case: &CaseInput,
        _provider: &mut dyn Provider,
    ) -> Vec<FilePrediction> {
        Vec::new()
    }
}

/// Naive baseline: crude filename parse, one `tmdb/search` call, top-1.
///
/// Expected response shape (provider-neutral, as normalized by the provider
/// adapter): `{"results": [{"kind": "show", "title": "...", "year": 2019,
/// "ids": {"tmdb": "tv:1"}}]}`.
#[derive(Debug, Default)]
pub struct Naive;

impl Recognizer for Naive {
    fn name(&self) -> &str {
        "naive"
    }

    fn recognize(&mut self, case: &CaseInput, provider: &mut dyn Provider) -> Vec<FilePrediction> {
        case.files
            .iter()
            .map(|file| {
                let parse = parse_name(&file.path);
                let mut prediction = FilePrediction {
                    path: file.path.clone(),
                    ..Default::default()
                };
                if let Some(title) = &parse.title {
                    let mut request = ProviderRequest::new("tmdb", "search").param("query", title);
                    if let Some(year) = parse.year {
                        request = request.param("year", year.to_string());
                    }
                    if let Ok(response) = provider.call(&request)
                        && let Some((work, confidence)) = top_result(&response, title, parse.year)
                    {
                        prediction.work = Some(work);
                        prediction.confidence = confidence;
                        prediction.episodes = numbering_from_parse(&parse);
                    }
                }
                prediction.parse = Some(parse);
                prediction
            })
            .collect()
    }
}

/// Product filename parser (`pa-recognition`, META-005/META-006) only.
///
/// Fills `parse` from the best hypothesis and abstains on the work: there is
/// no candidate search or title index yet (META-007, META-010), so this
/// recognizer measures parser accuracy, not identification.
#[derive(Debug, Default)]
pub struct Parser;

impl Recognizer for Parser {
    fn name(&self) -> &str {
        "parser"
    }

    fn recognize(&mut self, case: &CaseInput, _provider: &mut dyn Provider) -> Vec<FilePrediction> {
        case.files
            .iter()
            .map(|file| FilePrediction {
                path: file.path.clone(),
                parse: Some(from_product_parse(
                    &pa_recognition::parse::parse(&file.path).best().parsed,
                )),
                ..Default::default()
            })
            .collect()
    }
}

/// Maps the product parser output onto the corpus `parse` fields.
pub fn from_product_parse(p: &pa_recognition::parse::ParsedName) -> ParsedName {
    let numbers = |v: &[pa_recognition::parse::EpisodeNumber]| {
        (!v.is_empty()).then(|| v.iter().map(|n| n.as_f64()).collect())
    };
    ParsedName {
        title: p.title.clone(),
        year: p.year.map(u32::from),
        season: p.season,
        episodes: numbers(&p.episodes),
        absolute: numbers(&p.absolute),
        part: p.part,
        release_group: p.release_group.clone(),
        crc32: p.crc32.clone(),
        version: p.version,
    }
}

fn top_result(response: &Value, title: &str, year: Option<u32>) -> Option<(WorkGuess, f64)> {
    let top = response.get("results")?.as_array()?.first()?;
    let ids: Ids = serde_json::from_value(top.get("ids")?.clone()).ok()?;
    let kind = top.get("kind").and_then(Value::as_str).map(str::to_owned);
    let same_title = top
        .get("title")
        .and_then(Value::as_str)
        .is_some_and(|t| normalize(t) == normalize(title));
    let same_year = match (year, top.get("year").and_then(Value::as_u64)) {
        (Some(a), Some(b)) => u64::from(a) == b,
        _ => false,
    };
    let confidence = match (same_title, same_year) {
        (true, true) => 0.95,
        (true, false) => 0.85,
        (false, _) => 0.5,
    };
    Some((WorkGuess { kind, ids }, confidence))
}

fn numbering_from_parse(parse: &ParsedName) -> Vec<Numbering> {
    let whole = |v: &f64| (v.fract() == 0.0 && *v >= 0.0).then_some(*v as u32);
    if let (Some(season), Some(episodes)) = (parse.season, &parse.episodes) {
        return episodes
            .iter()
            .filter_map(whole)
            .map(|episode| Numbering {
                tmdb: Some(crate::corpus::SeasonEpisode { season, episode }),
                ..Default::default()
            })
            .collect();
    }
    if let Some(absolute) = &parse.absolute {
        return absolute
            .iter()
            .filter_map(whole)
            .map(|n| Numbering {
                tvdb_absolute: Some(n),
                ..Default::default()
            })
            .collect();
    }
    Vec::new()
}

/// Lowercases and keeps only alphanumerics: a deliberately crude title key.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Crude filename parser used by [`Naive`]. Not the product parser (META-005).
pub fn parse_name(path: &str) -> ParsedName {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = match name.rsplit_once('.') {
        Some((stem, ext)) if (1..=4).contains(&ext.len()) => stem,
        _ => name,
    };
    let mut parse = ParsedName::default();

    // Strip bracketed groups, harvesting group, CRC32 and year.
    let mut text = String::new();
    let mut chars = stem.chars().peekable();
    let mut at_start = true;
    while let Some(c) = chars.next() {
        let close = match c {
            '[' => ']',
            '(' => ')',
            _ => {
                text.push(c);
                if !c.is_whitespace() {
                    at_start = false;
                }
                continue;
            }
        };
        let inner: String = chars.by_ref().take_while(|&x| x != close).collect();
        let inner = inner.trim();
        if c == '[' && at_start && parse.release_group.is_none() && !inner.is_empty() {
            parse.release_group = Some(inner.to_owned());
        } else if inner.len() == 8 && inner.chars().all(|x| x.is_ascii_hexdigit()) {
            parse.crc32 = Some(inner.to_ascii_uppercase());
        } else if let Some(year) = as_year(inner) {
            parse.year.get_or_insert(year);
        }
        text.push(' ');
    }

    let text = text.replace(['.', '_'], " ");
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut title = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        if let Some((season, episodes)) = season_episode(tok) {
            parse.season = Some(season);
            parse.episodes = Some(episodes.into_iter().map(f64::from).collect());
            break;
        }
        if i > 0
            && let Some(year) = as_year(tok)
        {
            parse.year.get_or_insert(year);
            break;
        }
        if tok == "-"
            && let Some(n) = tokens.get(i + 1).and_then(|t| t.parse::<f64>().ok())
        {
            parse.absolute = Some(vec![n]);
            break;
        }
        if is_resolution(tok) {
            break;
        }
        title.push(tok);
        i += 1;
    }
    let title = title
        .join(" ")
        .trim_matches(|c: char| c == '-' || c.is_whitespace())
        .to_owned();
    if !title.is_empty() {
        parse.title = Some(title);
    }
    parse
}

fn as_year(s: &str) -> Option<u32> {
    let y: u32 = s.parse().ok().filter(|_| s.len() == 4)?;
    (1870..=2100).contains(&y).then_some(y)
}

fn is_resolution(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower
        .strip_suffix('p')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// `S01E02`, `s1e2e3` → `(1, [2, 3])`.
fn season_episode(tok: &str) -> Option<(u32, Vec<u32>)> {
    let lower = tok.to_ascii_lowercase();
    let rest = lower.strip_prefix('s')?;
    let mut parts = rest.split('e');
    let season = parts.next()?.parse().ok()?;
    let episodes: Vec<u32> = parts.map(|p| p.parse().ok()).collect::<Option<_>>()?;
    (!episodes.is_empty()).then_some((season, episodes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_anime_release() {
        let p = parse_name("Anime/X/[SubsPlease] One Piece - 1085 (1080p) [ABCD1234].mkv");
        assert_eq!(p.title.as_deref(), Some("One Piece"));
        assert_eq!(p.release_group.as_deref(), Some("SubsPlease"));
        assert_eq!(p.crc32.as_deref(), Some("ABCD1234"));
        assert_eq!(p.absolute, Some(vec![1085.0]));
    }

    #[test]
    fn parses_scene_release() {
        let p = parse_name("TV/Show.Name.S02E05E06.1080p.WEB.mkv");
        assert_eq!(p.title.as_deref(), Some("Show Name"));
        assert_eq!(p.season, Some(2));
        assert_eq!(p.episodes, Some(vec![5.0, 6.0]));
    }

    #[test]
    fn parses_movie_year() {
        let p = parse_name("Movies/Blade Runner (1982)/Blade.Runner.1982.mkv");
        assert_eq!(p.title.as_deref(), Some("Blade Runner"));
        assert_eq!(p.year, Some(1982));
    }
}
