// SPDX-License-Identifier: MPL-2.0
//! LIB-007 "done when": the taxonomy cases A10 (extras, trailers, samples),
//! A12 (disc structures) and A13 (split files) of the recognition corpus
//! pass the structural analysis.
//!
//! Every record of `tools/recognition-eval` (hand-written examples and
//! corpus v0, all splits) that has one of these categories is analyzed from
//! its `input.files` (paths, sizes, durations); each file label is checked:
//!
//! | label | expected |
//! |---|---|
//! | `unit: bdmv / video_ts` on a folder | a disc group with that root and unit and a main title |
//! | `main` on an `.iso` | a disc group (`iso`) with the file as main |
//! | `main` / `unknown` | [`Role::Main`] |
//! | `part` | [`Role::Part`] with the same number |
//! | `extra` | [`Role::Extra`] with the same `extra_type` |
//! | `sample` | [`Role::Sample`] |
//! | `sidecar` | subtitle, NFO or art by extension |
//! | `ignore` | [`Role::Ignore`] or not in any group |
//!
//! and no unlabeled video file (disc internals) becomes an item of its own.
//! Run with `-- --nocapture` for the table.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pa_scanner::structure::{
    DiscUnit, ExtraType, Group, GroupEntry, ListedFile, Role, StructureOptions, analyze,
};
use serde_json::Value;

const CATEGORIES: [&str; 3] = ["A10", "A12", "A13"];

fn eval_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tools/recognition-eval")
}

fn records() -> Vec<Value> {
    let dir = eval_dir();
    let mut files = vec![dir.join("examples/examples.jsonl")];
    for split in ["train", "dev", "test"] {
        files.push(dir.join(format!("corpus/v0/{split}.jsonl")));
    }
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            out.push(serde_json::from_str(line).expect("valid JSONL"));
        }
    }
    out
}

fn entry<'a>(groups: &'a [Group], path: &str) -> Option<(&'a Group, &'a GroupEntry)> {
    groups
        .iter()
        .find_map(|g| g.entry(path).map(|e| (g, e)))
}

/// Checks one label; `Err` explains the mismatch.
fn check(groups: &[Group], label: &Value) -> Result<(), String> {
    let path = label["path"].as_str().expect("label path");
    let role = label["role"].as_str().expect("label role");
    if let Some(unit) = label["unit"].as_str().filter(|u| *u != "file") {
        let want = DiscUnit::parse(unit).ok_or_else(|| format!("unknown unit {unit}"))?;
        let g = groups
            .iter()
            .find(|g| g.root == path)
            .ok_or("no group at the disc folder")?;
        if g.disc != Some(want) {
            return Err(format!("disc {:?}, want {want:?}", g.disc));
        }
        return if g.playable().next().is_some() {
            Ok(())
        } else {
            Err("disc without a main title".into())
        };
    }
    let found = entry(groups, path);
    let got = found.map(|(_, e)| e);
    match role {
        "main" | "unknown" => {
            let e = got.ok_or("not in any group")?;
            if e.role != Role::Main {
                return Err(format!("{:?}, want Main", e.role));
            }
            if path.to_lowercase().ends_with(".iso") {
                let (g, _) = found.expect("found");
                if g.disc != Some(DiscUnit::Iso) || g.root != path {
                    return Err("iso is not its own disc group".into());
                }
            }
            Ok(())
        }
        "part" => {
            let e = got.ok_or("not in any group")?;
            let want = label["part"].as_u64().map(|p| u32::try_from(p).unwrap());
            if e.role == Role::Part && e.part == want {
                Ok(())
            } else {
                Err(format!("{:?} {:?}, want Part {want:?}", e.role, e.part))
            }
        }
        "extra" => {
            let e = got.ok_or("not in any group")?;
            let want = label["extra_type"].as_str().and_then(ExtraType::parse);
            if e.role == Role::Extra && e.extra_type == want {
                Ok(())
            } else {
                Err(format!("{:?} {:?}, want Extra {want:?}", e.role, e.extra_type))
            }
        }
        "sample" => match got.map(|e| e.role) {
            Some(Role::Sample) => Ok(()),
            other => Err(format!("{other:?}, want Sample")),
        },
        "sidecar" => {
            let lower = path.to_lowercase();
            let want = if lower.ends_with(".nfo") {
                Role::Nfo
            } else if [".srt", ".ass", ".ssa", ".sub", ".vtt", ".sup"]
                .iter()
                .any(|x| lower.ends_with(x))
            {
                Role::Subtitle
            } else {
                Role::Art
            };
            match got.map(|e| e.role) {
                Some(r) if r == want => Ok(()),
                other => Err(format!("{other:?}, want {want:?}")),
            }
        }
        "ignore" => match got.map(|e| e.role) {
            None | Some(Role::Ignore) => Ok(()),
            other => Err(format!("{other:?}, want Ignore")),
        },
        other => Err(format!("unexpected label role {other}")),
    }
}

#[derive(Default)]
struct Row {
    cases: usize,
    cases_ok: usize,
    labels: usize,
    labels_ok: usize,
}

#[test]
fn taxonomy_a10_a12_a13_pass() {
    let mut table: BTreeMap<&str, Row> = BTreeMap::new();
    let mut failures = Vec::new();
    let options = StructureOptions::default();

    for rec in records() {
        let cats: Vec<&str> = rec["categories"]
            .as_array()
            .expect("categories")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let tested: Vec<&str> = CATEGORIES
            .iter()
            .copied()
            .filter(|c| cats.contains(c))
            .collect();
        if tested.is_empty() {
            continue;
        }
        let id = rec["id"].as_str().unwrap_or("?").to_owned();
        let files: Vec<ListedFile> = rec["input"]["files"]
            .as_array()
            .expect("input files")
            .iter()
            .map(|f| ListedFile {
                path: f["path"].as_str().expect("path").to_owned(),
                size: f["size"].as_u64().unwrap_or(0),
                duration_ms: f["duration_ms"].as_u64(),
            })
            .collect();
        let groups = analyze(&files, &options);

        let labels = rec["labels"]["files"].as_array().expect("label files");
        let mut ok_labels = 0;
        for label in labels {
            match check(&groups, label) {
                Ok(()) => ok_labels += 1,
                Err(why) => failures.push(format!("{id}: {}: {why}", label["path"])),
            }
        }
        // Unlabeled videos (disc internals) must not become items.
        let labeled: Vec<&str> = labels.iter().filter_map(|l| l["path"].as_str()).collect();
        let mut stray = 0;
        for f in &files {
            if labeled.contains(&f.path.as_str()) {
                continue;
            }
            if let Some((g, e)) = entry(&groups, &f.path)
                && matches!(e.role, Role::Main | Role::Part)
                && g.disc.is_none()
            {
                stray += 1;
                failures.push(format!("{id}: unlabeled {} became {:?}", f.path, e.role));
            }
        }
        let case_ok = ok_labels == labels.len() && stray == 0;
        for c in tested {
            let row = table.entry(c).or_default();
            row.cases += 1;
            row.cases_ok += usize::from(case_ok);
            row.labels += labels.len();
            row.labels_ok += ok_labels;
        }
    }

    println!("| category | cases | passed | file labels | passed |");
    println!("|---|---|---|---|---|");
    for (c, r) in &table {
        println!(
            "| {c} | {} | {} | {} | {} |",
            r.cases, r.cases_ok, r.labels, r.labels_ok
        );
    }
    for f in &failures {
        println!("FAIL {f}");
    }
    for c in CATEGORIES {
        let r = table.get(c).unwrap_or_else(|| panic!("no {c} cases found"));
        assert!(r.cases >= 50, "{c}: only {} cases", r.cases);
    }
    assert!(
        failures.is_empty(),
        "{} failures, first: {}",
        failures.len(),
        failures[0]
    );
}

#[test]
fn duration_rule_off_still_passes_named_cases() {
    // Without durations (probe not run yet) every case that is decided by
    // names alone still passes; only duration-decided labels may change.
    let options = StructureOptions { short_ratio: None };
    for rec in records() {
        let cats: Vec<&str> = rec["categories"]
            .as_array()
            .expect("categories")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        if !CATEGORIES.iter().any(|c| cats.contains(c)) {
            continue;
        }
        let files: Vec<ListedFile> = rec["input"]["files"]
            .as_array()
            .expect("input files")
            .iter()
            .map(|f| ListedFile::new(f["path"].as_str().expect("path"), 0))
            .collect();
        let groups = analyze(&files, &options);
        for label in rec["labels"]["files"].as_array().expect("labels") {
            if label["role"] == "ignore" {
                continue; // the 1-minute leftover part needs its duration
            }
            if let Err(why) = check(&groups, label) {
                panic!("{}: {}: {why}", rec["id"], label["path"]);
            }
        }
    }
}
