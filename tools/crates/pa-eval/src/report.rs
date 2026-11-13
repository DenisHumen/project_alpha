// SPDX-License-Identifier: MPL-2.0
//! Running a recognizer over a corpus, and rendering / comparing reports.
//!
//! Reports are byte-reproducible: no timestamps, no absolute paths, sorted
//! maps, ratios rounded to 6 decimals. Wall-clock time is deliberately not
//! part of the report (the CLI prints it to stderr).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::corpus::Corpus;
use crate::metrics::{ItemOutcome, Metrics, score_item};
use crate::provider::{Counting, Provider};
use crate::recognizer::Recognizer;

pub const REPORT_VERSION: u32 = 1;

/// Default auto-accept threshold (research 03 §4, step 7: auto at or above,
/// review below). Overridable per run.
pub const DEFAULT_THRESHOLD: f64 = 0.9;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CorpusInfo {
    /// Source file names (no directories, so reports do not leak paths).
    pub sources: Vec<String>,
    pub cases: usize,
    pub input_files: usize,
    pub splits: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Performance {
    pub provider_calls: u64,
    /// External requests per 1 000 input files.
    pub calls_per_1000_files: Option<f64>,
    pub tape_misses: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub report_version: u32,
    pub recognizer: String,
    pub auto_threshold: f64,
    pub corpus: CorpusInfo,
    pub overall: Metrics,
    pub per_category: BTreeMap<String, Metrics>,
    pub per_kind: BTreeMap<String, Metrics>,
    pub performance: Performance,
    /// Every labelled unit, sorted by `(case_id, path)`.
    pub items: Vec<ItemOutcome>,
}

/// Runs `recognizer` over every case and scores it.
pub fn run(
    corpus: &Corpus,
    recognizer: &mut dyn Recognizer,
    provider: &mut dyn Provider,
    threshold: f64,
) -> Report {
    let mut counting = Counting {
        inner: provider,
        calls: 0,
        misses: 0,
    };
    let mut items = Vec::new();
    let mut info = CorpusInfo {
        sources: corpus
            .sources
            .iter()
            .map(|s| s.rsplit(['/', '\\']).next().unwrap_or(s).to_owned())
            .collect(),
        ..Default::default()
    };
    for case in &corpus.cases {
        info.cases += 1;
        info.input_files += case.input.files.len();
        *info.splits.entry(case.split.clone()).or_default() += 1;
        let predictions = recognizer.recognize(&case.input, &mut counting);
        for label in &case.labels.files {
            let work = label.work.as_ref().and_then(|r| {
                case.labels
                    .works
                    .iter()
                    .find(|w| &w.reference == r)
                    .map(|w| (w.kind.as_str(), &w.ids))
            });
            items.push(score_item(
                &case.id,
                &case.categories,
                label,
                work,
                &predictions,
                threshold,
            ));
        }
    }
    items.sort_by(|a, b| (&a.case_id, &a.path).cmp(&(&b.case_id, &b.path)));

    let mut per_category: BTreeMap<String, Vec<&ItemOutcome>> = BTreeMap::new();
    let mut per_kind: BTreeMap<String, Vec<&ItemOutcome>> = BTreeMap::new();
    for item in &items {
        for cat in &item.categories {
            per_category.entry(cat.clone()).or_default().push(item);
        }
        if let Some(kind) = &item.kind {
            per_kind.entry(kind.clone()).or_default().push(item);
        }
    }
    let group = |m: BTreeMap<String, Vec<&ItemOutcome>>| {
        m.into_iter()
            .map(|(k, v)| (k, Metrics::from_items(v)))
            .collect()
    };

    Report {
        report_version: REPORT_VERSION,
        recognizer: recognizer.name().to_owned(),
        auto_threshold: threshold,
        overall: Metrics::from_items(&items),
        per_category: group(per_category),
        per_kind: group(per_kind),
        performance: Performance {
            provider_calls: counting.calls,
            calls_per_1000_files: (info.input_files > 0).then(|| {
                crate::metrics::round6(counting.calls as f64 * 1000.0 / info.input_files as f64)
            }),
            tape_misses: counting.misses,
        },
        corpus: info,
        items,
    }
}

impl Report {
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("report serializes");
        s.push('\n');
        s
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let report: Report = serde_json::from_str(text).map_err(|e| format!("bad report: {e}"))?;
        if report.report_version != REPORT_VERSION {
            return Err(format!(
                "report version {} is not supported (expected {REPORT_VERSION})",
                report.report_version
            ));
        }
        Ok(report)
    }

    pub fn to_markdown(&self) -> String {
        let mut o = String::new();
        let c = &self.corpus;
        let _ = writeln!(o, "# Отчёт распознавания: `{}`\n", self.recognizer);
        let _ = writeln!(
            o,
            "Корпус: {} · кейсов {} · файлов {} · порог авто {}\n",
            c.sources.join(", "),
            c.cases,
            c.input_files,
            self.auto_threshold
        );

        let _ = writeln!(o, "## Итог\n");
        metrics_header(&mut o, "Группа");
        metrics_row(&mut o, "всё", &self.overall);

        let _ = writeln!(o, "\n## По категориям\n");
        metrics_header(&mut o, "Категория");
        let mut cats: Vec<_> = self.per_category.iter().collect();
        cats.sort_by_key(|(k, _)| category_key(k));
        for (cat, m) in cats {
            metrics_row(&mut o, cat, m);
        }

        let _ = writeln!(o, "\n## По типам\n");
        metrics_header(&mut o, "Тип");
        for (kind, m) in &self.per_kind {
            metrics_row(&mut o, kind, m);
        }

        let _ = writeln!(o, "\n## Эпизоды по схемам нумерации\n");
        let _ = writeln!(
            o,
            "| Схема | Размечено | Авто | Верно | Точность | Полнота |"
        );
        let _ = writeln!(o, "|---|---:|---:|---:|---:|---:|");
        for (scheme, e) in &self.overall.episodes {
            let _ = writeln!(
                o,
                "| {scheme} | {} | {} | {} | {} | {} |",
                e.labelled,
                e.auto,
                e.correct,
                pct(e.precision),
                pct(e.recall)
            );
        }

        let _ = writeln!(o, "\n## Парсинг имён по полям\n");
        let _ = writeln!(o, "| Поле | Размечено | Верно | Точность |");
        let _ = writeln!(o, "|---|---:|---:|---:|");
        for (field, f) in &self.overall.parse {
            let _ = writeln!(
                o,
                "| {field} | {} | {} | {} |",
                f.labelled,
                f.correct,
                pct(f.accuracy)
            );
        }

        let _ = writeln!(o, "\n## Калибровка (reliability)\n");
        let _ = writeln!(o, "ECE: {}\n", num(self.overall.ece));
        let _ = writeln!(
            o,
            "| Уверенность | Элементов | Средняя уверенность | Доля верных |"
        );
        let _ = writeln!(o, "|---|---:|---:|---:|");
        for b in &self.overall.reliability {
            let _ = writeln!(
                o,
                "| {:.1}–{:.1} | {} | {:.4} | {:.4} |",
                b.lower, b.upper, b.count, b.mean_confidence, b.accuracy
            );
        }

        let p = &self.performance;
        let _ = writeln!(o, "\n## Производительность\n");
        let _ = writeln!(o, "- запросов к провайдерам: {}", p.provider_calls);
        let _ = writeln!(o, "- на 1 000 файлов: {}", num(p.calls_per_1000_files));
        let _ = writeln!(o, "- промахов кассеты (replay): {}", p.tape_misses);

        let silent: Vec<_> = self.items.iter().filter(|i| i.silent_error()).collect();
        let _ = writeln!(o, "\n## Тихие ошибки ({})\n", silent.len());
        for i in silent {
            let _ = writeln!(
                o,
                "- `{}` `{}` — уверенность {}",
                i.case_id,
                i.path,
                num(i.confidence)
            );
        }
        o
    }
}

fn metrics_header(o: &mut String, first: &str) {
    let _ = writeln!(
        o,
        "| {first} | Элементов | Оценено | Precision@auto | Coverage | Recall | Тихие ошибки | ECE | Проверка | Вручную |"
    );
    let _ = writeln!(o, "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
}

fn metrics_row(o: &mut String, name: &str, m: &Metrics) {
    let _ = writeln!(
        o,
        "| {name} | {} | {} | {} | {} | {} | {} ({}) | {} | {} | {} |",
        m.items,
        m.scorable,
        pct(m.precision_at_auto),
        pct(m.coverage),
        pct(m.recall),
        pct(m.silent_error_rate),
        m.silent_errors,
        num(m.ece),
        pct(m.review_rate),
        pct(m.manual_rate),
    );
}

fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_owned(), |v| format!("{:.2} %", v * 100.0))
}

fn num(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_owned(), |v| format!("{v:.4}"))
}

/// `A2` before `A10`; non-taxonomy codes sort lexically after.
fn category_key(code: &str) -> (String, u32, String) {
    let letters: String = code
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let n = code[letters.len()..].parse().unwrap_or(u32::MAX);
    (letters, n, code.to_owned())
}

/// Result of comparing a run against a stored baseline report.
#[derive(Debug, Clone, Default)]
pub struct Comparison {
    pub markdown: String,
    /// Groups whose silent-error rate grew by more than the allowed margin.
    pub regressions: Vec<String>,
}

/// Compares `current` with `baseline`. A group (overall or category) regresses
/// when its silent-error rate grows by more than `max_silent_pp` percentage
/// points.
pub fn compare(baseline: &Report, current: &Report, max_silent_pp: f64) -> Comparison {
    let mut out = Comparison::default();
    let o = &mut out.markdown;
    let _ = writeln!(
        o,
        "# Сравнение с бейзлайном: `{}` → `{}`\n",
        baseline.recognizer, current.recognizer
    );
    let _ = writeln!(
        o,
        "Допустимый рост тихих ошибок: {max_silent_pp} п. п.\n\n| Группа | Precision@auto | Coverage | Recall | Тихие ошибки | |\n|---|---:|---:|---:|---:|---|"
    );
    let mut groups = vec![(
        "всё".to_owned(),
        Some(&baseline.overall),
        Some(&current.overall),
    )];
    let mut cats: Vec<&String> = baseline
        .per_category
        .keys()
        .chain(current.per_category.keys())
        .collect();
    cats.sort_by_key(|k| category_key(k));
    cats.dedup();
    for cat in cats {
        groups.push((
            cat.clone(),
            baseline.per_category.get(cat),
            current.per_category.get(cat),
        ));
    }
    for (name, base, cur) in groups {
        let get = |m: Option<&Metrics>, f: fn(&Metrics) -> Option<f64>| m.and_then(f);
        let silent_delta = delta_pp(
            get(base, |m| m.silent_error_rate),
            get(cur, |m| m.silent_error_rate),
        );
        let regressed = silent_delta.is_some_and(|d| d > max_silent_pp + 1e-9);
        if regressed {
            out.regressions.push(name.clone());
        }
        let _ = writeln!(
            o,
            "| {name} | {} | {} | {} | {} | {} |",
            cell(
                get(base, |m| m.precision_at_auto),
                get(cur, |m| m.precision_at_auto)
            ),
            cell(get(base, |m| m.coverage), get(cur, |m| m.coverage)),
            cell(get(base, |m| m.recall), get(cur, |m| m.recall)),
            cell(
                get(base, |m| m.silent_error_rate),
                get(cur, |m| m.silent_error_rate)
            ),
            if regressed { "**регресс**" } else { "" }
        );
    }

    let before: BTreeMap<(&str, &str), &ItemOutcome> = baseline
        .items
        .iter()
        .map(|i| ((i.case_id.as_str(), i.path.as_str()), i))
        .collect();
    let mut new_silent = Vec::new();
    let mut new_errors = Vec::new();
    let mut fixed = Vec::new();
    for item in &current.items {
        let key = (item.case_id.as_str(), item.path.as_str());
        let prev = before.get(&key);
        if item.silent_error() && !prev.is_some_and(|p| p.silent_error()) {
            new_silent.push(item);
        } else if prev.is_some_and(|p| p.auto_correct()) && !item.auto_correct() {
            new_errors.push(item);
        } else if item.auto_correct() && prev.is_some_and(|p| !p.auto_correct()) {
            fixed.push(item);
        }
    }
    for (title, list) in [
        ("Новые тихие ошибки", &new_silent),
        ("Новые ошибки и потери авто-матча", &new_errors),
        ("Исправлено", &fixed),
    ] {
        let _ = writeln!(o, "\n## {title} ({})\n", list.len());
        for i in list {
            let _ = writeln!(o, "- `{}` `{}`", i.case_id, i.path);
        }
    }
    let _ = writeln!(
        o,
        "\nИтог: {}",
        if out.regressions.is_empty() {
            "регресса нет".to_owned()
        } else {
            format!("регресс в {}", out.regressions.join(", "))
        }
    );
    out
}

fn delta_pp(base: Option<f64>, cur: Option<f64>) -> Option<f64> {
    Some((cur? - base?) * 100.0)
}

fn cell(base: Option<f64>, cur: Option<f64>) -> String {
    match delta_pp(base, cur) {
        Some(d) => format!("{} ({d:+.2} п. п.)", pct(cur)),
        None => format!("{} → {}", pct(base), pct(cur)),
    }
}
