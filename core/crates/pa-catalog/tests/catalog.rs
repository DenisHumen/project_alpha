// SPDX-License-Identifier: MPL-2.0
//! End-to-end tests of the catalog on real folders: scan, recognition,
//! views, progress, Up Next, search, fixes and the v0 import.

use std::collections::HashMap;
use std::path::Path;

use pa_catalog::{Catalog, ItemKind, ItemRef, ScanPhase, ScanUpdate};
use pa_scanner::CancelToken;

/// Writes a fake media file with content unique to `seed`.
fn media(root: &Path, rel: &str, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..150_000u32)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

fn library_folder(root: &Path) {
    media(root, "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv", 1);
    media(root, "Movies/The Matrix 1999.mkv", 2);
    media(root, "Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv", 3);
    media(root, "Shows/Dark/Season 1/Dark.S01E02.Lies.1080p.mkv", 4);
    media(
        root,
        "Shows/Dark/Season 2/Dark.S02E01.Beginnings.1080p.mkv",
        5,
    );
    media(root, "Music/Artist/Album/01 - Song.mp3", 6);
}

fn scan_all(cat: &Catalog) -> Vec<ScanUpdate> {
    let mut updates = Vec::new();
    cat.scan(None, &HashMap::new(), &CancelToken::new(), &mut |u| {
        updates.push(u)
    })
    .unwrap();
    updates
}

fn setup() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let media_root = dir.path().join("media");
    library_folder(&media_root);
    let cat = Catalog::open(dir.path().join("db/library.db")).unwrap();
    let src = cat
        .add_source(None, "local", "Media", media_root.to_str().unwrap(), None)
        .unwrap();
    assert!(src.uri.starts_with("file:///"));
    assert_eq!(src.local_root.as_deref(), Some(media_root.as_path()));
    (dir, cat)
}

#[test]
fn scan_builds_movies_shows_episodes_and_tracks() {
    let (_dir, cat) = setup();
    let updates = scan_all(&cat);
    let last = updates.last().unwrap();
    assert_eq!(last.phase, ScanPhase::Done);
    assert_eq!(last.files_found, 6);
    assert!(updates.iter().all(|u| u.error.is_none()), "{updates:?}");

    let movies = cat.movies().unwrap();
    let titles: Vec<_> = movies.iter().map(|m| (m.title.as_str(), m.year)).collect();
    assert_eq!(
        titles,
        vec![("Dune", Some(2021)), ("The Matrix", Some(1999))]
    );
    let root = cat.sources().unwrap()[0].local_root.clone().unwrap();
    assert_eq!(
        movies[0].local_path.as_deref(),
        Some(
            root.join("Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv")
                .to_str()
                .unwrap()
        )
    );
    assert_eq!(
        movies[0].rel_path,
        "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv"
    );

    let shows = cat.shows().unwrap();
    assert_eq!(shows.len(), 1);
    assert_eq!(shows[0].title, "Dark");
    assert_eq!(shows[0].rel_path, "Shows/Dark");
    let eps = cat.episodes(shows[0].id).unwrap();
    let numbers: Vec<_> = eps.iter().map(|e| (e.season, e.episode)).collect();
    assert_eq!(
        numbers,
        vec![(Some(1), Some(1)), (Some(1), Some(2)), (Some(2), Some(1))]
    );
    assert_eq!(eps[0].show_id, Some(shows[0].id));
    assert_eq!(eps[1].title, "Lies");

    let tracks = cat.tracks().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].kind, ItemKind::Track);

    // A rescan without changes adds nothing.
    scan_all(&cat);
    assert_eq!(cat.movies().unwrap().len(), 2);
    assert_eq!(cat.episodes(shows[0].id).unwrap().len(), 3);

    let recent = cat.recently_added(ItemKind::Show, 10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(cat.item(movies[1].id).unwrap().unwrap().title, "The Matrix");
}

#[test]
fn progress_continue_watching_and_up_next() {
    let (_dir, cat) = setup();
    scan_all(&cat);
    let dune = cat.movies().unwrap()[0].id;
    let show = cat.shows().unwrap()[0].id;
    let eps = cat.episodes(show).unwrap();

    assert!(
        cat.save_progress(dune, 600_000, 9_000_000, 1_000, false)
            .unwrap()
    );
    let p = cat.progress(dune).unwrap().unwrap();
    assert_eq!(
        (p.position_ms, p.duration_ms, p.watched),
        (600_000, 9_000_000, false)
    );

    // Finish S01E01: Up Next offers S01E02.
    assert!(
        cat.save_progress(eps[0].id, 2_990_000, 3_000_000, 2_000, false)
            .unwrap()
    );
    assert!(cat.progress(eps[0].id).unwrap().unwrap().watched);

    let row = cat.continue_watching(50).unwrap();
    let ids: Vec<_> = row.iter().map(|c| (c.item.id, c.up_next)).collect();
    assert_eq!(ids, vec![(eps[1].id, true), (dune, false)]);
    // The duration seen by the player fills the item.
    assert_eq!(
        cat.item(dune).unwrap().unwrap().duration_ms,
        Some(9_000_000)
    );
    // Shows have no progress of their own.
    assert!(!cat.save_progress(show, 1, 2, 3, false).unwrap());
}

#[test]
fn search_and_fix_match() {
    let (_dir, cat) = setup();
    scan_all(&cat);
    let hits = cat.search("matrix", 10).unwrap();
    assert_eq!(hits[0].title, "The Matrix");
    let show = cat.shows().unwrap()[0].id;
    let ep = cat.episodes(show).unwrap()[0].id;

    cat.fix_match(ep, Some("Тьма"), Some(2017), Some(70523))
        .unwrap();
    let fixed = cat.item(show).unwrap().unwrap();
    assert_eq!(
        (fixed.title.as_str(), fixed.year, fixed.tmdb_id),
        ("Тьма", Some(2017), Some(70523))
    );
    assert_eq!(cat.search("Тьма", 10).unwrap()[0].id, show);

    // New files named the old way join the renamed show.
    let root = cat.sources().unwrap()[0].local_root.clone().unwrap();
    media(
        &root,
        "Shows/Dark/Season 2/Dark.S02E02.Lost.and.Found.mkv",
        9,
    );
    scan_all(&cat);
    assert_eq!(cat.shows().unwrap().len(), 1);
    assert_eq!(cat.episodes(show).unwrap().len(), 4);
}

#[test]
fn remove_source_drops_its_catalog() {
    let (_dir, cat) = setup();
    scan_all(&cat);
    let src = cat.sources().unwrap()[0].id;
    cat.remove_source(src).unwrap();
    assert!(cat.sources().unwrap().is_empty());
    assert!(cat.movies().unwrap().is_empty());
    assert!(cat.shows().unwrap().is_empty());
    assert!(cat.tracks().unwrap().is_empty());
    assert!(cat.search("dune", 10).unwrap().is_empty());
}

#[test]
fn offline_source_keeps_items() {
    let (dir, cat) = setup();
    scan_all(&cat);
    std::fs::rename(dir.path().join("media"), dir.path().join("away")).unwrap();
    let updates = scan_all(&cat);
    assert!(updates.iter().any(|u| u.error.is_some()));
    assert_eq!(cat.movies().unwrap().len(), 2);
}

/// `(started, finished, last_error)` of the only source (NET-011 status).
fn scan_status(dir: &Path) -> (Option<i64>, Option<i64>, Option<String>) {
    let conn = pa_library::open(dir.join("db/library.db")).unwrap();
    conn.query_row(
        "SELECT last_scan_started_at_ms, last_scan_finished_at_ms, last_error FROM sources",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

#[test]
fn scan_records_its_times_and_fatal_error_in_the_source_status() {
    let (dir, cat) = setup();
    assert_eq!(scan_status(dir.path()), (None, None, None));
    scan_all(&cat);
    let (started, finished, error) = scan_status(dir.path());
    assert!(started.is_some());
    assert!(finished >= started, "{started:?} {finished:?}");
    assert_eq!(error, None);

    std::fs::rename(dir.path().join("media"), dir.path().join("away")).unwrap();
    scan_all(&cat);
    let (_, finished, error) = scan_status(dir.path());
    assert!(finished.is_some());
    assert!(
        error.as_deref().unwrap().starts_with("folder not found: "),
        "{error:?}"
    );
}

#[test]
fn item_ids_are_strings_of_their_rows() {
    let (_dir, cat) = setup();
    scan_all(&cat);
    let id = cat.movies().unwrap()[0].id;
    assert!(matches!(id, ItemRef::Movie(_)));
    assert_eq!(id.to_string().parse::<ItemRef>().unwrap(), id);
}

// --- v0 import ---------------------------------------------------------------

const V0_SCHEMA: &str = "
CREATE TABLE sources (id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, path TEXT NOT NULL,
  username TEXT, position INTEGER NOT NULL DEFAULT 0);
CREATE TABLE items (id TEXT PRIMARY KEY, kind TEXT NOT NULL, source_id TEXT, title TEXT NOT NULL,
  original_title TEXT, year INTEGER, overview TEXT, poster_url TEXT, backdrop_url TEXT, show_id TEXT,
  season INTEGER, episode INTEGER, episode_end INTEGER, artist TEXT, album TEXT, track_number INTEGER,
  file_path TEXT NOT NULL, file_size INTEGER, file_mtime INTEGER, duration_ms INTEGER,
  match_confidence REAL, parsed_title TEXT, parsed_alt_title TEXT, parsed_year INTEGER,
  local_poster TEXT, tmdb_id INTEGER, meta_fetched_at INTEGER, locked INTEGER NOT NULL DEFAULT 0,
  added_at INTEGER NOT NULL, seen_scan INTEGER);
CREATE TABLE progress (item_id TEXT PRIMARY KEY, position_ms INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL, updated_at INTEGER NOT NULL, watched INTEGER NOT NULL DEFAULT 0);
CREATE TABLE tmdb_cache (key TEXT PRIMARY KEY, body TEXT NOT NULL, fetched_at INTEGER NOT NULL);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
PRAGMA user_version = 1;
";

fn v0_fixture(dir: &Path, media_root: &Path) -> std::path::PathBuf {
    let path = dir.join("kinolune.db");
    let db = pa_library::rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(V0_SCHEMA).unwrap();
    let root = media_root.to_str().unwrap();
    db.execute(
        "INSERT INTO sources VALUES ('src_1', 'localFolder', 'Media', ?1, NULL, 1),
                                    ('src_2', 'smb', 'NAS', 'smb://nas/video', 'me', 2)",
        [root],
    )
    .unwrap();
    let f = |rel: &str| format!("{root}/{rel}");
    let dune = f("Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv");
    let matrix = f("Movies/The Matrix 1999.mkv");
    let e1 = f("Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv");
    let e2 = f("Shows/Dark/Season 1/Dark.S01E02.Lies.1080p.mkv");
    db.execute_batch(&format!(
        "INSERT INTO items (id, kind, source_id, title, year, file_path, file_size, tmdb_id, locked, added_at)
         VALUES ('vdune', 'movie', 'src_1', 'Dune', 2021, '{dune}', 150000, 438631, 0, 10),
                ('vmatrix', 'movie', 'src_1', 'Матрица', 1999, '{matrix}', 150000, NULL, 1, 11),
                ('show:dark', 'show', 'src_1', 'Dark', 2017, '{root}/Shows/Dark', NULL, 70523, 0, 12),
                ('ve1', 'episode', 'src_1', 'Secrets', NULL, '{e1}', 150000, NULL, 0, 13),
                ('ve2', 'episode', 'src_1', 'Lies', NULL, '{e2}', 150000, NULL, 0, 14),
                ('vsmb', 'movie', 'src_2', 'Far Away', NULL, '/run/user/1/gvfs/x.mkv', 1, NULL, 0, 15);
         UPDATE items SET show_id = 'show:dark', season = 1, episode = 1 WHERE id = 've1';
         UPDATE items SET show_id = 'show:dark', season = 1, episode = 2 WHERE id = 've2';
         INSERT INTO progress VALUES ('vdune', 1000000, 9000000, 100, 0),
                                     ('vmatrix', 3000000, 8000000, 200, 0),
                                     ('ve1', 2900000, 3000000, 300, 1),
                                     ('vsmb', 10, 100, 400, 0);"
    ))
    .unwrap();
    path
}

#[test]
fn v0_import_moves_items_and_progress_once() {
    let dir = tempfile::tempdir().unwrap();
    let media_root = dir.path().join("media");
    library_folder(&media_root);
    let v0 = v0_fixture(dir.path(), &media_root);
    assert!(pa_catalog::is_legacy_v0(&v0).unwrap());
    assert!(
        Catalog::open(&v0).is_err(),
        "v0 file must not be migrated in place"
    );

    let cat = Catalog::open(dir.path().join("kinolune-core.db")).unwrap();
    let report = cat.import_v0(&v0).unwrap();
    assert_eq!(report.sources, 2);
    assert_eq!(report.files, 4);
    assert_eq!(report.matched, 4);
    assert_eq!(report.progress, 3);
    assert_eq!(report.progress_skipped, 1);
    assert_eq!(report.items_skipped, 1);

    let movies = cat.movies().unwrap();
    let titles: Vec<_> = movies
        .iter()
        .map(|m| (m.title.as_str(), m.tmdb_id))
        .collect();
    assert_eq!(titles, vec![("Dune", Some(438631)), ("Матрица", None)]);
    let show = cat.shows().unwrap();
    assert_eq!(
        (show[0].title.as_str(), show[0].tmdb_id),
        ("Dark", Some(70523))
    );
    let eps = cat.episodes(show[0].id).unwrap();
    assert_eq!(eps.len(), 2);

    // Progress of recognized items is under TMDB keys, others under file keys.
    let dune = cat.progress(movies[0].id).unwrap().unwrap();
    assert_eq!(dune.position_ms, 1_000_000);
    let matrix = cat.progress(movies[1].id).unwrap().unwrap();
    assert_eq!(matrix.position_ms, 3_000_000);
    assert!(cat.progress(eps[0].id).unwrap().unwrap().watched);
    let row = cat.continue_watching(50).unwrap();
    let ids: Vec<_> = row.iter().map(|c| c.item.id).collect();
    assert_eq!(ids, vec![eps[1].id, movies[1].id, movies[0].id]);

    // A second import changes nothing; the v0 file is untouched.
    let again = cat.import_v0(&v0).unwrap();
    assert_eq!((again.matched, again.progress), (0, 0));
    assert_eq!(cat.movies().unwrap().len(), 2);
    assert_eq!(cat.episodes(show[0].id).unwrap().len(), 2);
    assert!(pa_catalog::is_legacy_v0(&v0).unwrap());

    // The next scan keeps the imported matches (a locked one too).
    scan_all(&cat);
    let titles: Vec<_> = cat.movies().unwrap().into_iter().map(|m| m.title).collect();
    assert_eq!(titles, vec!["Dune", "Матрица"]);
    assert_eq!(cat.episodes(show[0].id).unwrap().len(), 3);
}
