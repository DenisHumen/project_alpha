// SPDX-License-Identifier: MPL-2.0
//! Library queries (LIB-009): filter combinations, sorts, paging, alphabet
//! index, facets, folder browsing, view preferences, smart shelves and the
//! 20 000-item response time.

use std::time::{Duration, Instant};

use pa_catalog::{
    Catalog, HdrFilter, ItemKind, ItemRef, LibraryFilter, LibraryQuery, LibrarySort, Resolution,
    SortField, ViewPrefs, WatchState,
};
use pa_library::rusqlite::{Connection, params};

/// A library database with a writable side connection for fixtures.
struct Fixture {
    _dir: tempfile::TempDir,
    cat: Catalog,
    conn: Connection,
    next_file: i64,
}

/// Video of a file: (width, height, HDR formats).
type Video = (u32, u32, &'static [&'static str]);

const UHD_DV: Video = (3840, 2160, &["dolby_vision", "hdr10"]);
const UHD_HDR10: Video = (3840, 1600, &["hdr10"]);
const FHD: Video = (1920, 800, &[]);
const HD: Video = (1280, 720, &[]);
const SD: Video = (720, 576, &[]);

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let cat = Catalog::open(&path).unwrap();
        let conn = pa_library::open(&path).unwrap();
        for (id, name) in [(1, "Films"), (2, "NAS")] {
            conn.execute(
                "INSERT INTO sources (id, kind, name, uri, created_at_ms)
                 VALUES (?1, 'local', ?2, 'file:///srv/' || ?2, 0)",
                params![id, name],
            )
            .unwrap();
        }
        Self {
            _dir: dir,
            cat,
            conn,
            next_file: 1,
        }
    }

    fn work(&self, id: i64, kind: &str, title: &str, year: Option<i64>, genres: &[&str]) {
        self.conn
            .execute(
                "INSERT INTO works (id, kind, title, year, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, 0, 0)",
                params![id, kind, title, year],
            )
            .unwrap();
        pa_library::views::set_work_genres(&self.conn, id, "tmdb", genres).unwrap();
    }

    fn tmdb(&self, work: i64, kind: &str, ext: i64) {
        self.conn
            .execute(
                "INSERT INTO external_ids (work_id, provider, ext_kind, ext_id) VALUES (?1, 'tmdb', ?2, ?3)",
                params![work, kind, ext.to_string()],
            )
            .unwrap();
    }

    /// A present file matched to `work` (and `episode`), returns the file id.
    #[allow(clippy::too_many_arguments)]
    fn file(
        &mut self,
        source: i64,
        path: &str,
        work: i64,
        episode: Option<i64>,
        video: Option<Video>,
        audio: &[&str],
        langs: &[&str],
        added: i64,
    ) -> i64 {
        let id = self.next_file;
        self.next_file += 1;
        let video_json = video.map(|(w, h, hdr)| {
            serde_json::json!({ "codec": "hevc", "width": w, "height": h, "hdr": hdr }).to_string()
        });
        let audio_json = (!audio.is_empty()).then(|| serde_json::to_string(audio).unwrap());
        let langs_json = (!langs.is_empty()).then(|| serde_json::to_string(langs).unwrap());
        self.conn
            .execute(
                "INSERT INTO files (id, source_id, path, size_bytes, added_at_ms, duration_ms,
                                    fingerprint, video_json, audio_formats, audio_langs, probed_at_ms)
                 VALUES (?1, ?2, ?3, 1000, ?4, ?5, printf('%08x', ?1), ?6, ?7, ?8, 1)",
                params![
                    id,
                    source,
                    path,
                    added,
                    60_000 * id,
                    video_json,
                    audio_json,
                    langs_json
                ],
            )
            .unwrap();
        self.conn
            .execute(
                "INSERT INTO matches (id, file_id, work_id, confidence, state, decided_by,
                                      created_at_ms, updated_at_ms)
                 VALUES (?1, ?1, ?2, 0.9, 'auto', 'rule', 0, 0)",
                params![id, work],
            )
            .unwrap();
        if let Some(ep) = episode {
            self.conn
                .execute(
                    "INSERT INTO match_episodes (match_id, position, episode_id) VALUES (?1, 0, ?2)",
                    params![id, ep],
                )
                .unwrap();
        }
        id
    }

    fn episode(&self, id: i64, work: i64, season: i64, number: i64) {
        self.conn
            .execute(
                "INSERT INTO episodes (id, work_id, season_number, episode_number) VALUES (?1, ?2, ?3, ?4)",
                params![id, work, season, number],
            )
            .unwrap();
    }

    fn progress(&self, key: &str, kind: &str, position: i64, watched: bool, at: i64) {
        self.conn
            .execute(
                "INSERT INTO progress (profile_id, media_key, source_type, position_ms, duration_ms,
                                       watched, updated_at_ms, device_id)
                 VALUES (1, ?1, ?2, ?3, 6000000, ?4, ?5, 'test')",
                params![key, kind, position, watched, at],
            )
            .unwrap();
    }

    fn titles(&self, q: &LibraryQuery) -> Vec<String> {
        let page = self.cat.query(q, 0, 1000).unwrap();
        assert_eq!(page.total, page.items.len(), "{q:?}");
        page.items.into_iter().map(|i| i.title).collect()
    }
}

/// Movies 1–8 and shows 20–21:
///
/// | id | title             | year | genres          | files                                   | progress      |
/// |----|-------------------|------|-----------------|-----------------------------------------|---------------|
/// | 1  | The Matrix        | 1999 | Action, Sci-Fi  | 4K DV+HDR10 truehd+atmos rus,eng (src 1)| watched (tmdb)|
/// | 2  | Inception         | 2010 | Action, Thriller| 1080p eac3 eng (src 1)                  | 40 % (file)   |
/// | 3  | Амели             | 2001 | Comedy          | 720p ac3 fra (src 2)                    | —             |
/// | 4  | Ёлки              | 2010 | Comedy          | SD aac rus (src 2)                      | —             |
/// | 5  | 12 стульев        | 1971 | Comedy          | unprobed (src 2)                        | —             |
/// | 6  | Dune              | 2021 | Sci-Fi          | 1080p (src 1) + 4K HDR10 (src 2)        | —             |
/// | 7  | alien             | —    | —               | 1080p dts_hd_ma eng (src 1)             | —             |
/// | 8  | Part 10           | 2000 | —               | missing file only                       | —             |
/// | 20 | Dark (show)       | 2017 | Drama, Sci-Fi   | S1E1, S1E2 1080p                        | E1 watched    |
/// | 21 | Friends (show)    | 1994 | Comedy          | S1E1 720p                               | E1 watched    |
fn library() -> Fixture {
    let mut fx = Fixture::new();
    fx.work(1, "movie", "The Matrix", Some(1999), &["Action", "Sci-Fi"]);
    fx.work(2, "movie", "Inception", Some(2010), &["Action", "Thriller"]);
    fx.work(3, "movie", "Амели", Some(2001), &["Comedy"]);
    fx.work(4, "movie", "Ёлки", Some(2010), &["Comedy"]);
    fx.work(5, "movie", "12 стульев", Some(1971), &["comedy"]);
    fx.work(6, "movie", "Dune", Some(2021), &["Sci-Fi"]);
    fx.work(7, "movie", "alien", None, &[]);
    fx.work(8, "movie", "Part 10", Some(2000), &[]);
    fx.tmdb(1, "movie", 603);
    fx.file(
        1,
        "/Movies/The Matrix (1999)/matrix.mkv",
        1,
        None,
        Some(UHD_DV),
        &["truehd", "atmos"],
        &["rus", "eng"],
        100,
    );
    let inception = fx.file(
        1,
        "/Movies/Inception.mkv",
        2,
        None,
        Some(FHD),
        &["eac3"],
        &["eng"],
        200,
    );
    fx.file(
        2,
        "/Кино/Амели.avi",
        3,
        None,
        Some(HD),
        &["ac3"],
        &["fra"],
        300,
    );
    fx.file(
        2,
        "/Кино/Ёлки.mkv",
        4,
        None,
        Some(SD),
        &["aac"],
        &["rus"],
        400,
    );
    fx.file(2, "/Кино/12 стульев.mkv", 5, None, None, &[], &[], 500);
    fx.file(
        1,
        "/Movies/Dune.1080p.mkv",
        6,
        None,
        Some(FHD),
        &["eac3"],
        &["eng"],
        50,
    );
    fx.file(
        2,
        "/Кино/Dune.2160p.mkv",
        6,
        None,
        Some(UHD_HDR10),
        &["truehd"],
        &["eng"],
        600,
    );
    fx.file(
        1,
        "/Movies/alien.mkv",
        7,
        None,
        Some(FHD),
        &["dts_hd_ma"],
        &["eng"],
        700,
    );
    let gone = fx.file(1, "/Movies/Part 10.mkv", 8, None, Some(FHD), &[], &[], 800);
    fx.conn
        .execute(
            "UPDATE files SET missing_since_ms = 1 WHERE id = ?1",
            [gone],
        )
        .unwrap();

    fx.work(20, "show", "Dark", Some(2017), &["Drama", "Sci-Fi"]);
    fx.work(21, "anime", "Friends", Some(1994), &["Comedy"]);
    fx.tmdb(20, "tv", 70523);
    fx.episode(201, 20, 1, 1);
    fx.episode(202, 20, 1, 2);
    fx.episode(211, 21, 1, 1);
    fx.file(
        1,
        "/TV/Dark/Season 1/Dark.S01E01.mkv",
        20,
        Some(201),
        Some(FHD),
        &["eac3"],
        &["deu"],
        900,
    );
    fx.file(
        1,
        "/TV/Dark/Season 1/Dark.S01E02.mkv",
        20,
        Some(202),
        Some(FHD),
        &["eac3"],
        &["deu"],
        901,
    );
    let friends = fx.file(
        1,
        "/TV/Friends/Friends.S01E01.mkv",
        21,
        Some(211),
        Some(HD),
        &["ac3"],
        &["eng"],
        902,
    );

    fx.progress("tmdb:movie:603", "movie", 7_000_000, true, 5_000);
    fx.progress(
        &format!("file:{inception:08x}"),
        "movie",
        2_400_000,
        false,
        6_000,
    );
    fx.progress("tmdb:episode:70523:1:1", "episode", 3_000_000, true, 7_000);
    fx.progress(
        &format!("file:{friends:08x}"),
        "episode",
        3_000_000,
        true,
        8_000,
    );
    fx
}

fn movies(filter: LibraryFilter) -> LibraryQuery {
    LibraryQuery {
        kind: ItemKind::Movie,
        filter,
        sort: LibrarySort::default(),
    }
}

fn shows(filter: LibraryFilter) -> LibraryQuery {
    LibraryQuery {
        kind: ItemKind::Show,
        ..movies(filter)
    }
}

#[test]
fn titles_sort_across_scripts_articles_and_numbers() {
    let fx = library();
    // `12 стульев` (digits) first; `alien` lower case among A; `The Matrix`
    // under M; Cyrillic after Latin with `Ёлки` as `Елки`; the movie whose
    // only file is missing is not listed.
    assert_eq!(
        fx.titles(&movies(LibraryFilter::default())),
        [
            "12 стульев",
            "alien",
            "Dune",
            "Inception",
            "The Matrix",
            "Амели",
            "Ёлки"
        ]
    );
}

#[test]
fn single_filters() {
    let fx = library();
    let f = |filter: LibraryFilter| fx.titles(&movies(filter));
    assert_eq!(
        f(LibraryFilter {
            genres: vec!["comedy".into()],
            ..Default::default()
        }),
        ["12 стульев", "Амели", "Ёлки"]
    );
    assert_eq!(
        f(LibraryFilter {
            year_min: Some(2001),
            year_max: Some(2010),
            ..Default::default()
        }),
        ["Inception", "Амели", "Ёлки"]
    );
    assert_eq!(
        f(LibraryFilter {
            resolutions: vec![Resolution::Uhd],
            ..Default::default()
        }),
        ["Dune", "The Matrix"]
    );
    assert_eq!(
        f(LibraryFilter {
            resolutions: vec![Resolution::Sd, Resolution::Hd],
            ..Default::default()
        }),
        ["Амели", "Ёлки"]
    );
    assert_eq!(
        f(LibraryFilter {
            hdr: vec![HdrFilter::DolbyVision],
            ..Default::default()
        }),
        ["The Matrix"]
    );
    // DV 8.1 carries HDR10 too.
    assert_eq!(
        f(LibraryFilter {
            hdr: vec![HdrFilter::Hdr10],
            ..Default::default()
        }),
        ["Dune", "The Matrix"]
    );
    // SDR needs a probed video; Dune has an SDR and an HDR file.
    assert_eq!(
        f(LibraryFilter {
            hdr: vec![HdrFilter::Sdr],
            ..Default::default()
        }),
        ["alien", "Dune", "Inception", "Амели", "Ёлки"]
    );
    assert_eq!(
        f(LibraryFilter {
            audio_formats: vec!["atmos".into(), "dts_hd_ma".into()],
            ..Default::default()
        }),
        ["alien", "The Matrix"]
    );
    assert_eq!(
        f(LibraryFilter {
            audio_languages: vec!["RUS".into()],
            ..Default::default()
        }),
        ["The Matrix", "Ёлки"]
    );
    assert_eq!(
        f(LibraryFilter {
            sources: vec![2],
            ..Default::default()
        }),
        ["12 стульев", "Dune", "Амели", "Ёлки"]
    );
    assert_eq!(
        f(LibraryFilter {
            watched: vec![WatchState::Watched],
            ..Default::default()
        }),
        ["The Matrix"]
    );
    assert_eq!(
        f(LibraryFilter {
            watched: vec![WatchState::InProgress],
            ..Default::default()
        }),
        ["Inception"]
    );
    assert_eq!(
        f(LibraryFilter {
            watched: vec![WatchState::Unwatched],
            ..Default::default()
        }),
        ["12 стульев", "alien", "Dune", "Амели", "Ёлки"]
    );
}

#[test]
fn filter_combinations() {
    let fx = library();
    let f = |filter: LibraryFilter| fx.titles(&movies(filter));
    // AND across fields, OR inside one.
    assert_eq!(
        f(LibraryFilter {
            genres: vec!["Sci-Fi".into(), "Thriller".into()],
            resolutions: vec![Resolution::Uhd],
            ..Default::default()
        }),
        ["Dune", "The Matrix"]
    );
    assert_eq!(
        f(LibraryFilter {
            genres: vec!["Action".into()],
            watched: vec![WatchState::Unwatched, WatchState::InProgress],
            ..Default::default()
        }),
        ["Inception"]
    );
    // File filters match a work if one file has all of them: Dune's HDR10
    // file is on source 2, its 1080p one on source 1.
    assert_eq!(
        f(LibraryFilter {
            hdr: vec![HdrFilter::AnyHdr],
            sources: vec![2],
            ..Default::default()
        }),
        ["Dune"]
    );
    assert!(
        f(LibraryFilter {
            hdr: vec![HdrFilter::AnyHdr],
            sources: vec![2],
            audio_formats: vec!["eac3".into()],
            ..Default::default()
        })
        .is_empty()
    );
    assert_eq!(
        f(LibraryFilter {
            genres: vec!["Comedy".into()],
            year_min: Some(2000),
            sources: vec![2],
            resolutions: vec![Resolution::Sd, Resolution::Hd, Resolution::FullHd],
            audio_languages: vec!["fra".into(), "rus".into()],
            watched: vec![WatchState::Unwatched],
            ..Default::default()
        }),
        ["Амели", "Ёлки"]
    );
    // Unprobed files match no video or audio filter.
    assert!(
        !f(LibraryFilter {
            resolutions: Resolution::ALL.to_vec(),
            ..Default::default()
        })
        .contains(&"12 стульев".to_owned())
    );
    assert!(
        f(LibraryFilter {
            genres: vec!["Western".into()],
            ..Default::default()
        })
        .is_empty()
    );
}

#[test]
fn show_watch_state_and_filters() {
    let fx = library();
    let f = |filter: LibraryFilter| fx.titles(&shows(filter));
    assert_eq!(f(LibraryFilter::default()), ["Dark", "Friends"]);
    // Dark: 1 of 2 episodes watched (TMDB key); Friends: its only episode
    // (file key).
    assert_eq!(
        f(LibraryFilter {
            watched: vec![WatchState::InProgress],
            ..Default::default()
        }),
        ["Dark"]
    );
    assert_eq!(
        f(LibraryFilter {
            watched: vec![WatchState::Watched],
            ..Default::default()
        }),
        ["Friends"]
    );
    assert!(
        f(LibraryFilter {
            watched: vec![WatchState::Unwatched],
            ..Default::default()
        })
        .is_empty()
    );
    assert_eq!(
        f(LibraryFilter {
            genres: vec!["Sci-Fi".into()],
            resolutions: vec![Resolution::FullHd],
            audio_languages: vec!["deu".into()],
            ..Default::default()
        }),
        ["Dark"]
    );
    // Second episode watched too: Dark is watched.
    fx.progress("tmdb:episode:70523:1:2", "episode", 3_000_000, true, 9_000);
    assert_eq!(
        f(LibraryFilter {
            watched: vec![WatchState::Watched],
            ..Default::default()
        }),
        ["Dark", "Friends"]
    );
}

#[test]
fn sorts_and_paging() {
    let fx = library();
    let sorted = |field, descending| {
        fx.titles(&LibraryQuery {
            kind: ItemKind::Movie,
            filter: LibraryFilter::default(),
            sort: LibrarySort { field, descending },
        })
    };
    // Missing years last in both directions; ties by title A→Z.
    assert_eq!(
        sorted(SortField::Year, false),
        [
            "12 стульев",
            "The Matrix",
            "Амели",
            "Inception",
            "Ёлки",
            "Dune",
            "alien"
        ]
    );
    assert_eq!(
        sorted(SortField::Year, true),
        [
            "Dune",
            "Inception",
            "Ёлки",
            "Амели",
            "The Matrix",
            "12 стульев",
            "alien"
        ]
    );
    // Added: the newest present file of the work (Dune: 600).
    assert_eq!(
        sorted(SortField::Added, true),
        [
            "alien",
            "Dune",
            "12 стульев",
            "Ёлки",
            "Амели",
            "Inception",
            "The Matrix"
        ]
    );
    assert_eq!(
        sorted(SortField::Title, true),
        [
            "Ёлки",
            "Амели",
            "The Matrix",
            "Inception",
            "Dune",
            "alien",
            "12 стульев"
        ]
    );
    // Last watched: Inception (6000), Matrix (5000), then the rest by title.
    assert_eq!(
        sorted(SortField::LastWatched, true)[..3],
        ["Inception", "The Matrix", "12 стульев"]
    );
    assert_eq!(sorted(SortField::Duration, false)[0], "The Matrix");

    let q = movies(LibraryFilter::default());
    let page = fx.cat.query(&q, 2, 3).unwrap();
    assert_eq!(page.total, 7);
    assert_eq!(page.offset, 2);
    assert_eq!(
        page.items
            .iter()
            .map(|i| i.title.as_str())
            .collect::<Vec<_>>(),
        ["Dune", "Inception", "The Matrix"]
    );
    assert_eq!(page.items[0].id, ItemRef::Movie(6));
    let past = fx.cat.query(&q, 50, 10).unwrap();
    assert!(past.items.is_empty());
    assert_eq!(past.total, 7);
    // Episodes and tracks are not query kinds.
    assert!(
        fx.cat
            .query(&LibraryQuery::new(ItemKind::Episode), 0, 10)
            .is_err()
    );
}

#[test]
fn alphabet_index_follows_title_order() {
    let fx = library();
    let idx = fx
        .cat
        .alphabet_index(&movies(LibraryFilter::default()))
        .unwrap();
    let got: Vec<(&str, usize, usize)> = idx
        .iter()
        .map(|e| (e.letter.as_str(), e.offset, e.count))
        .collect();
    assert_eq!(
        got,
        [
            ("#", 0, 1),
            ("A", 1, 1),
            ("D", 2, 1),
            ("I", 3, 1),
            ("M", 4, 1),
            ("А", 5, 1),
            ("Е", 6, 1)
        ]
    );
    // Same filter as the list; descending title sort reverses it.
    let mut q = movies(LibraryFilter {
        genres: vec!["Comedy".into()],
        ..Default::default()
    });
    q.sort.descending = true;
    let idx = fx.cat.alphabet_index(&q).unwrap();
    let got: Vec<(&str, usize)> = idx.iter().map(|e| (e.letter.as_str(), e.offset)).collect();
    assert_eq!(got, [("Е", 0), ("А", 1), ("#", 2)]);
}

#[test]
fn facets_list_present_values() {
    let fx = library();
    let f = fx.cat.facets(ItemKind::Movie).unwrap();
    let genres: Vec<(&str, usize)> = f
        .genres
        .iter()
        .map(|g| (g.value.as_str(), g.count))
        .collect();
    // `comedy` and `Comedy` are one genre.
    assert_eq!(
        genres,
        [("Action", 2), ("Comedy", 3), ("Sci-Fi", 2), ("Thriller", 1)]
    );
    assert_eq!((f.year_min, f.year_max), (Some(1971), Some(2021)));
    let res: Vec<(Resolution, usize)> = f.resolutions.iter().map(|r| (r.value, r.count)).collect();
    assert_eq!(
        res,
        [
            (Resolution::Sd, 1),
            (Resolution::Hd, 1),
            (Resolution::FullHd, 3),
            (Resolution::Uhd, 2)
        ]
    );
    let hdr: Vec<(HdrFilter, usize)> = f.hdr.iter().map(|h| (h.value, h.count)).collect();
    assert_eq!(
        hdr,
        [
            (HdrFilter::Sdr, 5),
            (HdrFilter::AnyHdr, 2),
            (HdrFilter::Hdr10, 2),
            (HdrFilter::DolbyVision, 1)
        ]
    );
    assert_eq!(f.audio_formats[0].value, "eac3");
    assert_eq!(f.audio_formats[0].count, 2);
    assert!(
        f.audio_formats
            .iter()
            .any(|a| a.value == "atmos" && a.count == 1)
    );
    assert_eq!(f.audio_languages[0].value, "eng");
    let sources: Vec<(i64, usize)> = f.sources.iter().map(|s| (s.value, s.count)).collect();
    assert_eq!(sources, [(1, 4), (2, 4)]);
    assert_eq!(
        fx.cat.genres(ItemRef::Movie(1)).unwrap(),
        ["Action", "Sci-Fi"]
    );
}

#[test]
fn browse_lists_folders_first_or_mixed() {
    let fx = library();
    let names = |dir: &str, sort: LibrarySort, folders_first: bool| -> Vec<String> {
        fx.cat
            .browse(1, dir, sort, folders_first)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect()
    };
    let by_title = LibrarySort::default();
    assert_eq!(names("", by_title, true), ["Movies", "TV"]);
    assert_eq!(
        names("Movies", by_title, true),
        [
            "The Matrix (1999)",
            "alien.mkv",
            "Dune.1080p.mkv",
            "Inception.mkv"
        ]
    );
    assert_eq!(
        names("/Movies/", by_title, false),
        [
            "alien.mkv",
            "Dune.1080p.mkv",
            "Inception.mkv",
            "The Matrix (1999)"
        ]
    );
    let newest = LibrarySort {
        field: SortField::Added,
        descending: true,
    };
    assert_eq!(
        names("Movies", newest, false),
        [
            "alien.mkv",
            "Inception.mkv",
            "The Matrix (1999)",
            "Dune.1080p.mkv"
        ]
    );
    let entries = fx.cat.browse(1, "Movies", by_title, true).unwrap();
    let matrix = &entries[0];
    assert!(matrix.is_folder);
    assert_eq!(matrix.rel_path, "Movies/The Matrix (1999)");
    assert_eq!(matrix.file_count, 1);
    assert_eq!(matrix.item, None);
    let inception = entries.iter().find(|e| e.name == "Inception.mkv").unwrap();
    assert_eq!(inception.item, Some(ItemRef::Movie(2)));
    assert_eq!(inception.rel_path, "Movies/Inception.mkv");
    let ep = fx
        .cat
        .browse(1, "TV/Dark/Season 1", by_title, true)
        .unwrap();
    assert_eq!(ep[0].item, Some(ItemRef::Episode(201)));
    // The missing file is not listed.
    assert!(!names("Movies", by_title, true).contains(&"Part 10.mkv".to_owned()));
}

#[test]
fn view_prefs_are_kept_per_view() {
    let fx = library();
    let defaults = fx.cat.view_prefs("movies").unwrap();
    assert_eq!(defaults.sort, LibrarySort::default());
    assert!(defaults.folders_first);
    assert_eq!(defaults.filter, None);

    let movies_prefs = ViewPrefs {
        view: "movies".into(),
        sort: LibrarySort {
            field: SortField::Added,
            descending: true,
        },
        folders_first: true,
        filter: Some(LibraryFilter {
            resolutions: vec![Resolution::Uhd],
            ..Default::default()
        }),
    };
    fx.cat.set_view_prefs(&movies_prefs).unwrap();
    let folder_prefs = ViewPrefs {
        view: "folder:1".into(),
        sort: LibrarySort {
            field: SortField::Title,
            descending: false,
        },
        folders_first: false,
        filter: None,
    };
    fx.cat.set_view_prefs(&folder_prefs).unwrap();
    assert_eq!(fx.cat.view_prefs("movies").unwrap(), movies_prefs);
    assert_eq!(fx.cat.view_prefs("folder:1").unwrap(), folder_prefs);
    assert_eq!(
        fx.cat.view_prefs("shows").unwrap().sort,
        LibrarySort::default()
    );
    // Overwrite.
    let mut changed = movies_prefs.clone();
    changed.sort.field = SortField::Year;
    changed.filter = None;
    fx.cat.set_view_prefs(&changed).unwrap();
    assert_eq!(fx.cat.view_prefs("movies").unwrap(), changed);
    // A sort slug from a newer build falls back to title.
    fx.conn
        .execute(
            "UPDATE view_prefs SET sort = 'rating_v9' WHERE view = 'movies'",
            [],
        )
        .unwrap();
    assert_eq!(
        fx.cat.view_prefs("movies").unwrap().sort.field,
        SortField::Title
    );
    let bad = ViewPrefs {
        view: String::new(),
        ..changed
    };
    assert!(fx.cat.set_view_prefs(&bad).is_err());
}

#[test]
fn smart_shelves_save_queries_and_pin_to_home() {
    let fx = library();
    let uhd = LibraryQuery {
        kind: ItemKind::Movie,
        filter: LibraryFilter {
            resolutions: vec![Resolution::Uhd],
            ..Default::default()
        },
        sort: LibrarySort {
            field: SortField::Added,
            descending: true,
        },
    };
    let a = fx.cat.create_shelf("4K", &uhd, true).unwrap();
    let comedy = movies(LibraryFilter {
        genres: vec!["Comedy".into()],
        ..Default::default()
    });
    let b = fx.cat.create_shelf("Comedy", &comedy, false).unwrap();
    let c = fx
        .cat
        .create_shelf(
            "Unwatched shows",
            &shows(LibraryFilter {
                watched: vec![WatchState::Unwatched, WatchState::InProgress],
                ..Default::default()
            }),
            true,
        )
        .unwrap();
    assert_eq!((a.position, b.position, c.position), (0, 1, 2));
    assert_eq!(a.query, uhd);

    let pinned: Vec<String> = fx
        .cat
        .shelves(true)
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(pinned, ["4K", "Unwatched shows"]);
    let page = fx.cat.shelf_items(a.id, 0, 10).unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|i| i.title.as_str())
            .collect::<Vec<_>>(),
        ["Dune", "The Matrix"]
    );
    let page = fx.cat.shelf_items(c.id, 0, 10).unwrap();
    assert_eq!(page.items[0].title, "Dark");

    fx.cat.reorder_shelves(&[c.id, a.id]).unwrap();
    let order: Vec<i64> = fx
        .cat
        .shelves(false)
        .unwrap()
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(order, [c.id, a.id, b.id]);

    fx.cat
        .update_shelf(b.id, "Comedies", &comedy, true)
        .unwrap();
    assert_eq!(fx.cat.shelves(true).unwrap().len(), 3);
    assert!(fx.cat.update_shelf(999, "x", &comedy, true).is_err());
    assert!(fx.cat.create_shelf("  ", &comedy, true).is_err());
    assert!(
        fx.cat
            .create_shelf("Tracks", &LibraryQuery::new(ItemKind::Track), true)
            .is_err()
    );
    fx.cat.delete_shelf(a.id).unwrap();
    assert_eq!(fx.cat.shelves(false).unwrap().len(), 2);
    assert!(fx.cat.shelf_items(a.id, 0, 10).is_err());
}

/// Deterministic pseudo-random numbers for the large library.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

#[test]
fn queries_over_20_000_items_are_fast() {
    const WORDS: &[&str] = &[
        "the",
        "dark",
        "night",
        "star",
        "love",
        "war",
        "king",
        "last",
        "city",
        "lost",
        "тайна",
        "ночь",
        "звезда",
        "любовь",
        "война",
        "король",
        "ёлка",
        "город",
        "игра",
        "1999",
        "2",
    ];
    const GENRES: &[&str] = &[
        "Action",
        "Comedy",
        "Drama",
        "Sci-Fi",
        "Thriller",
        "Horror",
        "Romance",
        "Animation",
    ];
    const VIDEOS: &[Video] = &[UHD_DV, UHD_HDR10, FHD, FHD, FHD, HD, SD];
    const AUDIO: &[&[&str]] = &[
        &["truehd", "atmos"],
        &["eac3"],
        &["ac3"],
        &["aac"],
        &["dts_hd_ma"],
    ];
    const LANGS: &[&[&str]] = &[&["rus", "eng"], &["eng"], &["rus"], &["jpn", "eng"]];

    let mut fx = Fixture::new();
    let mut rng = Lcg(7);
    let started = Instant::now();
    let tx = fx.conn.transaction().unwrap();
    {
        let mut work = tx
            .prepare(
                "INSERT INTO works (id, kind, title, year, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, 0, 0)",
            )
            .unwrap();
        let mut genre = tx
            .prepare("INSERT OR IGNORE INTO work_genres (work_id, provider, name) VALUES (?1, 'tmdb', ?2)")
            .unwrap();
        let mut tmdb = tx
            .prepare(
                "INSERT INTO external_ids (work_id, provider, ext_kind, ext_id) VALUES (?1, 'tmdb', ?2, ?3)",
            )
            .unwrap();
        let mut file = tx
            .prepare(
                "INSERT INTO files (id, source_id, path, size_bytes, added_at_ms, duration_ms,
                                    fingerprint, video_json, audio_formats, audio_langs, probed_at_ms)
                 VALUES (?1, ?2, ?3, 1000, ?4, ?5, printf('%08x', ?1), ?6, ?7, ?8, 1)",
            )
            .unwrap();
        let mut matched = tx
            .prepare(
                "INSERT INTO matches (id, file_id, work_id, confidence, state, decided_by,
                                      created_at_ms, updated_at_ms)
                 VALUES (?1, ?1, ?2, 0.9, 'auto', 'rule', 0, 0)",
            )
            .unwrap();
        let mut episode = tx
            .prepare(
                "INSERT INTO episodes (id, work_id, season_number, episode_number) VALUES (?1, ?2, 1, ?3)",
            )
            .unwrap();
        let mut match_ep = tx
            .prepare(
                "INSERT INTO match_episodes (match_id, position, episode_id) VALUES (?1, 0, ?2)",
            )
            .unwrap();
        let mut progress = tx
            .prepare(
                "INSERT OR IGNORE INTO progress (profile_id, media_key, source_type, position_ms, duration_ms,
                                       watched, updated_at_ms, device_id)
                 VALUES (1, ?1, ?2, ?3, 6000000, ?4, ?5, 'bench')",
            )
            .unwrap();
        let mut file_id = 0i64;
        let mut episode_id = 0i64;
        // 18 000 movies with one or two files, 2 000 shows with 1–10
        // episodes: 20 000 items, ~32 000 files.
        for id in 1..=20_000i64 {
            let is_show = id > 18_000;
            let n = 1 + rng.next(4) as usize;
            let title: Vec<&str> = (0..n)
                .map(|_| WORDS[rng.next(WORDS.len() as u64) as usize])
                .collect();
            let year = (rng.next(10) > 0).then(|| 1950 + rng.next(75) as i64);
            work.execute(params![
                id,
                if is_show { "show" } else { "movie" },
                title.join(" "),
                year
            ])
            .unwrap();
            for _ in 0..1 + rng.next(3) {
                genre
                    .execute(params![id, GENRES[rng.next(GENRES.len() as u64) as usize]])
                    .unwrap();
            }
            let has_tmdb = rng.next(2) == 0;
            if has_tmdb {
                tmdb.execute(params![
                    id,
                    if is_show { "tv" } else { "movie" },
                    id.to_string()
                ])
                .unwrap();
            }
            let files = if is_show {
                1 + rng.next(10)
            } else {
                1 + rng.next(2)
            };
            for k in 0..files {
                file_id += 1;
                let v = VIDEOS[rng.next(VIDEOS.len() as u64) as usize];
                let video =
                    serde_json::json!({"codec": "hevc", "width": v.0, "height": v.1, "hdr": v.2})
                        .to_string();
                let probed = rng.next(10) > 0;
                file.execute(params![
                    file_id,
                    1 + rng.next(3) as i64 % 2,
                    format!("/Lib/{id}/{k}.mkv"),
                    rng.next(1_000_000) as i64,
                    60_000 * (1 + rng.next(180) as i64),
                    probed.then_some(video),
                    probed.then(|| serde_json::to_string(
                        AUDIO[rng.next(AUDIO.len() as u64) as usize]
                    )
                    .unwrap()),
                    probed.then(|| serde_json::to_string(
                        LANGS[rng.next(LANGS.len() as u64) as usize]
                    )
                    .unwrap()),
                ])
                .unwrap();
                matched.execute(params![file_id, id]).unwrap();
                if is_show {
                    episode_id += 1;
                    episode
                        .execute(params![episode_id, id, k as i64 + 1])
                        .unwrap();
                    match_ep.execute(params![file_id, episode_id]).unwrap();
                }
                // A quarter of the items have progress, half of it finished.
                if rng.next(4) == 0 {
                    let key = if has_tmdb && is_show {
                        format!("tmdb:episode:{id}:1:{}", k + 1)
                    } else if has_tmdb {
                        format!("tmdb:movie:{id}")
                    } else {
                        format!("file:{file_id:08x}")
                    };
                    progress
                        .execute(params![
                            key,
                            if is_show { "episode" } else { "movie" },
                            rng.next(6_000_000) as i64,
                            rng.next(2) == 0,
                            rng.next(1_000_000) as i64
                        ])
                        .unwrap();
                }
            }
        }
    }
    tx.commit().unwrap();
    fx.conn.execute_batch("ANALYZE").unwrap();
    let files: i64 = fx
        .conn
        .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
        .unwrap();
    println!(
        "fixture: 20 000 works, {files} files in {:?}",
        started.elapsed()
    );

    let cases: Vec<(&str, LibraryQuery)> = vec![
        ("all movies by title", movies(LibraryFilter::default())),
        (
            "all movies, newest first",
            LibraryQuery {
                sort: LibrarySort {
                    field: SortField::Added,
                    descending: true,
                },
                ..movies(LibraryFilter::default())
            },
        ),
        (
            "genre + years",
            movies(LibraryFilter {
                genres: vec!["Drama".into(), "Comedy".into()],
                year_min: Some(1990),
                year_max: Some(2010),
                ..Default::default()
            }),
        ),
        (
            "4K Dolby Vision Atmos",
            movies(LibraryFilter {
                resolutions: vec![Resolution::Uhd],
                hdr: vec![HdrFilter::DolbyVision],
                audio_formats: vec!["atmos".into()],
                ..Default::default()
            }),
        ),
        (
            "unwatched, last watched sort",
            LibraryQuery {
                sort: LibrarySort {
                    field: SortField::LastWatched,
                    descending: true,
                },
                ..movies(LibraryFilter {
                    watched: vec![WatchState::Unwatched, WatchState::InProgress],
                    ..Default::default()
                })
            },
        ),
        (
            "everything combined",
            movies(LibraryFilter {
                genres: vec!["Action".into(), "Sci-Fi".into()],
                year_min: Some(1980),
                year_max: None,
                watched: vec![WatchState::Unwatched],
                resolutions: vec![Resolution::FullHd, Resolution::Uhd],
                hdr: vec![HdrFilter::Sdr, HdrFilter::Hdr10],
                audio_formats: vec!["eac3".into(), "truehd".into()],
                audio_languages: vec!["rus".into()],
                sources: vec![1],
            }),
        ),
        ("all shows", shows(LibraryFilter::default())),
        (
            "shows in progress",
            shows(LibraryFilter {
                watched: vec![WatchState::InProgress],
                ..Default::default()
            }),
        ),
    ];

    // The bound of the task is for release builds; debug builds get slack.
    let bound = if cfg!(debug_assertions) {
        Duration::from_millis(600)
    } else {
        Duration::from_millis(30)
    };
    // Best of three runs after a warm-up (statements prepared, pages
    // cached): the build server runs other workloads, and the bound is
    // about the query, not about scheduling noise.
    fn best<T>(mut f: impl FnMut() -> T) -> (T, Duration) {
        let mut out = f();
        let mut best = Duration::MAX;
        for _ in 0..3 {
            let t = Instant::now();
            out = f();
            best = best.min(t.elapsed());
        }
        (out, best)
    }
    // The first query after a change rebuilds the in-memory index.
    let t = Instant::now();
    fx.cat
        .query(&movies(LibraryFilter::default()), 0, 100)
        .unwrap();
    println!(
        "{:>32}: {:?} (not bounded)",
        "first query, index build",
        t.elapsed()
    );

    let mut worst = Duration::ZERO;
    for (name, q) in &cases {
        let (page, took) = best(|| fx.cat.query(q, 0, 100).unwrap());
        println!(
            "{name:>32}: {:>5} of {:>5} in {took:?}",
            page.items.len(),
            page.total
        );
        assert!(page.total > 0, "{name}");
        worst = worst.max(took);
        let (_, deep) = best(|| fx.cat.query(q, page.total.saturating_sub(50), 100).unwrap());
        println!("{:>32}  last page in {deep:?}", "");
        worst = worst.max(deep);
    }
    let q = movies(LibraryFilter::default());
    let (idx, took) = best(|| fx.cat.alphabet_index(&q).unwrap());
    println!(
        "{:>32}: {} letters in {took:?}",
        "alphabet index",
        idx.len()
    );
    worst = worst.max(took);
    assert!(idx.len() > 10);
    assert_eq!(idx.iter().map(|e| e.count).sum::<usize>(), 18_000);

    let t = Instant::now();
    let f = fx.cat.facets(ItemKind::Movie).unwrap();
    println!(
        "{:>32}: {} genres in {:?} (not bounded)",
        "facets",
        f.genres.len(),
        t.elapsed()
    );
    let t = Instant::now();
    let root = fx
        .cat
        .browse(1, "Lib", LibrarySort::default(), true)
        .unwrap();
    println!(
        "{:>32}: {} entries in {:?} (not bounded)",
        "browse",
        root.len(),
        t.elapsed()
    );

    assert!(worst < bound, "slowest query took {worst:?}");
}
