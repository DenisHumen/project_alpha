// SPDX-License-Identifier: MPL-2.0
//! The change watcher (LIB-005) on a real local folder: a new file shows
//! up in the library in under 5 s, a removed one leaves it, and only the
//! changed folders are rescanned.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use pa_catalog::watch::{LibraryWatcher, WatchEvent, WatchMode, WatchOptions};
use pa_catalog::{Catalog, RescanReport};
use pa_scanner::CancelToken;
use pa_vfs::VfsPath;

/// The `done when` of LIB-005.
const LIMIT: Duration = Duration::from_secs(5);

fn media(root: &Path, rel: &str, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..150_000u32)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

fn setup() -> (tempfile::TempDir, std::path::PathBuf, Arc<Catalog>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    media(&root, "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv", 1);
    media(&root, "Movies/The Matrix 1999.mkv", 2);
    media(&root, "Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv", 3);
    let cat = Catalog::open(dir.path().join("db/library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    (dir, root, Arc::new(cat))
}

fn start(cat: &Arc<Catalog>, options: WatchOptions) -> (LibraryWatcher, Receiver<WatchEvent>) {
    let (tx, rx) = mpsc::channel();
    let watcher = LibraryWatcher::start(
        Arc::clone(cat),
        &HashMap::new(),
        options,
        Box::new(move |e| {
            let _ = tx.send(e);
        }),
    )
    .unwrap();
    (watcher, rx)
}

fn movie_titles(cat: &Catalog) -> Vec<String> {
    cat.movies().unwrap().into_iter().map(|m| m.title).collect()
}

/// Polls `cond` until it holds; returns how long that took.
fn wait_for(limit: Duration, mut cond: impl FnMut() -> bool) -> Option<Duration> {
    let t = Instant::now();
    while t.elapsed() < limit {
        if cond() {
            return Some(t.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn a_new_file_appears_in_the_library_in_under_5_seconds() {
    let (_dir, root, cat) = setup();
    let (watcher, rx) = start(&cat, WatchOptions::default());
    assert_eq!(watcher.sources().len(), 1);
    assert_eq!(watcher.sources()[0].mode, WatchMode::Events);

    // The start-up scan.
    let first = rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert!(first.full && first.error.is_none(), "{first:?}");
    assert_eq!(movie_titles(&cat).len(), 2);

    // A new movie in a new folder.
    media(&root, "Movies/Arrival (2016)/Arrival.2016.1080p.mkv", 7);
    let took = wait_for(LIMIT, || {
        movie_titles(&cat).iter().any(|t| t.contains("Arrival"))
    });
    eprintln!("new file in the library after {took:?}");
    assert!(took.is_some(), "not in the library after {LIMIT:?}");

    let event = rx.recv_timeout(LIMIT).unwrap();
    assert!(!event.full, "an incremental rescan: {event:?}");
    assert!(event.report.added >= 1 && event.error.is_none(), "{event:?}");
    assert!(
        event.folders.iter().all(|f| f.starts_with("/Movies")),
        "only the changed folder: {event:?}"
    );

    // A removed file leaves the library.
    std::fs::remove_file(root.join("Movies/The Matrix 1999.mkv")).unwrap();
    let took = wait_for(LIMIT, || {
        !movie_titles(&cat).iter().any(|t| t.contains("Matrix"))
    });
    assert!(took.is_some(), "still in the library after {LIMIT:?}");
    watcher.stop();
}

#[test]
fn a_new_episode_joins_its_show() {
    let (_dir, root, cat) = setup();
    let (watcher, rx) = start(&cat, WatchOptions::default());
    rx.recv_timeout(Duration::from_secs(30)).unwrap();

    media(&root, "Shows/Dark/Season 1/Dark.S01E02.Lies.1080p.mkv", 8);
    let episodes = || {
        let shows = cat.shows().unwrap();
        cat.episodes(shows[0].id).unwrap().len()
    };
    assert!(wait_for(LIMIT, || episodes() == 2).is_some());
    let event = rx.recv_timeout(LIMIT).unwrap();
    assert_eq!(event.folders, vec!["/Shows/Dark/Season 1".to_owned()]);
    drop(watcher);
}

#[test]
fn hidden_and_unrelated_changes_rescan_nothing_visible() {
    let (_dir, root, cat) = setup();
    let (watcher, rx) = start(&cat, WatchOptions::default());
    rx.recv_timeout(Duration::from_secs(30)).unwrap();

    std::fs::write(root.join("Movies/.Dune.mkv.part"), b"partial").unwrap();
    std::fs::write(root.join("Movies/notes.txt"), b"text").unwrap();
    // notes.txt triggers a rescan of /Movies that finds nothing new.
    let event = rx.recv_timeout(LIMIT).unwrap();
    assert!(!event.full && !event.may_have_changed(), "{event:?}");
    assert_eq!(event.folders, vec!["/Movies".to_owned()]);
    drop(watcher);
}

#[test]
fn rescan_folders_without_changes_reports_none() {
    let (_dir, root, cat) = setup();
    cat.scan(None, &HashMap::new(), &CancelToken::new(), &mut |_| {})
        .unwrap();
    let id = cat.sources().unwrap()[0].id;
    let folders = [VfsPath::parse("/Movies").unwrap()];
    let r = cat
        .rescan_folders(id, &root, &folders, &CancelToken::new())
        .unwrap();
    assert_eq!(
        r,
        RescanReport {
            files_seen: 2,
            ..RescanReport::default()
        }
    );
    assert!(!r.has_changes());

    media(&root, "Movies/Heat (1995)/Heat.1995.mkv", 9);
    let r = cat
        .rescan_folders(id, &root, &folders, &CancelToken::new())
        .unwrap();
    assert_eq!((r.files_seen, r.added), (3, 1));
    assert!(movie_titles(&cat).iter().any(|t| t == "Heat"));

    // An offline root changes nothing.
    assert!(
        cat.rescan_folders(id, &root.join("gone"), &folders, &CancelToken::new())
            .is_err()
    );
}

#[test]
fn sources_without_auto_scan_are_not_followed() {
    let (_dir, _root, cat) = setup();
    let id = cat.sources().unwrap()[0].id;
    assert!(cat.sources().unwrap()[0].auto_scan);
    // NET-011's toggle, written as the source manager does.
    let conn = pa_library::open(cat.path()).unwrap();
    conn.execute("UPDATE sources SET auto_scan = 0 WHERE id = ?1", [id])
        .unwrap();
    assert!(!cat.sources().unwrap()[0].auto_scan);
    let (watcher, _rx) = start(&cat, WatchOptions::default());
    assert!(watcher.sources().is_empty());
}
