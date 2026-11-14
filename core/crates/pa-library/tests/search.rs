// SPDX-License-Identifier: MPL-2.0
//! Library search (LIB-008): transliteration, aliases, prefixes, ranking,
//! index maintenance and speed on 50 000 items.

use std::time::{Duration, Instant};

use pa_library::rusqlite::{Connection, params};
use pa_library::search::{self, SearchHit, SearchKind, SearchOptions};

fn db() -> Connection {
    let conn = pa_library::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO sources (id, kind, name, uri, created_at_ms) VALUES (1, 'local', 'Media', 'file:///media', 0)",
        [],
    )
    .unwrap();
    conn
}

fn work(conn: &Connection, id: i64, kind: &str, title: &str, year: Option<i64>) {
    conn.execute(
        "INSERT INTO works (id, kind, title, year, created_at_ms, updated_at_ms) VALUES (?1, ?2, ?3, ?4, 0, 0)",
        params![id, kind, title, year],
    )
    .unwrap();
}

fn alias(conn: &Connection, work: i64, lang: &str, text: &str, kind: &str) {
    conn.execute(
        "INSERT INTO titles (work_id, lang, text, kind) VALUES (?1, ?2, ?3, ?4)",
        params![work, lang, text, kind],
    )
    .unwrap();
}

fn file(conn: &Connection, id: i64, path: &str) {
    conn.execute(
        "INSERT INTO files (id, source_id, path, size_bytes, added_at_ms) VALUES (?1, 1, ?2, 1000, 0)",
        params![id, path],
    )
    .unwrap();
}

fn find(conn: &Connection, q: &str) -> Vec<SearchHit> {
    search::search(conn, q, &SearchOptions::default()).unwrap()
}

fn first(conn: &Connection, q: &str) -> (SearchKind, i64) {
    let hits = find(conn, q);
    assert!(!hits.is_empty(), "no hits for {q:?}");
    (hits[0].kind, hits[0].id)
}

fn ids(hits: &[SearchHit]) -> Vec<(SearchKind, i64)> {
    hits.iter().map(|h| (h.kind, h.id)).collect()
}

#[test]
fn russian_query_finds_transliterated_file_name() {
    let conn = db();
    file(
        &conn,
        10,
        "Movies/Ironiya.sudby.ili.S.legkim.parom.1975.BDRip.1080p.mkv",
    );
    file(
        &conn,
        11,
        "Movies/Ironiya.sudby.ili.S.legkim.parom.1975.nfo",
    );
    file(&conn, 12, "Movies/Brat.1997.mkv");

    let hits = find(&conn, "ирония судьбы");
    assert_eq!(ids(&hits), [(SearchKind::Video, 10)]);
    assert_eq!(
        hits[0].title,
        "Ironiya.sudby.ili.S.legkim.parom.1975.BDRip.1080p"
    );
    assert_eq!(
        hits[0].subtitle.as_deref(),
        Some("Movies/Ironiya.sudby.ili.S.legkim.parom.1975.BDRip.1080p.mkv")
    );
    // Other spellings and as-you-type prefixes in both scripts.
    for q in [
        "Ирония Судьбы",
        "ironia sudbi",
        "Ironija sud'by",
        "ирон",
        "ирони",
        "ирония суд",
        "iron",
        "легким паром",
        "лёгким паром 1975",
    ] {
        assert_eq!(first(&conn, q), (SearchKind::Video, 10), "{q}");
    }
    assert!(find(&conn, "ирония брат").is_empty());
}

#[test]
fn aliases_and_scripts() {
    let conn = db();
    work(&conn, 1, "show", "The Witcher", Some(2019));
    alias(&conn, 1, "ru", "Ведьмак", "official");
    work(&conn, 2, "movie", "Брат 2", Some(2000));
    work(&conn, 3, "movie", "Amélie", Some(2001));
    alias(
        &conn,
        3,
        "fr",
        "Le Fabuleux Destin d'Amélie Poulain",
        "official",
    );
    work(&conn, 4, "anime", "Shingeki no Kyojin", Some(2013));
    alias(&conn, 4, "en", "Attack on Titan", "official");
    alias(&conn, 4, "ru", "Атака титанов", "official");
    alias(&conn, 4, "ja", "進撃の巨人", "official");
    work(&conn, 5, "movie", "Rocky IV", Some(1985));
    work(&conn, 6, "movie", "Spider-Man: No Way Home", Some(2021));

    for (q, want) in [
        ("ведьмак", 1),
        ("Ведьм", 1),
        ("ВЕДЬМАК", 1),
        ("witcher", 1),
        ("the witcher", 1),
        ("vedmak", 1),
        ("brat 2", 2),
        ("брат ii", 2),
        ("amelie", 3),
        ("Amélie", 3),
        ("AMELIE", 3),
        ("fabuleux destin", 3),
        ("shingeki no kyoujin", 4),
        ("атака тит", 4),
        ("attack on", 4),
        ("進撃", 4),
        ("rocky 4", 5),
        ("rocky iv", 5),
        ("spiderman", 6),
        ("spider man no", 6),
        ("человек паук", 0),
    ] {
        let hits = find(&conn, q);
        if want == 0 {
            assert!(hits.is_empty(), "{q}: {hits:?}");
            continue;
        }
        assert!(!hits.is_empty(), "no hits for {q}");
        assert_eq!(hits[0].id, want, "{q}: {hits:?}");
    }

    let hit = &find(&conn, "ведьмак")[0];
    assert_eq!(hit.kind, SearchKind::Show);
    assert_eq!(hit.title, "The Witcher");
    assert_eq!(hit.matched, "Ведьмак");
    assert_eq!(hit.year, Some(2019));
    // The year is searchable together with the title.
    assert_eq!(first(&conn, "witcher 2019"), (SearchKind::Show, 1));
    assert!(find(&conn, "witcher 2020").is_empty());
    assert_eq!(first(&conn, "attack").0, SearchKind::Anime);
}

#[test]
fn ranking_prefers_exact_and_starting_titles() {
    let conn = db();
    // Many titles that contain or start with `up`.
    for i in 0..300 {
        work(
            &conn,
            1000 + i,
            "movie",
            &format!("Upgrade Story {i}"),
            None,
        );
        work(&conn, 2000 + i, "show", &format!("Grown Ups {i}"), None);
    }
    work(&conn, 1, "movie", "Up in the Air", Some(2009));
    work(&conn, 2, "movie", "Up", Some(2009));
    work(&conn, 3, "movie", "What's Up, Doc?", Some(1972));

    let hits = find(&conn, "up");
    assert_eq!(hits[0].id, 2, "{:?}", &hits[..3]);
    assert_eq!(hits[1].id, 1);
    // Starts-with beats contains.
    let pos = |id| hits.iter().position(|h| h.id == id);
    assert!(pos(1000).unwrap() < pos(3).unwrap_or(usize::MAX));

    // Work before its episodes and an unrecognised file of the same name.
    work(&conn, 10, "show", "Dark", Some(2017));
    conn.execute(
        "INSERT INTO episodes (id, work_id, season_number, episode_number, title) VALUES (100, 10, 1, 1, 'Dark')",
        [],
    )
    .unwrap();
    file(&conn, 50, "Dark.mkv");
    let hits = find(&conn, "dark");
    assert_eq!(
        ids(&hits[..3]),
        [
            (SearchKind::Show, 10),
            (SearchKind::Video, 50),
            (SearchKind::Episode, 100)
        ]
    );
    assert_eq!(hits[2].title, "Dark");
    assert_eq!(hits[2].subtitle.as_deref(), Some("Dark · S01E01"));

    // One hit per work even when several aliases match.
    work(&conn, 20, "anime", "Naruto Shippuden", None);
    alias(&conn, 20, "ja", "Naruto: Shippuuden", "romaji");
    alias(&conn, 20, "ru", "Наруто: Ураганные хроники", "official");
    let hits = find(&conn, "naruto");
    assert_eq!(hits.iter().filter(|h| h.id == 20).count(), 1);
}

#[test]
fn music_collections_kinds_and_groups() {
    let conn = db();
    conn.execute_batch(
        "INSERT INTO files (id, source_id, path, size_bytes, added_at_ms)
             VALUES (1, 1, 'Music/Kino/Gruppa krovi/01.flac', 1, 0),
                    (2, 1, 'Music/Kino/Gruppa krovi/02.flac', 1, 0);
         INSERT INTO music_artists (id, name, name_key, created_at_ms) VALUES (1, 'Кино', 'кино', 0);
         INSERT INTO music_albums (id, album_key, title, year, created_at_ms, updated_at_ms)
             VALUES (1, 'aa:кино|группа крови', 'Группа крови', 1988, 0, 0);
         INSERT INTO music_album_artists (album_id, artist_id, position) VALUES (1, 1, 0);
         INSERT INTO music_tracks (id, file_id, album_id, title, tagged_size_bytes, tagged_at_ms)
             VALUES (1, 1, 1, 'Группа крови', 1, 0), (2, 2, 1, 'Закрой за мной дверь', 1, 0);
         INSERT INTO music_track_artists (track_id, artist_id, role, position)
             VALUES (1, 1, 'artist', 0), (2, 1, 'artist', 0);
         INSERT INTO works (id, kind, title, created_at_ms, updated_at_ms)
             VALUES (1, 'movie', 'Кин-дза-дза!', 0, 0);
         INSERT INTO profiles (id, sync_id, name, created_at_ms, updated_at_ms) VALUES (2, 'kid', 'Kid', 0, 0);
         INSERT INTO collections (id, profile_id, name, kind, created_at_ms, updated_at_ms)
             VALUES (1, NULL, 'Kino classics', 'manual', 0, 0),
                    (2, 2, 'Kino for kids', 'manual', 0, 0);",
    )
    .unwrap();

    assert_eq!(first(&conn, "kino"), (SearchKind::Artist, 1));
    assert_eq!(first(&conn, "кино"), (SearchKind::Artist, 1));
    // Music files are not "unrecognised videos".
    assert!(find(&conn, "01").is_empty());

    let hits = find(&conn, "группа крови");
    let kinds: Vec<_> = ids(&hits);
    assert!(kinds.contains(&(SearchKind::Album, 1)), "{hits:?}");
    assert!(kinds.contains(&(SearchKind::Track, 1)), "{hits:?}");
    let album = hits.iter().find(|h| h.kind == SearchKind::Album).unwrap();
    assert_eq!(album.subtitle.as_deref(), Some("Кино"));
    assert_eq!(album.year, Some(1988));
    // Artist and title together.
    let track = &find(&conn, "kino zakroy")[0];
    assert_eq!((track.kind, track.id), (SearchKind::Track, 2));
    assert_eq!(track.subtitle.as_deref(), Some("Кино · Группа крови"));

    // Kinds filter.
    let only_tracks = search::search(
        &conn,
        "kino",
        &SearchOptions {
            kinds: vec![SearchKind::Track],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(only_tracks.iter().all(|h| h.kind == SearchKind::Track));
    assert_eq!(only_tracks.len(), 2);

    // Cap per kind (grouped results, UI-011).
    let capped = search::search(
        &conn,
        "kino",
        &SearchOptions {
            per_kind: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    let all = find(&conn, "kino");
    let kinds = |hits: &[SearchHit]| {
        let mut k: Vec<SearchKind> = hits.iter().map(|h| h.kind).collect();
        k.sort();
        k.dedup();
        k
    };
    assert_eq!(kinds(&capped), kinds(&all));
    assert_eq!(capped.len(), kinds(&all).len());
    assert!(all.len() > capped.len());

    // Collections of other profiles are hidden.
    let for_profile = |p| {
        search::search(
            &conn,
            "kino",
            &SearchOptions {
                kinds: vec![SearchKind::Collection],
                profile_id: Some(p),
                ..Default::default()
            },
        )
        .unwrap()
        .len()
    };
    assert_eq!(for_profile(1), 1);
    assert_eq!(for_profile(2), 2);

    // Groups follow the best hit.
    let groups = search::group(find(&conn, "kin"));
    assert_eq!(groups[0].kind, SearchKind::Artist);
    let kinds: Vec<_> = groups.iter().map(|g| g.kind).collect();
    for k in [
        SearchKind::Movie,
        SearchKind::Album,
        SearchKind::Track,
        SearchKind::Collection,
    ] {
        assert!(kinds.contains(&k), "{kinds:?}");
    }
}

#[test]
fn index_follows_catalogue_changes() {
    let conn = db();
    file(&conn, 1, "Downloads/Zhmurki.2005.mkv");
    assert_eq!(first(&conn, "жмурки"), (SearchKind::Video, 1));

    // Recognised: the file leaves the index, the work enters it.
    work(&conn, 7, "movie", "Жмурки", Some(2005));
    conn.execute(
        "INSERT INTO matches (id, file_id, work_id, confidence, state, decided_by, created_at_ms, updated_at_ms)
         VALUES (1, 1, 7, 0.9, 'auto', 'scorer', 0, 0)",
        [],
    )
    .unwrap();
    assert_eq!(ids(&find(&conn, "жмурки")), [(SearchKind::Movie, 7)]);

    // Rejected match: the file is unrecognised again.
    conn.execute("UPDATE matches SET state = 'rejected' WHERE id = 1", [])
        .unwrap();
    assert_eq!(find(&conn, "zhmurki").len(), 2);

    // Renamed and deleted rows.
    conn.execute(
        "UPDATE works SET title = 'Dead Man''s Bluff' WHERE id = 7",
        [],
    )
    .unwrap();
    assert_eq!(first(&conn, "bluff"), (SearchKind::Movie, 7));
    assert_eq!(ids(&find(&conn, "жмурки")), [(SearchKind::Video, 1)]);
    conn.execute("UPDATE files SET missing_since_ms = 1 WHERE id = 1", [])
        .unwrap();
    assert!(find(&conn, "жмурки").is_empty());
    conn.execute("DELETE FROM works WHERE id = 7", []).unwrap();
    assert!(find(&conn, "bluff").is_empty());
    let docs: i64 = conn
        .query_row("SELECT count(*) FROM search_docs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(docs, 0);

    // Cascades from a deleted source.
    file(&conn, 2, "Brat.1997.mkv");
    assert_eq!(find(&conn, "brat").len(), 1);
    conn.execute("DELETE FROM sources WHERE id = 1", [])
        .unwrap();
    assert!(find(&conn, "brat").is_empty());

    // An UPSERT overrides `OR IGNORE` inside triggers; the queue must not
    // fail on an entity that is already queued.
    let conn = db();
    for _ in 0..2 {
        conn.execute(
            "INSERT INTO files (source_id, path, size_bytes, added_at_ms) VALUES (1, 'Brat.mkv', 1, 0)
             ON CONFLICT (source_id, path) DO UPDATE SET missing_since_ms = NULL",
            [],
        )
        .unwrap();
    }
    assert_eq!(find(&conn, "brat").len(), 1);

    // Rebuild from scratch gives the same index.
    let conn = db();
    work(&conn, 1, "show", "The Witcher", None);
    alias(&conn, 1, "ru", "Ведьмак", "official");
    assert!(search::pending(&conn).unwrap());
    assert_eq!(search::sync(&conn).unwrap(), 1);
    assert!(!search::pending(&conn).unwrap());
    assert_eq!(search::rebuild(&conn).unwrap(), 1);
    assert_eq!(first(&conn, "ведьмак"), (SearchKind::Show, 1));
}

#[test]
fn sync_works_inside_a_transaction_and_bad_input_is_handled() {
    let mut conn = db();
    let tx = conn.transaction().unwrap();
    work(&tx, 1, "movie", "Солярис", None);
    assert_eq!(first(&tx, "solyaris"), (SearchKind::Movie, 1));
    tx.rollback().unwrap();
    assert!(find(&conn, "солярис").is_empty());

    for q in [
        "", "   ", "?!", "\"", "*", "a\"b", "NOT", "AND OR", "(x", "col:val",
    ] {
        search::search(&conn, q, &SearchOptions::default()).unwrap();
    }
    let bad = SearchOptions {
        limit: 0,
        ..Default::default()
    };
    assert!(search::search(&conn, "x", &bad).is_err());
}

/// Deterministic pseudo-random generator for the large library.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % n
    }
}

#[test]
fn search_over_50_000_items_is_fast() {
    const EN: &[&str] = &[
        "the", "dark", "night", "star", "love", "war", "king", "last", "city", "lost", "secret",
        "game", "black", "house", "dream", "time", "red", "river", "girl", "man", "story", "world",
        "blood", "fire", "ice", "summer", "winter", "road", "home", "ghost", "island", "storm",
    ];
    const RU: &[&str] = &[
        "тайна",
        "ночь",
        "звезда",
        "любовь",
        "война",
        "король",
        "город",
        "игра",
        "дом",
        "сон",
        "время",
        "река",
        "девушка",
        "история",
        "мир",
        "кровь",
        "огонь",
        "лёд",
        "лето",
        "зима",
        "дорога",
        "призрак",
        "остров",
        "буря",
        "брат",
        "судьба",
        "сердце",
        "небо",
    ];
    let mut conn = db();
    let mut rng = Lcg(42);
    let tx = conn.transaction().unwrap();
    {
        let mut ins_work = tx
            .prepare(
                "INSERT INTO works (id, kind, title, year, created_at_ms, updated_at_ms) VALUES (?1, ?2, ?3, ?4, 0, 0)",
            )
            .unwrap();
        let mut ins_title = tx
            .prepare(
                "INSERT INTO titles (work_id, lang, text, kind) VALUES (?1, 'ru', ?2, 'official')",
            )
            .unwrap();
        let mut ins_ep = tx
            .prepare(
                "INSERT INTO episodes (work_id, season_number, episode_number, title) VALUES (?1, 1, ?2, ?3)",
            )
            .unwrap();
        let mut ins_file = tx
            .prepare(
                "INSERT INTO files (source_id, path, size_bytes, added_at_ms) VALUES (1, ?1, 1, 0)",
            )
            .unwrap();
        let words = |rng: &mut Lcg, dict: &[&str], n: usize| {
            (0..n)
                .map(|_| dict[rng.next(dict.len())])
                .collect::<Vec<_>>()
                .join(" ")
        };
        // 20 000 works with a Russian alias, 20 000 episodes, 10 000 files.
        for id in 1..=20_000i64 {
            let n = 1 + rng.next(4);
            let kind = ["movie", "show", "anime"][rng.next(3)];
            let year = 1950 + rng.next(75) as i64;
            ins_work
                .execute(params![id, kind, words(&mut rng, EN, n), year])
                .unwrap();
            ins_title
                .execute(params![id, words(&mut rng, RU, n)])
                .unwrap();
        }
        for i in 0..20_000i64 {
            let work = 1 + rng.next(20_000) as i64;
            let n = 1 + rng.next(3);
            ins_ep
                .execute(params![work, i, words(&mut rng, EN, n)])
                .unwrap();
        }
        for i in 0..10_000 {
            let n = 1 + rng.next(4);
            let name = words(&mut rng, RU, n);
            let lat = pa_recognition::title::translit::to_latin(
                &name,
                pa_recognition::title::translit::Scheme::Common,
            );
            ins_file
                .execute(params![format!(
                    "Films/{}.{i}.1080p.mkv",
                    lat.replace(' ', ".")
                )])
                .unwrap();
        }
    }
    tx.commit().unwrap();
    let started = Instant::now();
    let n = search::sync(&conn).unwrap();
    println!("indexed {n} entities in {:?}", started.elapsed());
    assert_eq!(n, 50_000);

    // The bound of the task is for release builds; debug builds get slack.
    let bound = if cfg!(debug_assertions) {
        Duration::from_millis(500)
    } else {
        Duration::from_millis(50)
    };
    let mut worst = Duration::ZERO;
    for q in [
        "т",
        "the",
        "s",
        "dark night",
        "тайна",
        "тайна ночь",
        "taina",
        "звезд",
        "zvezd",
        "судьба брат",
        "sudba",
        "king of",
        "ghost island 1999",
        "нет такого",
    ] {
        let t = Instant::now();
        let hits = search::search(&conn, q, &SearchOptions::default()).unwrap();
        let took = t.elapsed();
        println!("{q:>20}: {:>3} hits in {took:?}", hits.len());
        worst = worst.max(took);
    }
    assert!(worst < bound, "slowest query took {worst:?}");
    // Russian query, Latin file names.
    assert!(
        find(&conn, "тайна")
            .iter()
            .any(|h| h.kind == SearchKind::Video || h.matched.contains("тайна"))
    );
}
