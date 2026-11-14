// SPDX-License-Identifier: MPL-2.0
//! NET-011 "done when": the password reaches neither SQLite nor logs.

use std::sync::Arc;

use pa_secrets::{MemoryStore, SecretString};
use pa_sources::{
    Error, NewSource, PasswordChange, SourceKind, SourceManager, SourceOptions, SourceUpdate,
};

const FIRST: &str = "Zq7-first-Pa55word";
const SECOND: &str = "Xw9-second-Pa55word";

/// Every byte of the database: main file, WAL and shared memory.
fn database_bytes(dir: &std::path::Path) -> Vec<u8> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        all.extend(std::fs::read(&path).unwrap());
    }
    all
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn password_is_not_stored_in_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    let secrets = Arc::new(MemoryStore::new());
    let m = SourceManager::open(&db, secrets.clone()).unwrap();

    let smb = m
        .create(
            NewSource::new(SourceKind::Smb, "NAS", "smb://127.0.0.1:1/media")
                .with_login("anna", SecretString::new(FIRST)),
        )
        .unwrap();
    let dav = m
        .create(
            NewSource::new(SourceKind::WebDav, "Cloud", "http://127.0.0.1:1/dav/")
                .with_login("anna", SecretString::new(FIRST))
                .with_options(SourceOptions {
                    timeout_secs: Some(2),
                    ..Default::default()
                }),
        )
        .unwrap();
    m.update(
        smb.id,
        SourceUpdate {
            password: PasswordChange::Set(SecretString::new(SECOND)),
            ..Default::default()
        },
    )
    .unwrap();
    // Failed connections write the status and the error into the row.
    m.check(dav.id).unwrap();
    m.check(smb.id).unwrap();
    m.record_scan_finished(dav.id, Some(&format!("scan failed with {FIRST}")))
        .unwrap();
    let dav_status = m.get(dav.id).unwrap().status;
    assert_eq!(dav_status.online, Some(false));
    let error = dav_status.last_error.unwrap();
    assert!(error.contains("***") && !error.contains(FIRST), "{error}");

    // Every text column, through SQL.
    {
        let conn = pa_library::open(&db).unwrap();
        let mut stmt = conn
            .prepare("SELECT * FROM sources")
            .unwrap();
        let columns = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..columns {
                let value: pa_library::rusqlite::types::Value = row.get(i).unwrap();
                let text = format!("{value:?}");
                assert!(!text.contains(FIRST) && !text.contains(SECOND), "{text}");
            }
        }
    }
    drop(m);
    // And the raw files, including the WAL with old row versions.
    let bytes = database_bytes(dir.path());
    assert!(!bytes.is_empty());
    assert!(!contains(&bytes, FIRST), "first password in the database files");
    assert!(!contains(&bytes, SECOND), "second password in the database files");
    // The secret store has them.
    assert_eq!(secrets.keys().len(), 2);
}

#[test]
fn debug_and_errors_do_not_print_the_password() {
    let secrets = Arc::new(MemoryStore::new());
    let m = SourceManager::new(pa_library::open_in_memory().unwrap(), secrets);
    let new = NewSource::new(SourceKind::WebDav, "Cloud", "http://127.0.0.1:1/")
        .with_login("anna", SecretString::new(FIRST));
    let update = SourceUpdate {
        password: PasswordChange::Set(SecretString::new(SECOND)),
        ..Default::default()
    };
    let debug = format!("{new:?} {update:?} {m:?}");
    assert!(!debug.contains(FIRST) && !debug.contains(SECOND), "{debug}");

    let s = m.create(new).unwrap();
    let debug = format!("{s:?}");
    assert!(!debug.contains(FIRST), "{debug}");

    let err = m
        .speed_test(s.id, Default::default())
        .expect_err("nothing listens on port 1");
    assert!(matches!(err, Error::Source { .. }), "{err:?}");
    let text = format!("{err} {err:?}");
    assert!(!text.contains(FIRST), "{text}");

    // An address with credentials is refused without echoing them.
    let err = m
        .create(NewSource::new(
            SourceKind::Smb,
            "NAS",
            format!("smb://anna:{FIRST}@nas/media"),
        ))
        .unwrap_err();
    let text = format!("{err} {err:?}");
    assert!(!text.contains(FIRST), "{text}");
}
