// SPDX-License-Identifier: MPL-2.0
//! Scanner behaviour on an in-memory Vfs and on the local file system.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pa_library::rusqlite::Connection;
use pa_scanner::{CancelToken, ScanError, ScanEvent, ScanOptions, ScanReport, scan};
use pa_vfs::{
    ByteSource, DirEntry, EntryKind, ErrorKind, LocalFs, Metadata, Result as VfsResult, Vfs,
    VfsError, VfsPath,
};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const T0: i64 = 1_800_000_000_000;

/// In-memory source: files with size and mtime, folders that can fail,
/// optional per-listing latency (a stand-in for a network round trip).
#[derive(Debug, Default)]
struct MemVfs {
    files: Mutex<BTreeMap<String, (u64, i64)>>,
    failing: Mutex<BTreeSet<String>>,
    latency: Duration,
}

impl MemVfs {
    fn with(files: &[(&str, u64)]) -> Self {
        let vfs = Self::default();
        for (p, size) in files {
            vfs.put(p, *size, T0);
        }
        vfs
    }
    fn put(&self, path: &str, size: u64, mtime: i64) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), (size, mtime));
    }
    fn remove(&self, path: &str) {
        self.files.lock().unwrap().remove(path);
    }
    fn fail(&self, dir: &str, on: bool) {
        let mut f = self.failing.lock().unwrap();
        if on {
            f.insert(dir.to_owned());
        } else {
            f.remove(dir);
        }
    }
}

impl Vfs for MemVfs {
    fn list(&self, dir: &VfsPath) -> VfsResult<Vec<DirEntry>> {
        if !self.latency.is_zero() {
            std::thread::sleep(self.latency);
        }
        if self.failing.lock().unwrap().contains(dir.as_str()) {
            return Err(VfsError::new(ErrorKind::Unavailable, dir.to_string()));
        }
        let prefix = if dir.is_root() {
            "/".to_owned()
        } else {
            format!("{dir}/")
        };
        let mut out: BTreeMap<String, DirEntry> = BTreeMap::new();
        for (path, (size, mtime)) in self.files.lock().unwrap().iter() {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            let (name, is_dir) = match rest.split_once('/') {
                Some((d, _)) => (d, true),
                None => (rest, false),
            };
            let metadata = if is_dir {
                Metadata {
                    kind: EntryKind::Dir,
                    size: 0,
                    modified_ms: None,
                }
            } else {
                Metadata {
                    kind: EntryKind::File,
                    size: *size,
                    modified_ms: Some(*mtime),
                }
            };
            out.entry(name.to_owned()).or_insert_with(|| DirEntry {
                name: name.to_owned(),
                path: dir.join(name).unwrap(),
                metadata,
            });
        }
        if out.is_empty() && !dir.is_root() {
            return Err(VfsError::new(ErrorKind::NotFound, dir.to_string()));
        }
        Ok(out.into_values().collect())
    }

    fn stat(&self, _path: &VfsPath) -> VfsResult<Metadata> {
        Err(VfsError::new(ErrorKind::Unsupported, "stat"))
    }

    fn open(&self, _path: &VfsPath) -> VfsResult<Box<dyn ByteSource>> {
        Err(VfsError::new(
            ErrorKind::Unsupported,
            "the scanner must not open files",
        ))
    }
}

fn library() -> Connection {
    let conn = pa_library::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO sources (id, kind, name, uri, created_at_ms) VALUES (1, 'smb', 'NAS', 'smb://nas/media', 0)",
        [],
    )
    .unwrap();
    conn
}

fn opts(now: i64) -> ScanOptions {
    ScanOptions {
        now_ms: Some(now),
        ..ScanOptions::default()
    }
}

fn run(conn: &mut Connection, vfs: &dyn Vfs, now: i64) -> ScanReport {
    scan(conn, 1, vfs, &opts(now), &CancelToken::new(), &mut |_| {}).unwrap()
}

/// (path, size, missing) of every row, sorted by path.
fn rows(conn: &Connection) -> Vec<(String, i64, bool)> {
    let mut stmt = conn
        .prepare("SELECT path, size_bytes, missing_since_ms IS NOT NULL FROM files ORDER BY path")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn paths(conn: &Connection) -> Vec<String> {
    rows(conn).into_iter().map(|r| r.0).collect()
}

#[test]
fn first_scan_records_media_and_applies_skip_rules() {
    let vfs = MemVfs::with(&[
        ("/Movies/Dune (2021)/Dune (2021).mkv", 100),
        ("/Movies/Dune (2021)/Dune-trailer.mkv", 5),
        ("/Movies/Dune (2021)/sample.mkv", 5),
        ("/Movies/Dune (2021)/Featurettes/Making.mkv", 5),
        ("/Movies/Dune (2021)/Dune.nfo", 1),
        ("/Movies/Dune (2021)/poster.jpg", 1),
        ("/Movies/Disc/BDMV/STREAM/00001.m2ts", 50),
        ("/Movies/@eaDir/Dune.mkv/SYNOVIDEO_VIDEO_SCREENSHOT.jpg", 1),
        ("/Movies/#recycle/Old.mkv", 50),
        ("/Movies/.hidden/Secret.mkv", 50),
        ("/Movies/._Dune.mkv", 1),
        ("/Shows/Show/Season 01/Show.S01E01.mkv", 70),
        ("/Music/Artist/01 - Track.FLAC", 30),
    ]);
    let mut conn = library();
    let mut events = Vec::new();
    let report = scan(
        &mut conn,
        1,
        &vfs,
        &opts(T0),
        &CancelToken::new(),
        &mut |e| events.push(e.clone()),
    )
    .unwrap();

    assert_eq!(
        paths(&conn),
        [
            // LIB-017: disc streams are recorded.
            "/Movies/Disc/BDMV/STREAM/00001.m2ts",
            "/Movies/Dune (2021)/Dune (2021).mkv",
            "/Music/Artist/01 - Track.FLAC",
            "/Shows/Show/Season 01/Show.S01E01.mkv",
        ]
    );
    assert_eq!(report.added, 4);
    assert_eq!(report.files_seen, 4);
    assert_eq!(report.dirs_failed, 0);
    assert!(!report.cancelled);

    let progress: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ScanEvent::Progress(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(progress.len(), report.dirs_listed);
    let last = progress.last().unwrap();
    assert_eq!((last.files_seen, last.added), (4, 4));
    assert_eq!(last.dirs_found, report.dirs_listed);

    let (added, scanned, mtime): (i64, i64, i64) = conn
        .query_row(
            "SELECT added_at_ms, scanned_at_ms, mtime_ms FROM files WHERE path LIKE '%Dune (2021).mkv'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((added, scanned, mtime), (T0, T0, T0));
}

#[test]
fn disc_streams_and_images_are_recorded() {
    let vfs = MemVfs::with(&[
        ("/Movies/Inception (2010)/BDMV/index.bdmv", 1),
        ("/Movies/Inception (2010)/BDMV/PLAYLIST/00800.mpls", 1),
        ("/Movies/Inception (2010)/BDMV/BACKUP/STREAM/00009.m2ts", 1),
        ("/Movies/Inception (2010)/BDMV/STREAM/00000.m2ts", 10),
        ("/Movies/Inception (2010)/BDMV/STREAM/00001.m2ts", 900),
        ("/Movies/Inception (2010)/CERTIFICATE/BACKUP/x.m2ts", 1),
        ("/Movies/Alien (1979)/VIDEO_TS/VIDEO_TS.IFO", 1),
        ("/Movies/Alien (1979)/VIDEO_TS/VTS_01_0.VOB", 5),
        ("/Movies/Alien (1979)/VIDEO_TS/VTS_01_1.VOB", 500),
        ("/Movies/Alien (1979)/AUDIO_TS/x.vob", 1),
        ("/Images/Spirited Away (2001).iso", 700),
        ("/Images/readme.txt", 1),
    ]);
    let mut conn = library();
    let report = run(&mut conn, &vfs, T0);
    assert_eq!(
        paths(&conn),
        [
            "/Images/Spirited Away (2001).iso",
            "/Movies/Alien (1979)/VIDEO_TS/VTS_01_0.VOB",
            "/Movies/Alien (1979)/VIDEO_TS/VTS_01_1.VOB",
            "/Movies/Inception (2010)/BDMV/STREAM/00000.m2ts",
            "/Movies/Inception (2010)/BDMV/STREAM/00001.m2ts",
        ]
    );
    assert_eq!(report.added, 5);

    // A rescan of the stream folder only (the watcher, LIB-005) sees it.
    let sub = ScanOptions {
        subtree: Some(VfsPath::parse("/Movies/Inception (2010)/BDMV/STREAM").unwrap()),
        ..opts(T0 + 1)
    };
    let again = scan(&mut conn, 1, &vfs, &sub, &CancelToken::new(), &mut |_| {}).unwrap();
    assert_eq!((again.files_seen, again.unchanged), (2, 2));

    // The old rules: disc folders skipped, images still recorded.
    let mut conn = library();
    let old_rules = ScanOptions {
        rules: pa_scanner::ScanRules {
            disc_streams: false,
            ..pa_scanner::ScanRules::default()
        },
        ..opts(T0)
    };
    scan(&mut conn, 1, &vfs, &old_rules, &CancelToken::new(), &mut |_| {}).unwrap();
    assert_eq!(paths(&conn), ["/Images/Spirited Away (2001).iso"]);
}

#[test]
fn rescan_without_changes_writes_nothing() {
    let vfs = MemVfs::with(&[("/a/1.mkv", 1), ("/a/2.mkv", 2), ("/b/3.mp3", 3)]);
    let mut conn = library();
    run(&mut conn, &vfs, T0);
    let before = conn.total_changes();
    let r = run(&mut conn, &vfs, T0 + 1000);
    assert_eq!(
        conn.total_changes(),
        before,
        "an unchanged rescan must not write"
    );
    assert_eq!(
        (r.added, r.changed, r.unchanged, r.marked_missing),
        (0, 0, 3, 0)
    );
}

#[test]
fn changed_size_or_mtime_updates_row_and_resets_derived_data() {
    let vfs = MemVfs::with(&[("/a.mkv", 10), ("/b.mkv", 20)]);
    let mut conn = library();
    run(&mut conn, &vfs, T0);
    conn.execute(
        "UPDATE files SET oshash = '0123456789abcdef', probed_at_ms = 1, hashed_at_ms = 1, duration_ms = 5",
        [],
    )
    .unwrap();

    vfs.put("/a.mkv", 11, T0); // size changed
    vfs.put("/b.mkv", 20, T0 + 5); // mtime changed
    let r = run(&mut conn, &vfs, T0 + DAY_MS);
    assert_eq!((r.changed, r.unchanged), (2, 0));

    let reset: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE oshash IS NULL AND probed_at_ms IS NULL
               AND hashed_at_ms IS NULL AND duration_ms IS NULL AND scanned_at_ms = ?1",
            [T0 + DAY_MS],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reset, 2);
    assert_eq!(rows(&conn)[0], ("/a.mkv".to_owned(), 11, false));
}

#[test]
fn missing_files_are_soft_deleted_restored_and_purged_after_grace() {
    let vfs = MemVfs::with(&[("/keep.mkv", 1), ("/gone.mkv", 2), ("/back.mkv", 3)]);
    let mut conn = library();
    run(&mut conn, &vfs, T0);
    let id_back: i64 = conn
        .query_row("SELECT id FROM files WHERE path = '/back.mkv'", [], |r| {
            r.get(0)
        })
        .unwrap();
    conn.execute(
        "UPDATE files SET oshash = '0123456789abcdef' WHERE id = ?1",
        [id_back],
    )
    .unwrap();

    vfs.remove("/gone.mkv");
    vfs.remove("/back.mkv");
    let r = run(&mut conn, &vfs, T0 + DAY_MS);
    assert_eq!(r.marked_missing, 2);
    assert_eq!(
        rows(&conn),
        [
            ("/back.mkv".to_owned(), 3, true),
            ("/gone.mkv".to_owned(), 2, true),
            ("/keep.mkv".to_owned(), 1, false),
        ]
    );

    // Comes back unchanged within the grace period: same row, data kept.
    vfs.put("/back.mkv", 3, T0);
    let r = run(&mut conn, &vfs, T0 + 2 * DAY_MS);
    assert_eq!((r.restored, r.still_missing, r.marked_missing), (1, 1, 0));
    let (id, oshash): (i64, String) = conn
        .query_row(
            "SELECT id, oshash FROM files WHERE path = '/back.mkv' AND missing_since_ms IS NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((id, oshash.as_str()), (id_back, "0123456789abcdef"));

    // Past the grace period (7 days from when it went missing): deleted.
    let r = run(&mut conn, &vfs, T0 + 8 * DAY_MS);
    assert_eq!(r.purged, 1);
    assert_eq!(paths(&conn), ["/back.mkv", "/keep.mkv"]);
}

#[test]
fn failed_folder_keeps_its_files() {
    let vfs = MemVfs::with(&[
        ("/ok/a.mkv", 1),
        ("/flaky/b.mkv", 2),
        ("/flaky/deep/c.mkv", 3),
    ]);
    let mut conn = library();
    run(&mut conn, &vfs, T0);

    vfs.fail("/flaky", true);
    vfs.remove("/ok/a.mkv");
    vfs.put("/ok/new.mkv", 4, T0);
    let mut failed = Vec::new();
    let r = scan(
        &mut conn,
        1,
        &vfs,
        &opts(T0 + DAY_MS),
        &CancelToken::new(),
        &mut |e| {
            if let ScanEvent::DirFailed { path, kind, .. } = e {
                failed.push((path.to_string(), *kind));
            }
        },
    )
    .unwrap();
    assert_eq!(failed, [("/flaky".to_owned(), ErrorKind::Unavailable)]);
    assert_eq!((r.dirs_failed, r.added, r.marked_missing), (1, 1, 1));
    assert_eq!(
        rows(&conn),
        [
            ("/flaky/b.mkv".to_owned(), 2, false),
            ("/flaky/deep/c.mkv".to_owned(), 3, false),
            ("/ok/a.mkv".to_owned(), 1, true),
            ("/ok/new.mkv".to_owned(), 4, false),
        ]
    );
}

#[test]
fn unavailable_root_changes_nothing() {
    let vfs = MemVfs::with(&[("/a.mkv", 1)]);
    let mut conn = library();
    run(&mut conn, &vfs, T0);
    vfs.fail("/", true);
    let before = conn.total_changes();
    let err = scan(
        &mut conn,
        1,
        &vfs,
        &opts(T0 + 30 * DAY_MS),
        &CancelToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(&err, ScanError::SourceUnavailable(e) if e.kind() == ErrorKind::Unavailable));
    assert_eq!(conn.total_changes(), before);
    assert_eq!(rows(&conn), [("/a.mkv".to_owned(), 1, false)]);
}

#[test]
fn unknown_source_is_an_error() {
    let vfs = MemVfs::default();
    let mut conn = library();
    let err = scan(
        &mut conn,
        42,
        &vfs,
        &opts(T0),
        &CancelToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(err, ScanError::SourceNotFound(42)));
}

#[test]
fn cancel_stops_early_and_marks_nothing_missing() {
    let mut files = Vec::new();
    for d in 0..200 {
        files.push((format!("/d{d:03}/f.mkv"), 1));
    }
    let refs: Vec<_> = files.iter().map(|(p, s)| (p.as_str(), *s)).collect();
    let mut vfs = MemVfs::with(&refs);
    let mut conn = library();
    run(&mut conn, &vfs, T0);

    vfs.latency = Duration::from_millis(5);
    for (p, _) in &files {
        vfs.remove(p);
    }
    for d in 0..200 {
        vfs.put(&format!("/n{d:03}/f.mkv"), 1, T0);
    }
    let cancel = CancelToken::new();
    let started = Instant::now();
    let r = scan(&mut conn, 1, &vfs, &opts(T0 + DAY_MS), &cancel, &mut |e| {
        if matches!(e, ScanEvent::Progress(p) if p.dirs_listed >= 10) {
            cancel.cancel();
        }
    })
    .unwrap();
    assert!(r.cancelled);
    assert!(r.dirs_listed < 200, "listed {}", r.dirs_listed);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(r.marked_missing, 0);
    let missing: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE missing_since_ms IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(missing, 0);
    // What was found before the cancel is kept.
    let kept: i64 = conn
        .query_row(
            "SELECT count(*) FROM files WHERE path LIKE '/n%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(usize::try_from(kept).unwrap(), r.added);
}

/// 10 000 files in 500 folders behind a 2 ms listing latency: the parallel
/// walk hides the round trips; the rescan writes nothing.
#[test]
fn ten_thousand_files_over_a_slow_source() {
    let mut vfs = MemVfs::default();
    for d in 0..500 {
        for f in 0..20 {
            vfs.put(&format!("/Show {d}/Season 01/E{f:02}.mkv"), 1_000 + f, T0);
        }
    }
    vfs.latency = Duration::from_millis(2);
    let mut conn = library();

    let t = Instant::now();
    let first = run(&mut conn, &vfs, T0);
    let first_time = t.elapsed();
    assert_eq!(first.added, 10_000);

    let t = Instant::now();
    let again = run(&mut conn, &vfs, T0 + 1);
    let again_time = t.elapsed();
    assert_eq!(again.unchanged, 10_000);
    eprintln!("10k files, 2 ms/listing: first {first_time:?}, rescan {again_time:?}");
    // 1001 listings × 2 ms = 2 s sequentially; 4 threads finish well below.
    assert!(first_time < Duration::from_secs(60));
    assert!(again_time < Duration::from_secs(10));
}

#[test]
fn local_fs_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("Movies/Dune (2021)")).unwrap();
    std::fs::create_dir_all(root.join("Movies/@eaDir")).unwrap();
    std::fs::write(root.join("Movies/Dune (2021)/Dune (2021).mkv"), b"12345").unwrap();
    std::fs::write(root.join("Movies/@eaDir/x.mkv"), b"1").unwrap();
    std::fs::write(root.join("Movies/readme.txt"), b"1").unwrap();
    let vfs = LocalFs::new(root).unwrap();
    let mut conn = library();

    let r = run(&mut conn, &vfs, T0);
    assert_eq!(r.added, 1);
    assert_eq!(
        rows(&conn),
        [("/Movies/Dune (2021)/Dune (2021).mkv".to_owned(), 5, false)]
    );
    let mtime: Option<i64> = conn
        .query_row("SELECT mtime_ms FROM files", [], |r| r.get(0))
        .unwrap();
    assert!(mtime.is_some());

    std::fs::write(root.join("Movies/Dune (2021)/Dune (2021).mkv"), b"1234567").unwrap();
    let r = run(&mut conn, &vfs, T0 + 1);
    assert_eq!(r.changed, 1);

    std::fs::remove_dir_all(root.join("Movies/Dune (2021)")).unwrap();
    let r = run(&mut conn, &vfs, T0 + 2);
    assert_eq!(r.marked_missing, 1);
}

/// Local-disk benchmark from the task's "done when" (10 000 files). Run with
/// `cargo test -p pa-scanner --release -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark: creates 10 000 files"]
fn bench_local_ten_thousand_files() {
    let dir = tempfile::tempdir().unwrap();
    for d in 0..500 {
        let folder = dir.path().join(format!("Show {d}/Season 01"));
        std::fs::create_dir_all(&folder).unwrap();
        for f in 0..20 {
            std::fs::write(folder.join(format!("E{f:02}.mkv")), b"x").unwrap();
        }
    }
    let vfs = LocalFs::new(dir.path()).unwrap();
    let mut conn = library();
    let t = Instant::now();
    assert_eq!(run(&mut conn, &vfs, T0).added, 10_000);
    let first = t.elapsed();
    let t = Instant::now();
    assert_eq!(run(&mut conn, &vfs, T0 + 1).unchanged, 10_000);
    let again = t.elapsed();
    eprintln!("local 10k files: first {first:?}, rescan {again:?}");
    assert!(first < Duration::from_secs(60) && again < Duration::from_secs(10));
}

fn run_subtree(conn: &mut Connection, vfs: &dyn Vfs, dir: &str, now: i64) -> ScanReport {
    let options = ScanOptions {
        subtree: Some(VfsPath::parse(dir).unwrap()),
        ..opts(now)
    };
    scan(conn, 1, vfs, &options, &CancelToken::new(), &mut |_| {}).unwrap()
}

#[test]
fn a_subtree_rescan_touches_only_that_folder() {
    let vfs = MemVfs::with(&[
        ("/Movies/Dune (2021)/Dune (2021).mkv", 100),
        ("/Movies/Heat (1995)/Heat (1995).mkv", 100),
        ("/Shows/Dark/Season 1/Dark.S01E01.mkv", 70),
    ]);
    let mut conn = library();
    assert_eq!(run(&mut conn, &vfs, T0).added, 3);

    // Changes inside and outside the rescanned folder.
    vfs.put("/Movies/Dune (2021)/Dune (2021) part2.mkv", 50, T0);
    vfs.remove("/Movies/Heat (1995)/Heat (1995).mkv");
    vfs.put("/Shows/Dark/Season 1/Dark.S01E02.mkv", 70, T0);
    let r = run_subtree(&mut conn, &vfs, "/Movies/Dune (2021)", T0 + 1);
    assert_eq!((r.files_seen, r.added, r.unchanged), (2, 1, 1));
    assert_eq!(r.marked_missing, 0, "Heat is outside the subtree");
    assert_eq!(rows(&conn).len(), 4);
    assert!(!rows(&conn).iter().any(|r| r.2));

    // A removed folder marks its rows missing; the rest stays.
    vfs.remove("/Movies/Dune (2021)/Dune (2021).mkv");
    vfs.remove("/Movies/Dune (2021)/Dune (2021) part2.mkv");
    let r = run_subtree(&mut conn, &vfs, "/Movies/Dune (2021)", T0 + 2);
    assert_eq!((r.files_seen, r.marked_missing), (0, 2));
    let missing: Vec<_> = rows(&conn)
        .into_iter()
        .filter(|r| r.2)
        .map(|r| r.0)
        .collect();
    assert_eq!(
        missing,
        vec![
            "/Movies/Dune (2021)/Dune (2021) part2.mkv".to_owned(),
            "/Movies/Dune (2021)/Dune (2021).mkv".to_owned(),
        ]
    );

    // A rescan from a parent folder covers everything below it.
    assert_eq!(run_subtree(&mut conn, &vfs, "/Shows", T0 + 3).added, 1);

    // Folders the full scan skips are not scanned on their own either.
    vfs.put("/Movies/.hidden/Secret.mkv", 1, T0);
    vfs.put("/Movies/Extras/Making.mkv", 1, T0);
    let hidden = run_subtree(&mut conn, &vfs, "/Movies/.hidden", T0 + 4);
    let extras = run_subtree(&mut conn, &vfs, "/Movies/Extras", T0 + 4);
    assert_eq!((hidden.files_seen, extras.files_seen), (0, 0));
}
