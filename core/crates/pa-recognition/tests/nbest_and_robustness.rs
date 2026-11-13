// SPDX-License-Identifier: MPL-2.0
//! N-best behaviour, serialization and robustness (fuzz-style) tests.

mod common;

use std::time::{Duration, Instant};

use common::in_nbest;
use pa_recognition::parse::{
    EpisodeNumber, MediaHint, ParseOptions, ParseResult, parse, parse_with,
};
use serde_json::json;

fn ep(n: u32) -> Vec<EpisodeNumber> {
    vec![EpisodeNumber::whole(n)]
}

fn check_invariants(input: &str, r: &ParseResult) {
    assert!(!r.hypotheses.is_empty(), "{input:?}: no hypotheses");
    assert!(r.hypotheses.len() <= ParseOptions::default().max_hypotheses);
    let mut sum = 0.0;
    for w in r.hypotheses.windows(2) {
        assert!(w[0].weight >= w[1].weight, "{input:?}: not sorted");
    }
    for h in &r.hypotheses {
        assert!(
            h.weight > 0.0 && h.weight <= 1.0 + 1e-6,
            "{input:?}: weight {}",
            h.weight
        );
        sum += h.weight;
    }
    assert!(sum <= 1.0 + 1e-4, "{input:?}: weights sum {sum}");
    for (i, a) in r.hypotheses.iter().enumerate() {
        for b in &r.hypotheses[i + 1..] {
            assert_ne!(a.parsed, b.parsed, "{input:?}: duplicate hypotheses");
        }
    }
}

#[test]
fn bare_number_is_episode_or_absolute() {
    let r = parse("Anime/Attack on Titan/Attack on Titan - 26.mkv");
    check_invariants("aot", &r);
    assert_eq!(r.best().parsed.episodes, ep(26));
    assert_eq!(r.hypotheses[1].parsed.absolute, ep(26));
    assert!(r.best().weight > r.hypotheses[1].weight);
    assert!(r.best().reasons.iter().any(|x| x.contains("episode")));
}

#[test]
fn three_digit_number_prefers_absolute() {
    let r = parse("[SubsPlease] One Piece - 1085 (1080p).mkv");
    assert_eq!(r.best().parsed.absolute, ep(1085));
    assert!(in_nbest(&r, |v| v["episodes"] == json!([1085])));
}

#[test]
fn season_folder_makes_episode_certain() {
    let r = parse("TV/Show/Season 2/Show - 05.mkv");
    assert_eq!(r.best().parsed.season, Some(2));
    assert!(r.best().weight > 0.8);
}

#[test]
fn year_like_title_keeps_alternatives() {
    let r = parse("1917.mkv");
    check_invariants("1917", &r);
    assert_eq!(r.best().parsed.title.as_deref(), Some("1917"));
    assert!(in_nbest(&r, |v| v["year"] == json!(1917)));

    let r = parse("Inception.2010.mkv");
    assert_eq!(r.best().parsed.year, Some(2010));
    assert!(in_nbest(&r, |v| v["title"] == json!("Inception 2010")));
}

#[test]
fn number_in_movie_title_keeps_episode_alternative() {
    let r = parse("Toy Story 3 (2010).mkv");
    assert_eq!(r.best().parsed.title.as_deref(), Some("Toy Story 3"));
    assert!(in_nbest(&r, |v| v["episodes"] == json!([3])));
}

#[test]
fn explicit_markers_are_unambiguous() {
    let r = parse("Breaking.Bad.S01E01.720p.mkv");
    assert_eq!(r.hypotheses.len(), 1);
    assert!((r.best().weight - 1.0).abs() < 1e-6);
}

#[test]
fn max_hypotheses_option() {
    let opts = ParseOptions {
        max_hypotheses: 1,
        ..ParseOptions::default()
    };
    assert_eq!(parse_with("Show - 26.mkv", &opts).hypotheses.len(), 1);
    let opts = ParseOptions {
        max_hypotheses: 0,
        ..ParseOptions::default()
    };
    assert_eq!(parse_with("Show - 26.mkv", &opts).hypotheses.len(), 1);
}

#[test]
fn folders_can_be_ignored() {
    let opts = ParseOptions {
        use_folders: false,
        ..ParseOptions::default()
    };
    let r = parse_with("TV/Shogun/Season 1/Shogun - 03.mkv", &opts);
    assert_eq!(r.best().parsed.season, None);
    assert_eq!(r.best().parsed.title.as_deref(), Some("Shogun"));
}

#[test]
fn media_hints() {
    assert_eq!(
        parse("Movie (2010).mkv").best().parsed.media,
        MediaHint::Movie
    );
    assert_eq!(
        parse("Show S01E01.mkv").best().parsed.media,
        MediaHint::Episode
    );
    assert_eq!(
        parse("Movie-trailer.mkv").best().parsed.media,
        MediaHint::Extra
    );
    assert_eq!(parse("").best().parsed.media, MediaHint::Unknown);
}

#[test]
fn serde_roundtrip_matches_corpus_format() {
    let r = parse("[Group] Re Zero - 18.5 [1080p][ABCDEF12].mkv");
    let v = serde_json::to_value(&r).unwrap();
    let best = &v["hypotheses"][0]["parsed"];
    assert_eq!(best["episodes"], json!([18.5]));
    assert_eq!(best["crc32"], json!("ABCDEF12"));
    assert!(best.get("season").is_none(), "None fields are omitted");
    let back: ParseResult = serde_json::from_value(v).unwrap();
    assert_eq!(back, r);

    let r = parse("The.Daily.Show.2025.03.14.mkv");
    let v = serde_json::to_value(&r.best().parsed).unwrap();
    assert_eq!(v["date"], json!("2025-03-14"));
    assert_eq!(
        serde_json::from_value::<pa_recognition::parse::ParsedName>(v).unwrap(),
        r.best().parsed
    );
}

#[test]
fn deterministic() {
    for p in [
        "[Group] Title - 05 [1080p].mkv",
        "1917.mkv",
        "Show.S01E01E02.mkv",
    ] {
        assert_eq!(parse(p), parse(p));
    }
}

/// xorshift64*: deterministic pseudo-random generator for the fuzz loop.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

const PIECES: &[&str] = &[
    "[",
    "]",
    "(",
    ")",
    "{",
    "}",
    "【",
    "】",
    "「",
    "」",
    ".",
    "_",
    " ",
    "-",
    " - ",
    "/",
    "∕",
    "|",
    "\\",
    "~",
    "#",
    "v2",
    "S01",
    "E02",
    "S01E01",
    "1x02",
    "01-02",
    "18.5",
    "2019",
    "1917",
    "1080p",
    "x264",
    "WEB-DL",
    "BluRay",
    "NCOP",
    "OVA",
    "SP1",
    "第2話",
    "第",
    "話",
    "Сезон",
    "серия",
    "cd1",
    "Part",
    "2",
    "05",
    "1085",
    "ABCD1234",
    "Title",
    "Мастер",
    "葬送",
    "e",
    "s",
    "x",
    ".mkv",
    "..",
    "Season",
    "2nd",
    "Episode",
    "Rus",
    "LostFilm",
    "sample",
    "-trailer",
    "BDMV",
    "VIDEO_TS",
    "0",
    "99999999999",
    "٣",
    "é",
    "\u{0301}",
    "\u{200b}",
    "\u{feff}",
    "'",
    "\"",
    ";",
    ",",
    "+",
];

#[test]
fn fuzz_never_panics_and_keeps_invariants() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let len = 1 + rng.next() % 16;
        let s: String = (0..len).map(|_| rng.pick(PIECES)).collect();
        let r = parse(&s);
        check_invariants(&s, &r);
    }
}

#[test]
fn random_bytes_as_utf8_never_panic() {
    let mut rng = Rng(42);
    for _ in 0..5_000 {
        let len = (rng.next() % 64) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let s = String::from_utf8_lossy(&bytes);
        check_invariants(&s, &parse(&s));
    }
}

/// Pathological inputs must stay fast (no super-linear behaviour).
#[test]
fn pathological_inputs_are_fast() {
    let inputs = [
        "[".repeat(50_000),
        "]".repeat(50_000),
        "([{".repeat(20_000),
        "-.".repeat(50_000),
        "a-".repeat(50_000),
        "01-".repeat(30_000),
        "S01E01".repeat(10_000),
        "1x01x".repeat(10_000),
        "第".repeat(30_000),
        format!("{}.mkv", "Title ".repeat(20_000)),
        "a/".repeat(50_000),
        "[Group] ".repeat(10_000),
        "2019 ".repeat(20_000),
    ];
    for s in &inputs {
        let start = Instant::now();
        let r = parse(s);
        let took = start.elapsed();
        check_invariants("pathological", &r);
        assert!(
            took < Duration::from_secs(2),
            "{}… took {took:?}",
            s.chars().take(20).collect::<String>()
        );
    }
}
