// SPDX-License-Identifier: MPL-2.0
//! META-007 plumbing: candidate recall of the title index on the META-001
//! example corpus, without any provider.

use pa_eval::Corpus;
use pa_eval::candidates::{DEFAULT_KS, evaluate};
use pa_recognition::title::{TitleIndex, TitleRecord};
use serde_json::Value;

const EXAMPLES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../recognition-eval/examples/examples.jsonl"
);

#[test]
fn index_from_labels_finds_the_works() {
    let text = std::fs::read_to_string(EXAMPLES).unwrap();
    let corpus = Corpus::load(&[(EXAMPLES.to_owned(), text.clone())], true).unwrap();
    let mut records = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line).unwrap();
        for w in v["labels"]["works"].as_array().unwrap() {
            let Some(title) = w["title"].as_str() else {
                continue;
            };
            let ids = w["ids"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    let v = v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                    (k.clone(), v)
                })
                .collect();
            records.push(TitleRecord {
                ids,
                titles: vec![title.to_owned()],
                ..Default::default()
            });
        }
    }
    let index = TitleIndex::from_records(records);
    let report = evaluate(&corpus, &index, &DEFAULT_KS);
    assert!(report.overall.items > 50, "{}", report.overall.items);
    let r10 = report.overall.recall[DEFAULT_KS.iter().position(|&k| k == 10).unwrap()];
    assert!(r10 >= 0.8, "{:?}", report.overall);
    assert_eq!(report, evaluate(&corpus, &index, &DEFAULT_KS));
    assert!(report.to_markdown().contains("R@10"));
}
