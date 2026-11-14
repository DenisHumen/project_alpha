// SPDX-License-Identifier: MPL-2.0
//! Checks signed remote-config files exactly as the app does
//! (`just core remote-config-verify`, `just core remote-config-release-check`).
//!
//! ```text
//! check --release              fail while PRODUCTION_PUBLIC_KEY is a placeholder
//! check [--pubkey FILE] DOC…   verify DOC with DOC.minisig, then parse it
//! ```
//! Without `--pubkey` the production key compiled into the app is used.

use std::path::Path;
use std::process::ExitCode;

use pa_remote_config::{RemoteConfig, TrustedKey, production_key_is_placeholder};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("--release") {
        if production_key_is_placeholder() {
            eprintln!(
                "error: PRODUCTION_PUBLIC_KEY in core/crates/pa-remote-config/src/verify.rs is a \
                 placeholder; release builds would ignore every remote config \
                 (docs/dev/remote-config.md)"
            );
            return ExitCode::FAILURE;
        }
        return match TrustedKey::production() {
            Ok(_) => {
                println!("ok: production minisign key is set");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let key = if args.peek().map(String::as_str) == Some("--pubkey") {
        args.next();
        let Some(path) = args.next() else {
            eprintln!("error: --pubkey needs a file");
            return ExitCode::FAILURE;
        };
        std::fs::read_to_string(&path)
            .map_err(|e| pa_remote_config::Error::Io(e.to_string()))
            .and_then(|text| TrustedKey::decode(&text))
    } else {
        TrustedKey::production()
    };
    let key = match key {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let docs: Vec<String> = args.collect();
    if docs.is_empty() {
        eprintln!("usage: check [--release] | [--pubkey FILE] DOC.json…");
        return ExitCode::FAILURE;
    }
    let mut ok = true;
    for doc in &docs {
        match check(&key, Path::new(doc)) {
            Ok(c) => println!("ok: {doc} (schema {}, serial {})", c.schema, c.serial),
            Err(e) => {
                eprintln!("error: {doc}: {e}");
                ok = false;
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn check(key: &TrustedKey, doc: &Path) -> Result<RemoteConfig, String> {
    let body = std::fs::read(doc).map_err(|e| e.to_string())?;
    let sig_path = format!("{}.minisig", doc.display());
    let sig = std::fs::read_to_string(&sig_path).map_err(|e| format!("{sig_path}: {e}"))?;
    let name = doc
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("bad file name")?;
    key.verify(name, &body, &sig).map_err(|e| e.to_string())?;
    RemoteConfig::parse(&body).map_err(|e| e.to_string())
}
