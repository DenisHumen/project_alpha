// SPDX-License-Identifier: MPL-2.0
//! Progress, history, "Continue watching" and Up Next (LIB-011): thresholds,
//! monotonic "watched", the merged row, Up Next across season boundaries and
//! around specials, and the sink contract used by pa-sync.

use pa_library::Error;
use pa_library::progress::{
    self, ContinueQuery, ContinueReason, LocalOnly, PositionReport, ProgressChange, ProgressSink,
    SinkError, Thresholds,
};
use pa_library::rusqlite::{Connection, params};
use pa_model::{MediaKey, MediaKind};

const T0: i64 = 1_763_000_000_000;
const MIN: i64 = 60_000;

fn key(s: &str) -> MediaKey {
    s.parse().unwrap()
}

/// Sink that remembers every change.
#[derive(Default)]
struct Recorder {
    changes: Vec<ProgressChange>,
    fail: bool,
}

impl ProgressSink for Recorder {
    fn device_id(&self) -> &str {
        "phone"
    }

    fn record_change(
        &mut self,
        _: &Connection,
        change: ProgressChange,
        _: i64,
    ) -> Result<(), SinkError> {
        if self.fail {
            return Err("outbox is full".into());
        }
        self.changes.push(change);
        Ok(())
    }
}

fn report(
    conn: &mut Connection,
    sink: &mut dyn ProgressSink,
    k: &str,
    kind: MediaKind,
    position_ms: u64,
    duration_ms: Option<u64>,
    at: i64,
) -> pa_model::PlaybackItem {
    let r = PositionReport {
        profile_id: 1,
        key: key(k),
        kind,
        position_ms,
        duration_ms,
    };
    progress::report_position(conn, sink, &Thresholds::default(), &r, at).unwrap()
}

fn play(conn: &mut Connection, k: &str, kind: MediaKind, percent: u64, at: i64) {
    let mut sink = LocalOnly::new("tv");
    report(conn, &mut sink, k, kind, percent * 1_000, Some(100_000), at);
}

fn row_keys(conn: &Connection, query: &ContinueQuery) -> Vec<(String, ContinueReason)> {
    progress::continue_watching(conn, &Thresholds::default(), 1, query)
        .unwrap()
        .into_iter()
        .map(|e| (e.item.media_key.to_string(), e.reason))
        .collect()
}

fn keys_of(conn: &Connection) -> Vec<String> {
    row_keys(conn, &ContinueQuery::default())
        .into_iter()
        .map(|(k, _)| k)
        .collect()
}

// Thresholds -----------------------------------------------------------------

#[test]
fn thresholds_match_release_0_1() {
    let th = Thresholds::default();
    assert_eq!(th.in_progress_min, 0.02);
    assert_eq!(th.watched_after, 0.95);
    let d = Some(100_000);
    assert!(!th.is_in_progress(1_999, d));
    assert!(th.is_in_progress(2_000, d));
    assert!(th.is_in_progress(95_000, d));
    assert!(!th.is_complete(95_000, d));
    assert!(!th.is_in_progress(95_001, d));
    assert!(th.is_complete(95_001, d));
    // Unknown or zero duration: never in progress, never complete.
    assert!(!th.is_in_progress(50_000, None));
    assert!(!th.is_in_progress(50_000, Some(0)));
    assert!(!th.is_complete(u64::MAX, None));
}

#[test]
fn thresholds_are_configurable_and_checked() {
    let th = Thresholds::new(0.03, 0.90).unwrap();
    assert!(!th.is_in_progress(2_500, Some(100_000)));
    assert!(th.is_in_progress(3_000, Some(100_000)));
    assert!(th.is_complete(90_500, Some(100_000)));
    for (min, max) in [(0.5, 0.5), (0.9, 0.1), (-0.1, 0.9), (0.02, 1.5), (f64::NAN, 0.9)] {
        assert!(
            matches!(Thresholds::new(min, max), Err(Error::Invalid(_))),
            "{min} {max}"
        );
    }
}

#[test]
fn row_boundaries_follow_the_thresholds() {
    let mut conn = pa_library::open_in_memory().unwrap();
    play(&mut conn, "tmdb:movie:1", MediaKind::Movie, 1, T0);
    play(&mut conn, "tmdb:movie:2", MediaKind::Movie, 2, T0 + 1);
    play(&mut conn, "tmdb:movie:3", MediaKind::Movie, 95, T0 + 2);
    play(&mut conn, "tmdb:movie:4", MediaKind::Movie, 96, T0 + 3);
    assert_eq!(keys_of(&conn), ["tmdb:movie:3", "tmdb:movie:2"]);
    let watched = |k| progress::get(&conn, 1, &key(k)).unwrap().unwrap().watched;
    assert!(!watched("tmdb:movie:3"));
    assert!(watched("tmdb:movie:4"));
    // Custom thresholds move the window.
    let th = Thresholds::new(0.01, 0.90).unwrap();
    let row = progress::continue_watching(&conn, &th, 1, &ContinueQuery::default()).unwrap();
    let keys: Vec<_> = row.iter().map(|e| e.item.media_key.to_string()).collect();
    assert_eq!(keys, ["tmdb:movie:2", "tmdb:movie:1"]);
}

#[test]
fn unknown_duration_is_kept_and_reused() {
    let mut conn = pa_library::open_in_memory().unwrap();
    let mut sink = LocalOnly::new("tv");
    report(&mut conn, &mut sink, "tmdb:movie:1", MediaKind::Movie, 10_000, Some(100_000), T0);
    // The player lost the duration: the stored one is kept.
    let item = report(&mut conn, &mut sink, "tmdb:movie:1", MediaKind::Movie, 96_000, None, T0 + 1);
    assert_eq!(item.duration_ms, Some(100_000));
    assert!(item.watched);
    // No duration at all: stored, but not in the row.
    report(&mut conn, &mut sink, "tmdb:movie:2", MediaKind::Movie, 10_000, None, T0 + 2);
    assert!(keys_of(&conn).is_empty());
}

// Watched ---------------------------------------------------------------------

#[test]
fn watched_is_monotonic_and_sent_once() {
    let mut conn = pa_library::open_in_memory().unwrap();
    let mut sink = Recorder::default();
    let k = "tmdb:movie:603";
    report(&mut conn, &mut sink, k, MediaKind::Movie, 50_000, Some(100_000), T0);
    report(&mut conn, &mut sink, k, MediaKind::Movie, 97_000, Some(100_000), T0 + 1);
    report(&mut conn, &mut sink, k, MediaKind::Movie, 99_000, Some(100_000), T0 + 2);
    // Rewatching from the start does not clear "watched".
    let item = report(&mut conn, &mut sink, k, MediaKind::Movie, 10_000, Some(100_000), T0 + 3);
    assert!(item.watched);
    assert_eq!(item.position_ms, 10_000);
    assert_eq!(item.device_id, "phone");
    assert!(keys_of(&conn).is_empty());
    let watched: Vec<_> = sink
        .changes
        .iter()
        .filter(|c| matches!(c, ProgressChange::Watched { .. }))
        .collect();
    assert_eq!(watched.len(), 1);
    assert_eq!(sink.changes.len(), 5);
    assert_eq!(
        sink.changes[1],
        ProgressChange::Position {
            profile: "default".into(),
            key: key(k),
            kind: MediaKind::Movie,
            position_ms: 97_000,
            duration_ms: Some(100_000),
        }
    );
    assert_eq!(
        sink.changes[2],
        ProgressChange::Watched {
            profile: "default".into(),
            key: key(k)
        }
    );
}

#[test]
fn mark_watched_and_unwatched() {
    let mut conn = pa_library::open_in_memory().unwrap();
    let mut sink = Recorder::default();
    let k = key("tmdb:episode:1399:1:1");
    // Never played here: a row is created with the kind of the key.
    assert!(progress::mark_watched(&mut conn, &mut sink, 1, &k, T0).unwrap());
    assert!(!progress::mark_watched(&mut conn, &mut sink, 1, &k, T0 + 1).unwrap());
    let item = progress::get(&conn, 1, &k).unwrap().unwrap();
    assert!(item.watched);
    assert_eq!(item.source_type, MediaKind::Episode);
    assert_eq!(sink.changes.len(), 1);

    assert!(progress::mark_unwatched(&mut conn, &mut sink, 1, &k, T0 + 2).unwrap());
    let item = progress::get(&conn, 1, &k).unwrap().unwrap();
    assert!(!item.watched);
    assert_eq!(item.position_ms, 0);
    assert_eq!(item.updated_at_ms, T0 + 2);
    assert!(matches!(sink.changes[1], ProgressChange::Unwatched { .. }));
    assert!(matches!(
        sink.changes[2],
        ProgressChange::Position { position_ms: 0, .. }
    ));
    // Nothing to clear any more.
    assert!(!progress::mark_unwatched(&mut conn, &mut sink, 1, &k, T0 + 3).unwrap());
    assert!(!progress::mark_unwatched(&mut conn, &mut sink, 1, &key("tmdb:movie:9"), T0).unwrap());
    assert_eq!(sink.changes.len(), 3);
}

#[test]
fn sink_failure_rolls_the_change_back() {
    let mut conn = pa_library::open_in_memory().unwrap();
    let mut sink = Recorder {
        fail: true,
        ..Recorder::default()
    };
    let r = PositionReport {
        profile_id: 1,
        key: key("tmdb:movie:1"),
        kind: MediaKind::Movie,
        position_ms: 10_000,
        duration_ms: Some(100_000),
    };
    let err = progress::report_position(&mut conn, &mut sink, &Thresholds::default(), &r, T0);
    assert!(matches!(err, Err(Error::Sync(_))));
    assert_eq!(progress::get(&conn, 1, &r.key).unwrap(), None);
    let err = progress::mark_watched(&mut conn, &mut sink, 1, &r.key, T0);
    assert!(matches!(err, Err(Error::Sync(_))));
    assert_eq!(progress::get(&conn, 1, &r.key).unwrap(), None);
}

#[test]
fn unknown_profile_is_rejected() {
    let mut conn = pa_library::open_in_memory().unwrap();
    let r = PositionReport {
        profile_id: 42,
        key: key("tmdb:movie:1"),
        kind: MediaKind::Movie,
        position_ms: 1,
        duration_ms: None,
    };
    let err = progress::report_position(
        &mut conn,
        &mut LocalOnly::new("tv"),
        &Thresholds::default(),
        &r,
        T0,
    );
    assert!(matches!(err, Err(Error::Invalid("profile"))));
}

// Continue watching and history --------------------------------------------------

#[test]
fn row_merges_domains_latest_first_without_tracks_and_clips() {
    let mut conn = pa_library::open_in_memory().unwrap();
    play(&mut conn, "tmdb:movie:1", MediaKind::Movie, 30, T0);
    play(&mut conn, "twitch:vod:2000", MediaKind::TwitchVod, 40, T0 + MIN);
    play(&mut conn, "mb:recording:0f2a9cb8-7d34-4b5d-9f43-8c3d1f0c6a11", MediaKind::Track, 50, T0 + 2 * MIN);
    play(&mut conn, "twitch:clip:SomeSlug", MediaKind::TwitchClip, 50, T0 + 3 * MIN);
    play(&mut conn, "tmdb:episode:1399:1:1", MediaKind::Episode, 20, T0 + 4 * MIN);
    play(&mut conn, "file:abc123", MediaKind::Video, 20, T0 + 5 * MIN);
    play(&mut conn, "twitch:rec:777", MediaKind::TwitchRecording, 20, T0 + 6 * MIN);
    assert_eq!(
        keys_of(&conn),
        [
            "twitch:rec:777",
            "file:abc123",
            "tmdb:episode:1399:1:1",
            "twitch:vod:2000",
            "tmdb:movie:1"
        ]
    );
    // Section rows: Twitch only, movies only.
    let twitch = ContinueQuery {
        kinds: vec![MediaKind::TwitchVod, MediaKind::TwitchRecording],
        ..ContinueQuery::default()
    };
    assert_eq!(row_keys(&conn, &twitch).len(), 2);
    let movies = ContinueQuery {
        kinds: vec![MediaKind::Movie],
        limit: 1,
        ..ContinueQuery::default()
    };
    assert_eq!(
        row_keys(&conn, &movies),
        [("tmdb:movie:1".to_owned(), ContinueReason::InProgress)]
    );
    // An update moves an item to the front.
    play(&mut conn, "tmdb:movie:1", MediaKind::Movie, 35, T0 + 7 * MIN);
    assert_eq!(keys_of(&conn)[0], "tmdb:movie:1");
    let limited = ContinueQuery {
        limit: 2,
        ..ContinueQuery::default()
    };
    assert_eq!(row_keys(&conn, &limited).len(), 2);
}

#[test]
fn rows_are_per_profile() {
    let mut conn = pa_library::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO profiles (id, sync_id, name, created_at_ms, updated_at_ms)
         VALUES (2, 'kid', 'Kid', 0, 0)",
        [],
    )
    .unwrap();
    play(&mut conn, "tmdb:movie:1", MediaKind::Movie, 30, T0);
    let row = progress::continue_watching(&conn, &Thresholds::default(), 2, &ContinueQuery::default())
        .unwrap();
    assert!(row.is_empty());
}

#[test]
fn removed_entry_returns_after_the_next_playback() {
    let mut conn = pa_library::open_in_memory().unwrap();
    play(&mut conn, "tmdb:movie:1", MediaKind::Movie, 30, T0);
    play(&mut conn, "tmdb:movie:2", MediaKind::Movie, 30, T0 + 1);
    assert!(progress::hide_from_continue(&conn, 1, &key("tmdb:movie:1"), T0 + 10).unwrap());
    assert!(!progress::hide_from_continue(&conn, 1, &key("tmdb:movie:5"), T0 + 10).unwrap());
    assert_eq!(keys_of(&conn), ["tmdb:movie:2"]);
    // Hidden even if the device clock is behind the row.
    assert!(progress::hide_from_continue(&conn, 1, &key("tmdb:movie:2"), T0 - 1_000).unwrap());
    assert!(keys_of(&conn).is_empty());
    play(&mut conn, "tmdb:movie:1", MediaKind::Movie, 31, T0 + 20);
    assert_eq!(keys_of(&conn), ["tmdb:movie:1"]);
}

#[test]
fn history_lists_everything_latest_first_and_pages() {
    let mut conn = pa_library::open_in_memory().unwrap();
    play(&mut conn, "tmdb:movie:1", MediaKind::Movie, 30, T0);
    play(&mut conn, "tmdb:movie:2", MediaKind::Movie, 99, T0 + 1);
    play(&mut conn, "tmdb:movie:3", MediaKind::Movie, 1, T0 + 2);
    play(&mut conn, "tmdb:movie:4", MediaKind::Movie, 0, T0 + 3);
    let all: Vec<_> = progress::history(&conn, 1, None, 10)
        .unwrap()
        .into_iter()
        .map(|i| i.media_key.to_string())
        .collect();
    assert_eq!(all, ["tmdb:movie:3", "tmdb:movie:2", "tmdb:movie:1"]);
    let page1 = progress::history(&conn, 1, None, 2).unwrap();
    let page2 = progress::history(&conn, 1, Some(page1[1].updated_at_ms), 2).unwrap();
    assert_eq!(page2.len(), 1);
    assert_eq!(page2[0].media_key, key("tmdb:movie:1"));
}

// Up Next ------------------------------------------------------------------------

/// A show with a TMDB id and its episodes; each `(season, episode, type,
/// has_file)`. Returns the episode ids in the same order.
struct Show {
    work_id: i64,
    episodes: Vec<i64>,
}

fn add_show(conn: &Connection, tmdb_id: Option<u64>, eps: &[(u32, u32, &str, bool)]) -> Show {
    conn.execute(
        "INSERT OR IGNORE INTO sources (id, kind, name, uri, created_at_ms)
         VALUES (1, 'local', 'Media', 'file:///media', 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO works (kind, title, created_at_ms, updated_at_ms) VALUES ('show', 'Show', 0, 0)",
        [],
    )
    .unwrap();
    let work_id = conn.last_insert_rowid();
    if let Some(id) = tmdb_id {
        conn.execute(
            "INSERT INTO external_ids (work_id, provider, ext_kind, ext_id) VALUES (?1, 'tmdb', 'tv', ?2)",
            params![work_id, id.to_string()],
        )
        .unwrap();
    }
    let mut episodes = Vec::new();
    for (s, e, ty, has_file) in eps {
        conn.execute(
            "INSERT INTO episodes (work_id, episode_type, season_number, episode_number, runtime_ms)
             VALUES (?1, ?2, ?3, ?4, 2400000)",
            params![work_id, ty, s, e],
        )
        .unwrap();
        let ep = conn.last_insert_rowid();
        if *has_file {
            let fp = format!("{:04x}{:04x}", work_id, ep);
            conn.execute(
                "INSERT INTO files (source_id, path, size_bytes, added_at_ms, fingerprint)
                 VALUES (1, ?1, 1, 0, ?2)",
                params![format!("show{work_id}/s{s}e{e}-{ep}.mkv"), fp],
            )
            .unwrap();
            let file = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO matches (file_id, work_id, confidence, state, decided_by,
                                      created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, 0.99, 'auto', 'scorer', 0, 0)",
                params![file, work_id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO match_episodes (match_id, position, episode_id) VALUES (?1, 0, ?2)",
                params![conn.last_insert_rowid(), ep],
            )
            .unwrap();
        }
        episodes.push(ep);
    }
    Show { work_id, episodes }
}

fn next_of(conn: &Connection, episode_id: i64) -> Option<(Option<u32>, Option<u32>)> {
    progress::next_episode(conn, episode_id)
        .unwrap()
        .map(|e| (e.season_number, e.episode_number))
}

#[test]
fn next_episode_crosses_season_boundaries() {
    let conn = pa_library::open_in_memory().unwrap();
    let show = add_show(
        &conn,
        Some(1399),
        &[
            (1, 1, "regular", true),
            (1, 2, "regular", true),
            (2, 1, "regular", true),
            (2, 2, "regular", true),
        ],
    );
    let [e11, e12, e21, e22] = show.episodes[..] else {
        unreachable!()
    };
    assert_eq!(next_of(&conn, e11), Some((Some(1), Some(2))));
    assert_eq!(next_of(&conn, e12), Some((Some(2), Some(1))));
    assert_eq!(next_of(&conn, e21), Some((Some(2), Some(2))));
    assert_eq!(next_of(&conn, e22), None);
    let next = progress::next_episode(&conn, e12).unwrap().unwrap();
    assert_eq!(next.key, key("tmdb:episode:1399:2:1"));
    assert_eq!(next.work_id, show.work_id);
    assert_eq!(next.runtime_ms, Some(2_400_000));
}

#[test]
fn next_episode_skips_specials_extras_and_missing_files() {
    let conn = pa_library::open_in_memory().unwrap();
    let show = add_show(
        &conn,
        Some(1399),
        &[
            (0, 1, "regular", true),  // S0E1 special
            (0, 2, "regular", true),  // S0E2 special
            (0, 3, "trailer", true),  // never next
            (1, 1, "regular", true),
            (1, 2, "credit", true),   // OP/ED in a regular season
            (1, 3, "special", true),  // typed special inside S1
            (1, 4, "regular", false), // no file
            (1, 5, "regular", true),
        ],
    );
    let e = &show.episodes;
    // Regular chain: S1E1 -> S1E5 (credit, special and the missing E4 skipped).
    assert_eq!(next_of(&conn, e[3]), Some((Some(1), Some(5))));
    // Specials chain: S0E1 -> S0E2 -> S1E3 (typed special) -> end.
    assert_eq!(next_of(&conn, e[0]), Some((Some(0), Some(2))));
    assert_eq!(next_of(&conn, e[1]), Some((Some(1), Some(3))));
    assert_eq!(next_of(&conn, e[5]), None);
    // A missing file that comes back is next again.
    conn.execute(
        "UPDATE files SET missing_since_ms = 1 WHERE fingerprint = ?1",
        params![format!("{:04x}{:04x}", show.work_id, e[7])],
    )
    .unwrap();
    assert_eq!(next_of(&conn, e[3]), None);
}

#[test]
fn up_next_follows_the_latest_finished_episode() {
    let mut conn = pa_library::open_in_memory().unwrap();
    let show = add_show(
        &conn,
        Some(1399),
        &[
            (0, 1, "regular", true),
            (1, 1, "regular", true),
            (1, 2, "regular", true),
            (2, 1, "regular", true),
            (2, 2, "regular", true),
        ],
    );
    play(&mut conn, "tmdb:episode:1399:1:1", MediaKind::Episode, 99, T0);
    play(&mut conn, "tmdb:movie:7", MediaKind::Movie, 50, T0 + MIN);
    assert_eq!(
        row_keys(&conn, &ContinueQuery::default()),
        [
            ("tmdb:movie:7".to_owned(), ContinueReason::InProgress),
            ("tmdb:episode:1399:1:2".to_owned(), ContinueReason::UpNext),
        ]
    );
    // Finishing the season finale offers the next season.
    play(&mut conn, "tmdb:episode:1399:1:2", MediaKind::Episode, 99, T0 + 2 * MIN);
    let row = progress::continue_watching(&conn, &Thresholds::default(), 1, &ContinueQuery::default())
        .unwrap();
    let up = &row[0];
    assert_eq!(up.reason, ContinueReason::UpNext);
    assert_eq!(up.item.media_key, key("tmdb:episode:1399:2:1"));
    assert_eq!(up.item.position_ms, 0);
    assert_eq!(up.item.duration_ms, Some(2_400_000));
    assert_eq!(up.item.updated_at_ms, T0 + 2 * MIN);
    assert_eq!(up.anchor_key, key("tmdb:episode:1399:1:2"));
    assert_eq!(up.episode.as_ref().unwrap().episode_id, show.episodes[3]);
    // Watching a special later does not move Up Next.
    play(&mut conn, "tmdb:episode:1399:0:1", MediaKind::Episode, 99, T0 + 3 * MIN);
    let ups: Vec<_> = row_keys(&conn, &ContinueQuery::default())
        .into_iter()
        .filter(|(_, r)| *r == ContinueReason::UpNext)
        .collect();
    assert_eq!(ups, [("tmdb:episode:1399:2:1".to_owned(), ContinueReason::UpNext)]);
    // Already watched episodes ahead are skipped.
    let mut sink = LocalOnly::new("tv");
    progress::mark_watched(&mut conn, &mut sink, 1, &key("tmdb:episode:1399:2:1"), T0 + 4 * MIN)
        .unwrap();
    assert_eq!(keys_of(&conn)[0], "tmdb:episode:1399:2:2");
    // The last episode finished: nothing next.
    progress::mark_watched(&mut conn, &mut sink, 1, &key("tmdb:episode:1399:2:2"), T0 + 5 * MIN)
        .unwrap();
    assert_eq!(keys_of(&conn), ["tmdb:movie:7"]);
    // Without Up Next only in-progress items.
    let plain = ContinueQuery {
        up_next: false,
        ..ContinueQuery::default()
    };
    assert_eq!(row_keys(&conn, &plain).len(), 1);
}

#[test]
fn in_progress_episode_replaces_up_next_of_its_show() {
    let mut conn = pa_library::open_in_memory().unwrap();
    add_show(
        &conn,
        Some(1399),
        &[
            (1, 1, "regular", true),
            (1, 2, "regular", true),
            (1, 3, "regular", true),
        ],
    );
    play(&mut conn, "tmdb:episode:1399:1:1", MediaKind::Episode, 99, T0);
    play(&mut conn, "tmdb:episode:1399:1:2", MediaKind::Episode, 40, T0 + MIN);
    let row = progress::continue_watching(&conn, &Thresholds::default(), 1, &ContinueQuery::default())
        .unwrap();
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].reason, ContinueReason::InProgress);
    assert_eq!(row[0].item.media_key, key("tmdb:episode:1399:1:2"));
    assert!(row[0].episode.is_some());
    // An episode started below 2 % is not in progress: Up Next shows it with
    // its own position.
    play(&mut conn, "tmdb:episode:1399:1:2", MediaKind::Episode, 1, T0 + 2 * MIN);
    let row = progress::continue_watching(&conn, &Thresholds::default(), 1, &ContinueQuery::default())
        .unwrap();
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].reason, ContinueReason::UpNext);
    assert_eq!(row[0].item.position_ms, 1_000);
    assert_eq!(row[0].item.duration_ms, Some(100_000));
}

#[test]
fn removed_up_next_returns_after_the_next_episode_is_finished() {
    let mut conn = pa_library::open_in_memory().unwrap();
    add_show(
        &conn,
        Some(1399),
        &[
            (1, 1, "regular", true),
            (1, 2, "regular", true),
            (1, 3, "regular", true),
        ],
    );
    play(&mut conn, "tmdb:episode:1399:1:1", MediaKind::Episode, 99, T0);
    let row = progress::continue_watching(&conn, &Thresholds::default(), 1, &ContinueQuery::default())
        .unwrap();
    progress::hide_from_continue(&conn, 1, &row[0].anchor_key, T0 + 1).unwrap();
    assert!(keys_of(&conn).is_empty());
    play(&mut conn, "tmdb:episode:1399:1:2", MediaKind::Episode, 99, T0 + 2);
    assert_eq!(keys_of(&conn), ["tmdb:episode:1399:1:3"]);
}

#[test]
fn up_next_works_for_shows_without_tmdb_through_file_keys() {
    let mut conn = pa_library::open_in_memory().unwrap();
    let show = add_show(
        &conn,
        None,
        &[(1, 1, "regular", true), (1, 2, "regular", true)],
    );
    let keys1 = progress::episode_keys(&conn, show.episodes[0]).unwrap();
    assert_eq!(keys1.len(), 1);
    assert!(matches!(keys1[0], MediaKey::File { .. }));
    assert_eq!(
        progress::episode_for_key(&conn, &keys1[0]).unwrap(),
        Some(show.episodes[0])
    );
    play(&mut conn, &keys1[0].to_string(), MediaKind::Episode, 99, T0);
    let next = progress::episode_keys(&conn, show.episodes[1]).unwrap();
    assert_eq!(keys_of(&conn), [next[0].to_string()]);
    // A watched mark on the file key counts as the episode being watched.
    assert!(progress::is_episode_watched(&conn, 1, show.episodes[0]).unwrap());
    assert!(!progress::is_episode_watched(&conn, 1, show.episodes[1]).unwrap());
}

#[test]
fn up_next_is_one_per_show() {
    let mut conn = pa_library::open_in_memory().unwrap();
    add_show(
        &conn,
        Some(1),
        &[(1, 1, "regular", true), (1, 2, "regular", true), (1, 3, "regular", true)],
    );
    add_show(
        &conn,
        Some(2),
        &[(1, 1, "regular", true), (1, 2, "regular", true)],
    );
    play(&mut conn, "tmdb:episode:1:1:1", MediaKind::Episode, 99, T0);
    play(&mut conn, "tmdb:episode:2:1:1", MediaKind::Episode, 99, T0 + 1);
    play(&mut conn, "tmdb:episode:1:1:2", MediaKind::Episode, 99, T0 + 2);
    assert_eq!(keys_of(&conn), ["tmdb:episode:1:1:3", "tmdb:episode:2:1:2"]);
}

// Remote changes -------------------------------------------------------------------

#[test]
fn remote_changes_follow_lww_and_monotonic_watched() {
    let conn = pa_library::open_in_memory().unwrap();
    let k = key("tmdb:movie:1");
    let pos = |p: u64| ProgressChange::Position {
        profile: "default".into(),
        key: k.clone(),
        kind: MediaKind::Movie,
        position_ms: p,
        duration_ms: Some(100_000),
    };
    let watched = ProgressChange::Watched {
        profile: "default".into(),
        key: k.clone(),
    };
    let unwatched = ProgressChange::Unwatched {
        profile: "default".into(),
        key: k.clone(),
    };
    assert!(progress::apply_remote(&conn, &pos(30_000), T0 + 10, "tv").unwrap());
    // Older position: ignored.
    assert!(!progress::apply_remote(&conn, &pos(10_000), T0 + 5, "phone").unwrap());
    assert_eq!(progress::get(&conn, 1, &k).unwrap().unwrap().position_ms, 30_000);
    // Watched even if older, once.
    assert!(progress::apply_remote(&conn, &watched, T0 + 1, "phone").unwrap());
    assert!(!progress::apply_remote(&conn, &watched, T0 + 20, "phone").unwrap());
    let item = progress::get(&conn, 1, &k).unwrap().unwrap();
    assert!(item.watched);
    assert_eq!(item.updated_at_ms, T0 + 10);
    // A position never clears it.
    assert!(progress::apply_remote(&conn, &pos(5_000), T0 + 30, "tv").unwrap());
    assert!(progress::get(&conn, 1, &k).unwrap().unwrap().watched);
    // An older unwatch loses, a newer one wins.
    assert!(!progress::apply_remote(&conn, &unwatched, T0 + 29, "tv").unwrap());
    assert!(progress::apply_remote(&conn, &unwatched, T0 + 31, "tv").unwrap());
    assert!(!progress::get(&conn, 1, &k).unwrap().unwrap().watched);
    // A watched mark for an item never seen here creates the row.
    let other = ProgressChange::Watched {
        profile: "default".into(),
        key: key("twitch:vod:5"),
    };
    assert!(progress::apply_remote(&conn, &other, T0, "tv").unwrap());
    let item = progress::get(&conn, 1, &key("twitch:vod:5")).unwrap().unwrap();
    assert_eq!(item.source_type, MediaKind::TwitchVod);
    // Unknown profile: skipped.
    let stranger = ProgressChange::Watched {
        profile: "guest".into(),
        key: k,
    };
    assert!(!progress::apply_remote(&conn, &stranger, T0, "tv").unwrap());
}

#[test]
fn hidden_column_comes_from_a_lib_011_migration() {
    let conn = pa_library::open_in_memory().unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('progress') WHERE name = 'hidden_at_ms'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    assert!(
        pa_library::MIGRATIONS
            .iter()
            .any(|m| m.name.contains("LIB-011") && m.version > 202_611_141_200)
    );
}
