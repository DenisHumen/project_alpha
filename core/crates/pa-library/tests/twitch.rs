// SPDX-License-Identifier: MPL-2.0
//! TWI-007: Twitch data model queries, the "Streams" section, FTS5 search and
//! the 24 h cache cap.

use pa_library::progress::{self, ContinueQuery, LocalOnly, PositionReport, Thresholds};
use pa_library::rusqlite::{Connection, params};
use pa_library::twitch::{
    self, Asset, AssetKind, CACHE_TTL_MS, Category, CategorySegment, ChatLog, Clip,
    FavoriteSource, FollowChange, LiveSession, RecordRule, SearchKind, SegmentOwner, VideoQuery,
};
use pa_library::{Error, open_in_memory};
use pa_model::{BroadcasterType, MediaKey, MediaKind, Streamer, Video, VideoAvailability, VideoType};

const T0: i64 = 1_763_000_000_000;
const HOUR: i64 = 60 * 60 * 1000;
const DAY: i64 = 24 * HOUR;

fn streamer(user_id: &str, login: &str, name: &str) -> Streamer {
    Streamer {
        user_id: user_id.into(),
        login: login.into(),
        display_name: name.into(),
        broadcaster_type: BroadcasterType::Affiliate,
        description: Some("about".into()),
        avatar_url: Some("https://example.invalid/a.png".into()),
        offline_banner_url: None,
        last_seen_live_at_ms: Some(T0 - HOUR),
        updated_at_ms: T0,
    }
}

fn video(video_id: &str, user_id: &str, title: &str, published_at_ms: i64) -> Video {
    Video {
        video_id: video_id.into(),
        user_id: user_id.into(),
        stream_id: None,
        video_type: VideoType::Archive,
        title: title.into(),
        published_at_ms,
        duration_ms: 3 * HOUR as u64,
        view_count: 10,
        thumbnail_url: None,
        is_sub_only: Some(false),
        availability: VideoAvailability::Online,
    }
}

fn segment(game_id: &str, start_ms: u64, end_ms: Option<u64>) -> CategorySegment {
    CategorySegment {
        game_id: game_id.into(),
        title_at_time: None,
        start_ms,
        end_ms,
        source: "eventsub".into(),
    }
}

fn add_file(conn: &Connection, id: i64, path: &str) {
    conn.execute(
        "INSERT OR IGNORE INTO sources (id, kind, name, uri, created_at_ms)
         VALUES (1, 'local', 'Disk', 'file:///media', 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files (id, source_id, path, size_bytes, added_at_ms) VALUES (?1, 1, ?2, 1, 0)",
        params![id, path],
    )
    .unwrap();
}

fn vod_asset(file_id: i64, user_id: &str, video_id: &str) -> Asset {
    Asset {
        id: 0,
        file_id,
        kind: AssetKind::Vod,
        user_id: user_id.into(),
        video_id: Some(video_id.into()),
        stream_id: None,
        clip_slug: None,
        title: "Saved VOD".into(),
        recorded_at_ms: None,
        duration_ms: Some(3 * HOUR as u64),
        quality: Some("1080p60".into()),
        has_ads_marked: false,
        offset_rec_to_vod_ms: None,
        origin: "server".into(),
        created_at_ms: 0,
    }
}

fn recording_asset(file_id: i64, user_id: &str, stream_id: &str) -> Asset {
    Asset {
        kind: AssetKind::Recording,
        video_id: None,
        stream_id: Some(stream_id.into()),
        title: "Recording".into(),
        recorded_at_ms: Some(T0),
        ..vod_asset(file_id, user_id, "1")
    }
}

#[test]
fn streamers_round_trip_and_keep_the_local_avatar() {
    let conn = open_in_memory().unwrap();
    let s = streamer("141981764", "twitchdev", "TwitchDev");
    twitch::upsert_streamer(&conn, &s, T0).unwrap();
    assert_eq!(twitch::streamer(&conn, "141981764").unwrap().unwrap(), s);
    assert!(twitch::set_avatar_local(&conn, "141981764", Some("avatars/141981764.png")).unwrap());

    let renamed = Streamer {
        display_name: "TwitchDev2".into(),
        last_seen_live_at_ms: None,
        ..s.clone()
    };
    twitch::upsert_streamer(&conn, &renamed, T0 + 1).unwrap();
    let back = twitch::streamer(&conn, "141981764").unwrap().unwrap();
    assert_eq!(back.display_name, "TwitchDev2");
    // A report without "last live" keeps the known one.
    assert_eq!(back.last_seen_live_at_ms, s.last_seen_live_at_ms);
    let local: Option<String> = conn
        .query_row(
            "SELECT avatar_local FROM twitch_streamers WHERE user_id = '141981764'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(local.as_deref(), Some("avatars/141981764.png"));

    let bad = Streamer {
        user_id: "abc".into(),
        ..s
    };
    assert!(matches!(
        twitch::upsert_streamer(&conn, &bad, T0),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn favorites_work_without_login() {
    let mut conn = open_in_memory().unwrap();
    let mut sink = LocalOnly::new("phone");
    twitch::upsert_streamer(&conn, &streamer("2", "bravo", "Bravo"), T0).unwrap();
    twitch::upsert_streamer(&conn, &streamer("1", "alpha", "Alpha"), T0).unwrap();
    for (id, source) in [
        ("2", FavoriteSource::Local),
        ("1", FavoriteSource::TwitchFollow),
        ("3", FavoriteSource::Local),
    ] {
        assert!(twitch::set_favorite(&mut conn, &mut sink, 1, id, source, true, T0).unwrap());
    }
    // Already a favourite: unchanged, keeps its source.
    assert!(
        !twitch::set_favorite(&mut conn, &mut sink, 1, "1", FavoriteSource::Local, true, T0)
            .unwrap()
    );
    let favs = twitch::favorites(&conn, 1).unwrap();
    let ids: Vec<_> = favs.iter().map(|f| f.user_id.as_str()).collect();
    // By display name; the channel without metadata last.
    assert_eq!(ids, ["1", "2", "3"]);
    assert_eq!(favs[0].source, FavoriteSource::TwitchFollow);
    assert!(favs[0].notify);
    assert_eq!(favs[1].streamer.as_ref().unwrap().display_name, "Bravo");
    assert!(favs[2].streamer.is_none());

    assert!(twitch::set_notify(&conn, 1, "2", false).unwrap());
    assert!(!twitch::favorites(&conn, 1).unwrap()[1].notify);

    assert!(
        twitch::set_favorite(&mut conn, &mut sink, 1, "3", FavoriteSource::Local, false, T0)
            .unwrap()
    );
    assert_eq!(twitch::favorites(&conn, 1).unwrap().len(), 2);
    assert!(matches!(
        twitch::set_favorite(&mut conn, &mut sink, 99, "3", FavoriteSource::Local, true, T0),
        Err(Error::Invalid("profile"))
    ));
}

#[test]
fn remote_follows_are_applied_by_profile_sync_id() {
    let conn = open_in_memory().unwrap();
    let change = |followed| FollowChange {
        profile: "default".into(),
        user_id: "77".into(),
        followed,
    };
    assert!(twitch::apply_remote_follow(&conn, &change(true), T0).unwrap());
    assert!(!twitch::apply_remote_follow(&conn, &change(true), T0).unwrap());
    let favs = twitch::favorites(&conn, 1).unwrap();
    assert_eq!(favs[0].source, FavoriteSource::Local);
    assert_eq!(favs[0].added_at_ms, T0);
    assert!(twitch::apply_remote_follow(&conn, &change(false), T0).unwrap());
    let unknown = FollowChange {
        profile: "nobody".into(),
        ..change(true)
    };
    assert!(!twitch::apply_remote_follow(&conn, &unknown, T0).unwrap());
    let bad_id = FollowChange {
        user_id: "not-a-twitch-id".into(),
        ..change(true)
    };
    assert!(!twitch::apply_remote_follow(&conn, &bad_id, T0).unwrap());
}

#[test]
fn vod_filter_uses_chapters_not_the_current_category() {
    let mut conn = open_in_memory().unwrap();
    twitch::upsert_streamer(&conn, &streamer("1", "alpha", "Alpha"), T0).unwrap();
    twitch::upsert_video(&conn, &video("101", "1", "Monday", T0 - 2 * DAY), T0).unwrap();
    twitch::upsert_video(&conn, &video("102", "1", "Tuesday", T0 - DAY), T0).unwrap();
    // The channel is in "Just Chatting" now; game X was on air in VOD 101.
    twitch::replace_segments(
        &mut conn,
        &SegmentOwner::Video("101".into()),
        &[segment("509658", 0, Some(600_000)), segment("33214", 600_000, None)],
    )
    .unwrap();
    twitch::replace_segments(
        &mut conn,
        &SegmentOwner::Video("102".into()),
        &[segment("509658", 0, None)],
    )
    .unwrap();

    let all = twitch::channel_videos(&conn, 1, "1", &VideoQuery::default()).unwrap();
    let ids: Vec<_> = all.iter().map(|e| e.video.video_id.as_str()).collect();
    assert_eq!(ids, ["102", "101"]);
    // Affiliate archive: deleted by Twitch after 14 days.
    assert_eq!(all[0].expires_at_ms, Some(T0 - DAY + 14 * DAY));

    let game_x = VideoQuery {
        game_id: Some("33214".into()),
        ..Default::default()
    };
    let hits = twitch::channel_videos(&conn, 1, "1", &game_x).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].video.video_id, "101");

    let chapters = twitch::segments(&conn, &SegmentOwner::Video("101".into())).unwrap();
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[1].game_id, "33214");
    assert_eq!(chapters[1].end_ms, None);

    let expiring = VideoQuery {
        expires_before_ms: Some(T0 + 12 * DAY + HOUR),
        ..Default::default()
    };
    let soon = twitch::channel_videos(&conn, 1, "1", &expiring).unwrap();
    assert_eq!(soon.len(), 1);
    assert_eq!(soon[0].video.video_id, "101");

    let limited = VideoQuery {
        limit: 1,
        ..Default::default()
    };
    assert_eq!(
        twitch::channel_videos(&conn, 1, "1", &limited).unwrap().len(),
        1
    );
}

#[test]
fn vod_progress_is_shared_by_online_and_saved_copies() {
    let mut conn = open_in_memory().unwrap();
    twitch::upsert_video(&conn, &video("101", "1", "Monday", T0), T0).unwrap();
    let key: MediaKey = "twitch:vod:101".parse().unwrap();
    let th = Thresholds::default();
    let mut sink = LocalOnly::new("tv");
    let report = PositionReport {
        profile_id: 1,
        key: key.clone(),
        kind: MediaKind::TwitchVod,
        position_ms: HOUR as u64,
        duration_ms: Some(3 * HOUR as u64),
    };
    progress::report_position(&mut conn, &mut sink, &th, &report, T0).unwrap();

    add_file(&conn, 10, "Twitch/alpha/101.mkv");
    let id = twitch::add_asset(&conn, &vod_asset(10, "1", "101"), T0).unwrap();
    let saved = twitch::assets_for_key(&conn, &key).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, id);
    assert_eq!(saved[0].media_key(), Some(key.clone()));

    let entries = twitch::channel_videos(&conn, 1, "1", &VideoQuery::default()).unwrap();
    assert_eq!(entries[0].saved_files, 1);
    assert_eq!(
        entries[0].progress.as_ref().unwrap().position_ms,
        HOUR as u64
    );
    let saved_only = VideoQuery {
        saved_only: true,
        ..Default::default()
    };
    assert_eq!(
        twitch::channel_videos(&conn, 1, "1", &saved_only).unwrap().len(),
        1
    );

    // "Continue watching" does not depend on the metadata cache: a purge
    // leaves progress alone (and this VOD is saved, so it stays too).
    twitch::purge_expired(&conn, T0 + 2 * DAY).unwrap();
    assert!(twitch::video(&conn, "101").unwrap().is_some());
    let row = progress::continue_watching(&conn, &th, 1, &ContinueQuery::default()).unwrap();
    assert_eq!(row[0].item.media_key, key);
}

#[test]
fn recordings_link_to_their_vod() {
    let mut conn = open_in_memory().unwrap();
    twitch::upsert_live_session(
        &conn,
        &LiveSession {
            stream_id: "555".into(),
            user_id: "1".into(),
            started_at_ms: T0,
            ended_at_ms: None,
            title_last: Some("Speedrun".into()),
            peak_viewers: Some(100),
            video_id: None,
        },
        T0,
    )
    .unwrap();
    // A later report: higher peak, end known; title kept when absent.
    twitch::upsert_live_session(
        &conn,
        &LiveSession {
            stream_id: "555".into(),
            user_id: "1".into(),
            started_at_ms: T0,
            ended_at_ms: Some(T0 + 2 * HOUR),
            title_last: None,
            peak_viewers: Some(50),
            video_id: None,
        },
        T0 + 2 * HOUR,
    )
    .unwrap();
    let s = twitch::live_session(&conn, "555").unwrap().unwrap();
    assert_eq!(s.peak_viewers, Some(100));
    assert_eq!(s.title_last.as_deref(), Some("Speedrun"));
    assert_eq!(s.ended_at_ms, Some(T0 + 2 * HOUR));

    add_file(&conn, 20, "Twitch/alpha/rec-555.mkv");
    let rec = twitch::add_asset(&conn, &recording_asset(20, "1", "555"), T0).unwrap();
    let rec_key: MediaKey = "twitch:rec:555".parse().unwrap();
    assert_eq!(twitch::assets_for_key(&conn, &rec_key).unwrap()[0].id, rec);

    // An offset needs a linked VOD.
    let bad = Asset {
        offset_rec_to_vod_ms: Some(1),
        ..recording_asset(20, "1", "555")
    };
    assert!(twitch::add_asset(&conn, &bad, T0).is_err());

    assert_eq!(twitch::link_recording(&mut conn, "555", "9001", -4_000).unwrap(), 1);
    let linked = twitch::assets_for_key(&conn, &"twitch:vod:9001".parse().unwrap()).unwrap();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].kind, AssetKind::Recording);
    assert_eq!(linked[0].offset_rec_to_vod_ms, Some(-4_000));
    // The recording keeps its own key.
    assert_eq!(linked[0].media_key(), Some(rec_key));
    assert_eq!(
        twitch::live_session(&conn, "555").unwrap().unwrap().video_id.as_deref(),
        Some("9001")
    );

    // Downloaded copies come before linked recordings.
    add_file(&conn, 21, "Twitch/alpha/9001.mkv");
    twitch::add_asset(&conn, &vod_asset(21, "1", "9001"), T0 + 1).unwrap();
    let both = twitch::assets_for_key(&conn, &"twitch:vod:9001".parse().unwrap()).unwrap();
    assert_eq!(
        both.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [AssetKind::Vod, AssetKind::Recording]
    );
    assert_eq!(twitch::channel_assets(&conn, "1").unwrap().len(), 2);
}

#[test]
fn chat_logs_belong_to_saved_files() {
    let conn = open_in_memory().unwrap();
    add_file(&conn, 10, "Twitch/alpha/101.mkv");
    let asset = twitch::add_asset(&conn, &vod_asset(10, "1", "101"), T0).unwrap();
    let log = ChatLog {
        path: "Twitch/alpha/101.chat.json.gz".into(),
        format: "tdl_json_v1".into(),
        message_count: 12_345,
        first_offset_ms: Some(0),
        last_offset_ms: Some(3 * HOUR as u64),
        emote_snapshot: true,
    };
    twitch::set_chat_log(&conn, asset, &log, T0).unwrap();
    let updated = ChatLog {
        message_count: 12_400,
        ..log
    };
    twitch::set_chat_log(&conn, asset, &updated, T0 + 1).unwrap();
    assert_eq!(twitch::chat_log(&conn, asset).unwrap().unwrap(), updated);

    // Removing the file removes the asset and its chat log.
    conn.execute("DELETE FROM files WHERE id = 10", []).unwrap();
    assert!(twitch::chat_log(&conn, asset).unwrap().is_none());
    let assets: i64 = conn
        .query_row("SELECT count(*) FROM twitch_assets", [], |r| r.get(0))
        .unwrap();
    assert_eq!(assets, 0);
}

#[test]
fn clips_and_categories_round_trip() {
    let conn = open_in_memory().unwrap();
    let clip = Clip {
        slug: "Funny-Clip_1".into(),
        broadcaster_id: "1".into(),
        creator_name: Some("viewer".into()),
        video_id: Some("101".into()),
        vod_offset_ms: Some(60_000),
        game_id: Some("509658".into()),
        title: "Big moment".into(),
        created_at_ms: T0,
        duration_ms: 30_000,
        thumbnail_url: None,
        view_count: 5,
    };
    twitch::upsert_clip(&conn, &clip, T0).unwrap();
    assert_eq!(twitch::clip(&conn, "Funny-Clip_1").unwrap().unwrap(), clip);
    assert_eq!(clip.media_key().to_string(), "twitch:clip:Funny-Clip_1");
    let bad = Clip {
        slug: "no spaces".into(),
        ..clip
    };
    assert!(twitch::upsert_clip(&conn, &bad, T0).is_err());

    let cat = Category {
        game_id: "509658".into(),
        name: "Just Chatting".into(),
        box_art_url: None,
        igdb_id: None,
    };
    twitch::upsert_category(&conn, &cat, T0).unwrap();
    assert_eq!(twitch::category(&conn, "509658").unwrap().unwrap(), cat);
}

#[test]
fn record_rules_round_trip() {
    let conn = open_in_memory().unwrap();
    let rule = RecordRule {
        profile_id: 1,
        user_id: "1".into(),
        quality: "best".into(),
        with_chat: true,
        retention_days: Some(30),
        keep_last_n: None,
        categories: Some(vec!["509658".into(), "33214".into()]),
        replace_with_vod: true,
        enabled: true,
    };
    let id = twitch::upsert_record_rule(&conn, &rule, T0).unwrap();
    let all = RecordRule {
        user_id: "2".into(),
        categories: None,
        ..rule.clone()
    };
    twitch::upsert_record_rule(&conn, &all, T0).unwrap();
    // Upsert keeps the id.
    let disabled = RecordRule {
        enabled: false,
        ..rule.clone()
    };
    assert_eq!(twitch::upsert_record_rule(&conn, &disabled, T0 + 1).unwrap(), id);
    assert_eq!(twitch::record_rules(&conn, Some(1)).unwrap(), [disabled, all]);
    assert!(twitch::record_rules(&conn, Some(2)).unwrap().is_empty());
    assert!(twitch::delete_record_rule(&conn, 1, "2").unwrap());
    assert_eq!(twitch::record_rules(&conn, None).unwrap().len(), 1);
    let bad = RecordRule {
        categories: Some(vec!["x".into()]),
        ..rule
    };
    assert!(twitch::upsert_record_rule(&conn, &bad, T0).is_err());
}

#[test]
fn channels_section_lists_favorites_and_saved_channels() {
    let mut conn = open_in_memory().unwrap();
    let mut sink = LocalOnly::new("phone");
    twitch::upsert_streamer(&conn, &streamer("1", "alpha", "Alpha"), T0).unwrap();
    twitch::upsert_streamer(&conn, &streamer("2", "bravo", "Bravo"), T0).unwrap();
    twitch::upsert_streamer(&conn, &streamer("3", "charlie", "Charlie"), T0).unwrap();
    twitch::set_favorite(&mut conn, &mut sink, 1, "2", FavoriteSource::Local, true, T0).unwrap();
    add_file(&conn, 10, "Twitch/alpha/101.mkv");
    twitch::add_asset(&conn, &vod_asset(10, "1", "101"), T0).unwrap();

    let channels = twitch::channels(&conn, 1).unwrap();
    let summary: Vec<_> = channels
        .iter()
        .map(|c| (c.user_id.as_str(), c.favorite, c.saved_files))
        .collect();
    assert_eq!(summary, [("1", false, 1), ("2", true, 0)]);
    assert_eq!(channels[0].streamer.as_ref().unwrap().login, "alpha");
}

#[test]
fn search_indexes_streamers_vods_categories_clips_and_lives() {
    let conn = open_in_memory().unwrap();
    twitch::upsert_streamer(&conn, &streamer("1", "xqc", "xQc"), T0).unwrap();
    twitch::upsert_streamer(&conn, &streamer("2", "bratishkinoff", "Братишкин"), T0).unwrap();
    twitch::upsert_video(&conn, &video("101", "1", "Elden Ring any% world record", T0), T0)
        .unwrap();
    twitch::upsert_video(&conn, &video("102", "2", "Смотрим кино с чатом", T0), T0).unwrap();
    twitch::upsert_category(
        &conn,
        &Category {
            game_id: "512953".into(),
            name: "Elden Ring".into(),
            box_art_url: None,
            igdb_id: None,
        },
        T0,
    )
    .unwrap();
    twitch::upsert_live_session(
        &conn,
        &LiveSession {
            stream_id: "555".into(),
            user_id: "1".into(),
            started_at_ms: T0,
            ended_at_ms: None,
            title_last: Some("Marathon".into()),
            peak_viewers: None,
            video_id: None,
        },
        T0,
    )
    .unwrap();

    let kinds = |q: &str| -> Vec<(SearchKind, String)> {
        twitch::search(&conn, q, 10)
            .unwrap()
            .into_iter()
            .map(|h| (h.kind, h.twitch_id))
            .collect()
    };
    let mut elden = kinds("elden");
    elden.sort_by_key(|(k, _)| *k as u8);
    assert_eq!(
        elden,
        [
            (SearchKind::Video, "101".to_owned()),
            (SearchKind::Category, "512953".to_owned())
        ]
    );
    // Prefixes, case and Cyrillic.
    assert_eq!(kinds("XQ"), [(SearchKind::Streamer, "1".to_owned())]);
    assert_eq!(kinds("брат"), [(SearchKind::Streamer, "2".to_owned())]);
    assert_eq!(kinds("кино чат"), [(SearchKind::Video, "102".to_owned())]);
    assert_eq!(kinds("marath"), [(SearchKind::LiveSession, "555".to_owned())]);
    // FTS5 syntax in user input is taken literally.
    assert!(kinds("\"elden\" OR NEAR(*").is_empty());
    assert!(kinds("  ").is_empty());

    // Renames and deletes keep the index current.
    twitch::upsert_video(&conn, &video("101", "1", "Sekiro", T0), T0).unwrap();
    assert_eq!(kinds("elden"), [(SearchKind::Category, "512953".to_owned())]);
    assert_eq!(kinds("sekiro"), [(SearchKind::Video, "101".to_owned())]);
    conn.execute("DELETE FROM twitch_streamers WHERE user_id = '1'", [])
        .unwrap();
    assert!(kinds("xqc").is_empty());
}

#[test]
fn purge_keeps_only_what_the_user_saved() {
    let mut conn = open_in_memory().unwrap();
    let mut sink = LocalOnly::new("phone");
    // Channel 1: a favourite. Channel 2: a saved VOD. Channel 3: a record
    // rule. Channel 4: just browsed.
    for (id, login) in [("1", "fav"), ("2", "saved"), ("3", "rule"), ("4", "browsed")] {
        twitch::upsert_streamer(&conn, &streamer(id, login, login), T0).unwrap();
    }
    twitch::set_favorite(&mut conn, &mut sink, 1, "1", FavoriteSource::Local, true, T0).unwrap();
    twitch::upsert_record_rule(
        &conn,
        &RecordRule {
            profile_id: 1,
            user_id: "3".into(),
            quality: "best".into(),
            with_chat: false,
            retention_days: None,
            keep_last_n: None,
            categories: None,
            replace_with_vod: false,
            enabled: true,
        },
        T0,
    )
    .unwrap();
    twitch::upsert_video(&conn, &video("201", "2", "Saved", T0), T0).unwrap();
    twitch::upsert_video(&conn, &video("401", "4", "Browsed", T0), T0).unwrap();
    for (vid, game) in [("201", "11"), ("401", "44")] {
        twitch::replace_segments(
            &mut conn,
            &SegmentOwner::Video(vid.into()),
            &[segment(game, 0, None)],
        )
        .unwrap();
    }
    for game in ["11", "44"] {
        twitch::upsert_category(
            &conn,
            &Category {
                game_id: game.into(),
                name: format!("Game {game}"),
                box_art_url: None,
                igdb_id: None,
            },
            T0,
        )
        .unwrap();
    }
    add_file(&conn, 10, "Twitch/saved/201.mkv");
    twitch::add_asset(&conn, &vod_asset(10, "2", "201"), T0).unwrap();
    twitch::upsert_live_session(
        &conn,
        &LiveSession {
            stream_id: "499".into(),
            user_id: "4".into(),
            started_at_ms: T0,
            ended_at_ms: None,
            title_last: Some("Gone".into()),
            peak_viewers: None,
            video_id: None,
        },
        T0,
    )
    .unwrap();
    twitch::upsert_clip(
        &conn,
        &Clip {
            slug: "BrowsedClip".into(),
            broadcaster_id: "4".into(),
            creator_name: None,
            video_id: None,
            vod_offset_ms: None,
            game_id: Some("44".into()),
            title: "Browsed clip".into(),
            created_at_ms: T0,
            duration_ms: 1,
            thumbnail_url: None,
            view_count: 0,
        },
        T0,
    )
    .unwrap();

    // Within 24 h nothing goes.
    let none = twitch::purge_expired(&conn, T0 + CACHE_TTL_MS).unwrap();
    assert_eq!(none, Default::default());

    let report = twitch::purge_expired(&conn, T0 + CACHE_TTL_MS + 1).unwrap();
    assert_eq!(
        report,
        twitch::PurgeReport {
            videos: 1,
            live_sessions: 1,
            clips: 1,
            categories: 1,
            streamers: 1,
        }
    );
    for id in ["1", "2", "3"] {
        assert!(twitch::streamer(&conn, id).unwrap().is_some(), "{id}");
    }
    assert!(twitch::streamer(&conn, "4").unwrap().is_none());
    assert!(twitch::video(&conn, "201").unwrap().is_some());
    assert!(twitch::video(&conn, "401").unwrap().is_none());
    // Chapters go with their VOD; the category of a kept chapter stays.
    assert_eq!(
        twitch::segments(&conn, &SegmentOwner::Video("401".into()))
            .unwrap()
            .len(),
        0
    );
    assert!(twitch::category(&conn, "11").unwrap().is_some());
    assert!(twitch::category(&conn, "44").unwrap().is_none());
    // The search index follows.
    assert!(twitch::search(&conn, "browsed", 10).unwrap().is_empty());
    assert_eq!(twitch::search(&conn, "saved", 10).unwrap().len(), 2);
}

#[test]
fn deleted_vods_go_at_once_unless_saved() {
    let conn = open_in_memory().unwrap();
    twitch::upsert_video(&conn, &video("101", "1", "Deleted", T0), T0).unwrap();
    twitch::upsert_video(&conn, &video("102", "1", "Deleted but saved", T0), T0).unwrap();
    add_file(&conn, 10, "Twitch/alpha/102.mkv");
    twitch::add_asset(&conn, &vod_asset(10, "1", "102"), T0).unwrap();
    for id in ["101", "102"] {
        assert!(twitch::set_availability(&conn, id, VideoAvailability::Deleted, T0 + 1).unwrap());
    }
    let report = twitch::purge_expired(&conn, T0 + 2).unwrap();
    assert_eq!(report.videos, 1);
    assert!(twitch::video(&conn, "101").unwrap().is_none());
    assert_eq!(
        twitch::video(&conn, "102").unwrap().unwrap().availability,
        VideoAvailability::Deleted
    );
}

#[test]
fn vod_expiry_follows_the_channel_type() {
    use twitch::vod_expires_at_ms as exp;
    assert_eq!(
        exp(VideoType::Archive, BroadcasterType::Normal, 0),
        Some(7 * DAY)
    );
    assert_eq!(
        exp(VideoType::Archive, BroadcasterType::Affiliate, 0),
        Some(14 * DAY)
    );
    assert_eq!(
        exp(VideoType::Archive, BroadcasterType::Partner, 0),
        Some(60 * DAY)
    );
    assert_eq!(exp(VideoType::Highlight, BroadcasterType::Normal, 0), None);
    assert_eq!(exp(VideoType::Upload, BroadcasterType::Partner, 0), None);
}
