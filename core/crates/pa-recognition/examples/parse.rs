// SPDX-License-Identifier: MPL-2.0
//! Parses paths from arguments or stdin (one per line) and prints JSON lines.
//!
//! ```text
//! cargo run -p pa-recognition --example parse -- "[Group] Title - 05 [1080p].mkv"
//! find /media -type f | cargo run -p pa-recognition --example parse
//! ```

use std::io::{self, BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut emit = |path: &str| {
        let r = pa_recognition::parse::parse(path);
        let line = serde_json::json!({ "input": path, "result": r });
        writeln!(out, "{line}").expect("stdout");
    };
    if args.is_empty() {
        for line in io::stdin().lock().lines() {
            emit(&line.expect("utf-8 input"));
        }
    } else {
        args.iter().for_each(|a| emit(a));
    }
}
