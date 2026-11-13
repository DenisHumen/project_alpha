// SPDX-License-Identifier: MPL-2.0
//! End-to-end tests of the runner on the META-001 example corpus.

use std::collections::HashMap;
use std::process::Command;

use pa_eval::corpus::{Corpus, Ids};
use pa_eval::metrics::{Decision, WorkOutcome};
use pa_eval::provider::{Provider, ProviderError, ProviderRequest};
use pa_eval::recognizer::{Naive, normalize};
use pa_eval::{
    CaseInput, FilePrediction, Recognizer, RecordingProvider, ReplayProvider, Report, Tape,
    WorkGuess, compare, run,
};
use serde_json::{Value, json};

const EXAMPLES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../recognition-eval/examples/examples.jsonl"
);

fn corpus() -> Corpus {
    let text = std::fs::read_to_string(EXAMPLES).unwrap();
    Corpus::load(&[(EXAMPLES.to_owned(), text)], true).unwrap()
}

fn key(input: &CaseInput) -> String {
    serde_json::to_string(input).unwrap()
}

/// Answers from the labels; `wrong` swaps ids for a bogus one at full confidence.
struct Oracle {
    answers: HashMap<String, Vec<FilePrediction>>,
    name: &'static str,
}

impl Oracle {
    fn new(corpus: &Corpus, wrong: bool) -> Self {
        let mut answers = HashMap::new();
        for case in &corpus.cases {
            let preds = case
                .labels
                .files
                .iter()
                .filter_map(|f| {
                    let work = case
                        .labels
                        .works
                        .iter()
                        .find(|w| Some(&w.reference) == f.work.as_ref())?;
                    let ids = if wrong {
                        Ids {
                            imdb: Some("tt0000001".into()),
                            tmdb: Some("movie:1".into()),
                            anidb: Some(1),
                            ..Default::default()
                        }
                    } else {
                        work.ids.clone()
                    };
                    Some(FilePrediction {
                        path: f.path.clone(),
                        work: Some(WorkGuess {
                            kind: Some(work.kind.clone()),
                            ids,
                        }),
                        episodes: f
                            .episodes
                            .iter()
                            .filter_map(|e| e.numbering.clone())
                            .collect(),
                        confidence: 1.0,
                        parse: f.parse.clone(),
                    })
                })
                .collect();
            answers.insert(key(&case.input), preds);
        }
        let name = if wrong { "liar" } else { "oracle" };
        Self { answers, name }
    }
}

impl Recognizer for Oracle {
    fn name(&self) -> &str {
        self.name
    }
    fn recognize(&mut self, case: &CaseInput, _: &mut dyn Provider) -> Vec<FilePrediction> {
        self.answers.get(&key(case)).cloned().unwrap_or_default()
    }
}

/// Stand-in for a live provider: answers `tmdb/search` from the labels.
struct LabelSearch {
    by_title: HashMap<String, Value>,
}

impl LabelSearch {
    fn new(corpus: &Corpus) -> Self {
        let mut by_title = HashMap::new();
        for case in &corpus.cases {
            for f in &case.labels.files {
                let (Some(parse), Some(work)) = (&f.parse, &f.work) else {
                    continue;
                };
                let Some(title) = &parse.title else { continue };
                let Some(w) = case.labels.works.iter().find(|w| &w.reference == work) else {
                    continue;
                };
                by_title.entry(normalize(title)).or_insert_with(
                    || json!({"results": [{"kind": w.kind, "title": title, "year": w.year, "ids": w.ids}]}),
                );
            }
        }
        Self { by_title }
    }
}

impl Provider for LabelSearch {
    fn call(&mut self, req: &ProviderRequest) -> Result<Value, ProviderError> {
        let q = req
            .params
            .get("query")
            .map(|q| normalize(q))
            .unwrap_or_default();
        Ok(self
            .by_title
            .get(&q)
            .cloned()
            .unwrap_or_else(|| json!({"results": []})))
    }
}

#[test]
fn oracle_is_perfect() {
    let corpus = corpus();
    let report = run(
        &corpus,
        &mut Oracle::new(&corpus, false),
        &mut ReplayProvider::default(),
        0.9,
    );
    let m = &report.overall;
    assert!(m.scorable > 0);
    assert_eq!(m.precision_at_auto, Some(1.0));
    assert_eq!(m.coverage, Some(1.0));
    assert_eq!(m.silent_errors, 0);
    assert_eq!(m.ece, Some(0.0));
    for (scheme, e) in &m.episodes {
        assert_eq!(e.recall, Some(1.0), "{scheme}");
    }
    for (field, c) in &m.parse {
        assert_eq!(c.accuracy, Some(1.0), "{field}");
    }
    assert_eq!(report.per_category.len(), 17);
}

#[test]
fn confident_wrong_answers_are_silent_errors() {
    let corpus = corpus();
    let report = run(
        &corpus,
        &mut Oracle::new(&corpus, true),
        &mut ReplayProvider::default(),
        0.9,
    );
    let m = &report.overall;
    assert_eq!(m.silent_errors, m.scorable);
    assert_eq!(m.silent_error_rate, Some(1.0));
    assert_eq!(m.precision_at_auto, Some(0.0));
    assert_eq!(m.ece, Some(1.0));
    assert!(
        report
            .items
            .iter()
            .all(|i| !i.scorable() || i.work == WorkOutcome::Wrong)
    );

    // With an unreachable threshold nothing is auto: errors go to review.
    let strict = run(
        &corpus,
        &mut Oracle::new(&corpus, true),
        &mut ReplayProvider::default(),
        1.5,
    );
    assert_eq!(strict.overall.silent_errors, 0);
    assert_eq!(strict.overall.review_rate, Some(1.0));
    assert!(
        strict
            .items
            .iter()
            .all(|i| !i.scorable() || i.decision == Decision::Review)
    );
}

#[test]
fn record_then_replay_is_byte_identical() {
    let corpus = corpus();
    let mut recording = RecordingProvider::new(LabelSearch::new(&corpus));
    let live = run(&corpus, &mut Naive, &mut recording, 0.9);
    let tape_text = recording.into_tape().to_jsonl();
    assert!(!tape_text.is_empty());
    let tape = Tape::parse(&tape_text).unwrap();
    assert_eq!(tape.to_jsonl(), tape_text, "tape is canonical");

    let first = run(
        &corpus,
        &mut Naive,
        &mut ReplayProvider::new(tape.clone()),
        0.9,
    );
    let second = run(&corpus, &mut Naive, &mut ReplayProvider::new(tape), 0.9);
    assert_eq!(first.performance.tape_misses, 0);
    assert!(first.overall.auto > 0, "naive baseline matches something");
    assert_eq!(live.to_json(), first.to_json());
    assert_eq!(first.to_json(), second.to_json());
    assert_eq!(first.to_markdown(), second.to_markdown());
    assert_eq!(Report::from_json(&first.to_json()).unwrap(), first);
}

#[test]
fn compare_flags_silent_error_regression() {
    let corpus = corpus();
    let good = run(
        &corpus,
        &mut Oracle::new(&corpus, false),
        &mut ReplayProvider::default(),
        0.9,
    );
    let bad = run(
        &corpus,
        &mut Oracle::new(&corpus, true),
        &mut ReplayProvider::default(),
        0.9,
    );

    let same = compare(&good, &good, 0.2);
    assert!(same.regressions.is_empty());

    let worse = compare(&good, &bad, 0.2);
    assert!(worse.regressions.contains(&"всё".to_owned()));
    assert!(worse.regressions.contains(&"A1".to_owned()));
    assert!(worse.markdown.contains("Новые тихие ошибки"));

    let better = compare(&bad, &good, 0.2);
    assert!(better.regressions.is_empty());
}

#[test]
fn cli_report_is_reproducible() {
    let dir = std::env::temp_dir().join(format!("pa-eval-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = env!("CARGO_BIN_EXE_pa-eval");
    let mut outputs = Vec::new();
    for n in 0..2 {
        let json = dir.join(format!("r{n}.json"));
        let md = dir.join(format!("r{n}.md"));
        let status = Command::new(exe)
            .args(["run", "--json"])
            .arg(&json)
            .arg("--md")
            .arg(&md)
            .arg(EXAMPLES)
            .status()
            .unwrap();
        assert!(status.success());
        outputs.push((std::fs::read(&json).unwrap(), std::fs::read(&md).unwrap()));
    }
    assert_eq!(outputs[0], outputs[1]);

    let status = Command::new(exe)
        .args(["compare", "--baseline"])
        .arg(dir.join("r0.json"))
        .arg("--current")
        .arg(dir.join("r1.json"))
        .arg("--md")
        .arg(dir.join("cmp.md"))
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(&dir).unwrap();
}
