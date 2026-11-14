// SPDX-License-Identifier: MPL-2.0
//! `pa-eval` CLI.
//!
//! ```text
//! pa-eval run [--recognizer naive|parser|abstain|scorer|tmdb] [--tape TAPE.jsonl]
//!             [--synthetic-tmdb RECORDS.jsonl] [--record-tmdb OUT.jsonl] [--threshold X]
//!             [--anime AOD.jsonl] [--catalog RECORDS.jsonl]... [--signals OUT.jsonl]
//!             [--kinds movie,show] [--no-validate] [--json OUT.json] [--md OUT.md]
//!             [--misses OUT.jsonl] CORPUS.jsonl...
//! pa-eval compare --baseline BASE.json --current CUR.json
//!             [--max-silent-regression-pp 0.2] [--md OUT.md]
//! pa-eval candidates [--anime AOD.jsonl] [--catalog RECORDS.jsonl]... [--k 1,3,5,10]
//!             [--no-validate] [--json OUT.json] [--md OUT.md] CORPUS.jsonl...
//! ```
//! Exit code: 0 ok, 1 regression (compare), 2 usage, I/O or invalid input.

use std::collections::BTreeSet;
use std::process::ExitCode;
use std::time::Instant;

use pa_eval::provider::{Provider, ProviderError, ProviderRequest, RecordingProvider};
use pa_eval::recognizer::{Abstain, Naive, Parser};
use pa_eval::scorer::{Scorer, SignalLine};
use pa_eval::tmdb::{SyntheticTmdb, Tmdb};
use pa_eval::{Corpus, DEFAULT_THRESHOLD, Recognizer, ReplayProvider, Report, Tape};
use serde_json::Value;

/// Replays a tape and remembers the requests it could not answer, so a tape
/// can be filled for exactly the queries a recognizer makes (`--misses`).
struct MissLog {
    inner: ReplayProvider,
    missed: BTreeSet<String>,
}

impl Provider for MissLog {
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        let result = self.inner.call(request);
        if matches!(result, Err(ProviderError::TapeMiss(_))) {
            self.missed.insert(request.key());
        }
        result
    }
}

const USAGE: &str = "usage:
  pa-eval run [--recognizer naive|parser|abstain|scorer|tmdb] [--tape TAPE.jsonl]
              [--synthetic-tmdb RECORDS.jsonl] [--record-tmdb OUT.jsonl] [--threshold X]
              [--anime AOD.jsonl] [--catalog RECORDS.jsonl]... [--signals OUT.jsonl]
              [--kinds movie,show] [--no-validate] [--json OUT.json] [--md OUT.md]
              [--misses OUT.jsonl] CORPUS.jsonl...
  pa-eval compare --baseline BASE.json --current CUR.json
              [--max-silent-regression-pp 0.2] [--md OUT.md]
  pa-eval candidates [--anime AOD.jsonl] [--catalog RECORDS.jsonl]... [--k 1,3,5,10]
              [--no-validate] [--json OUT.json] [--md OUT.md] CORPUS.jsonl...";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("compare") => cmd_compare(&args[1..]),
        Some("candidates") => cmd_candidates(&args[1..]),
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
        &[
            "--recognizer",
            "--tape",
            "--threshold",
            "--json",
            "--md",
            "--misses",
            "--anime",
            "--catalog",
            "--signals",
            "--synthetic-tmdb",
            "--record-tmdb",
            "--kinds",
        ],
        &["--no-validate"],
    )?;
    if files.is_empty() {
        return Err(USAGE.to_owned());
    }
    let sources = ["--tape", "--synthetic-tmdb", "--record-tmdb"]
        .iter()
        .filter(|o| opt(&opts, o).is_some())
        .count();
    if sources > 1 {
        return Err("use one of --tape, --synthetic-tmdb, --record-tmdb".to_owned());
    }
    let kinds: Vec<String> = opt(&opts, "--kinds")
        .map(|k| {
            k.split(',')
                .map(|k| k.trim().to_owned())
                .filter(|k| !k.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut scorer = None;
    let mut recognizer: Box<dyn Recognizer> = match opt(&opts, "--recognizer").unwrap_or("naive") {
        "naive" => Box::new(Naive),
        "parser" => Box::new(Parser),
        "abstain" => Box::new(Abstain),
        "tmdb" => Box::new(Tmdb),
        "scorer" => {
            scorer = Some(Scorer::new(build_index(&opts)?));
            Box::new(Abstain)
        }
        other => return Err(format!("unknown recognizer {other}")),
    };
    if opt(&opts, "--signals").is_some() && scorer.is_none() {
        return Err("--signals needs --recognizer scorer".to_owned());
    }
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

    let mut replay = MissLog {
        inner: ReplayProvider::new(tape),
        missed: BTreeSet::new(),
    };
    let mut synthetic = match opt(&opts, "--synthetic-tmdb") {
        Some(path) => {
            let records = pa_eval::candidates::load_catalog(&read(path)?)
                .map_err(|e| format!("{path}: {e}"))?;
            Some(SyntheticTmdb::new(&records))
        }
        None => None,
    };
    let mut recording = match opt(&opts, "--record-tmdb") {
        Some(_) => Some(RecordingProvider::new(live_tmdb()?)),
        None => None,
    };
    let provider: &mut dyn Provider = match (&mut synthetic, &mut recording) {
        (Some(s), _) => s,
        (_, Some(r)) => r,
        _ => &mut replay,
    };

    let started = Instant::now();
    let report = match scorer.as_mut() {
        Some(s) => pa_eval::run(&corpus, s, provider, threshold),
        None => pa_eval::run(&corpus, recognizer.as_mut(), provider, threshold),
    };
    let report = if kinds.is_empty() {
        report
    } else {
        report.restrict_kinds(&kinds)
    };
    if let (Some(path), Some(r)) = (opt(&opts, "--record-tmdb"), recording) {
        write(path, &r.into_tape().to_jsonl())?;
    }
    let provider = replay;
    if let (Some(path), Some(s)) = (opt(&opts, "--signals"), &scorer) {
        let mut text = String::new();
        for (seq, file, m) in &s.log {
            let line = SignalLine {
                case_id: &corpus.cases[*seq].id,
                path: file,
                confidence: m.confidence,
                chosen: s.title(m.record),
                runner_up: s.title(m.signals.runner_up),
                signals: &m.signals,
                alternatives: s.alternatives(m),
            };
            text.push_str(&serde_json::to_string(&line).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        write(path, &text)?;
    }
    if let Some(path) = opt(&opts, "--misses") {
        let mut text = String::new();
        for key in &provider.missed {
            text.push_str(key);
            text.push('\n');
        }
        write(path, &text)?;
    }
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

/// A live TMDB client for `--record-tmdb`: the user's key from
/// `TMDB_API_KEY`, the default language (`ru-RU`, as the app), an in-memory
/// cache. A recorded tape holds TMDB data: keep it local (TMDB terms), do
/// not commit it.
fn live_tmdb() -> Result<pa_catalog::pa_metadata::tmdb::TmdbClient, String> {
    use pa_catalog::pa_metadata::{cache::MemoryCache, tmdb};
    let key = std::env::var("TMDB_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .ok_or("--record-tmdb needs the TMDB_API_KEY environment variable")?;
    Ok(tmdb::TmdbClient::live(
        tmdb::TmdbConfig::default(),
        Some(&key),
        std::sync::Arc::new(MemoryCache::default()),
    ))
}

/// Title index from `--anime` and `--catalog` options.
fn build_index(opts: &Opts) -> Result<pa_recognition::title::TitleIndex, String> {
    use pa_eval::candidates;
    let mut records = Vec::new();
    if let Some(path) = opt(opts, "--anime") {
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        records.extend(candidates::load_anime(&bytes).map_err(|e| format!("{path}: {e}"))?);
    }
    for (_, path) in opts.iter().filter(|(k, _)| k == "--catalog") {
        records.extend(candidates::load_catalog(&read(path)?).map_err(|e| format!("{path}: {e}"))?);
    }
    Ok(pa_recognition::title::TitleIndex::from_records(records))
}

fn cmd_candidates(args: &[String]) -> Result<ExitCode, String> {
    use pa_eval::candidates;
    let (opts, files) = parse(
        args,
        &["--anime", "--catalog", "--k", "--json", "--md"],
        &["--no-validate"],
    )?;
    if files.is_empty() {
        return Err(USAGE.to_owned());
    }
    let ks: Vec<usize> = match opt(&opts, "--k") {
        Some(list) => list
            .split(',')
            .map(|k| k.trim().parse::<usize>().ok().filter(|k| *k > 0))
            .collect::<Option<_>>()
            .ok_or("--k must be a comma-separated list of positive integers")?,
        None => candidates::DEFAULT_KS.to_vec(),
    };
    let started = Instant::now();
    let index = build_index(&opts)?;
    let built = started.elapsed();

    let docs = files
        .iter()
        .map(|f| Ok((f.clone(), read(f)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let corpus = Corpus::load(&docs, opt(&opts, "--no-validate").is_none())?;
    let started = Instant::now();
    let report = candidates::evaluate(&corpus, &index, &ks);
    let elapsed = started.elapsed();
    eprintln!(
        "pa-eval: index {} records / {} titles built in {:.3} s; {} units searched in {:.3} s \
         ({:.3} ms each; wall time is not part of the report)",
        index.len(),
        index.title_count(),
        built.as_secs_f64(),
        report.overall.items,
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / report.overall.items.max(1) as f64,
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
