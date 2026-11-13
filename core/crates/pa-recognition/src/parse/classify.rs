// SPDX-License-Identifier: MPL-2.0
//! Token classifiers: give every token a [`Tag`].
//!
//! Classification is deterministic and unambiguous here; the genuinely
//! ambiguous tokens (bare numbers, year-like numbers) are tagged
//! [`Tag::Num`] and resolved into N-best hypotheses by `interpret`.
//! All matchers are hand-written scanners over the token text — no
//! backtracking regular expressions, so the cost is linear in the input.

use std::ops::Range;

use super::lexicon::{self, Entry, Lex, PHRASES};
use super::token::{Bracket, Token};
use super::types::{Date, EpisodeKind, EpisodeNumber, ExtraKind};

/// Plausible release years.
pub(crate) const YEAR_MIN: u32 = 1888;
pub(crate) const YEAR_MAX: u32 = 2039;
/// Longest episode range we expand (`01-12` → 12 numbers).
const MAX_RANGE: u32 = 300;

/// A bare number token: `05`, `1085`, `18.5`, `01v2`, `01-02`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NumTok {
    /// Values (a range is expanded).
    pub values: Vec<EpisodeNumber>,
    /// Digits in the first number as written (`008` → 3).
    pub digits: usize,
    /// `01v2` → 2.
    pub version: Option<u32>,
    /// The token is a plain 4-digit number in the year range.
    pub year_like: Option<u16>,
    /// The token is a range (`01-02`).
    pub range: bool,
}

/// Meaning of a token.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tag {
    /// Plain word: title material.
    Free,
    /// Ambiguous bare number.
    Num(NumTok),
    /// Year in its own bracket: `(2010)`.
    Year(u16),
    /// Season and/or episodes from an explicit marker (`S01E02`, `1x02`, `E05`, `第5話`).
    SeasonEp {
        season: Option<u32>,
        episodes: Vec<EpisodeNumber>,
        version: Option<u32>,
    },
    /// Season marker alone (`S02`, `Season 2`, `2nd Season`).
    Season(u32),
    /// Episode kind with an optional number (`NCOP1`, `OVA`, `SP2`).
    Kind(EpisodeKind, Option<EpisodeNumber>),
    /// Part of a split file (`cd2`, `Part 2`).
    Part(u32),
    /// Air date.
    Date(Date),
    Resolution,
    Source,
    VideoCodec,
    AudioCodec,
    Hdr,
    /// ISO 639-2 code, or `""` for "multi/dual" markers.
    Lang(&'static str),
    Edition,
    Other,
    Extra(ExtraKind),
    Crc(String),
    Version(u32),
    /// Known release group word.
    Group,
    /// Dubbing studio.
    Dub,
    /// Absorbed into the multi-token tag of an earlier token.
    Consumed,
}

impl Tag {
    /// Release information (not title material, not a number).
    pub fn is_release_info(&self) -> bool {
        matches!(
            self,
            Tag::Resolution
                | Tag::Source
                | Tag::VideoCodec
                | Tag::AudioCodec
                | Tag::Hdr
                | Tag::Lang(_)
                | Tag::Edition
                | Tag::Other
                | Tag::Crc(_)
                | Tag::Version(_)
                | Tag::Group
                | Tag::Dub
                | Tag::Extra(_)
        )
    }

    /// Markers that fix season/episode explicitly.
    pub fn is_episode_marker(&self) -> bool {
        matches!(
            self,
            Tag::SeasonEp { .. } | Tag::Date(_) | Tag::Kind(_, Some(_))
        )
    }
}

/// A classified token.
#[derive(Debug, Clone)]
pub(crate) struct Classified {
    pub tok: Token,
    pub tag: Tag,
    /// Display text of the tag (multi-token phrases are joined with spaces).
    pub display: String,
    /// Index range of tokens this tag covers (itself plus consumed ones).
    pub covers: Range<usize>,
}

/// Trim punctuation that is not part of a release token.
pub(crate) fn core(text: &str) -> &str {
    text.trim_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | '"' | '\'' | '«' | '»' | '“' | '”' | '#'
        )
    })
}

fn ascii_digits_value(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 6 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Map full-width digits to ASCII so `第０１話` works.
fn normalize_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '０'..='９' => char::from(b'0' + (c as u32 - '０' as u32) as u8),
            _ => c,
        })
        .collect()
}

/// Parses `18`, `18.5`, `01v2`, `01-02`, `01~12`, `01-02v2`.
pub(crate) fn parse_num(s: &str) -> Option<NumTok> {
    let (body, version) = match s.find(['v', 'V']) {
        Some(p) => {
            let v = ascii_digits_value(&s[p + 1..])?;
            (&s[..p], Some(v))
        }
        None => (s, None),
    };
    if let Some((p, sep)) = body.char_indices().find(|&(_, c)| matches!(c, '-' | '~' | '–')) {
        let (a, b) = (&body[..p], &body[p + sep.len_utf8()..]);
        let (x, y) = (ascii_digits_value(a)?, ascii_digits_value(b)?);
        if y <= x || y - x > MAX_RANGE || a.len() > 4 {
            return None;
        }
        return Some(NumTok {
            values: (x..=y).map(EpisodeNumber::whole).collect(),
            digits: a.len(),
            version,
            year_like: None,
            range: true,
        });
    }
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) if f.len() == 1 => (i, Some(f)),
        Some(_) => return None,
        None => (body, None),
    };
    let n = ascii_digits_value(int)?;
    let fraction = match frac {
        Some(f) => Some(ascii_digits_value(f)? as u8),
        None => None,
    };
    let year_like = (int.len() == 4
        && fraction.is_none()
        && version.is_none()
        && (YEAR_MIN..=YEAR_MAX).contains(&n))
    .then_some(n as u16);
    Some(NumTok {
        values: vec![EpisodeNumber {
            number: n,
            fraction,
        }],
        digits: int.len(),
        version,
        year_like,
        range: false,
    })
}

/// A small cursor over ASCII bytes of a lower-cased token.
struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}

impl Cur<'_> {
    fn eat(&mut self, c: u8) -> bool {
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn eat_str(&mut self, s: &str) -> bool {
        if self.b[self.i..].starts_with(s.as_bytes()) {
            self.i += s.len();
            true
        } else {
            false
        }
    }
    fn num(&mut self, max_digits: usize) -> Option<u32> {
        let start = self.i;
        while self.i < self.b.len()
            && self.b[self.i].is_ascii_digit()
            && self.i - start < max_digits
        {
            self.i += 1;
        }
        if self.i == start || (self.i < self.b.len() && self.b[self.i].is_ascii_digit()) {
            self.i = start;
            return None;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
    fn done(&self) -> bool {
        self.i == self.b.len()
    }
}

fn push_episode(eps: &mut Vec<EpisodeNumber>, n: u32, range: bool) -> Option<()> {
    if range {
        let last = eps.last()?.number;
        if n <= last || n - last > MAX_RANGE {
            return None;
        }
        eps.extend((last + 1..=n).map(EpisodeNumber::whole));
    } else {
        eps.push(EpisodeNumber::whole(n));
    }
    Some(())
}

/// Episode list after the first number: `e02`, `-03`, `-e03`, `x02`, `v2`.
fn episode_tail(
    c: &mut Cur<'_>,
    eps: &mut Vec<EpisodeNumber>,
    x_style: bool,
) -> Option<Option<u32>> {
    let mut version = None;
    loop {
        if c.done() {
            return Some(version);
        }
        if c.eat(b'v') {
            version = Some(c.num(2)?);
            return c.done().then_some(version);
        }
        let range = c.eat(b'-');
        let sep = c.eat(b'e') || (x_style && c.eat(b'x'));
        if !range && !sep {
            return None;
        }
        let n = c.num(4)?;
        push_episode(eps, n, range)?;
    }
}

/// `s01e02`, `s01e01e02`, `s01e01-e03`, `s1e2v2`, `s01` (season only).
fn parse_sxxexx(lower: &str) -> Option<Tag> {
    let mut c = Cur {
        b: lower.as_bytes(),
        i: 0,
    };
    if !c.eat(b's') {
        return None;
    }
    let season = c.num(4)?;
    if c.done() {
        return Some(Tag::Season(season));
    }
    if c.eat(b'-') && c.eat(b's') {
        // Season range `s01-s03`: keep the first season.
        c.num(4)?;
        return c.done().then_some(Tag::Season(season));
    }
    c.eat(b'.');
    if !c.eat(b'e') {
        return None;
    }
    let first = c.num(4)?;
    let mut eps = vec![EpisodeNumber::whole(first)];
    let version = episode_tail(&mut c, &mut eps, false)?;
    Some(Tag::SeasonEp {
        season: Some(season),
        episodes: eps,
        version,
    })
}

/// `1x02`, `1x01-02`, `1x01x02`.
fn parse_nxnn(lower: &str) -> Option<Tag> {
    let mut c = Cur {
        b: lower.as_bytes(),
        i: 0,
    };
    let season = c.num(2)?;
    if !c.eat(b'x') {
        return None;
    }
    let first = c.num(4)?;
    let mut eps = vec![EpisodeNumber::whole(first)];
    let version = episode_tail(&mut c, &mut eps, true)?;
    Some(Tag::SeasonEp {
        season: Some(season),
        episodes: eps,
        version,
    })
}

/// `e05`, `ep05`, `ep.05`, `episode05`, `e01-e03`, `ep05v2`, `#05`.
fn parse_episode_glued(lower: &str) -> Option<Tag> {
    let mut c = Cur {
        b: lower.as_bytes(),
        i: 0,
    };
    if !(c.eat_str("episode") || c.eat_str("ep") || c.eat(b'e') || c.eat(b'#')) {
        return None;
    }
    c.eat(b'.');
    let first = c.num(4)?;
    let mut eps = vec![EpisodeNumber::whole(first)];
    // Fractional `ep18.5`.
    if c.eat(b'.') {
        let f = c.num(1)?;
        eps[0].fraction = Some(f as u8);
        return c.done().then_some(Tag::SeasonEp {
            season: None,
            episodes: eps,
            version: None,
        });
    }
    let version = episode_tail(&mut c, &mut eps, false)?;
    Some(Tag::SeasonEp {
        season: None,
        episodes: eps,
        version,
    })
}

/// `part1`, `pt2`, `cd1`, `disc2`, `disk1`.
fn parse_part_glued(lower: &str) -> Option<u32> {
    for p in ["part", "pt", "cd", "disc", "disk"] {
        if let Some(rest) = lower.strip_prefix(p) {
            let rest = rest.strip_prefix(['.', '-']).unwrap_or(rest);
            if let Some(n) = ascii_digits_value(rest)
                && rest.len() <= 2
                && (1..=20).contains(&n)
            {
                return Some(n);
            }
        }
    }
    None
}

/// `season2`, `season02`.
fn parse_season_glued(lower: &str) -> Option<u32> {
    for p in ["season", "saison", "сезон"] {
        if let Some(rest) = lower.strip_prefix(p)
            && let Some(n) = ascii_digits_value(rest)
            && rest.len() <= 3
        {
            return Some(n);
        }
    }
    None
}

/// CJK markers: `第01話`, `第1话`, `第1集`, `01話`, `第2期`, `第2季`.
fn parse_cjk(token: &str) -> Option<Tag> {
    let t = normalize_digits(token);
    let t = t.strip_prefix('第').unwrap_or(&t);
    let digits_end = t.find(|c: char| !c.is_ascii_digit())?;
    let n = ascii_digits_value(&t[..digits_end])?;
    let rest = &t[digits_end..];
    match rest {
        "話" | "话" | "集" | "回" | "话完" | "話完" => Some(Tag::SeasonEp {
            season: None,
            episodes: vec![EpisodeNumber::whole(n)],
            version: None,
        }),
        "期" | "季" | "部" if token.starts_with('第') => Some(Tag::Season(n)),
        _ => None,
    }
}

/// `2025.03.14`, `2025-03-14`.
fn parse_date(s: &str) -> Option<Date> {
    let b = s.as_bytes();
    if b.len() != 10 || !(b[4] == b'.' || b[4] == b'-') || b[7] != b[4] {
        return None;
    }
    let y = ascii_digits_value(&s[0..4])?;
    let m = ascii_digits_value(&s[5..7])?;
    let d = ascii_digits_value(&s[8..10])?;
    valid_date(y, m, d)
}

fn valid_date(y: u32, m: u32, d: u32) -> Option<Date> {
    ((YEAR_MIN..=YEAR_MAX).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d))
        .then_some(Date {
            year: y as u16,
            month: m as u8,
            day: d as u8,
        })
}

/// `1080p`, `720i`, `4k`, `1920x1080`.
fn is_resolution(lower: &str) -> bool {
    if matches!(lower, "4k" | "8k" | "2k" | "uhd4k") {
        return true;
    }
    if let Some(num) = lower.strip_suffix(['p', 'i'])
        && let Some(n) = ascii_digits_value(num)
    {
        return (3..=4).contains(&num.len()) && (240..=4320).contains(&n);
    }
    if let Some((w, h)) = lower.split_once('x')
        && let (Some(w), Some(_)) = (ascii_digits_value(w), ascii_digits_value(h))
    {
        return (3..=4).contains(&h.len()) && w >= 320;
    }
    false
}

/// `v2`.
fn parse_version(lower: &str) -> Option<u32> {
    let rest = lower.strip_prefix('v')?;
    (rest.len() <= 2)
        .then(|| ascii_digits_value(rest))
        .flatten()
}

/// `ncop1`, `nced2`, `ova2`, `sp1`, `op1`, `ed2`, `special2`.
fn parse_kind_glued(lower: &str) -> Option<Tag> {
    let split = lower.find(|c: char| c.is_ascii_digit())?;
    if split == 0 {
        return None;
    }
    let (word, num) = (&lower[..split], &lower[split..]);
    let (num, _version) = match num.split_once('v') {
        Some((n, v)) => (n, Some(ascii_digits_value(v)?)),
        None => (num, None),
    };
    let n = ascii_digits_value(num).filter(|_| num.len() <= 3)?;
    let ep = Some(EpisodeNumber::whole(n));
    Some(match word {
        "ncop" | "nc-op" => Tag::Kind(EpisodeKind::Ncop, ep),
        "nced" | "nc-ed" => Tag::Kind(EpisodeKind::Nced, ep),
        "ova" | "oad" | "ova-" => Tag::Kind(EpisodeKind::Ova, ep),
        "ona" => Tag::Kind(EpisodeKind::Ona, ep),
        "sp" | "sp-" => Tag::Kind(EpisodeKind::Sp, ep),
        "special" | "specials" => Tag::Kind(EpisodeKind::Special, ep),
        "recap" => Tag::Kind(EpisodeKind::Recap, ep),
        "op" | "opening" => Tag::Extra(ExtraKind::Opening),
        "ed" | "ending" => Tag::Extra(ExtraKind::Ending),
        "pv" | "cm" => Tag::Extra(ExtraKind::Preview),
        _ => return None,
    })
}

fn tag_from_lex(lex: Lex) -> Tag {
    match lex {
        Lex::Source => Tag::Source,
        Lex::VideoCodec => Tag::VideoCodec,
        Lex::AudioCodec => Tag::AudioCodec,
        Lex::Hdr => Tag::Hdr,
        Lex::Lang(l) => Tag::Lang(l),
        Lex::Edition => Tag::Edition,
        Lex::Other => Tag::Other,
        Lex::Extra(e) => Tag::Extra(e),
        Lex::Kind(k) => Tag::Kind(k, None),
        Lex::Group => Tag::Group,
        Lex::Dub => Tag::Dub,
    }
}

/// Classifies one token on its own. Returns the tag and whether it is weak.
fn classify_single(text: &str, in_bracket_alone: bool, in_bracket: bool) -> (Tag, bool) {
    let c = core(text);
    if c.is_empty() {
        return (Tag::Free, false);
    }
    let lower = c.to_lowercase();
    // `#05` keeps its `#` (core() strips it).
    let lower_raw = text.to_lowercase();
    if let Some(d) = parse_date(&lower) {
        return (Tag::Date(d), false);
    }
    if in_bracket_alone
        && c.len() == 8
        && c.bytes().all(|b| b.is_ascii_hexdigit())
        && parse_num(c).is_none_or(|n| n.values[0].number > 99_999_999)
    {
        return (Tag::Crc(c.to_ascii_uppercase()), false);
    }
    if in_bracket_alone
        && let Some(n) = parse_num(c)
        && let Some(y) = n.year_like
    {
        return (Tag::Year(y), false);
    }
    if let Some(e) = lexicon::lookup(&lower) {
        // `WWE Raw`: a lower/mixed-case `Raw` outside brackets is a title word.
        if matches!(lower.as_str(), "raw" | "raws")
            && !in_bracket
            && c.chars().any(|ch| ch.is_lowercase())
        {
            return (Tag::Free, false);
        }
        // A dictionary hit wins over patterns (`x264`, `dd5.1` via the next check).
        return (tag_from_lex(e.lex), e.weak);
    }
    if lexicon::is_audio_with_channels(&lower) {
        return (Tag::AudioCodec, false);
    }
    if is_resolution(&lower) {
        return (Tag::Resolution, false);
    }
    if lower_raw.starts_with('#')
        && let Some(t) = parse_episode_glued(&lower_raw)
    {
        return (t, false);
    }
    if let Some(t) = parse_sxxexx(&lower) {
        return (t, false);
    }
    if let Some(t) = parse_nxnn(&lower) {
        return (t, false);
    }
    if let Some(n) = parse_num(c) {
        return (Tag::Num(n), false);
    }
    if let Some(t) = parse_episode_glued(&lower) {
        return (t, false);
    }
    if let Some(p) = parse_part_glued(&lower) {
        return (Tag::Part(p), false);
    }
    if let Some(s) = parse_season_glued(&lower) {
        return (Tag::Season(s), false);
    }
    if let Some(v) = parse_version(&lower) {
        return (Tag::Version(v), false);
    }
    if let Some(t) = parse_kind_glued(&lower) {
        return (t, false);
    }
    if let Some(t) = parse_cjk(c) {
        return (t, false);
    }
    (Tag::Free, false)
}

fn is_tech(entry: Option<Entry>, lower: &str) -> bool {
    match entry {
        Some(e) => matches!(
            e.lex,
            Lex::Source | Lex::VideoCodec | Lex::AudioCodec | Lex::Hdr | Lex::Lang(_) | Lex::Other
        ),
        None => is_resolution(lower) || lexicon::is_audio_with_channels(lower),
    }
}

fn extra_suffix(lower: &str) -> Option<ExtraKind> {
    Some(match lower {
        "trailer" => ExtraKind::Trailer,
        "teaser" => ExtraKind::Teaser,
        "sample" => ExtraKind::Sample,
        "featurette" => ExtraKind::Featurette,
        "behindthescenes" => ExtraKind::BehindTheScenes,
        "deleted" => ExtraKind::DeletedScene,
        "interview" => ExtraKind::Interview,
        "scene" => ExtraKind::Scene,
        "short" => ExtraKind::Short,
        "other" | "clip" | "extra" => ExtraKind::Other,
        _ => return None,
    })
}

/// Splits hyphenated tokens that glue a release token to a group or an extra
/// suffix: `x264-GROUP` → `x264`, `GROUP`; `Inception-trailer` → `Inception`, `trailer`.
fn split_glued(tokens: Vec<Token>, brackets: &mut [Bracket]) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    for t in tokens {
        let lower = core(&t.text).to_lowercase();
        let whole_known = lexicon::lookup(&lower).is_some()
            || parse_date(&lower).is_some()
            || parse_num(core(&t.text)).is_some()
            || parse_sxxexx(&lower).is_some()
            || parse_nxnn(&lower).is_some()
            || parse_episode_glued(&lower).is_some()
            || parse_kind_glued(&lower).is_some();
        let split = if whole_known {
            None
        } else {
            // `H.264-Erai-raws`: the first hyphen after a release token;
            // `Inception-trailer`: the last hyphen before an extra suffix.
            let left_tech = |p: usize| {
                let l = &t.text[..p];
                let ll = core(l).to_lowercase();
                !l.is_empty()
                    && p + 1 < t.text.len()
                    && (is_tech(lexicon::lookup(&ll), &ll)
                        || parse_sxxexx(&ll).is_some()
                        || parse_nxnn(&ll).is_some())
            };
            let right_extra = |p: usize| {
                p > 0 && extra_suffix(&core(&t.text[p + 1..]).to_lowercase()).is_some()
            };
            t.text
                .match_indices('-')
                .map(|(p, _)| p)
                .find(|&p| left_tech(p))
                .or_else(|| t.text.rfind('-').filter(|&p| right_extra(p)))
        };
        match split {
            Some(p) => {
                let s = t.span.start;
                out.push(Token {
                    text: t.text[..p].to_string(),
                    span: s..s + p,
                    glued_hyphen: false,
                    ..t.clone()
                });
                out.push(Token {
                    text: t.text[p + 1..].to_string(),
                    span: s + p + 1..t.span.end,
                    dash_before: false,
                    slash_before: false,
                    glued_hyphen: true,
                    bracket: t.bracket,
                });
                // Shift bracket token ranges after this point.
                for b in brackets.iter_mut() {
                    if b.tokens.start > out.len() - 2 {
                        b.tokens.start += 1;
                    }
                    if b.tokens.end > out.len() - 2 {
                        b.tokens.end += 1;
                    }
                }
            }
            None => out.push(t),
        }
    }
    out
}

fn ordinal(lower: &str) -> Option<u32> {
    match lower {
        "first" => return Some(1),
        "second" => return Some(2),
        "third" => return Some(3),
        "fourth" => return Some(4),
        "fifth" => return Some(5),
        _ => {}
    }
    let digits_end = lower.find(|c: char| !c.is_ascii_digit())?;
    let n = ascii_digits_value(&lower[..digits_end])?;
    matches!(&lower[digits_end..], "st" | "nd" | "rd" | "th").then_some(n)
}

/// Classifies all tokens of a name.
pub(crate) fn classify(tokens: Vec<Token>, brackets: &mut [Bracket]) -> Vec<Classified> {
    let tokens = split_glued(tokens, brackets);
    let n = tokens.len();
    let lowers: Vec<String> = tokens
        .iter()
        .map(|t| core(&t.text).to_lowercase())
        .collect();
    let bracket_len = |t: &Token| t.bracket.map(|b| brackets[b].tokens.len());
    // `same_run(i, j)`: tokens i..=j may form a phrase (same bracket, no delimiter).
    let same_run = |i: usize, j: usize| {
        j < n
            && tokens[j].bracket == tokens[i].bracket
            && (i + 1..=j).all(|k| !tokens[k].dash_before && !tokens[k].slash_before)
    };

    let mut out: Vec<Classified> = Vec::with_capacity(n);
    let mut weak = vec![false; n];
    let mut i = 0;
    while i < n {
        let t = &tokens[i];
        let mut tag = None;
        let mut covers = i..i + 1;

        // Multi-word dictionary phrases.
        for (words, lex) in PHRASES {
            let len = words.len();
            if same_run(i, i + len - 1) && (0..len).all(|k| lowers[i + k] == words[k]) {
                tag = Some(tag_from_lex(*lex));
                covers = i..i + len;
                break;
            }
        }

        // Two-token patterns.
        if tag.is_none() && same_run(i, i + 1) {
            let (a, b) = (lowers[i].as_str(), lowers[i + 1].as_str());
            let next_num = parse_num(core(&tokens[i + 1].text));
            let prev_num = parse_num(core(&t.text));
            let small = |n: &Option<NumTok>, max: u32| {
                n.as_ref()
                    .filter(|n| {
                        n.year_like.is_none() && n.values[0].number <= max && n.version.is_none()
                    })
                    .map(|n| n.values[0].number)
            };
            tag = match a {
                "season" | "saison" | "сезон" | "temporada" | "staffel" => {
                    small(&next_num, 99).map(Tag::Season)
                }
                "episode" | "ep" | "e" | "серия" | "серии" | "эпизод" | "session" | "folge"
                | "seriya" | "serii" | "seriia" | "serija" => {
                    next_num
                        .as_ref()
                        .filter(|n| n.year_like.is_none())
                        .map(|n| Tag::SeasonEp {
                            season: None,
                            episodes: n.values.clone(),
                            version: n.version,
                        })
                }
                // `Kill Bill - Volume 2` is a title; only the short forms are release info.
                "vol" | "том" => small(&next_num, 99).map(|_| Tag::Other),
                // `серии 1-8 из 10`, `[11 из 11]`: the total after a number is dropped.
                "из" | "of"
                    if out
                        .iter()
                        .rev()
                        .find(|p| p.tag != Tag::Consumed)
                        .is_some_and(|p| matches!(p.tag, Tag::Num(_) | Tag::SeasonEp { .. }))
                        && small(&next_num, 9999).is_some() =>
                {
                    Some(Tag::Consumed)
                }
                "part" | "pt" | "cd" | "disc" | "disk" | "часть" => {
                    small(&next_num, 20).filter(|&p| p >= 1).map(Tag::Part)
                }
                "ova" | "oad" | "ona" | "sp" | "special" | "ncop" | "nced" => small(&next_num, 999)
                    .map(|e| {
                        let kind = match a {
                            "ova" | "oad" => EpisodeKind::Ova,
                            "ona" => EpisodeKind::Ona,
                            "sp" => EpisodeKind::Sp,
                            "special" => EpisodeKind::Special,
                            "ncop" => EpisodeKind::Ncop,
                            _ => EpisodeKind::Nced,
                        };
                        Tag::Kind(kind, Some(EpisodeNumber::whole(e)))
                    }),
                _ => None,
            };
            if tag.is_none() {
                tag = match b {
                    "season" | "сезон" if !t.text.contains(['-', '.']) => {
                        ordinal(a).or_else(|| small(&prev_num, 99)).map(Tag::Season)
                    }
                    "серия" | "серии" | "эпизод" | "seriya" | "serii" | "seriia" | "serija" => {
                        prev_num.as_ref().map(|n| Tag::SeasonEp {
                            season: None,
                            episodes: n.values.clone(),
                            version: n.version,
                        })
                    }
                    _ => None,
                };
            }
            // `14.03 2025` → date (dd.mm yyyy).
            if tag.is_none()
                && let Some((d, m)) = a.split_once('.')
                && let (Some(d), Some(m)) = (ascii_digits_value(d), ascii_digits_value(m))
                && let Some(y) = ascii_digits_value(b).filter(|_| b.len() == 4)
            {
                tag = valid_date(y, m, d).map(Tag::Date);
            }
            if tag.is_some() {
                covers = i..i + 2;
            }
        }

        let tag = match tag {
            Some(tag) => tag,
            None => {
                let alone = bracket_len(t) == Some(1);
                let (tag, w) = classify_single(&t.text, alone, t.bracket.is_some());
                weak[i] = w && !t.glued_hyphen;
                tag
            }
        };
        let display = if covers.len() == 1 {
            core(&t.text).to_string()
        } else {
            covers
                .clone()
                .map(|k| core(&tokens[k].text))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let end = covers.end;
        out.push(Classified {
            tok: t.clone(),
            tag,
            display,
            covers: covers.clone(),
        });
        for (k, tok) in tokens
            .iter()
            .enumerate()
            .take(covers.end)
            .skip(covers.start + 1)
        {
            out.push(Classified {
                tok: tok.clone(),
                tag: Tag::Consumed,
                display: String::new(),
                covers: k..k + 1,
            });
        }
        i = end;
    }

    // Two adjacent markers `S01` `E02` → one SeasonEp.
    for k in 0..out.len().saturating_sub(1) {
        if let Tag::Season(s) = out[k].tag
            && let Tag::SeasonEp {
                season: None,
                episodes,
                version,
            } = out[k + 1].tag.clone()
            && !out[k + 1].tok.dash_before
        {
            out[k].tag = Tag::SeasonEp {
                season: Some(s),
                episodes,
                version,
            };
            out[k].covers.end = out[k + 1].covers.end;
            out[k + 1].tag = Tag::Consumed;
        }
    }

    demote_weak(&mut out, &weak);
    out
}

/// Weak dictionary words count as release info only inside brackets, after
/// the first strong marker, or next to another release token. Otherwise they
/// are title words (`Web of Lies`, `The Trailer`, `City of Ember`).
fn demote_weak(out: &mut [Classified], weak: &[bool]) {
    let strong =
        |c: &Classified, w: bool| !w && !matches!(c.tag, Tag::Free | Tag::Num(_) | Tag::Consumed);
    let first_strong = out
        .iter()
        .enumerate()
        .position(|(k, c)| c.tok.bracket.is_none() && strong(c, weak[k]))
        .unwrap_or(usize::MAX);
    let tagged = |c: &Classified| !matches!(c.tag, Tag::Free | Tag::Num(_) | Tag::Consumed);
    let mut demote = Vec::new();
    for k in 0..out.len() {
        if !weak[k] || out[k].tok.bracket.is_some() || k > first_strong {
            continue;
        }
        let left = k.checked_sub(1).map(|j| &out[j]);
        let right = out.get(k + 1);
        let neighbour = left.is_some_and(|c| tagged(c) && !weak[k - 1])
            || right.is_some_and(|c| tagged(c) && !weak[k + 1])
            || right.is_some_and(|c| c.tok.bracket.is_some() && tagged(c))
            || (right.is_none() && left.is_some_and(tagged));
        if !neighbour {
            demote.push(k);
        }
    }
    for k in demote {
        out[k].tag = Tag::Free;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::token::tokenize;

    fn tags(name: &str) -> Vec<Tag> {
        let (t, mut b) = tokenize(name);
        classify(t, &mut b).into_iter().map(|c| c.tag).collect()
    }

    #[test]
    fn sxxexx_forms() {
        let ep = |s, e: &[u32]| Tag::SeasonEp {
            season: s,
            episodes: e.iter().copied().map(EpisodeNumber::whole).collect(),
            version: None,
        };
        assert_eq!(parse_sxxexx("s01e02"), Some(ep(Some(1), &[2])));
        assert_eq!(parse_sxxexx("s01e01e02"), Some(ep(Some(1), &[1, 2])));
        assert_eq!(parse_sxxexx("s01e01-e03"), Some(ep(Some(1), &[1, 2, 3])));
        assert_eq!(parse_sxxexx("s01e01-03"), Some(ep(Some(1), &[1, 2, 3])));
        assert_eq!(parse_sxxexx("s02"), Some(Tag::Season(2)));
        assert_eq!(parse_nxnn("1x06-07"), Some(ep(Some(1), &[6, 7])));
        assert_eq!(parse_nxnn("1x01x02"), Some(ep(Some(1), &[1, 2])));
        assert_eq!(parse_sxxexx("s01e03-e01"), None);
        assert_eq!(parse_sxxexx("steins"), None);
    }

    #[test]
    fn numbers() {
        let n = parse_num("01v2").unwrap();
        assert_eq!(n.version, Some(2));
        assert_eq!(n.values, vec![EpisodeNumber::whole(1)]);
        let n = parse_num("18.5").unwrap();
        assert_eq!(n.values[0].fraction, Some(5));
        assert_eq!(parse_num("01-03").unwrap().values.len(), 3);
        assert_eq!(parse_num("2019").unwrap().year_like, Some(2019));
        assert_eq!(parse_num("1085").unwrap().year_like, None);
        assert!(parse_num("9-1-1").is_none());
        assert!(parse_num("03-01").is_none());
    }

    #[test]
    fn glued_group_split() {
        let t = tags("Show.S01E05.1080p.x265-GROUP");
        assert_eq!(t.last(), Some(&Tag::Free));
        assert_eq!(t[t.len() - 2], Tag::VideoCodec);
    }

    #[test]
    fn weak_words_stay_in_titles() {
        assert_eq!(tags("Web of Lies 2010")[0], Tag::Free);
        assert_eq!(tags("Shogun.S01E01.1080p.WEB.h264")[3], Tag::Source);
    }

    #[test]
    fn phrases_and_two_token_patterns() {
        assert_eq!(tags("Movie Director's Cut 1080p")[1], Tag::Edition);
        assert_eq!(tags("Show Season 2 Episode 5")[1], Tag::Season(2));
        assert!(matches!(tags("Show 2nd Season - 01")[1], Tag::Season(2)));
        assert_eq!(tags("Movie Part 2 1080p")[1], Tag::Part(2));
        assert_eq!(tags("Movie Part Two")[1], Tag::Free);
    }
}
