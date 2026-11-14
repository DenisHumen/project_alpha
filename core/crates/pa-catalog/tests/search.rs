// SPDX-License-Identifier: MPL-2.0
//! Grouped search of the Search screen (UI-011): group order and limits,
//! and the response time on a 50 000-item library. The bound of the task
//! (results under 100 ms after input) covers the app's debounce (50 ms) and
//! this query, so the query itself must stay under 50 ms in release builds.

use std::time::{Duration, Instant};

use pa_catalog::{Catalog, ItemKind};
use pa_library::rusqlite::params;

/// Deterministic pseudo-random numbers for the large library.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % n
    }

    fn words(&mut self, dict: &[&str], n: usize) -> String {
        (0..n)
            .map(|_| dict[self.next(dict.len())])
            .collect::<Vec<_>>()
            .join(" ")
    }
}

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
];

const MOVIES: i64 = 25_000;
const SHOWS: i64 = 2_500;
const EPISODES: i64 = 15_000;
const TRACKS: i64 = 7_500;

/// 25 000 movies and 2 500 shows (each with a Russian alias), 15 000
/// episodes and 7 500 tracks, all with a present file: 50 000 items.
fn library() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let cat = Catalog::open(&path).unwrap();
    let mut conn = pa_library::open(&path).unwrap();
    let mut rng = Lcg(11);
    let tx = conn.transaction().unwrap();
    {
        tx.execute(
            "INSERT INTO sources (id, kind, name, uri, created_at_ms)
             VALUES (1, 'local', 'Media', 'file:///srv/media', 0)",
            [],
        )
        .unwrap();
        let mut work = tx
            .prepare(
                "INSERT INTO works (id, kind, title, year, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, 0, 0)",
            )
            .unwrap();
        let mut alias = tx
            .prepare(
                "INSERT INTO titles (work_id, lang, text, kind) VALUES (?1, 'ru', ?2, 'official')",
            )
            .unwrap();
        let mut file = tx
            .prepare(
                "INSERT INTO files (id, source_id, path, size_bytes, added_at_ms)
                 VALUES (?1, 1, ?2, 1000, ?1)",
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
                "INSERT INTO episodes (id, work_id, season_number, episode_number, title)
                 VALUES (?1, ?2, 1, ?3, ?4)",
            )
            .unwrap();
        let mut match_ep = tx
            .prepare(
                "INSERT INTO match_episodes (match_id, position, episode_id) VALUES (?1, 0, ?2)",
            )
            .unwrap();
        let mut artist = tx
            .prepare(
                "INSERT INTO music_artists (id, name, name_key, created_at_ms) VALUES (?1, ?2, ?3, 0)",
            )
            .unwrap();
        let mut track = tx
            .prepare(
                "INSERT INTO music_tracks (id, file_id, title, track_number, tagged_size_bytes, tagged_at_ms)
                 VALUES (?1, ?2, ?3, 1, 1000, 0)",
            )
            .unwrap();
        let mut track_artist = tx
            .prepare(
                "INSERT INTO music_track_artists (track_id, artist_id, role, position)
                 VALUES (?1, ?2, 'artist', 0)",
            )
            .unwrap();

        let mut file_id = 0i64;
        for id in 1..=MOVIES + SHOWS {
            let n = 1 + rng.next(4);
            let kind = if id > MOVIES { "show" } else { "movie" };
            let title = rng.words(EN, n);
            work.execute(params![id, kind, title, 1950 + rng.next(75) as i64])
                .unwrap();
            alias.execute(params![id, rng.words(RU, n)]).unwrap();
            if kind == "movie" {
                file_id += 1;
                file.execute(params![file_id, format!("Movies/{title}.{id}.mkv")])
                    .unwrap();
                matched.execute(params![file_id, id]).unwrap();
            }
        }
        for id in 1..=EPISODES {
            let show = MOVIES + 1 + (id - 1) % SHOWS;
            file_id += 1;
            file.execute(params![file_id, format!("Shows/{show}/E{id}.mkv")])
                .unwrap();
            matched.execute(params![file_id, show]).unwrap();
            episode
                .execute(params![id, show, id, {
                    let n = 1 + rng.next(3);
                    rng.words(EN, n)
                }])
                .unwrap();
            match_ep.execute(params![file_id, id]).unwrap();
        }
        for id in 1..=500i64 {
            let name = format!("{} {id}", rng.words(EN, 2));
            artist.execute(params![id, name, name]).unwrap();
        }
        for id in 1..=TRACKS {
            file_id += 1;
            file.execute(params![file_id, format!("Music/{id}.flac")])
                .unwrap();
            track
                .execute(params![id, file_id, {
                    let n = 1 + rng.next(4);
                    rng.words(EN, n)
                }])
                .unwrap();
            track_artist
                .execute(params![id, 1 + rng.next(500) as i64])
                .unwrap();
        }
        file_id += 1;
        file.execute(params![file_id, "Music/zyxwv.flac"]).unwrap();
        track
            .execute(params![TRACKS + 1, file_id, "Zyxwv Waltz"])
            .unwrap();
    }
    tx.commit().unwrap();
    let started = Instant::now();
    let indexed = pa_library::search::sync(&conn).unwrap();
    println!("indexed {indexed} entities in {:?}", started.elapsed());
    (dir, cat)
}

#[test]
fn grouped_search_over_50_000_items_is_fast() {
    let (_dir, cat) = library();
    assert_eq!(
        cat.movies().unwrap().len() + cat.shows().unwrap().len() + cat.tracks().unwrap().len(),
        (MOVIES + SHOWS + TRACKS + 1) as usize
    );

    // Groups in display order, each capped; many movie hits do not crowd
    // out episodes and tracks.
    let groups = cat.search_grouped("dark", 20).unwrap();
    let kinds: Vec<ItemKind> = groups.iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        [
            ItemKind::Movie,
            ItemKind::Show,
            ItemKind::Episode,
            ItemKind::Track
        ]
    );
    for g in &groups {
        assert_eq!(g.items.len(), 20, "{:?}", g.kind);
        assert!(g.items.iter().all(|i| i.kind == g.kind), "{:?}", g.kind);
    }
    // Even one letter, matching most of the library, fills every group.
    for q in ["t", "т"] {
        let groups = cat.search_grouped(q, 20).unwrap();
        assert_eq!(groups.len(), 4, "{q}");
        assert!(groups.iter().all(|g| g.items.len() == 20), "{q}");
    }
    // A Russian query finds works through their aliases.
    let ru = cat.search_grouped("тайна", 5).unwrap();
    assert!(
        ru.iter()
            .any(|g| g.kind == ItemKind::Movie && g.items.len() == 5)
    );
    // Only tracks match; an empty query matches nothing.
    let one = cat.search_grouped("zyxw", 20).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].kind, ItemKind::Track);
    assert_eq!(one[0].items[0].title, "Zyxwv Waltz");
    assert!(cat.search_grouped("  ", 20).unwrap().is_empty());
    // The flat search keeps working.
    assert_eq!(cat.search("zyxwv waltz", 10).unwrap().len(), 1);

    // The bound of the task is for release builds; debug builds get slack.
    let bound = if cfg!(debug_assertions) {
        Duration::from_millis(1500)
    } else {
        Duration::from_millis(50)
    };
    let mut worst = Duration::ZERO;
    // As typed, letter by letter, plus whole words and misses.
    let typed = |w: &str| {
        w.char_indices()
            .map(|(i, c)| w[..i + c.len_utf8()].to_owned())
            .collect::<Vec<_>>()
    };
    let mut queries = typed("ghost island");
    queries.extend(typed("тайна ночь"));
    queries.extend(
        [
            "t",
            "the",
            "s",
            "dark night",
            "taina",
            "звезд",
            "zvezd",
            "king of",
            "red river 1999",
            "нет такого",
        ]
        .map(str::to_owned),
    );
    for q in &queries {
        let t = Instant::now();
        let groups = cat.search_grouped(q, 20).unwrap();
        let took = t.elapsed();
        let n: usize = groups.iter().map(|g| g.items.len()).sum();
        println!("{q:>16}: {n:>3} items in {} groups, {took:?}", groups.len());
        worst = worst.max(took);
    }
    println!("slowest: {worst:?} (bound {bound:?})");
    assert!(worst < bound, "slowest query took {worst:?}");
}
