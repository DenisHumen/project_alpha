// SPDX-License-Identifier: MPL-2.0
//! Corpus v0 (META-002) stays valid: no validator errors, verified records
//! carry no warnings (the validator escalates them), every category has at
//! least 50 records and the corpus holds at least 2 000 cases.

use pa_recognition_eval::{CorpusValidator, Severity};

const SPLITS: [(&str, &str); 3] = [
    ("train.jsonl", include_str!("../corpus/v0/train.jsonl")),
    ("dev.jsonl", include_str!("../corpus/v0/dev.jsonl")),
    ("test.jsonl", include_str!("../corpus/v0/test.jsonl")),
];

#[test]
fn corpus_v0_is_valid_and_balanced() {
    let validator = CorpusValidator::new().expect("embedded schema loads");
    let mut session = validator.session();
    for (name, text) in SPLITS {
        session.add_jsonl(name, text);
    }
    let report = session.finish();

    let errors: Vec<String> = report
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    assert!(
        errors.is_empty(),
        "corpus v0 has errors:\n{}",
        errors.join("\n")
    );

    assert!(report.records >= 2000, "only {} records", report.records);
    for cat in validator.categories() {
        let n = report.per_category.get(cat).copied().unwrap_or(0);
        assert!(n >= 50, "category {cat} has {n} records, need >= 50");
    }
}

#[test]
fn corpus_v0_splits_match_file_names() {
    for (name, text) in SPLITS {
        let split = name.trim_end_matches(".jsonl");
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: serde_json::Value = serde_json::from_str(line).expect("valid JSON line");
            assert_eq!(v["split"], split, "{} is in {name}", v["id"]);
        }
    }
}
