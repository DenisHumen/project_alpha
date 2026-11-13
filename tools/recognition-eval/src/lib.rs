// SPDX-License-Identifier: MPL-2.0
//! Recognition test-corpus format (META-001).
//!
//! A corpus is a set of JSONL files; every line is one *case*: a directory
//! tree with ground-truth labels. Structure is defined by the JSON Schema in
//! `schema/corpus-record.v1.schema.json`; this crate adds the cross-field rules
//! that JSON Schema cannot express (coverage of every file, references between
//! works and files, numbering completeness, per-category requirements).
//! The labelling rules themselves are documented in `GUIDE.md`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use serde_json::Value;

/// JSON Schema of one corpus record (draft 2020-12).
pub const SCHEMA_JSON: &str = include_str!("../schema/corpus-record.v1.schema.json");
/// Category taxonomy A1–A17.
pub const TAXONOMY_JSON: &str = include_str!("../schema/taxonomy.v1.json");

/// Issue severity. Warnings mark incomplete-but-acceptable drafts and become
/// errors with `--strict` or when a record is `verified`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// One validation finding, tied to a JSONL line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Name of the document passed to [`Session::add_jsonl`].
    pub origin: String,
    pub line: usize,
    pub record_id: Option<String>,
    pub severity: Severity,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        match &self.record_id {
            Some(id) => write!(
                f,
                "{}:{}: {} [{}]: {}",
                self.origin, self.line, sev, id, self.message
            ),
            None => write!(
                f,
                "{}:{}: {}: {}",
                self.origin, self.line, sev, self.message
            ),
        }
    }
}

/// Result of validating one or more JSONL documents.
#[derive(Debug, Default)]
pub struct Report {
    pub issues: Vec<Issue>,
    pub records: usize,
    /// Number of records tagged with each taxonomy category (any position).
    pub per_category: BTreeMap<String, usize>,
    /// Number of records per review status.
    pub per_status: BTreeMap<String, usize>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }
    pub fn warnings(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count()
    }
}

/// Corpus validator: JSON Schema + taxonomy + semantic rules.
pub struct CorpusValidator {
    schema: jsonschema::Validator,
    categories: Vec<String>,
}

impl CorpusValidator {
    /// Builds a validator from the embedded schema and taxonomy.
    pub fn new() -> Result<Self, String> {
        Self::from_parts(SCHEMA_JSON, TAXONOMY_JSON)
    }

    /// Builds a validator from explicit schema and taxonomy documents.
    pub fn from_parts(schema_json: &str, taxonomy_json: &str) -> Result<Self, String> {
        let schema: Value =
            serde_json::from_str(schema_json).map_err(|e| format!("schema is not JSON: {e}"))?;
        let schema = jsonschema::options()
            .should_validate_formats(true)
            .build(&schema)
            .map_err(|e| format!("invalid schema: {e}"))?;
        let taxonomy: Value = serde_json::from_str(taxonomy_json)
            .map_err(|e| format!("taxonomy is not JSON: {e}"))?;
        let categories = taxonomy["categories"]
            .as_array()
            .ok_or("taxonomy has no 'categories' array")?
            .iter()
            .map(|c| {
                c["code"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("category without code")
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { schema, categories })
    }

    /// Taxonomy codes in declaration order.
    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    /// Validates one JSONL document. Duplicate ids are detected across all
    /// documents passed to the same `Session`.
    pub fn session(&self) -> Session<'_> {
        Session {
            validator: self,
            seen_ids: HashMap::new(),
            report: Report::default(),
        }
    }
}

/// Accumulates a report over several JSONL documents.
pub struct Session<'v> {
    validator: &'v CorpusValidator,
    seen_ids: HashMap<String, String>,
    report: Report,
}

impl Session<'_> {
    /// Validates a JSONL document; `origin` is used in duplicate-id messages.
    pub fn add_jsonl(&mut self, origin: &str, text: &str) {
        for (idx, raw) in text.lines().enumerate() {
            let line = idx + 1;
            if raw.trim().is_empty() {
                continue;
            }
            let value: Value = match serde_json::from_str(raw) {
                Ok(v) => v,
                Err(e) => {
                    self.push(
                        origin,
                        line,
                        None,
                        Severity::Error,
                        format!("not valid JSON: {e}"),
                    );
                    continue;
                }
            };
            self.add_record(origin, line, &value);
        }
    }

    /// Finishes the session and returns the report.
    pub fn finish(self) -> Report {
        self.report
    }

    fn push(
        &mut self,
        origin: &str,
        line: usize,
        id: Option<&str>,
        severity: Severity,
        message: String,
    ) {
        self.report.issues.push(Issue {
            origin: origin.to_owned(),
            line,
            record_id: id.map(str::to_owned),
            severity,
            message,
        });
    }

    fn add_record(&mut self, origin: &str, line: usize, record: &Value) {
        self.report.records += 1;
        let id = record.get("id").and_then(Value::as_str).map(str::to_owned);

        let schema_errors: Vec<String> = self
            .validator
            .schema
            .iter_errors(record)
            .map(|e| {
                let at = e.instance_path().to_string();
                if at.is_empty() {
                    e.to_string()
                } else {
                    format!("{at}: {e}")
                }
            })
            .collect();
        if !schema_errors.is_empty() {
            for msg in schema_errors {
                self.push(
                    origin,
                    line,
                    id.as_deref(),
                    Severity::Error,
                    format!("schema: {msg}"),
                );
            }
            return;
        }
        let id = id.expect("schema guarantees id");

        if let Some(prev) = self.seen_ids.insert(id.clone(), format!("{origin}:{line}")) {
            self.push(
                origin,
                line,
                Some(&id),
                Severity::Error,
                format!("duplicate id, first seen at {prev}"),
            );
        }

        let status = record["review"]["status"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        *self.report.per_status.entry(status.clone()).or_default() += 1;
        for cat in record["categories"].as_array().into_iter().flatten() {
            let cat = cat.as_str().unwrap_or_default();
            if self.validator.categories.iter().any(|c| c == cat) {
                *self.report.per_category.entry(cat.to_owned()).or_default() += 1;
            }
        }

        let mut found = Vec::new();
        check_record(record, &self.validator.categories, &mut found);
        // Incomplete labels are acceptable in drafts only.
        let verified = status == "verified";
        for (sev, msg) in found {
            let sev = if verified { Severity::Error } else { sev };
            self.push(origin, line, Some(&id), sev, msg);
        }
    }
}

type Found = Vec<(Severity, String)>;

fn err(found: &mut Found, msg: impl Into<String>) {
    found.push((Severity::Error, msg.into()));
}

fn warn(found: &mut Found, msg: impl Into<String>) {
    found.push((Severity::Warning, msg.into()));
}

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Semantic rules for a record that already passed the JSON Schema.
fn check_record(record: &Value, taxonomy: &[String], found: &mut Found) {
    let categories: BTreeSet<&str> = arr(record, "categories")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for cat in &categories {
        if !taxonomy.iter().any(|c| c == cat) {
            err(found, format!("unknown category {cat}"));
        }
    }

    let license = record["source"]["license"].as_str().unwrap_or_default();
    if s(record, "split") == Some("example") && license.contains("GPL") {
        err(
            found,
            "examples ship in the product repository and must not carry GPL/LGPL content",
        );
    }

    if record["review"]["double_labelled"].as_bool() == Some(true)
        && arr(&record["review"], "annotators").len() < 2
    {
        err(found, "double_labelled requires at least two annotators");
    }

    // ---- input paths ----
    let files = arr(&record["input"], "files");
    let mut input_paths: Vec<&str> = Vec::new();
    for f in files {
        let p = s(f, "path").unwrap_or_default();
        check_path(p, "input", found);
        if input_paths.contains(&p) {
            err(found, format!("duplicate input path {p:?}"));
        }
        input_paths.push(p);
    }

    // ---- works ----
    let labels = &record["labels"];
    let works = arr(labels, "works");
    let mut work_by_ref: BTreeMap<&str, &Value> = BTreeMap::new();
    for w in works {
        let r = s(w, "ref").unwrap_or_default();
        if work_by_ref.insert(r, w).is_some() {
            err(found, format!("duplicate work ref {r}"));
        }
        check_work(w, found);
    }

    // ---- file labels and coverage ----
    let file_labels = arr(labels, "files");
    let mut covered: BTreeMap<&str, usize> = input_paths.iter().map(|p| (*p, 0)).collect();
    let mut label_paths: BTreeSet<&str> = BTreeSet::new();
    let mut used_works: BTreeSet<&str> = BTreeSet::new();
    // (work, scheme key) -> count, for duplicate-episode detection on main files
    let mut claims: BTreeMap<(String, String), usize> = BTreeMap::new();
    // (work, episode key) -> part numbers
    let mut parts: BTreeMap<(String, String), Vec<u64>> = BTreeMap::new();

    for l in file_labels {
        let p = s(l, "path").unwrap_or_default();
        check_path(p, "label", found);
        if !label_paths.insert(p) {
            err(found, format!("path {p:?} is labelled more than once"));
        }
        let unit = s(l, "unit").unwrap_or("file");
        match unit {
            "file" => match covered.get_mut(p) {
                Some(n) => *n += 1,
                None => err(found, format!("label path {p:?} is not an input file")),
            },
            disc => {
                let prefix = format!("{p}/");
                let marker = if disc == "bdmv" { "BDMV" } else { "VIDEO_TS" };
                let mut any = false;
                let mut has_marker = false;
                for (ip, n) in covered.iter_mut() {
                    if let Some(rest) = ip.strip_prefix(&prefix) {
                        any = true;
                        *n += 1;
                        if p.split('/').chain(rest.split('/')).any(|seg| seg == marker) {
                            has_marker = true;
                        }
                    }
                }
                if !any {
                    err(found, format!("disc label {p:?} covers no input files"));
                } else if !has_marker {
                    err(
                        found,
                        format!("unit {disc} at {p:?} has no {marker} directory"),
                    );
                }
            }
        }

        let role = s(l, "role").unwrap_or_default();
        let work_ref = s(l, "work");
        let work = match work_ref {
            Some(r) => match work_by_ref.get(r) {
                Some(w) => {
                    used_works.insert(r);
                    Some(*w)
                }
                None => {
                    err(found, format!("{p:?}: unknown work ref {r}"));
                    None
                }
            },
            None => None,
        };
        check_file_label(l, p, role, work, found);

        let kind = work.and_then(|w| s(w, "kind")).unwrap_or_default();
        let episodes = arr(l, "episodes");
        for ep in episodes {
            check_episode(ep, p, kind, &categories, found);
        }
        if let Some(r) = work_ref {
            let ep_key = episodes
                .iter()
                .map(|e| e["numbering"].to_string())
                .collect::<Vec<_>>()
                .join("+");
            if role == "main" {
                for ep in episodes {
                    if let Some(num) = ep["numbering"].as_object() {
                        for (scheme, v) in num {
                            *claims
                                .entry((r.to_owned(), format!("{scheme}={v}")))
                                .or_default() += 1;
                        }
                    }
                }
            }
            if role == "part" {
                if let Some(n) = l.get("part").and_then(Value::as_u64) {
                    parts.entry((r.to_owned(), ep_key)).or_default().push(n);
                }
            }
        }
    }

    for (p, n) in &covered {
        match n {
            0 => err(found, format!("input file {p:?} has no label")),
            1 => {}
            _ => err(found, format!("input file {p:?} is covered by {n} labels")),
        }
    }
    for r in work_by_ref.keys() {
        if !used_works.contains(r) {
            err(
                found,
                format!("work {r} is not referenced by any file label"),
            );
        }
    }
    for ((w, key), n) in &claims {
        if *n > 1 {
            warn(
                found,
                format!(
                    "{n} main files of {w} claim the same episode ({key}); alternate versions are allowed, superseded copies should be role ignore"
                ),
            );
        }
    }
    for ((w, _), mut nums) in parts {
        nums.sort_unstable();
        let expected: Vec<u64> = (1..=nums.len() as u64).collect();
        if nums != expected {
            err(
                found,
                format!("parts of {w} must be numbered 1..N without gaps, got {nums:?}"),
            );
        } else if nums.len() < 2 {
            err(found, format!("work {w} has a single part; use role main"));
        }
    }

    check_category_requirements(&categories, record, found);
}

fn check_path(p: &str, what: &str, found: &mut Found) {
    let segments: Vec<&str> = p.split('/').collect();
    if segments
        .iter()
        .any(|seg| seg.is_empty() || *seg == "." || *seg == "..")
    {
        err(
            found,
            format!("{what} path {p:?} has empty, '.' or '..' segments"),
        );
    }
    if segments
        .first()
        .is_some_and(|first| first.len() == 2 && first.ends_with(':'))
    {
        err(
            found,
            format!("{what} path {p:?} looks like a drive-letter path; use relative paths"),
        );
    }
    // Privacy: real home directories and e-mail-like tokens must be masked.
    if segments
        .first()
        .is_some_and(|first| ["home", "users", "root"].contains(&first.to_lowercase().as_str()))
    {
        err(
            found,
            format!("{what} path {p:?} starts with a home directory; strip it"),
        );
    }
    if p.contains('@') {
        err(
            found,
            format!("{what} path {p:?} contains '@' (possible e-mail); mask it"),
        );
    }
}

fn check_work(w: &Value, found: &mut Found) {
    let r = s(w, "ref").unwrap_or_default();
    let kind = s(w, "kind").unwrap_or_default();
    let ids = &w["ids"];
    let tmdb = s(ids, "tmdb");
    match kind {
        "movie" | "show" => {
            let want = if kind == "movie" { "movie:" } else { "tv:" };
            match tmdb {
                None => warn(found, format!("{r}: canonical id ids.tmdb is missing")),
                Some(t) if !t.starts_with(want) => err(
                    found,
                    format!("{r}: kind {kind} requires a tmdb id of type {want}, got {t}"),
                ),
                _ => {}
            }
            if let Some(t) = s(ids, "tvdb") {
                if kind == "movie" && !t.starts_with("movie:")
                    || kind == "show" && !t.starts_with("series:")
                {
                    err(
                        found,
                        format!("{r}: tvdb id type {t} does not match kind {kind}"),
                    );
                }
            }
        }
        "anime" if ids.get("anidb").is_none() => {
            warn(found, format!("{r}: canonical id ids.anidb is missing"));
        }
        _ => {}
    }
}

fn check_file_label(l: &Value, p: &str, role: &str, work: Option<&Value>, found: &mut Found) {
    let has = |k: &str| l.get(k).is_some();
    let has_eps = has("episodes");
    match role {
        "main" | "part" => {
            if !has("work") {
                err(found, format!("{p:?}: role {role} requires work"));
            }
        }
        "ignore" | "unknown" | "sidecar" => {
            if role != "sidecar" && has("work") {
                err(
                    found,
                    format!("{p:?}: role {role} must not reference a work"),
                );
            }
            if has_eps {
                err(found, format!("{p:?}: role {role} must not have episodes"));
            }
        }
        "sample" if has_eps => {
            err(found, format!("{p:?}: role sample must not have episodes"));
        }
        _ => {}
    }
    if (role == "part") != has("part") {
        err(
            found,
            format!("{p:?}: field part is required for role part and only for it"),
        );
    }
    if (role == "extra") != has("extra_type") {
        err(
            found,
            format!("{p:?}: field extra_type is required for role extra and only for it"),
        );
    }
    if s(l, "unit").is_some_and(|u| u != "file") && !matches!(role, "main" | "part" | "extra") {
        err(
            found,
            format!("{p:?}: disc units must be main, part or extra"),
        );
    }

    let Some(w) = work else { return };
    let kind = s(w, "kind").unwrap_or_default();
    let anime_movie = kind == "anime" && s(w, "anime_type") == Some("movie");
    let episodic = kind == "show" || (kind == "anime" && !anime_movie);
    if (kind == "movie" || anime_movie) && has_eps {
        err(found, format!("{p:?}: movies have no episodes"));
    }
    if episodic && matches!(role, "main" | "part") && !has_eps {
        err(
            found,
            format!("{p:?}: episodic work requires episodes on role {role}"),
        );
    }
}

fn check_episode(ep: &Value, p: &str, kind: &str, categories: &BTreeSet<&str>, found: &mut Found) {
    let ty = s(ep, "type").unwrap_or_default();
    let num = &ep["numbering"];
    let has = |k: &str| num.get(k).is_some();
    let provider_tv = has("tmdb")
        || has("tvdb_aired")
        || has("tvdb_dvd")
        || has("tvdb_absolute")
        || has("tmdb_group");

    // Credits, trailers and parodies exist only in AniDB; no second scheme expected.
    let anidb_only = matches!(ty, "credit" | "trailer" | "parody");
    match kind {
        "anime" if !anidb_only => {
            if !has("anidb") {
                warn(found, format!("{p:?}: anime episode lacks anidb numbering"));
            }
            if !provider_tv {
                warn(found, format!("{p:?}: anime episode needs a second scheme (tmdb or tvdb_*)"));
            }
        }
        "show"
            // Daily shows are identified by air_date; numbering is optional there.
            if !has("tmdb") && !has("tmdb_group") && ep.get("air_date").is_none() => {
                warn(found, format!("{p:?}: show episode lacks tmdb numbering"));
            }
        _ => {}
    }

    if let Some(at) = num.get("anidb").and_then(|a| s(a, "type")) {
        let ok = match ty {
            "recap" => at == "regular" || at == "special",
            other => at == other,
        };
        if !ok {
            err(
                found,
                format!("{p:?}: episode type {ty} disagrees with anidb type {at}"),
            );
        }
    }
    if kind == "show"
        && ty == "regular"
        && num.get("tmdb").and_then(|t| t["season"].as_u64()) == Some(0)
    {
        warn(
            found,
            format!("{p:?}: regular episode mapped to TMDB season 0"),
        );
    }
    if categories.contains("A11") && ep.get("air_date").is_none() {
        err(
            found,
            format!("{p:?}: A11 (daily) episodes require air_date"),
        );
    }
}

/// Each category promises that the record exercises its hard case.
fn check_category_requirements(categories: &BTreeSet<&str>, record: &Value, found: &mut Found) {
    let labels = arr(&record["labels"], "files");
    let works = arr(&record["labels"], "works");
    let eps = || labels.iter().flat_map(|l| arr(l, "episodes"));
    let parse = || labels.iter().filter_map(|l| l.get("parse"));
    let schemes = |e: &Value| e["numbering"].as_object().map_or(0, |o| o.len());

    let mut need = |cat: &str, ok: bool, what: &str| {
        if categories.contains(cat) && !ok {
            err(found, format!("{cat} requires {what}"));
        }
    };
    need(
        "A1",
        parse().any(|p| p.get("absolute").is_some()),
        "parse.absolute on at least one file",
    );
    need(
        "A3",
        eps().any(|e| schemes(e) >= 2),
        "an episode labelled in at least two schemes",
    );
    need(
        "A4",
        eps().any(|e| s(e, "type") != Some("regular"))
            || labels
                .iter()
                .any(|l| matches!(s(l, "extra_type"), Some("opening" | "ending")))
            || works
                .iter()
                .any(|w| matches!(s(w, "anime_type"), Some("ova" | "ona" | "special"))),
        "a non-regular episode, an opening/ending extra or an OVA/ONA/special work",
    );
    need(
        "A5",
        parse().any(|p| !arr(p, "alt_titles").is_empty())
            || works.len() > 1
            || labels
                .iter()
                .filter_map(|l| l.get("parse").and_then(|p| s(p, "title")))
                .collect::<BTreeSet<_>>()
                .len()
                > 1,
        "differently titled files or parse.alt_titles",
    );
    need(
        "A6",
        parse().any(|p| p.get("release_group").is_some() || p.get("crc32").is_some()),
        "parse.release_group or parse.crc32",
    );
    need(
        "A9",
        labels.iter().any(|l| arr(l, "episodes").len() >= 2),
        "a file with two or more episodes",
    );
    need(
        "A10",
        labels
            .iter()
            .any(|l| matches!(s(l, "role"), Some("extra" | "sample"))),
        "an extra or sample file",
    );
    need(
        "A11",
        eps().any(|e| e.get("air_date").is_some()),
        "air_date on episodes",
    );
    need(
        "A12",
        labels
            .iter()
            .any(|l| matches!(s(l, "unit"), Some("bdmv" | "video_ts")))
            || labels
                .iter()
                .any(|l| s(l, "path").is_some_and(|p| p.to_lowercase().ends_with(".iso"))),
        "a bdmv/video_ts unit or an .iso file",
    );
    need(
        "A13",
        labels.iter().any(|l| s(l, "role") == Some("part")),
        "files with role part",
    );
    need(
        "A14",
        parse().any(|p| p.get("title").is_some()),
        "parse.title on at least one file",
    );
    need("A15", works.len() >= 2, "at least two works");
    need(
        "A16",
        eps().any(|e| {
            let n = &e["numbering"];
            [
                "tmdb",
                "tvdb_aired",
                "tvdb_dvd",
                "tvdb_absolute",
                "tmdb_group",
            ]
            .iter()
            .filter(|k| n.get(**k).is_some())
            .count()
                >= 2
        }),
        "an episode labelled in at least two TMDB/TVDB orders",
    );
    need(
        "A17",
        works
            .iter()
            .any(|w| s(w, "kind") == Some("anime") && s(w, "anime_type") == Some("movie")),
        "an anime work with anime_type movie",
    );
}
