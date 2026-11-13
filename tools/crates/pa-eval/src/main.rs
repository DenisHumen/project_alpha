// SPDX-License-Identifier: MPL-2.0
//! `pa-eval` CLI.
//!
//! ```text
//! pa-eval run [--recognizer naive|abstain] [--tape TAPE.jsonl] [--threshold X]
//!             [--no-validate] [--json OUT.json] [--md OUT.md] CORPUS.jsonl...
//! pa-eval compare --baseline BASE.json --current CUR.json
//!             [--max-silent-regression-pp 0.2] [--md OUT.md]
//! ```
//! Exit code: 0 ok, 1 regression (compare), 2 usage, I/O or invalid input.

use std::process::ExitCode;
use std::time::Instant;

use pa_eval::recognizer::{Abstain, Naive};
use pa_eval::{Corpus, DEFAULT_THRESHOLD, Recognizer, ReplayProvider, Report, Tape};

const USAGE: &str = "usage:
  pa-eval run [--recognizer naive|abstain] [--tape TAPE.jsonl] [--threshold X]
              [--no-validate] [--json OUT.json] [--md OUT.md] CORPUS.jsonl...
  pa-eval compare --baseline BASE.json --current CUR.json
              [--max-silent-regression-pp 0.2] [--md OUT.md]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("compare") => cmd_compare(&args[1..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

/// Parsed `--flag value` pairs (switches carry an empty value).
type Opts = Vec<(String, String)>;

/// Splits `--flag value` options from positional arguments.
fn parse(
    args: &[String],
    with_value: &[&str],
    switches: &[&str],
) -> Result<(Opts, Vec<String>), String> {
    let mut opts = Vec::new();
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if with_value.contains(&a.as_str()) {
            let v = it.next().ok_or(format!("{a} needs a value\n{USAGE}"))?;
            opts.push((a.clone(), v.clone()));
        } else if switches.contains(&a.as_str()) {
            opts.push((a.clone(), String::new()));
        } else if a.starts_with('-') {
            return Err(format!("unknown option {a}\n{USAGE}"));
        } else {
            rest.push(a.clone());
        }
    }
    Ok((opts, rest))
}

fn opt<'a>(opts: &'a Opts, name: &str) -> Option<&'a str> {
    opts.iter()
        .rev()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn write(path: &str, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))
}

fn cmd_run(args: &[String]) -> Result<ExitCode, String> {
    let (opts, files) = parse(
        args,
        &["--recognizer", "--tape", "--threshold", "--json", "--md"],
        &["--no-validate"],
    )?;
    if files.is_empty() {
        return Err(USAGE.to_owned());
    }
    let mut recognizer: Box<dyn Recognizer> = match opt(&opts, "--recognizer").unwrap_or("naive") {
        "naive" => Box::new(Naive),
        "abstain" => Box::new(Abstain),
        other => return Err(format!("unknown recognizer {other}")),
    };
    let threshold = match opt(&opts, "--threshold") {
        Some(t) => t
            .parse::<f64>()
            .ok()
            .filter(|t| (0.0..=1.0).contains(t))
            .ok_or("--threshold must be in [0, 1]")?,
        None => DEFAULT_THRESHOLD,
    };
    let tape = match opt(&opts, "--tape") {
        Some(path) => Tape::parse(&read(path)?).map_err(|e| format!("{path}: {e}"))?,
        None => Tape::default(),
    };
    let docs = files
        .iter()
        .map(|f| Ok((f.clone(), read(f)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let corpus = Corpus::load(&docs, opt(&opts, "--no-validate").is_none())?;

    let started = Instant::now();
    let mut provider = ReplayProvider::new(tape);
    let report = pa_eval::run(&corpus, recognizer.as_mut(), &mut provider, threshold);
    eprintln!(
        "pa-eval: {} cases, {} files in {:.3} s (wall time is not part of the report)",
        report.corpus.cases,
        report.corpus.input_files,
        started.elapsed().as_secs_f64()
    );

    let json = opt(&opts, "--json");
    let md = opt(&opts, "--md");
    if let Some(path) = json {
        write(path, &report.to_json())?;
    }
    if let Some(path) = md {
        write(path, &report.to_markdown())?;
    }
    if json.is_none() && md.is_none() {
        print!("{}", report.to_markdown());
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_compare(args: &[String]) -> Result<ExitCode, String> {
    let (opts, rest) = parse(
        args,
        &[
            "--baseline",
            "--current",
            "--max-silent-regression-pp",
            "--md",
        ],
        &[],
    )?;
    if !rest.is_empty() {
        return Err(USAGE.to_owned());
    }
    let load = |name: &str| -> Result<Report, String> {
        let path = opt(&opts, name).ok_or(format!("{name} is required\n{USAGE}"))?;
        Report::from_json(&read(path)?).map_err(|e| format!("{path}: {e}"))
    };
    let baseline = load("--baseline")?;
    let current = load("--current")?;
    let max_pp = match opt(&opts, "--max-silent-regression-pp") {
        Some(v) => v
            .parse::<f64>()
            .ok()
            .filter(|v| *v >= 0.0)
            .ok_or("--max-silent-regression-pp must be a non-negative number")?,
        None => 0.2,
    };
    let cmp = pa_eval::compare(&baseline, &current, max_pp);
    match opt(&opts, "--md") {
        Some(path) => write(path, &cmp.markdown)?,
        None => print!("{}", cmp.markdown),
    }
    Ok(if cmp.regressions.is_empty() {
        ExitCode::SUCCESS
    } else {
        eprintln!("silent-error regression in: {}", cmp.regressions.join(", "));
        ExitCode::from(1)
    })
}
