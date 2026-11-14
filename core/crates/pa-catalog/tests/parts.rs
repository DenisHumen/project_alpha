// SPDX-License-Identifier: MPL-2.0
//! LIB-020: a movie split over several files (`cd1`/`cd2`, the VOBs of a
//! DVD title set) plays all its parts in order; other files play alone.
//! Synthetic files (bytes only); the player side is checked in the app and
//! on real libmpv (`pa_player/test/edl_libmpv_test.dart`).

use std::path::Path;

use pa_catalog::{Catalog, PlayPart};
use pa_scanner::CancelToken;

fn write(root: &Path, rel: &str, size: usize, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..size)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

const CD1: &str = "Movies/Heat (1995)/Heat.1995.DVDRip.cd1.avi";
const CD2: &str = "Movies/Heat (1995)/Heat.1995.DVDRip.cd2.avi";
const DVD: &str = "Movies/Alien (1979)/VIDEO_TS";
const ONE: &str = "Movies/Arrival (2016)/Arrival.2016.1080p.mkv";
const ISO: &str = "Movies/Spirited Away (2001).iso";

fn setup() -> (tempfile::TempDir, std::path::PathBuf, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    write(&root, CD1, 120_000, 1);
    write(&root, CD2, 110_000, 2);
    write(&root, &format!("{DVD}/VIDEO_TS.IFO"), 200, 3);
    write(&root, &format!("{DVD}/VIDEO_TS.VOB"), 9_000, 4);
    write(&root, &format!("{DVD}/VTS_01_0.VOB"), 5_000, 5);
    write(&root, &format!("{DVD}/VTS_01_1.VOB"), 160_000, 6);
    write(&root, &format!("{DVD}/VTS_01_2.VOB"), 90_000, 7);
    write(&root, &format!("{DVD}/VTS_02_1.VOB"), 30_000, 8);
    write(&root, ONE, 150_000, 9);
    write(&root, ISO, 70_000, 10);
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    (dir, root, cat)
}

fn paths(parts: &[PlayPart]) -> Vec<&str> {
    parts.iter().map(|p| p.rel_path.as_str()).collect()
}

#[test]
fn split_movie_and_dvd_title_set_play_all_parts_in_order() {
    let (_dir, root, cat) = setup();
    let movies = cat.movies().unwrap();
    let movie = |title: &str| {
        movies
            .iter()
            .find(|m| m.title == title)
            .unwrap_or_else(|| panic!("no {title} in {movies:#?}"))
            .clone()
    };

    // cd1/cd2: one movie, one version, the item points at part 1.
    let heat = movie("Heat");
    assert_eq!(heat.rel_path, CD1);
    assert_eq!(cat.versions(heat.id).unwrap().len(), 1);
    let parts = cat.play_parts(heat.source_id, &heat.rel_path).unwrap();
    assert_eq!(paths(&parts), [CD1, CD2]);
    assert_eq!(
        parts[1].local_path.as_deref(),
        Some(root.join(CD2).to_str().unwrap())
    );
    assert_eq!((parts[0].size_bytes, parts[1].size_bytes), (120_000, 110_000));
    // From a later part: still the whole movie from part 1.
    let from_two = cat.play_parts(heat.source_id, CD2).unwrap();
    assert_eq!(paths(&from_two), [CD1, CD2]);
    // A leading slash is accepted like in `files.path`.
    assert_eq!(
        cat.play_parts(heat.source_id, &format!("/{CD1}")).unwrap(),
        parts
    );

    // DVD: the VOBs of the main title set, without menus and other sets.
    let alien = movie("Alien");
    let parts = cat.play_parts(alien.source_id, &alien.rel_path).unwrap();
    assert_eq!(
        paths(&parts),
        [
            format!("{DVD}/VTS_01_1.VOB").as_str(),
            &format!("{DVD}/VTS_01_2.VOB")
        ]
    );

    // A single file and a disc image play alone.
    let arrival = movie("Arrival");
    assert_eq!(
        paths(&cat.play_parts(arrival.source_id, &arrival.rel_path).unwrap()),
        [ONE]
    );
    let iso = movie("Spirited Away");
    assert_eq!(
        paths(&cat.play_parts(iso.source_id, &iso.rel_path).unwrap()),
        [ISO]
    );

    // Unknown files and sources: nothing to play.
    assert!(cat.play_parts(heat.source_id, "nope.mkv").unwrap().is_empty());
    assert!(cat.play_parts(999, CD1).unwrap().is_empty());
}

#[test]
fn a_missing_part_is_left_out() {
    let (_dir, root, cat) = setup();
    std::fs::remove_file(root.join(CD2)).unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    let heat = cat
        .movies()
        .unwrap()
        .into_iter()
        .find(|m| m.title == "Heat")
        .unwrap();
    let parts = cat.play_parts(heat.source_id, &heat.rel_path).unwrap();
    assert_eq!(paths(&parts), [CD1]);
}
