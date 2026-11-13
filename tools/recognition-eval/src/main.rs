// SPDX-License-Identifier: MPL-2.0
//! `corpus-validate` — checks recognition corpus JSONL files.
//!
//! ```text
//! corpus-validate [--strict] [--min-per-category N] [--quiet] FILE.jsonl...
//! ```
//! Exit code 0 when valid, 1 on validation failure, 2 on usage or I/O errors.

use std::process::ExitCode;

use pa_recognition_eval::{CorpusValidator, Severity};

const USAGE: &str =
    "usage: corpus-validate [--strict] [--min-per-category N] [--quiet] FILE.jsonl...";

fn main() -> ExitCode {
    let mut strict = false;
    let mut quiet = false;
    let mut min_per_category: Option<usize> = None;
    let mut files = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--strict" => strict = true,
            "--quiet" => quiet = true,
            "--min-per-category" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => min_per_category = Some(n),
                None => {
                    eprintln!("--min-per-category needs a number\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other if other.starts_with('-') => {
                eprintln!("unknown option {other}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => files.push(arg),
        }
    }
    if files.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    let validator = match CorpusValidator::new() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot load schema: {e}");
            return ExitCode::from(2);
        }
    };

    let mut failed = false;
    let mut session = validator.session();
    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{file}: {e}");
                return ExitCode::from(2);
            }
        };
        session.add_jsonl(file, &text);
    }
    let report = session.finish();

    for issue in &report.issues {
        if issue.severity == Severity::Error || !quiet {
            println!("{issue}");
        }
    }

    let errors = report.errors();
    let warnings = report.warnings();
    if errors > 0 || (strict && warnings > 0) {
        failed = true;
    }

    if !quiet {
        println!();
        println!("records: {}", report.records);
        for (status, n) in &report.per_status {
            println!("  {status}: {n}");
        }
        println!("per category:");
    }
    let mut short = Vec::new();
    for cat in validator.categories() {
        let n = report.per_category.get(cat).copied().unwrap_or(0);
        if !quiet {
            println!("  {cat:>4}: {n}");
        }
        if min_per_category.is_some_and(|min| n < min) {
            short.push(format!("{cat}={n}"));
        }
    }
    if let Some(min) = min_per_category {
        if !short.is_empty() {
            println!("categories below {min}: {}", short.join(", "));
            failed = true;
        }
    }
    println!(
        "{errors} error(s), {warnings} warning(s){}",
        if strict { " (strict)" } else { "" }
    );

    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
