// SPDX-License-Identifier: MPL-2.0
//! Runs the parser against the `parse` blocks of the corpus examples
//! (`tools/recognition-eval/examples/examples.jsonl`) and reports field accuracy.
//!
//! Only fields present in a label are compared (labels describe what is
//! written in the name; extra fields we output are not penalized). Strings
//! are compared case-insensitively, lists as sets. `alt_titles` that need
//! outside knowledge (translations not written anywhere in the path) are
//! counted, so the score is a lower bound.
//!
//! Run `cargo test -p pa-recognition --test corpus_examples -- --nocapture`
//! to see the per-file report.

use std::collections::BTreeMap;

use pa_recognition::parse::parse;
use serde_json::Value;

const EXAMPLES: &str = include_str!("../../../../tools/recognition-eval/examples/examples.jsonl");

fn norm(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(s.to_lowercase()),
        Value::Array(a) => {
            let mut items: Vec<String> = a.iter().map(|x| norm(x).to_string()).collect();
            items.sort();
            items.dedup();
            Value::Array(items.into_iter().map(Value::String).collect())
        }
        Value::Number(n) => Value::String(n.as_f64().unwrap_or(f64::NAN).to_string()),
        other => other.clone(),
    }
}

fn matches(expected: &Value, got: Option<&Value>) -> bool {
    got.is_some_and(|g| norm(expected) == norm(g))
}

struct Score {
    fields: usize,
    fields_ok: usize,
    files: usize,
    files_ok: usize,
    files_ok_nbest: usize,
    per_field: BTreeMap<String, (usize, usize)>,
    failures: Vec<String>,
}

fn run() -> Score {
    let mut s = Score {
        fields: 0,
        fields_ok: 0,
        files: 0,
        files_ok: 0,
        files_ok_nbest: 0,
        per_field: BTreeMap::new(),
        failures: Vec::new(),
    };
    for line in EXAMPLES.lines().filter(|l| !l.trim().is_empty()) {
        let rec: Value = serde_json::from_str(line).expect("valid JSONL");
        for f in rec["labels"]["files"].as_array().expect("files") {
            let Some(expected) = f.get("parse").and_then(Value::as_object) else {
                continue;
            };
            let path = f["path"].as_str().expect("path");
            let result = parse(path);
            let hyps: Vec<Value> = result
                .hypotheses
                .iter()
                .map(|h| serde_json::to_value(&h.parsed).expect("serializable"))
                .collect();
            s.files += 1;
            let mut all_ok = true;
            let mut bad = Vec::new();
            for (k, v) in expected {
                let ok = matches(v, hyps[0].get(k));
                s.fields += 1;
                let e = s.per_field.entry(k.clone()).or_default();
                e.0 += 1;
                if ok {
                    s.fields_ok += 1;
                    e.1 += 1;
                } else {
                    all_ok = false;
                    bad.push(format!("{k}: want {v}, got {:?}", hyps[0].get(k)));
                }
            }
            if all_ok {
                s.files_ok += 1;
            } else {
                s.failures
                    .push(format!("{path}\n    {}", bad.join("\n    ")));
            }
            if hyps
                .iter()
                .any(|h| expected.iter().all(|(k, v)| matches(v, h.get(k))))
            {
                s.files_ok_nbest += 1;
            }
        }
    }
    s
}

#[test]
fn corpus_examples_score() {
    let s = run();
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    println!(
        "fields: {}/{} ({:.1}%), files top-1: {}/{} ({:.1}%), files in N-best: {}/{} ({:.1}%)",
        s.fields_ok,
        s.fields,
        pct(s.fields_ok, s.fields),
        s.files_ok,
        s.files,
        pct(s.files_ok, s.files),
        s.files_ok_nbest,
        s.files,
        pct(s.files_ok_nbest, s.files)
    );
    for (k, (n, ok)) in &s.per_field {
        println!("  {k:14} {ok:3}/{n:3}");
    }
    for f in &s.failures {
        println!("FAIL {f}");
    }
    assert!(s.files >= 70, "examples were not loaded");
    // Regression floor (see PR for the current numbers).
    assert!(pct(s.fields_ok, s.fields) >= 95.0, "field accuracy dropped");
    assert!(
        pct(s.files_ok_nbest, s.files) >= 90.0,
        "N-best file accuracy dropped"
    );
}
