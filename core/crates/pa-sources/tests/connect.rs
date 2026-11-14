// SPDX-License-Identifier: MPL-2.0
//! Backends by kind, the reachability check and the speed test on local
//! sources (network backends have their own contract tests).

use std::sync::Arc;
use std::time::Duration;

use pa_secrets::MemoryStore;
use pa_sources::{Error, NewSource, SourceKind, SourceManager, SpeedTestOptions};
use pa_vfs::{ErrorKind, VfsPath};

fn manager() -> SourceManager {
    SourceManager::new(
        pa_library::open_in_memory().unwrap(),
        Arc::new(MemoryStore::new()),
    )
}

#[test]
fn local_source_connects_and_checks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.mkv"), b"data").unwrap();
    let m = manager();
    let s = m
        .create(NewSource::new(
            SourceKind::Local,
            "Films",
            dir.path().to_str().unwrap(),
        ))
        .unwrap();
    let vfs = m.connect(s.id).unwrap();
    let names: Vec<String> = vfs
        .list(&VfsPath::root())
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["a.mkv"]);

    let status = m.check(s.id).unwrap();
    assert_eq!(status.online, Some(true));
    assert!(status.checked_at_ms.is_some() && status.last_error.is_none());
}

#[test]
fn missing_folder_is_offline() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("unplugged-disk");
    let m = manager();
    let s = m
        .create(NewSource::new(
            SourceKind::Local,
            "USB",
            gone.to_str().unwrap(),
        ))
        .unwrap();
    let status = m.check(s.id).unwrap();
    assert_eq!(status.online, Some(false));
    assert!(status.last_error.is_some() && status.last_error_at_ms.is_some());
    let err = m.connect(s.id).unwrap_err();
    assert!(matches!(err, Error::Source { .. }), "{err:?}");
}

#[test]
fn speed_test_reads_the_largest_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Show")).unwrap();
    std::fs::write(dir.path().join("small.nfo"), b"x").unwrap();
    std::fs::write(dir.path().join("Show/ep1.mkv"), vec![7u8; 3 * 1024 * 1024]).unwrap();
    let m = manager();
    let s = m
        .create(NewSource::new(
            SourceKind::Local,
            "Films",
            dir.path().to_str().unwrap(),
        ))
        .unwrap();

    let limits = SpeedTestOptions {
        max_bytes: 2 * 1024 * 1024,
        chunk_size: 256 * 1024,
        max_duration: Duration::from_secs(10),
        ..Default::default()
    };
    let report = m.speed_test(s.id, limits).unwrap();
    assert_eq!(report.file.as_ref().map(VfsPath::as_str), Some("/Show/ep1.mkv"));
    assert_eq!(report.bytes_read, 2 * 1024 * 1024);
    assert!(report.bytes_per_sec.unwrap() > 0);

    let status = m.get(s.id).unwrap().status;
    assert_eq!(status.online, Some(true));
    let speed = status.speed.unwrap();
    assert_eq!(speed.bytes_per_sec, report.bytes_per_sec);
    assert_eq!(speed.latency_ms, report.latency_ms);
}

#[test]
fn speed_test_of_an_empty_source_has_latency_only() {
    let dir = tempfile::tempdir().unwrap();
    let m = manager();
    let s = m
        .create(NewSource::new(
            SourceKind::Local,
            "Empty",
            dir.path().to_str().unwrap(),
        ))
        .unwrap();
    let report = m.speed_test(s.id, Default::default()).unwrap();
    assert_eq!((report.file, report.bytes_read, report.bytes_per_sec), (None, 0, None));
    let speed = m.get(s.id).unwrap().status.speed.unwrap();
    assert_eq!(speed.bytes_per_sec, None);
}

#[test]
fn network_kinds_build_their_backends() {
    let m = manager();
    let dav = m
        .create(NewSource::new(SourceKind::WebDav, "Cloud", "http://127.0.0.1:1/dav/"))
        .unwrap();
    // Building the client does not touch the network.
    m.connect(dav.id).unwrap();

    let smb = m
        .create(NewSource::new(SourceKind::Smb, "NAS", "smb://127.0.0.1:1/media"))
        .unwrap();
    // Without libsmb2 at run time the SMB backend is unsupported (ADR-0033).
    match m.connect(smb.id) {
        Ok(_) => {}
        Err(e) => assert_eq!(e.source_kind(), Some(ErrorKind::Unsupported), "{e:?}"),
    }
}
