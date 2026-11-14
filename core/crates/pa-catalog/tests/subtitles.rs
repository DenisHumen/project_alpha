// SPDX-License-Identifier: MPL-2.0
//! Sidecar subtitles of a played file (PLAY-007) from the LIB-007 groups.

use std::path::Path;

use pa_catalog::{Catalog, Sidecar};
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

fn subtitle(root: &Path, rel: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();
}

fn short(s: &Sidecar) -> (&str, Option<&str>, bool, bool) {
    (
        s.rel_path.as_str(),
        s.lang.as_deref(),
        s.forced,
        s.hearing_impaired,
    )
}

#[test]
fn sidecars_of_movies_and_episodes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    media(&root, "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv", 1);
    subtitle(&root, "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.ru.forced.srt");
    subtitle(&root, "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.en.sdh.srt");
    subtitle(&root, "Movies/Dune (2021)/Subs/English.srt");
    media(&root, "Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv", 3);
    media(&root, "Shows/Dark/Season 1/Dark.S01E02.Lies.1080p.mkv", 4);
    subtitle(&root, "Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.rus.srt");
    subtitle(&root, "Shows/Dark/Season 1/Dark.S01E02.Lies.1080p.eng.default.ass");
    // Not named after an episode: nothing says which one it is for.
    subtitle(&root, "Shows/Dark/Season 1/whatever.srt");

    let cat = Catalog::open(dir.path().join("db/library.db")).unwrap();
    let source = cat
        .add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();

    let movie = &cat.movies().unwrap()[0];
    let subs = cat
        .sidecar_subtitles(movie.source_id, &movie.rel_path)
        .unwrap();
    let got: Vec<_> = subs.iter().map(short).collect();
    assert_eq!(
        got,
        [
            (
                "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.en.sdh.srt",
                Some("eng"),
                false,
                true
            ),
            (
                "Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.ru.forced.srt",
                Some("rus"),
                true,
                false
            ),
            ("Movies/Dune (2021)/Subs/English.srt", Some("eng"), false, false),
        ],
        "{subs:?}"
    );
    // Local sources carry the absolute path; the leading slash is optional.
    let local = root.join("Movies/Dune (2021)/Subs/English.srt");
    assert_eq!(subs[2].local_path.as_deref(), local.to_str());
    assert_eq!(
        cat.sidecar_subtitles(source.id, &format!("/{}", movie.rel_path))
            .unwrap(),
        subs
    );

    let show = &cat.shows().unwrap()[0];
    let episodes = cat.episodes(show.id).unwrap();
    let of = |n: i64| {
        let e = episodes.iter().find(|e| e.episode == Some(n)).unwrap();
        cat.sidecar_subtitles(e.source_id, &e.rel_path).unwrap()
    };
    let first = of(1);
    assert_eq!(
        first.iter().map(short).collect::<Vec<_>>(),
        [(
            "Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.rus.srt",
            Some("rus"),
            false,
            false
        )]
    );
    let second = of(2);
    assert_eq!(second.len(), 1, "{second:?}");
    assert_eq!(second[0].lang.as_deref(), Some("eng"));
    assert!(second[0].default);

    assert!(cat.sidecar_subtitles(source.id, "nope.mkv").unwrap().is_empty());
    assert!(
        cat.sidecar_subtitles(source.id + 1, &movie.rel_path)
            .unwrap()
            .is_empty()
    );
}
