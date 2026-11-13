// SPDX-License-Identifier: MPL-2.0
//! Merge rules, the replica and the SQLite outbox (SYNC-001).

use pa_model::{MediaKey, MediaKind, SCHEMA_VERSION};
use pa_sync::{Error, Hlc, Op, OpBody, OpId, Replica, SyncState, hlc::MAX_DRIFT_MS, outbox};
use uuid::Uuid;

const T0: i64 = 1_763_000_000_000;

fn movie() -> MediaKey {
    "tmdb:movie:603".parse().unwrap()
}

fn op(n: u128, wall_ms: u64, device: &str, body: OpBody) -> Op {
    Op {
        op_id: OpId(Uuid::from_u128(n)),
        hlc: Hlc {
            wall_ms,
            counter: 0,
            device: device.into(),
        },
        body,
    }
}

fn progress(position_ms: u64) -> OpBody {
    OpBody::ProgressUpsert {
        profile: "default".into(),
        key: movie(),
        kind: MediaKind::Movie,
        position_ms,
        duration_ms: Some(8_160_000),
    }
}

fn watched(on: bool) -> OpBody {
    let (profile, key) = ("default".into(), movie());
    if on {
        OpBody::Watched { profile, key }
    } else {
        OpBody::Unwatch { profile, key }
    }
}

#[test]
fn newest_progress_wins_regardless_of_arrival() {
    let mut s = SyncState::default();
    assert!(s.apply(&op(2, 20, "tv", progress(2_000))));
    assert!(!s.apply(&op(1, 10, "phone", progress(1_000))));
    let reg = &s.progress[&("default".to_owned(), movie())];
    assert_eq!(reg.value.position_ms, 2_000);
}

#[test]
fn progress_never_clears_watched_and_unwatch_needs_a_newer_hlc() {
    let mut s = SyncState::default();
    s.apply(&op(1, 10, "tv", watched(true)));
    // A device that has not seen the mark keeps sending progress.
    s.apply(&op(2, 20, "phone", progress(5_000)));
    assert!(s.is_watched("default", &movie()));
    // An older unwatch loses.
    s.apply(&op(3, 5, "phone", watched(false)));
    assert!(s.is_watched("default", &movie()));
    // A newer one wins.
    s.apply(&op(4, 30, "phone", watched(false)));
    assert!(!s.is_watched("default", &movie()));
}

#[test]
fn profiles_do_not_conflict() {
    let mut s = SyncState::default();
    s.apply(&op(1, 10, "tv", progress(1_000)));
    let mut kid = progress(9_000);
    if let OpBody::ProgressUpsert { profile, .. } = &mut kid {
        *profile = "kid".into();
    }
    s.apply(&op(2, 20, "tv", kid));
    assert_eq!(s.progress.len(), 2);
}

#[test]
fn deleted_playlist_stays_deleted_against_older_writes() {
    let mut s = SyncState::default();
    let upsert = |name: &str| OpBody::PlaylistUpsert {
        profile: "default".into(),
        playlist: "p1".into(),
        name: name.into(),
        video: false,
    };
    s.apply(&op(1, 10, "a", upsert("Mix")));
    s.apply(&op(
        2,
        11,
        "a",
        OpBody::PlaylistItems {
            profile: "default".into(),
            playlist: "p1".into(),
            items: vec![movie()],
        },
    ));
    assert_eq!(s.playlist("default", "p1").unwrap().1, &[movie()]);
    s.apply(&op(
        3,
        20,
        "b",
        OpBody::PlaylistDelete {
            profile: "default".into(),
            playlist: "p1".into(),
        },
    ));
    s.apply(&op(4, 15, "a", upsert("Renamed late")));
    assert!(s.playlist("default", "p1").is_none());
}

#[test]
fn invalid_ops_are_rejected() {
    let conn = pa_library::open_in_memory().unwrap();
    let mut r = Replica::new("phone").unwrap();
    let bad = [
        OpBody::Setting {
            key: "sources.nas.password".into(),
            value: Some("\"hunter2\"".into()),
        },
        OpBody::Setting {
            key: "player.speed".into(),
            value: Some("not json".into()),
        },
        OpBody::Rating {
            profile: "default".into(),
            target: "tmdb:tv:1".into(),
            rating: Some(11),
        },
        OpBody::Favorite {
            profile: String::new(),
            target: "tmdb:tv:1".into(),
            favorite: true,
        },
    ];
    for body in bad {
        assert!(matches!(r.record(&conn, body, T0), Err(Error::Invalid(_))));
    }
    assert_eq!(outbox::len(&conn).unwrap(), 0);
}

#[test]
fn record_and_receive_between_devices() {
    let conn = pa_library::open_in_memory().unwrap();
    let mut phone = Replica::new("phone").unwrap();
    let mut tv = Replica::new("tv").unwrap();
    let a = phone.record(&conn, progress(1_000), T0).unwrap();
    // The TV's clock is behind; its next op still orders after the phone's.
    assert!(tv.receive(&a, T0 - 60_000).unwrap());
    assert!(!tv.receive(&a, T0 - 60_000).unwrap(), "duplicate");
    let b = tv.record(&conn, progress(2_000), T0 - 60_000).unwrap();
    assert!(b.hlc > a.hlc);
    assert!(phone.receive(&b, T0).unwrap());
    assert_eq!(phone.state(), tv.state());
    // A peer with a clock far in the future is refused.
    let mut far = b.clone();
    far.hlc.wall_ms = (T0 as u64) + MAX_DRIFT_MS + 1;
    far.op_id = OpId::new_v7();
    assert!(matches!(
        phone.receive(&far, T0),
        Err(Error::ClockDrift { .. })
    ));
}

#[test]
fn wire_form_round_trips_and_is_versioned() {
    let o = op(7, 10, "tv", progress(1_000));
    let payload = outbox::encode(&o).unwrap();
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(v["schema_version"], SCHEMA_VERSION);
    assert_eq!(v["data"]["op"], "progress.upsert");
    assert_eq!(v["data"]["key"], "tmdb:movie:603");
    assert_eq!(v["data"]["op_id"], "00000000-0000-0000-0000-000000000007");
    assert_eq!(outbox::decode(&payload).unwrap(), o);

    let newer = payload.replace(
        &format!("\"schema_version\":{SCHEMA_VERSION}"),
        &format!("\"schema_version\":{}", SCHEMA_VERSION + 1),
    );
    assert!(outbox::decode(&newer).is_err());
}

#[test]
fn outbox_is_idempotent_and_retries_with_backoff() {
    let conn = pa_library::open_in_memory().unwrap();
    let a = op(1, 10, "phone", progress(1_000));
    let b = op(2, 11, "phone", watched(true));
    assert!(outbox::enqueue(&conn, &a, T0).unwrap());
    assert!(!outbox::enqueue(&conn, &a, T0).unwrap(), "same op_id");
    assert!(outbox::enqueue(&conn, &b, T0).unwrap());
    assert_eq!(outbox::len(&conn).unwrap(), 2);

    let due = outbox::due(&conn, T0, 10).unwrap();
    assert_eq!(due.iter().map(|p| &p.op).collect::<Vec<_>>(), [&a, &b]);

    // `a` fails twice: 1 s, then 2 s.
    assert_eq!(
        outbox::fail(&conn, a.op_id, T0, "offline").unwrap(),
        Some(1)
    );
    assert_eq!(outbox::due(&conn, T0, 10).unwrap().len(), 1);
    assert_eq!(outbox::next_attempt_at(&conn).unwrap(), Some(T0));
    let retry = outbox::due(&conn, T0 + 1_000, 10).unwrap();
    assert_eq!(retry.len(), 2);
    assert_eq!(retry[0].attempts, 1);
    assert_eq!(
        outbox::fail(&conn, a.op_id, T0 + 1_000, "offline").unwrap(),
        Some(2)
    );
    assert!(outbox::ack(&conn, b.op_id).unwrap());
    assert!(!outbox::ack(&conn, b.op_id).unwrap(), "duplicate ack");
    assert_eq!(outbox::due(&conn, T0 + 2_999, 10).unwrap(), []);
    assert_eq!(outbox::next_attempt_at(&conn).unwrap(), Some(T0 + 3_000));
    assert_eq!(outbox::due(&conn, T0 + 3_000, 10).unwrap()[0].attempts, 2);

    assert!(outbox::ack(&conn, a.op_id).unwrap());
    assert_eq!(outbox::len(&conn).unwrap(), 0);
    assert_eq!(outbox::fail(&conn, a.op_id, T0, "x").unwrap(), None);
    assert_eq!(outbox::next_attempt_at(&conn).unwrap(), None);
}

#[test]
fn outbox_survives_reopen() {
    let dir = std::env::temp_dir().join(format!("pa-sync-outbox-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("library.db");
    let a = op(1, 10, "phone", progress(1_000));
    {
        let conn = pa_library::open(&path).unwrap();
        outbox::enqueue(&conn, &a, T0).unwrap();
    }
    let conn = pa_library::open(&path).unwrap();
    assert_eq!(outbox::due(&conn, T0, 10).unwrap()[0].op, a);
    drop(conn);
    std::fs::remove_dir_all(&dir).unwrap();
}
