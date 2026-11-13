// SPDX-License-Identifier: MPL-2.0
//! Serde round-trip of every domain type.

use std::fmt::Debug;

use pa_model::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).unwrap();
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, value, "{json}");
    json
}

fn work() -> Work {
    Work {
        id: 7,
        kind: WorkKind::Anime,
        anime_type: Some(AnimeType::Tv),
        title: "Frieren".into(),
        original_title: Some("葬送のフリーレン".into()),
        original_lang: Some("jpn".into()),
        year: Some(2023),
        ids: ExternalIds {
            tmdb: Some("tv:209867".into()),
            anidb: Some(17617),
            ..Default::default()
        },
    }
}

fn playback() -> PlaybackItem {
    PlaybackItem {
        media_key: "twitch:vod:2189457123".parse().unwrap(),
        source_type: MediaKind::TwitchVod,
        position_ms: 2_471_200,
        duration_ms: Some(10_800_000),
        updated_at_ms: 1_794_000_000_000,
        device_id: "device-a".into(),
        watched: false,
    }
}

#[test]
fn video_entities() {
    let json = round_trip(&work());
    assert!(json.contains(r#""kind":"anime""#));
    assert!(json.contains(r#""anime_type":"tv""#));
    assert!(!json.contains("imdb"), "None fields are skipped: {json}");

    round_trip(&Work {
        id: 1,
        kind: WorkKind::Movie,
        anime_type: None,
        title: "The Matrix".into(),
        original_title: None,
        original_lang: None,
        year: Some(1999),
        ids: ExternalIds::default(),
    });
    round_trip(&Season {
        id: 2,
        work_id: 7,
        scheme: NumberingScheme::TvdbAbsolute,
        number: 0,
        title: Some("Specials".into()),
        year: None,
    });
    round_trip(&Episode {
        id: 3,
        work_id: 7,
        season_id: Some(2),
        episode_type: EpisodeType::Recap,
        season_number: Some(1),
        episode_number: Some(12),
        absolute_number: Some(12),
        air_date: Some("2023-11-24".into()),
        title: None,
        runtime_ms: Some(1_440_000),
    });
    round_trip(&MediaFile {
        id: 4,
        source_id: 1,
        path: "Anime/Frieren/S01E12.mkv".into(),
        size_bytes: 1_400_000_000,
        mtime_ms: Some(1_700_000_000_000),
        container: Some("matroska".into()),
        duration_ms: Some(1_440_000),
        fingerprint: Some("8e245d9679d31e12".into()),
        oshash: Some("8e245d9679d31e12".into()),
    });
}

#[test]
fn music_entities() {
    round_trip(&Artist {
        id: 1,
        name: "Artist".into(),
        sort_name: None,
        mbid: Some("5b11f4ce-a62d-471e-81fc-a69a8278c7da".into()),
    });
    round_trip(&Album {
        id: 2,
        title: "Album".into(),
        artist_ids: vec![1],
        year: Some(1991),
        mbid: None,
    });
    let track = Track {
        id: 3,
        title: "Track".into(),
        album_id: Some(2),
        artist_ids: vec![1],
        disc_number: Some(1),
        track_number: Some(4),
        duration_ms: Some(301_000),
        recording_mbid: Some("b1a9c0e9-d987-4042-ae91-78d6a3267d69".into()),
        file_id: None,
    };
    round_trip(&track);
    assert_eq!(
        track.media_key().unwrap().to_string(),
        "mb:recording:b1a9c0e9-d987-4042-ae91-78d6a3267d69"
    );
}

#[test]
fn twitch_entities() {
    round_trip(&Streamer {
        user_id: "141981764".into(),
        login: "twitchdev".into(),
        display_name: "TwitchDev".into(),
        broadcaster_type: BroadcasterType::Partner,
        description: None,
        avatar_url: None,
        offline_banner_url: None,
        last_seen_live_at_ms: Some(1_794_000_000_000),
        updated_at_ms: 1_794_000_000_000,
    });
    let video = Video {
        video_id: "2189457123".into(),
        user_id: "141981764".into(),
        stream_id: Some("41375541868".into()),
        video_type: VideoType::Archive,
        title: "Stream".into(),
        published_at_ms: 1_794_000_000_000,
        duration_ms: 10_800_000,
        view_count: 12,
        thumbnail_url: None,
        is_sub_only: Some(false),
        availability: VideoAvailability::Online,
    };
    let json = round_trip(&video);
    assert!(json.contains(r#""video_type":"archive""#));
    assert_eq!(video.media_key().to_string(), "twitch:vod:2189457123");
}

#[test]
fn items() {
    round_trip(&MediaItem {
        key: "tmdb:episode:209867:1:12".parse().unwrap(),
        kind: MediaKind::Episode,
        title: "Episode 12".into(),
        subtitle: Some("Frieren · S1·E12".into()),
        duration_ms: Some(1_440_000),
        work_id: Some(7),
        episode_id: Some(3),
        track_id: None,
        file_ids: vec![4],
    });
    let json = round_trip(&playback());
    assert!(json.contains(r#""media_key":"twitch:vod:2189457123""#));
    assert!(json.contains(r#""source_type":"twitch_vod""#));
}

#[test]
fn unknown_fields_and_missing_defaults_are_tolerated() {
    let json = r#"{"media_key":"tmdb:movie:603","source_type":"movie","position_ms":1,
        "updated_at_ms":5,"device_id":"d","added_in_v2":{"x":1}}"#;
    let item: PlaybackItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.duration_ms, None);
    assert!(!item.watched);
}

#[test]
fn versioned_envelope() {
    let wrapped = Versioned::new(vec![playback()]);
    let json = round_trip(&wrapped);
    assert!(json.starts_with(r#"{"schema_version":1,"data":"#));
    assert_eq!(wrapped.clone().into_supported().unwrap(), vec![playback()]);

    let newer = Versioned {
        schema_version: SCHEMA_VERSION + 1,
        data: (),
    };
    assert_eq!(
        newer.into_supported(),
        Err(UnsupportedSchema {
            found: SCHEMA_VERSION + 1,
            supported: SCHEMA_VERSION
        })
    );
}

#[test]
fn keys_from_entities() {
    let movie = Work {
        kind: WorkKind::Movie,
        anime_type: None,
        ids: ExternalIds {
            tmdb: Some("movie:603".into()),
            ..Default::default()
        },
        ..work()
    };
    assert_eq!(movie.media_key(), Some(MediaKey::TmdbMovie { id: 603 }));
    assert_eq!(work().media_key(), None, "a TV anime has no work-level key");
    assert_eq!(work().ids.tmdb_tv(), Some(209_867));

    let file = MediaFile {
        id: 1,
        source_id: 1,
        path: "a.mkv".into(),
        size_bytes: 1,
        mtime_ms: None,
        container: None,
        duration_ms: None,
        fingerprint: Some("00ff".into()),
        oshash: None,
    };
    assert_eq!(file.fingerprint_key().unwrap().to_string(), "file:00ff");
    let bad = MediaFile {
        fingerprint: Some("00FF".into()),
        ..file
    };
    assert_eq!(bad.fingerprint_key(), None);
}

#[test]
fn continue_watching_window() {
    let mut item = playback();
    assert!(item.is_in_progress());
    item.position_ms = 100_000; // < 2 %
    assert!(!item.is_in_progress());
    item.position_ms = 10_700_000; // > 95 %
    assert!(!item.is_in_progress());
    item.position_ms = 20_000_000;
    assert_eq!(item.progress(), Some(1.0));
    item.duration_ms = None;
    assert!(!item.is_in_progress());
    item.duration_ms = Some(10_800_000);
    item.position_ms = 2_471_200;
    item.watched = true;
    assert!(!item.is_in_progress());
}
