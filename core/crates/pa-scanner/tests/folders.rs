// SPDX-License-Identifier: MPL-2.0
//! `analyze_folders` (LIB-007) on a real folder tree: groups, members and
//! assets in the library, NFO ids, durations from `files`, idempotent reruns.

use std::fs;
use std::path::Path;

use pa_library::groups::{
    ArtKind, DiscUnit, ExtraType, GroupKind, Role, group_of_file, source_groups,
};
use pa_library::rusqlite::Connection;
use pa_scanner::{
    CancelToken, FolderOptions, ScanOptions, analyze_folders, scan, structure::StructureOptions,
};
use pa_vfs::LocalFs;

fn write(root: &Path, rel: &str, body: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn library() -> Connection {
    let conn = pa_library::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO sources (id, kind, name, uri, created_at_ms) VALUES (1, 'local', 'Media', 'file:///media', 0)",
        [],
    )
    .unwrap();
    conn
}

fn tree(root: &Path) {
    // A10: movie with extras, sample, sidecars and art.
    write(root, "Movies/The Matrix (1999)/The Matrix (1999).mkv", &[0; 4000]);
    write(root, "Movies/The Matrix (1999)/The Matrix (1999)-trailer.mkv", b"t");
    write(root, "Movies/The Matrix (1999)/sample.mkv", b"s");
    write(root, "Movies/The Matrix (1999)/Featurettes/Bullet Time.mkv", b"f");
    write(root, "Movies/The Matrix (1999)/Making of.mkv", b"m");
    write(root, "Movies/The Matrix (1999)/The Matrix (1999).ru.forced.srt", b"1");
    write(
        root,
        "Movies/The Matrix (1999)/The Matrix (1999).nfo",
        br#"<movie><uniqueid type="tmdb">603</uniqueid><uniqueid type="imdb">tt0133093</uniqueid></movie>"#,
    );
    write(root, "Movies/The Matrix (1999)/poster.jpg", b"p");
    write(root, "Movies/The Matrix (1999)/fanart.jpg", b"f");
    write(root, "Movies/The Matrix (1999)/clearlogo.png", b"l");
    // A12: Blu-ray folder and an ISO.
    write(root, "Movies/Inception (2010)/BDMV/index.bdmv", b"i");
    write(root, "Movies/Inception (2010)/BDMV/BACKUP/index.bdmv", b"i");
    write(root, "Movies/Inception (2010)/BDMV/STREAM/00000.m2ts", b"menu");
    write(root, "Movies/Inception (2010)/BDMV/STREAM/00001.m2ts", &[0; 2000]);
    write(root, "Movies/Inception (2010)/CERTIFICATE/id.bdmv", b"c");
    write(root, "Images/Spirited Away (2001).iso", b"iso");
    // A13: parts.
    write(root, "Movies/Dune (1984)/Dune (1984) - part1.mkv", b"1");
    write(root, "Movies/Dune (1984)/Dune (1984) - part2.mkv", b"2");
    // Music is not a group.
    write(root, "Music/Artist/Album/cover.jpg", b"c");
    write(root, "Music/Artist/Album/01.flac", b"a");
}

fn opts(now: i64) -> FolderOptions {
    FolderOptions {
        threads: 2,
        now_ms: Some(now),
        ..FolderOptions::default()
    }
}

#[test]
fn groups_are_stored_and_reruns_write_nothing() {
    let dir = tempfile::tempdir().unwrap();
    tree(dir.path());
    let vfs = LocalFs::new(dir.path()).unwrap();
    let mut conn = library();
    let cancel = CancelToken::new();
    scan(&mut conn, 1, &vfs, &ScanOptions::default(), &cancel, &mut |_| {}).unwrap();
    // Durations as pa-probe would store them.
    conn.execute_batch(
        "UPDATE files SET duration_ms = 8160000 WHERE path = '/Movies/The Matrix (1999)/The Matrix (1999).mkv';
         UPDATE files SET duration_ms = 600000 WHERE path = '/Movies/The Matrix (1999)/Making of.mkv';",
    )
    .unwrap();

    let report = analyze_folders(&mut conn, 1, &vfs, &opts(10), &cancel, &mut |_| {}).unwrap();
    assert!(!report.cancelled);
    assert_eq!(report.nfo_read, 1);
    let groups = source_groups(&conn, 1).unwrap();
    let roots: Vec<_> = groups.iter().map(|g| g.group.root.as_str()).collect();
    assert_eq!(
        roots,
        [
            "/Images/Spirited Away (2001).iso",
            "/Movies/Dune (1984)",
            "/Movies/Inception (2010)",
            "/Movies/The Matrix (1999)",
        ]
    );
    assert_eq!(report.written, 4);

    let by_root = |root: &str| {
        groups
            .iter()
            .find(|g| g.group.root == root)
            .unwrap()
            .group
            .clone()
    };
    let matrix = by_root("/Movies/The Matrix (1999)");
    assert_eq!(matrix.kind, GroupKind::MovieFolder);
    let role = |path: &str| matrix.entry(path).unwrap_or_else(|| panic!("{path}")).clone();
    let main = role("/Movies/The Matrix (1999)/The Matrix (1999).mkv");
    assert_eq!(main.role, Role::Main);
    assert!(main.file_id.is_some(), "main links to its files row");
    let trailer = role("/Movies/The Matrix (1999)/The Matrix (1999)-trailer.mkv");
    assert_eq!((trailer.role, trailer.extra_type), (Role::Extra, Some(ExtraType::Trailer)));
    assert_eq!(role("/Movies/The Matrix (1999)/sample.mkv").role, Role::Sample);
    let feat = role("/Movies/The Matrix (1999)/Featurettes/Bullet Time.mkv");
    assert_eq!(feat.extra_type, Some(ExtraType::Featurette));
    // Decided by duration (10 min against 2 h 16 min); the scan recorded it,
    // so it is a member with its files row.
    let making = role("/Movies/The Matrix (1999)/Making of.mkv");
    assert_eq!((making.role, making.extra_type), (Role::Extra, Some(ExtraType::Other)));
    assert!(making.file_id.is_some());
    let sub = role("/Movies/The Matrix (1999)/The Matrix (1999).ru.forced.srt");
    assert_eq!(sub.role, Role::Subtitle);
    let info = sub.subtitle.unwrap();
    assert_eq!((info.lang.as_deref(), info.forced), (Some("rus"), true));
    assert_eq!(sub.target.as_deref(), Some(main.path.as_str()));
    let nfo = role("/Movies/The Matrix (1999)/The Matrix (1999).nfo");
    assert_eq!(nfo.ids.get("tmdb").map(String::as_str), Some("movie:603"));
    assert_eq!(nfo.ids.get("imdb").map(String::as_str), Some("tt0133093"));
    for (path, kind) in [
        ("poster.jpg", ArtKind::Poster),
        ("fanart.jpg", ArtKind::Fanart),
        ("clearlogo.png", ArtKind::Clearlogo),
    ] {
        let e = role(&format!("/Movies/The Matrix (1999)/{path}"));
        assert_eq!((e.role, e.art), (Role::Art, Some(kind)), "{path}");
    }

    let disc = by_root("/Movies/Inception (2010)");
    assert_eq!((disc.kind, disc.disc), (GroupKind::Disc, Some(DiscUnit::Bdmv)));
    let titles: Vec<_> = disc.playable().map(|e| e.path.as_str()).collect();
    assert_eq!(titles, ["/Movies/Inception (2010)/BDMV/STREAM/00001.m2ts"]);
    assert_eq!(by_root("/Images/Spirited Away (2001).iso").disc, Some(DiscUnit::Iso));

    let dune = by_root("/Movies/Dune (1984)");
    let parts: Vec<_> = dune.playable().map(|e| (e.role, e.part)).collect();
    assert_eq!(parts, [(Role::Part, Some(1)), (Role::Part, Some(2))]);
    let part1: i64 = conn
        .query_row(
            "SELECT id FROM files WHERE path = '/Movies/Dune (1984)/Dune (1984) - part1.mkv'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        group_of_file(&conn, part1).unwrap().unwrap().group.root,
        "/Movies/Dune (1984)"
    );

    // Rerun on the same tree: nothing written.
    let again = analyze_folders(&mut conn, 1, &vfs, &opts(20), &cancel, &mut |_| {}).unwrap();
    assert_eq!((again.written, again.unchanged, again.removed), (0, 4, 0));
    let stamp: i64 = conn
        .query_row("SELECT max(analyzed_at_ms) FROM file_groups", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stamp, 10);

    // A change rewrites only its group; a removed folder loses its group.
    write(dir.path(), "Movies/Dune (1984)/Dune (1984) - part3.mkv", b"3");
    fs::remove_dir_all(dir.path().join("Movies/Inception (2010)")).unwrap();
    let third = analyze_folders(&mut conn, 1, &vfs, &opts(30), &cancel, &mut |_| {}).unwrap();
    assert_eq!((third.written, third.unchanged, third.removed), (1, 2, 1));
}

#[test]
fn duration_rule_can_be_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    tree(dir.path());
    let vfs = LocalFs::new(dir.path()).unwrap();
    let mut conn = library();
    let cancel = CancelToken::new();
    scan(&mut conn, 1, &vfs, &ScanOptions::default(), &cancel, &mut |_| {}).unwrap();
    conn.execute_batch(
        "UPDATE files SET duration_ms = 8160000 WHERE path = '/Movies/The Matrix (1999)/The Matrix (1999).mkv';
         UPDATE files SET duration_ms = 600000 WHERE path = '/Movies/The Matrix (1999)/Making of.mkv';",
    )
    .unwrap();
    let options = FolderOptions {
        structure: StructureOptions { short_ratio: None },
        ..opts(1)
    };
    analyze_folders(&mut conn, 1, &vfs, &options, &cancel, &mut |_| {}).unwrap();
    let groups = source_groups(&conn, 1).unwrap();
    let matrix = &groups
        .iter()
        .find(|g| g.group.root == "/Movies/The Matrix (1999)")
        .unwrap()
        .group;
    assert_eq!(
        matrix
            .entry("/Movies/The Matrix (1999)/Making of.mkv")
            .unwrap()
            .role,
        Role::Main
    );
    assert_eq!(matrix.kind, GroupKind::Loose);
}

#[test]
fn cancelled_run_writes_nothing_and_unknown_source_fails() {
    let dir = tempfile::tempdir().unwrap();
    tree(dir.path());
    let vfs = LocalFs::new(dir.path()).unwrap();
    let mut conn = library();
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = analyze_folders(&mut conn, 1, &vfs, &opts(1), &cancel, &mut |_| {}).unwrap();
    assert!(r.cancelled);
    assert!(source_groups(&conn, 1).unwrap().is_empty());
    assert!(analyze_folders(&mut conn, 9, &vfs, &opts(1), &CancelToken::new(), &mut |_| {}).is_err());
}
