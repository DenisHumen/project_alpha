// SPDX-License-Identifier: MPL-2.0
//! Filename and path parser with N-best hypotheses (stage \[3\] of the
//! recognition pipeline, `docs/research/03-metadata-recognition.md` §4.2).
//!
//! # Pipeline
//!
//! 1. **Path** — the path is split into folders and a name; the extension is
//!    removed; disc structures (`BDMV/`, `VIDEO_TS/`, `.iso`) take their name
//!    from the parent folder.
//! 2. **Tokenizer** — brackets `[] () {} 【】`, separators `. _ space`, dash
//!    and slash delimiters (see `token`).
//! 3. **Classifiers** — dictionaries and hand-written linear scanners tag
//!    tokens: `S01E02`, `1x02`, `第2話`, `Season 2`, `NCOP1`, resolution,
//!    source, codecs, languages, CRC32, `v2`, `cd1`, dates…
//! 4. **Hypotheses** — ambiguous numbers (`- 05`: episode / absolute /
//!    title; `1917`: title / year) produce weighted options; every
//!    combination becomes a [`Hypothesis`] with the product weight.
//! 5. **Folder context** — season folders, the show folder (title fallback,
//!    alternative title in another script), extras folders, library roots.
//!
//! Anime and Russian release rules (META-006): season, part and kind
//! qualifiers before ` - NN` stay in the title as AniDB names the entry
//! (`Title 2nd Season`, the bare `Title` becomes an alternative title);
//! `86 [01]` and `86 - NCOP` keep the number as the title; transliterated
//! `14.seriya` and `серии 1–8 из 10`; dubbing studios and trackers; trailing
//! scene groups after `H.264-` and before `CD1`. Whether a bare `- 35` counts
//! within a season or across the franchise cannot be told from the name: it
//! goes to `episodes` (or `absolute` for 3+ digits) and the other reading
//! stays in the N-best list for the matcher.
//!
//! No regular expressions are used, every stage is linear in the input, so
//! the parser is immune to ReDoS. The rules are our own; ideas follow the
//! public conventions also used by anitomy (MPL-2.0) and hunch (MIT), no code
//! from GPL/LGPL parsers is used.
//!
//! # Example
//!
//! ```
//! use pa_recognition::parse::{parse, EpisodeNumber};
//!
//! let r = parse("TV/Breaking Bad/Season 1/1x06-07.mkv");
//! let best = &r.best().parsed;
//! assert_eq!(best.title.as_deref(), Some("Breaking Bad")); // from the folder
//! assert_eq!(best.season, Some(1));
//! assert_eq!(best.episodes, [EpisodeNumber::whole(6), EpisodeNumber::whole(7)]);
//!
//! // Ambiguity is kept: `- 26` is most likely an episode, maybe absolute.
//! let r = parse("Attack on Titan - 26.mkv");
//! assert!(r.hypotheses.iter().any(|h| h.parsed.absolute == [EpisodeNumber::whole(26)]));
//! ```

mod classify;
mod interpret;
mod lexicon;
mod token;
mod types;

pub use types::{
    Date, DiscKind, EpisodeKind, EpisodeNumber, ExtraKind, Hypothesis, MediaHint, ParseOptions,
    ParseResult, ParsedName,
};

use interpret::{Analysis, Context};
use lexicon::RootKind;

/// Version of the parser rules; bump when results change.
pub const PARSER_VERSION: &str = "0.2.0";

/// Longest input we look at, in bytes. Longer paths are truncated from the
/// left (folders), keeping the name: protects against pathological input.
const MAX_INPUT: usize = 4096;

/// Parses a relative media path (or a bare file name) with default options.
///
/// The result always has at least one hypothesis.
pub fn parse(path: &str) -> ParseResult {
    parse_with(path, &ParseOptions::default())
}

/// Parses a relative media path with explicit options.
pub fn parse_with(path: &str, opts: &ParseOptions) -> ParseResult {
    let path = truncate_left(path, MAX_INPUT);
    let split = split_path(path);
    let folder = if opts.use_folders {
        folder_context(&split.folders)
    } else {
        FolderContext::default()
    };
    let ctx = Context {
        season_known: folder.season.is_some(),
    };
    let mut hyps = parse_name(&split.name, ctx);
    for h in &mut hyps {
        apply_path(&mut h.parsed, &mut h.reasons, &split, &folder);
    }
    finish(hyps, opts.max_hypotheses.max(1))
}

fn truncate_left(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// Path split into its parts.
#[derive(Debug, Default)]
struct SplitPath {
    /// Folders above the name, outermost first.
    folders: Vec<String>,
    /// The name that is parsed (file name without extension, or the disc folder).
    name: String,
    container: Option<String>,
    disc: Option<DiscKind>,
}

fn split_path(path: &str) -> SplitPath {
    let comps: Vec<&str> = path
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .collect();
    let Some((&last, dirs)) = comps.split_last() else {
        return SplitPath::default();
    };
    let (stem, ext) = match last.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && lexicon::is_known_extension(&ext.to_ascii_lowercase()) =>
        {
            (stem, Some(ext.to_ascii_lowercase()))
        }
        _ => (last, None),
    };
    let mut out = SplitPath {
        container: ext.clone(),
        ..Default::default()
    };
    if matches!(ext.as_deref(), Some("iso" | "img")) {
        out.disc = Some(DiscKind::Iso);
    }
    // Disc structure: the name is the folder that contains BDMV/VIDEO_TS.
    if let Some(pos) = comps.iter().position(|c| {
        let l = c.to_ascii_lowercase();
        l == "bdmv" || l == "video_ts"
    }) {
        out.disc = Some(if comps[pos].eq_ignore_ascii_case("bdmv") {
            DiscKind::Bdmv
        } else {
            DiscKind::VideoTs
        });
        if pos > 0 {
            out.name = comps[pos - 1].to_string();
            out.folders = comps[..pos - 1].iter().map(|s| s.to_string()).collect();
            return out;
        }
    }
    out.name = stem.to_string();
    out.folders = dirs.iter().map(|s| s.to_string()).collect();
    out
}

/// What the folders say.
#[derive(Debug, Default, Clone)]
struct FolderContext {
    title: Option<String>,
    year: Option<u16>,
    season: Option<u32>,
    extra: Option<ExtraKind>,
    part: Option<u32>,
    root: Option<RootKind>,
}

/// `Season 2`, `S02`, `Сезон 2`, `2 сезон`, `Specials` → season number.
fn season_folder(name: &str) -> Option<u32> {
    let lower = name.to_lowercase();
    if matches!(
        lower.as_str(),
        "specials"
            | "special"
            | "sp"
            | "sps"
            | "спецвыпуски"
            | "спешлы"
            | "season 0"
            | "s0"
            | "s00"
    ) {
        return Some(0);
    }
    let words: Vec<&str> = lower
        .split(|c: char| c.is_whitespace() || c == '.' || c == '_' || c == '-')
        .filter(|w| !w.is_empty())
        .collect();
    let num = |w: &str| w.parse::<u32>().ok().filter(|n| *n < 100 && w.len() <= 3);
    match words.as_slice() {
        [w, n]
            if matches!(
                *w,
                "season" | "saison" | "сезон" | "temporada" | "staffel" | "series"
            ) =>
        {
            num(n)
        }
        [n, w] if matches!(*w, "сезон" | "season") => num(n),
        [w] => w
            .strip_prefix('s')
            .and_then(num)
            .or_else(|| w.strip_prefix("season").and_then(num)),
        _ => None,
    }
}

fn part_folder(name: &str) -> Option<u32> {
    let lower: String = name
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    for p in ["cd", "disc", "disk", "part"] {
        if let Some(rest) = lower.strip_prefix(p)
            && let Ok(n) = rest.parse::<u32>()
            && rest.len() <= 2
            && (1..=20).contains(&n)
        {
            return Some(n);
        }
    }
    None
}

fn folder_context(folders: &[String]) -> FolderContext {
    let mut ctx = FolderContext::default();
    let mut found_work = false;
    for (depth, f) in folders.iter().rev().enumerate() {
        let lower = f.to_lowercase();
        if lexicon::is_disc_internal_folder(&lower) {
            continue;
        }
        if let Some(root) = lexicon::library_root(&lower) {
            ctx.root = Some(root);
            break;
        }
        if found_work {
            continue;
        }
        if let Some(s) = season_folder(f) {
            ctx.season = ctx.season.or(Some(s));
            continue;
        }
        if let Some(e) = lexicon::extras_folder(&lower) {
            if depth <= 1 {
                ctx.extra = ctx.extra.or(Some(e));
            }
            continue;
        }
        if let Some(p) = part_folder(f) {
            ctx.part = ctx.part.or(Some(p));
            continue;
        }
        // The nearest other folder names the work.
        let hyps = parse_name(f, Context::default());
        if let Some(best) = hyps.first() {
            let p = &best.parsed;
            ctx.title = p.title.clone();
            ctx.year = p.year;
            ctx.season = ctx.season.or(p.season);
        }
        found_work = true;
    }
    ctx
}

/// Parses a single name (no folders) into weighted hypotheses (unsorted).
fn parse_name(name: &str, ctx: Context) -> Vec<Hypothesis> {
    let (tokens, mut brackets) = token::tokenize(name);
    let classified = classify::classify(tokens, &mut brackets);
    let analysis: Analysis = interpret::analyze(name, classified, brackets, ctx);
    interpret::choices(&analysis.points)
        .into_iter()
        .map(|(choice, weight)| {
            let (parsed, reasons) = interpret::build(&analysis, &choice);
            Hypothesis {
                parsed,
                weight,
                reasons,
            }
        })
        .collect()
}

/// Normalized form for title comparison: lower-case letters and digits only.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn apply_path(p: &mut ParsedName, reasons: &mut Vec<String>, split: &SplitPath, f: &FolderContext) {
    p.container = split.container.clone();
    p.disc = p.disc.or(split.disc);
    let same_work = match (&p.title, &f.title) {
        (Some(a), Some(b)) => {
            let (a, b) = (norm(a), norm(b));
            !a.is_empty() && !b.is_empty() && (a.contains(&b) || b.contains(&a))
        }
        _ => false,
    };
    match (&p.title, &f.title) {
        (None, Some(ft)) => {
            p.title = Some(ft.clone());
            reasons.push("title:folder".into());
        }
        (Some(_), Some(ft)) if !same_work && !p.alt_titles.iter().any(|a| norm(a) == norm(ft)) => {
            p.alt_titles.push(ft.clone());
        }
        _ => {}
    }
    if p.year.is_none()
        && f.year.is_some()
        && (same_work || reasons.iter().any(|r| r == "title:folder"))
    {
        p.year = f.year;
    }
    let episodic = !p.episodes.is_empty() || p.episode_kind.is_some();
    if p.season.is_none() && episodic && p.absolute.is_empty() {
        p.season = f.season;
    }
    if p.extra.is_none() {
        p.extra = f.extra;
    }
    if p.part.is_none() {
        p.part = f.part;
    }
    if matches!(f.root, Some(RootKind::Anime | RootKind::AnimeMovies)) {
        p.anime_hint = true;
    }
    p.media = interpret::media_hint(p);
    if p.media == MediaHint::Unknown {
        p.media = match f.root {
            Some(RootKind::Movies | RootKind::AnimeMovies) => MediaHint::Movie,
            Some(RootKind::Shows) if f.season.is_some() => MediaHint::Episode,
            _ if p.title.is_some() && f.season.is_none() => MediaHint::Movie,
            _ => MediaHint::Unknown,
        };
    }
}

/// Merges duplicates, normalizes weights, sorts, truncates.
fn finish(hyps: Vec<Hypothesis>, max: usize) -> ParseResult {
    let total: f32 = hyps.iter().map(|h| h.weight).sum();
    let mut merged: Vec<Hypothesis> = Vec::new();
    for mut h in hyps {
        h.weight = if total > 0.0 { h.weight / total } else { 1.0 };
        match merged.iter_mut().find(|m| m.parsed == h.parsed) {
            Some(m) => {
                if h.weight > m.weight {
                    m.reasons = std::mem::take(&mut h.reasons);
                }
                m.weight += h.weight;
            }
            None => merged.push(h),
        }
    }
    // Stable sort keeps the deterministic generation order for equal weights.
    merged.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    merged.truncate(max);
    if merged.is_empty() {
        merged.push(Hypothesis {
            parsed: ParsedName::default(),
            weight: 1.0,
            reasons: Vec::new(),
        });
    }
    ParseResult {
        parser_version: PARSER_VERSION.to_string(),
        hypotheses: merged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_folders() {
        assert_eq!(season_folder("Season 2"), Some(2));
        assert_eq!(season_folder("Season.02"), Some(2));
        assert_eq!(season_folder("S03"), Some(3));
        assert_eq!(season_folder("Сезон 4"), Some(4));
        assert_eq!(season_folder("5 сезон"), Some(5));
        assert_eq!(season_folder("Specials"), Some(0));
        assert_eq!(season_folder("Shogun"), None);
        assert_eq!(season_folder("Seasons"), None);
    }

    #[test]
    fn disc_split() {
        let s = split_path("Movies/Inception (2010)/BDMV/STREAM/00001.m2ts");
        assert_eq!(s.name, "Inception (2010)");
        assert_eq!(s.disc, Some(DiscKind::Bdmv));
        assert_eq!(s.folders, ["Movies"]);
    }

    #[test]
    fn empty_input_has_a_hypothesis() {
        assert_eq!(parse("").hypotheses.len(), 1);
        assert_eq!(parse("///").hypotheses.len(), 1);
    }
}
