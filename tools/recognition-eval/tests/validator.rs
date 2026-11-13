// SPDX-License-Identifier: MPL-2.0
use pa_recognition_eval::{CorpusValidator, Report, Severity};
use serde_json::{json, Value};

const EXAMPLES: &str = include_str!("../examples/examples.jsonl");

fn run(records: &[Value]) -> Report {
    let v = CorpusValidator::new().expect("embedded schema loads");
    let mut session = v.session();
    let text: String = records.iter().map(|r| format!("{r}\n")).collect();
    session.add_jsonl("test.jsonl", &text);
    session.finish()
}

fn messages(report: &Report, severity: Severity) -> Vec<String> {
    report
        .issues
        .iter()
        .filter(|i| i.severity == severity)
        .map(|i| i.message.clone())
        .collect()
}

fn assert_error(report: &Report, needle: &str) {
    let errors = messages(report, Severity::Error);
    assert!(
        errors.iter().any(|m| m.contains(needle)),
        "expected an error containing {needle:?}, got {errors:#?}"
    );
}

/// A minimal valid record: one TV episode.
fn base() -> Value {
    json!({
        "schema_version": 1,
        "id": "t-base",
        "categories": ["A14"],
        "split": "dev",
        "source": { "kind": "synthetic", "license": "MPL-2.0" },
        "input": { "files": [
            { "path": "TV/24/24.S02E03.mkv", "duration_ms": 2580000, "audio_langs": ["eng"] }
        ]},
        "labels": {
            "works": [ { "ref": "w1", "kind": "show", "title": "24", "year": 2001, "ids": { "tmdb": "tv:1973" } } ],
            "files": [ {
                "path": "TV/24/24.S02E03.mkv", "role": "main", "work": "w1",
                "episodes": [ { "type": "regular", "numbering": { "tmdb": { "season": 2, "episode": 3 } } } ],
                "parse": { "title": "24", "season": 2, "episodes": [3] }
            } ]
        },
        "review": { "status": "verified", "annotators": ["alice"] }
    })
}

#[test]
fn bundled_examples_are_valid_and_cover_every_category() {
    let v = CorpusValidator::new().unwrap();
    let mut session = v.session();
    session.add_jsonl("examples.jsonl", EXAMPLES);
    let report = session.finish();
    assert_eq!(report.errors(), 0, "{:#?}", report.issues);
    assert!(
        report.records >= 50,
        "META-001 asks for at least 50 examples"
    );
    for cat in v.categories() {
        let n = report.per_category.get(cat).copied().unwrap_or(0);
        assert!(n >= 3, "category {cat} has only {n} examples");
    }
}

#[test]
fn base_record_is_clean() {
    let report = run(&[base()]);
    assert!(report.issues.is_empty(), "{:#?}", report.issues);
}

#[test]
fn schema_violation_is_reported() {
    let mut r = base();
    r["split"] = json!("holdout");
    assert_error(&run(&[r]), "schema:");
}

#[test]
fn duplicate_ids_across_lines() {
    assert_error(&run(&[base(), base()]), "duplicate id");
}

#[test]
fn unknown_category() {
    let mut r = base();
    r["categories"] = json!(["A99"]);
    assert_error(&run(&[r]), "unknown category A99");
}

#[test]
fn every_input_file_needs_a_label() {
    let mut r = base();
    r["input"]["files"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "path": "TV/24/sample.mkv" }));
    assert_error(&run(&[r]), "has no label");
}

#[test]
fn label_must_point_to_input() {
    let mut r = base();
    r["labels"]["files"][0]["path"] = json!("TV/24/other.mkv");
    let report = run(&[r]);
    assert_error(&report, "is not an input file");
    assert_error(&report, "has no label");
}

#[test]
fn private_paths_are_rejected() {
    for bad in [
        "home/alice/TV/24.mkv",
        "Users/bob/24.mkv",
        "C:/TV/24.mkv",
        "TV/../24.mkv",
        "TV/me@example.org/24.mkv",
    ] {
        let mut r = base();
        r["input"]["files"][0]["path"] = json!(bad);
        r["labels"]["files"][0]["path"] = json!(bad);
        let errors = messages(&run(&[r]), Severity::Error);
        assert!(!errors.is_empty(), "path {bad:?} was accepted");
    }
}

#[test]
fn absolute_path_fails_schema() {
    let mut r = base();
    r["input"]["files"][0]["path"] = json!("/srv/TV/24.mkv");
    assert_error(&run(&[r]), "schema:");
}

#[test]
fn unknown_and_unused_work_refs() {
    let mut r = base();
    r["labels"]["files"][0]["work"] = json!("w2");
    let report = run(&[r]);
    assert_error(&report, "unknown work ref w2");
    assert_error(&report, "work w1 is not referenced");
}

#[test]
fn episodic_main_needs_episodes() {
    let mut r = base();
    r["labels"]["files"][0]
        .as_object_mut()
        .unwrap()
        .remove("episodes");
    assert_error(&run(&[r]), "requires episodes");
}

#[test]
fn movies_have_no_episodes() {
    let mut r = base();
    r["labels"]["works"][0]["kind"] = json!("movie");
    r["labels"]["works"][0]["ids"]["tmdb"] = json!("movie:62");
    assert_error(&run(&[r]), "movies have no episodes");
}

#[test]
fn tmdb_type_must_match_kind() {
    let mut r = base();
    r["labels"]["works"][0]["ids"]["tmdb"] = json!("movie:1973");
    assert_error(&run(&[r]), "requires a tmdb id of type tv:");
}

#[test]
fn anime_needs_anime_type() {
    let mut r = base();
    r["labels"]["works"][0]["kind"] = json!("anime");
    assert_error(&run(&[r]), "schema:");
}

#[test]
fn parts_must_be_contiguous() {
    let mut r = base();
    r["categories"] = json!(["A13"]);
    r["labels"]["works"][0] =
        json!({ "ref": "w1", "kind": "movie", "title": "X", "ids": { "tmdb": "movie:603" } });
    r["input"]["files"] = json!([{ "path": "M/x.cd1.avi" }, { "path": "M/x.cd3.avi" }]);
    r["labels"]["files"] = json!([
        { "path": "M/x.cd1.avi", "role": "part", "work": "w1", "part": 1 },
        { "path": "M/x.cd3.avi", "role": "part", "work": "w1", "part": 3 }
    ]);
    assert_error(&run(&[r]), "numbered 1..N without gaps");
}

#[test]
fn disc_unit_covers_directory() {
    let mut r = base();
    r["categories"] = json!(["A12"]);
    r["labels"]["works"][0] =
        json!({ "ref": "w1", "kind": "movie", "title": "X", "ids": { "tmdb": "movie:603" } });
    r["input"]["files"] = json!([
        { "path": "M/X (1999)/BDMV/index.bdmv" },
        { "path": "M/X (1999)/BDMV/STREAM/00001.m2ts" }
    ]);
    r["labels"]["files"] =
        json!([{ "path": "M/X (1999)", "unit": "bdmv", "role": "main", "work": "w1" }]);
    let report = run(&[r.clone()]);
    assert_eq!(report.errors(), 0, "{:#?}", report.issues);

    r["labels"]["files"][0]["unit"] = json!("video_ts");
    assert_error(&run(&[r]), "has no VIDEO_TS directory");
}

#[test]
fn daily_category_requires_air_date() {
    let mut r = base();
    r["categories"] = json!(["A11"]);
    assert_error(&run(&[r]), "require air_date");
}

#[test]
fn category_requirement_is_checked() {
    let mut r = base();
    r["categories"] = json!(["A9"]);
    assert_error(&run(&[r]), "A9 requires");
}

#[test]
fn gpl_content_is_not_allowed_in_examples() {
    let mut r = base();
    r["split"] = json!("example");
    r["source"]["license"] = json!("LGPL-3.0-only");
    assert_error(&run(&[r]), "GPL");
}

#[test]
fn anidb_type_must_agree_with_episode_type() {
    let mut r = base();
    r["labels"]["works"][0] = json!({ "ref": "w1", "kind": "anime", "anime_type": "tv", "title": "X", "ids": { "anidb": 1 } });
    r["labels"]["files"][0]["episodes"] = json!([
        { "type": "special", "numbering": { "anidb": { "type": "regular", "number": 1 }, "tmdb": { "season": 0, "episode": 1 } } }
    ]);
    assert_error(&run(&[r]), "disagrees with anidb type");
}

#[test]
fn incomplete_labels_warn_in_draft_and_fail_when_verified() {
    let mut r = base();
    r["labels"]["works"][0]["ids"] = json!({});
    r["review"]["status"] = json!("draft");
    let draft = run(&[r.clone()]);
    assert_eq!(draft.errors(), 0);
    assert!(messages(&draft, Severity::Warning)
        .iter()
        .any(|m| m.contains("ids.tmdb is missing")));

    r["review"]["status"] = json!("verified");
    assert_error(&run(&[r]), "ids.tmdb is missing");
}

#[test]
fn double_labelling_needs_two_annotators() {
    let mut r = base();
    r["review"]["double_labelled"] = json!(true);
    assert_error(&run(&[r]), "at least two annotators");
}

#[test]
fn broken_json_line() {
    let v = CorpusValidator::new().unwrap();
    let mut s = v.session();
    s.add_jsonl("x.jsonl", "{not json}\n\n");
    let report = s.finish();
    assert_eq!(report.records, 0);
    assert_eq!(report.errors(), 1);
}
