// SPDX-License-Identifier: MPL-2.0
//! LIB-017 "done when": a Blu-ray folder, a DVD folder and a disc image are
//! scanned into the library as one movie each, get provider metadata and
//! play their main title. Synthetic files (a few KiB of bytes, no real disc
//! data) and synthetic provider responses; no network.

use std::path::Path;
use std::sync::Arc;

use pa_catalog::pa_metadata::cache::MemoryCache;
use pa_catalog::pa_metadata::clock::ManualClock;
use pa_catalog::pa_metadata::http::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use pa_catalog::pa_metadata::key::Keys;
use pa_catalog::pa_metadata::tmdb::{TmdbClient, TmdbConfig};
use pa_catalog::{
    Catalog, ItemRef, MetadataOptions, MetadataProviders, ScanPhase, ScanUpdate, Unplayable,
};
use pa_library::rusqlite::Connection;
use pa_library::search::{SearchKind, SearchOptions};
use pa_scanner::CancelToken;

/// Writes `size` bytes unique to `seed` at `rel` under `root`.
fn write(root: &Path, rel: &str, size: usize, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..size)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

const BD: &str = "Movies/Inception (2010)";
const DVD: &str = "Movies/Alien (1979)";
const ISO: &str = "Movies/Spirited Away (2001).iso";

fn library_folder(root: &Path) {
    // Blu-ray folder: the feature is the largest stream; a menu clip and a
    // bonus clip around it; playlists, backup and certificate are not
    // listed.
    write(root, &format!("{BD}/BDMV/index.bdmv"), 120, 1);
    write(root, &format!("{BD}/BDMV/MovieObject.bdmv"), 120, 2);
    write(root, &format!("{BD}/BDMV/PLAYLIST/00800.mpls"), 300, 3);
    write(root, &format!("{BD}/BDMV/CLIPINF/00001.clpi"), 300, 4);
    write(root, &format!("{BD}/BDMV/BACKUP/index.bdmv"), 120, 5);
    write(root, &format!("{BD}/BDMV/STREAM/00000.m2ts"), 12_000, 6);
    write(root, &format!("{BD}/BDMV/STREAM/00001.m2ts"), 180_000, 7);
    write(root, &format!("{BD}/BDMV/STREAM/00002.m2ts"), 40_000, 8);
    write(root, &format!("{BD}/CERTIFICATE/id.bdmv"), 64, 9);
    write(root, &format!("{BD}/poster.jpg"), 500, 10);
    // DVD folder: title set 1 is the feature in two VOBs, set 2 a bonus.
    write(root, &format!("{DVD}/VIDEO_TS/VIDEO_TS.IFO"), 200, 11);
    write(root, &format!("{DVD}/VIDEO_TS/VIDEO_TS.VOB"), 9_000, 12);
    write(root, &format!("{DVD}/VIDEO_TS/VTS_01_0.IFO"), 200, 13);
    write(root, &format!("{DVD}/VIDEO_TS/VTS_01_0.VOB"), 5_000, 14);
    write(root, &format!("{DVD}/VIDEO_TS/VTS_01_1.VOB"), 160_000, 15);
    write(root, &format!("{DVD}/VIDEO_TS/VTS_01_2.VOB"), 90_000, 16);
    write(root, &format!("{DVD}/VIDEO_TS/VTS_02_1.VOB"), 30_000, 17);
    write(root, &format!("{DVD}/AUDIO_TS/.keep"), 1, 18);
    // Disc image.
    write(root, ISO, 70_000, 19);
    // An ordinary movie next to them.
    write(root, "Movies/Heat (1995)/Heat.1995.1080p.mkv", 150_000, 20);
}

/// TMDB answers by path and query; everything else is 404.
struct Tmdb;

fn ok(body: &str) -> Result<HttpResponse, TransportError> {
    Ok(HttpResponse {
        status: 200,
        headers: vec![],
        body: body.to_owned(),
    })
}

impl HttpTransport for Tmdb {
    fn get(&self, r: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let query = r
            .query
            .iter()
            .find(|(k, _)| k == "query")
            .map_or("", |(_, v)| v.as_str());
        let search = |id: u32, title: &str, date: &str| {
            format!(
                r#"{{"page":1,"results":[{{"id":{id},"title":"{title}","original_title":"{title}",
                    "release_date":"{date}","popularity":50}}],"total_pages":1,"total_results":1}}"#
            )
        };
        let details = |id: u32, title: &str, date: &str, overview: &str| {
            format!(
                r#"{{"id":{id},"title":"{title}","original_title":"{title}","original_language":"en",
                    "overview":"{overview}","release_date":"{date}","status":"Released",
                    "runtime":120,"poster_path":"/p{id}.jpg","backdrop_path":"/b{id}.jpg"}}"#
            )
        };
        match (r.path.as_str(), query) {
            ("/search/movie", "Inception") => ok(&search(27205, "Inception", "2010-07-15")),
            ("/search/movie", "Alien") => ok(&search(348, "Alien", "1979-05-25")),
            ("/search/movie", "Spirited Away") => ok(&search(129, "Spirited Away", "2001-07-20")),
            ("/search/movie", "Heat") => ok(&search(949, "Heat", "1995-12-15")),
            ("/movie/27205", _) => ok(&details(27205, "Inception", "2010-07-15", "Dreams.")),
            ("/movie/348", _) => ok(&details(348, "Alien", "1979-05-25", "In space.")),
            ("/movie/129", _) => ok(&details(129, "Spirited Away", "2001-07-20", "Spirits.")),
            ("/movie/949", _) => ok(&details(949, "Heat", "1995-12-15", "A heist.")),
            ("/search/movie", _) => {
                ok(r#"{"page":1,"results":[],"total_pages":0,"total_results":0}"#)
            }
            (other, _) => Ok(HttpResponse {
                status: 404,
                headers: vec![],
                body: format!(r#"{{"status_message":"no fixture for {other}"}}"#),
            }),
        }
    }

    fn post_json(&self, _: &HttpRequest, _: &str) -> Result<HttpResponse, TransportError> {
        Err(TransportError("no POST in this test".into()))
    }
}

fn providers() -> MetadataProviders {
    let mut config = TmdbConfig::default();
    config.retry.max_retries = 0;
    MetadataProviders {
        tmdb: Some(Arc::new(TmdbClient::new(
            config,
            Keys::none().with_user_key(Some("test-key")),
            Arc::new(Tmdb),
            Arc::new(MemoryCache::default()),
            Arc::new(ManualClock::at_unix(1_800_000_000)),
        ))),
        anilist: None,
        shikimori: None,
    }
}

fn scan(cat: &Catalog) -> Vec<ScanUpdate> {
    let mut updates = Vec::new();
    cat.scan(None, &CancelToken::new(), &mut |u| updates.push(u))
        .unwrap();
    updates
}

#[test]
fn disc_folders_and_images_are_one_movie_each_with_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    library_folder(&root);
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();

    let updates = scan(&cat);
    assert_eq!(updates.last().unwrap().phase, ScanPhase::Done);
    assert!(updates.iter().all(|u| u.error.is_none()), "{updates:?}");

    // The scan recorded the disc streams and the image in `files`.
    let db = Connection::open(dir.path().join("library.db")).unwrap();
    let mut recorded: Vec<String> = db
        .prepare("SELECT path FROM files ORDER BY path")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    recorded.retain(|p| !p.ends_with(".mkv"));
    assert_eq!(
        recorded,
        [
            format!("/{DVD}/VIDEO_TS/VIDEO_TS.VOB"),
            format!("/{DVD}/VIDEO_TS/VTS_01_0.VOB"),
            format!("/{DVD}/VIDEO_TS/VTS_01_1.VOB"),
            format!("/{DVD}/VIDEO_TS/VTS_01_2.VOB"),
            format!("/{DVD}/VIDEO_TS/VTS_02_1.VOB"),
            format!("/{BD}/BDMV/STREAM/00000.m2ts"),
            format!("/{BD}/BDMV/STREAM/00001.m2ts"),
            format!("/{BD}/BDMV/STREAM/00002.m2ts"),
            format!("/{ISO}"),
        ]
    );

    // One movie per disc, named by the disc folder or image.
    let movies = cat.movies().unwrap();
    let titles: Vec<_> = movies.iter().map(|m| (m.title.as_str(), m.year)).collect();
    assert_eq!(
        titles,
        [
            ("Alien", Some(1979)),
            ("Heat", Some(1995)),
            ("Inception", Some(2010)),
            ("Spirited Away", Some(2001)),
        ],
        "{movies:#?}"
    );
    let movie = |title: &str| movies.iter().find(|m| m.title == title).unwrap().clone();

    // Blu-ray: the largest stream plays, directly from the local folder.
    let bd = movie("Inception");
    let bd_main = format!("{BD}/BDMV/STREAM/00001.m2ts");
    assert_eq!(bd.rel_path, bd_main);
    assert_eq!(
        bd.local_path.as_deref(),
        Some(root.join(&bd_main).to_str().unwrap())
    );
    assert!(Path::new(bd.local_path.as_deref().unwrap()).is_file());
    assert_eq!((bd.version_count, bd.unplayable), (1, None));
    assert_eq!(bd.local_poster.as_deref(), Some(&*format!("{BD}/poster.jpg")));

    // DVD: the first VOB of the largest title set; the set is one version.
    let dvd = movie("Alien");
    assert_eq!(dvd.rel_path, format!("{DVD}/VIDEO_TS/VTS_01_1.VOB"));
    assert_eq!((dvd.version_count, dvd.unplayable), (1, None));
    let versions = cat.versions(dvd.id).unwrap();
    assert_eq!(versions.len(), 1, "{versions:?}");
    assert_eq!(versions[0].rel_path, dvd.rel_path);

    // Disc image: one movie, marked unplayable for the LGPL player build.
    let iso = movie("Spirited Away");
    assert_eq!(iso.rel_path, ISO);
    assert_eq!(iso.unplayable, Some(Unplayable::DiscImage));
    assert_eq!(iso.unplayable.unwrap().code(), "disc_image");
    let versions = cat.versions(iso.id).unwrap();
    assert_eq!(versions[0].unplayable(), Some(Unplayable::DiscImage));
    assert_eq!(movie("Heat").unplayable, None);

    // The other streams of the discs are no items and no search hits.
    let matched: i64 = db
        .query_row(
            "SELECT count(*) FROM matches m JOIN files f ON f.id = m.file_id
             WHERE f.path LIKE '%.m2ts' OR f.path LIKE '%.VOB' OR f.path LIKE '%.iso'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(matched, 4, "00001.m2ts, VTS_01_1/2.VOB and the image");
    let videos = |text: &str| {
        let opts = SearchOptions {
            kinds: vec![SearchKind::Video],
            ..SearchOptions::default()
        };
        pa_library::search::search(&db, text, &opts).unwrap()
    };
    for leftover in ["00000", "00002", "VIDEO_TS", "VTS_02_1"] {
        assert!(videos(leftover).is_empty(), "{leftover}");
    }
    let found = cat.search("Inception", 10).unwrap();
    assert_eq!(
        found.iter().map(|i| i.id).collect::<Vec<_>>(),
        [bd.id],
        "{found:#?}"
    );

    // Provider metadata reaches the disc movies like any other.
    let report = cat
        .fetch_metadata(
            &providers(),
            &MetadataOptions::default(),
            &CancelToken::new(),
            &mut |_, _| {},
        )
        .unwrap();
    assert!(report.error.is_none(), "{report:?}");
    let movies = cat.movies().unwrap();
    let meta = |title: &str| {
        let m = movies.iter().find(|m| m.title == title).unwrap();
        (m.tmdb_id, m.overview.clone(), m.poster_url.clone())
    };
    for (title, id, overview) in [
        ("Inception", 27205, "Dreams."),
        ("Alien", 348, "In space."),
        ("Spirited Away", 129, "Spirits."),
    ] {
        let (tmdb, text, poster) = meta(title);
        assert_eq!(tmdb, Some(id), "{title}");
        assert_eq!(text.as_deref(), Some(overview), "{title}");
        assert_eq!(
            poster,
            Some(format!("https://image.tmdb.org/t/p/w500/p{id}.jpg")),
            "{title}"
        );
    }
    assert_eq!(movies.len(), 4);

    // A rescan changes nothing: still one movie per disc, same main title.
    scan(&cat);
    let again = cat.movies().unwrap();
    assert_eq!(again.len(), 4);
    assert!(matches!(bd.id, ItemRef::Movie(_)));
    assert_eq!(cat.item(bd.id).unwrap().unwrap().rel_path, bd_main);
}
