// SPDX-License-Identifier: MPL-2.0
//! Build the bundled ODbL database from an upstream anime-offline-database
//! JSONL. Used by `data/anime-mappings/build.sh`.
//!
//! Usage: `anime-mappings-build <upstream.jsonl> <out.jsonl> <release-tag>`

use std::fs;
use std::io::BufWriter;
use std::process::ExitCode;

use pa_anime_mappings::offline_db;
use pa_anime_mappings::update::sha256_hex;

fn run(args: &[String]) -> Result<String, String> {
    let [input, output, tag] = args else {
        return Err(
            "usage: anime-mappings-build <upstream.jsonl> <out.jsonl> <release-tag>".into(),
        );
    };
    let bytes = fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let (meta, entries) =
        offline_db::parse_upstream_jsonl(bytes.as_slice()).map_err(|e| e.to_string())?;
    let header = offline_db::bundle_header(&meta, tag, &sha256_hex(&bytes));
    let file = fs::File::create(output).map_err(|e| format!("{output}: {e}"))?;
    let mut w = BufWriter::new(file);
    offline_db::write_bundle(&mut w, &header, &entries).map_err(|e| e.to_string())?;
    w.into_inner()
        .map_err(|e| e.to_string())?
        .sync_all()
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{output}: {} entries from {} ({tag}, lastUpdate {})",
        entries.len(),
        meta.repository,
        meta.last_update
    ))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("anime-mappings-build: {e}");
            ExitCode::FAILURE
        }
    }
}
