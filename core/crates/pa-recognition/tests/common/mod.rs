// SPDX-License-Identifier: MPL-2.0
#![allow(dead_code)] // each test binary uses a subset
//! Helpers shared by the table-driven parser tests.

use pa_recognition::parse::{ParseResult, parse};
use serde_json::Value;

fn norm(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(s.to_lowercase()),
        Value::Number(n) => Value::String(n.as_f64().unwrap_or(f64::NAN).to_string()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        other => other.clone(),
    }
}

/// Checks that the best hypothesis has every field of `expected`.
/// `null` in `expected` means "the field must be absent".
pub fn check_best(path: &str, expected: &Value) -> Result<(), String> {
    let result = parse(path);
    let got = serde_json::to_value(&result.best().parsed).expect("serializable");
    let mut errors = Vec::new();
    for (k, v) in expected.as_object().expect("object") {
        let g = got.get(k).cloned().unwrap_or(Value::Null);
        if norm(v) != norm(&g) {
            errors.push(format!("  {k}: want {v}, got {g}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("{path}\n{}\n  full: {got}", errors.join("\n")))
    }
}

/// Runs a table of cases and reports all failures at once.
pub fn run_table(cases: &[(&str, Value)]) {
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(p, e)| check_best(p, e).err())
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// Whether some hypothesis (not necessarily the best) matches.
#[allow(dead_code)]
pub fn in_nbest(result: &ParseResult, pred: impl Fn(&Value) -> bool) -> bool {
    result
        .hypotheses
        .iter()
        .map(|h| serde_json::to_value(&h.parsed).expect("serializable"))
        .any(|v| pred(&v))
}
