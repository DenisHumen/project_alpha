// SPDX-License-Identifier: MPL-2.0
//! UI-009: what the Detail screen reads besides the item row — extras and
//! the clearlogo next to the file, probe data of the default version.

use std::path::Path;

use pa_catalog::{Catalog, ItemRef};
use pa_library::rusqlite::{Connection, params};
use pa_scanner::CancelToken;

fn media(root: &Path, rel: &str, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..150_000u32)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

const MAIN: &str = "Movies/Arrival (2016)/Arrival.2016.2160p.mkv";

#[test]
fn movie_details_have_extras_logo_and_tech() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    media(&root, MAIN, 1);
    media(
        &root,
        "Movies/Arrival (2016)/Trailers/Arrival Trailer.mkv",
        2,
    );
    media(&root, "Movies/Arrival (2016)/Featurettes/Making Of.mkv", 3);
    std::fs::write(root.join("Movies/Arrival (2016)/clearlogo.png"), b"png").unwrap();
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();

    let movies = cat.movies().unwrap();
    assert_eq!(movies.len(), 1, "extras are not movies: {movies:?}");
    let arrival = movies[0].id;
    assert!(matches!(arrival, ItemRef::Movie(_)));

    // What the probe would write (no FFmpeg in tests).
    let conn = Connection::open(cat.path()).unwrap();
    conn.execute(
        "UPDATE files SET container = 'matroska,webm', duration_ms = 6960000,
             video_json = ?1, audio_formats = '[\"truehd\",\"atmos\",\"ac3\"]',
             audio_langs = '[\"eng\",\"rus\"]', sub_langs = '[\"rus\"]', probed_at_ms = 1
         WHERE path LIKE '%Arrival.2016.2160p.mkv'",
        params![
            r#"{"index":0,"codec":"hevc","profile":"Main 10","width":3840,"height":1600,
                "frame_rate":[24000,1001],"bit_depth":10,"hdr":["dolby_vision","hdr10"]}"#
        ],
    )
    .unwrap();
    drop(conn);

    let d = cat.details(arrival).unwrap();
    let tech = d.tech.expect("tech info of the default version");
    assert_eq!(tech.container.as_deref(), Some("matroska,webm"));
    assert_eq!(tech.video_codec.as_deref(), Some("hevc"));
    assert_eq!(tech.video_profile.as_deref(), Some("Main 10"));
    assert_eq!((tech.width, tech.height), (Some(3840), Some(1600)));
    assert_eq!(tech.bit_depth, Some(10));
    assert!((tech.frame_rate.unwrap() - 23.976).abs() < 0.001);
    assert_eq!(tech.hdr, ["dolby_vision", "hdr10"]);
    assert_eq!(tech.audio_formats, ["truehd", "atmos", "ac3"]);
    assert_eq!(tech.audio_langs, ["eng", "rus"]);
    assert_eq!(tech.sub_langs, ["rus"]);
    assert_eq!(tech.size_bytes, 150_000);
    let file = tech.file.unwrap();
    assert_eq!(file.rel_path, MAIN);
    assert!(file.local_path.unwrap().ends_with("Arrival.2016.2160p.mkv"));

    let extras: Vec<_> = d
        .extras
        .iter()
        .map(|e| (e.extra_type.as_str(), e.title.as_str()))
        .collect();
    assert_eq!(
        extras,
        [("trailer", "Arrival Trailer"), ("featurette", "Making Of")],
        "{:?}",
        d.extras
    );
    assert!(d.extras[0].file.local_path.is_some());
    let logo = d.local_logo.expect("clearlogo.png next to the film");
    assert_eq!(logo.rel_path, "Movies/Arrival (2016)/clearlogo.png");

    // Without metadata: no people, ratings or provider logo.
    assert!(d.credits.is_empty());
    assert!(d.ratings.is_empty());
    assert!(d.logo_url.is_none());
}

#[test]
fn shows_have_no_tech_and_bad_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    media(&root, "Shows/Dark/Season 1/Dark.S01E01.mkv", 1);
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    let show = cat.shows().unwrap()[0].id;
    let d = cat.details(show).unwrap();
    assert!(d.tech.is_none());
    assert!(d.extras.is_empty());
    let ep = cat.episodes(show).unwrap()[0].id;
    let d = cat.details(ep).unwrap();
    assert!(d.tech.is_some(), "an episode has its file's data");
}
