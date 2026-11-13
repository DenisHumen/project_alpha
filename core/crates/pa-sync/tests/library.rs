// SPDX-License-Identifier: MPL-2.0
//! LIB-011 integration: local progress changes queue sync ops, received ops
//! land in the library of the other device.

use pa_library::progress::{self, PositionReport, Thresholds};
use pa_model::{MediaKey, MediaKind};
use pa_sync::{OpBody, Replica, outbox};

const T0: i64 = 1_763_000_000_000;

fn key() -> MediaKey {
    "tmdb:episode:1399:3:9".parse().unwrap()
}

fn report(position_ms: u64) -> PositionReport {
    PositionReport {
        profile_id: 1,
        key: key(),
        kind: MediaKind::Episode,
        position_ms,
        duration_ms: Some(3_000_000),
    }
}

fn queued_ops(conn: &pa_library::rusqlite::Connection) -> Vec<OpBody> {
    outbox::due(conn, i64::MAX, 100)
        .unwrap()
        .into_iter()
        .map(|p| p.op.body)
        .collect()
}

#[test]
fn local_changes_enqueue_ops() {
    let mut db = pa_library::open_in_memory().unwrap();
    let mut phone = Replica::new("phone").unwrap();
    let th = Thresholds::default();
    progress::report_position(&mut db, &mut phone, &th, &report(600_000), T0).unwrap();
    progress::report_position(&mut db, &mut phone, &th, &report(2_900_000), T0 + 10_000).unwrap();
    progress::mark_unwatched(&mut db, &mut phone, 1, &key(), T0 + 20_000).unwrap();
    let ops = queued_ops(&db);
    let names: Vec<_> = ops
        .iter()
        .map(|b| match b {
            OpBody::ProgressUpsert { position_ms, .. } => format!("upsert {position_ms}"),
            OpBody::Watched { .. } => "watched".to_owned(),
            OpBody::Unwatch { .. } => "unwatch".to_owned(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        [
            "upsert 600000",
            "upsert 2900000",
            "watched",
            "unwatch",
            "upsert 0"
        ]
    );
    assert!(matches!(&ops[0], OpBody::ProgressUpsert { profile, .. } if profile == "default"));
    // The replica state agrees with the table.
    assert!(!phone.state().is_watched("default", &key()));
    let item = progress::get(&db, 1, &key()).unwrap().unwrap();
    assert!(!item.watched);
    assert_eq!(item.device_id, "phone");
}

#[test]
fn received_ops_update_the_other_library_in_any_order() {
    let mut phone_db = pa_library::open_in_memory().unwrap();
    let mut phone = Replica::new("phone").unwrap();
    let th = Thresholds::default();
    progress::report_position(&mut phone_db, &mut phone, &th, &report(600_000), T0).unwrap();
    progress::report_position(
        &mut phone_db,
        &mut phone,
        &th,
        &report(2_900_000),
        T0 + 10_000,
    )
    .unwrap();
    let ops: Vec<_> = outbox::due(&phone_db, i64::MAX, 100)
        .unwrap()
        .into_iter()
        .map(|p| p.op)
        .collect();

    for order in [ops.clone(), ops.iter().rev().cloned().collect()] {
        let tv_db = pa_library::open_in_memory().unwrap();
        let mut tv = Replica::new("tv").unwrap();
        for op in &order {
            tv.receive_into(&tv_db, op, T0 + 20_000).unwrap();
            // Duplicates change nothing.
            assert!(!tv.receive_into(&tv_db, op, T0 + 20_000).unwrap());
        }
        let item = progress::get(&tv_db, 1, &key()).unwrap().unwrap();
        assert_eq!(item.position_ms, 2_900_000);
        assert!(item.watched);
        assert_eq!(item.device_id, "phone");
        // Received ops are not queued again.
        assert_eq!(outbox::len(&tv_db).unwrap(), 0);
    }
}

#[test]
fn a_late_unwatch_from_another_device_clears_watched() {
    let mut phone_db = pa_library::open_in_memory().unwrap();
    let mut tv_db = pa_library::open_in_memory().unwrap();
    let mut phone = Replica::new("phone").unwrap();
    let mut tv = Replica::new("tv").unwrap();
    let th = Thresholds::default();
    progress::report_position(&mut phone_db, &mut phone, &th, &report(2_900_000), T0).unwrap();
    for p in outbox::due(&phone_db, i64::MAX, 100).unwrap() {
        tv.receive_into(&tv_db, &p.op, T0 + 1).unwrap();
        outbox::ack(&phone_db, p.op.op_id).unwrap();
    }
    progress::mark_unwatched(&mut tv_db, &mut tv, 1, &key(), T0 + 5_000).unwrap();
    for p in outbox::due(&tv_db, i64::MAX, 100).unwrap() {
        phone.receive_into(&phone_db, &p.op, T0 + 6_000).unwrap();
    }
    let item = progress::get(&phone_db, 1, &key()).unwrap().unwrap();
    assert!(!item.watched);
    assert_eq!(item.position_ms, 0);
    assert_eq!(item.device_id, "tv");
}

#[test]
fn twitch_favorites_sync_as_follow_ops() {
    use pa_library::twitch::{self, FavoriteSource};

    const CHANNEL: &str = "141981764";
    let mut phone_db = pa_library::open_in_memory().unwrap();
    let tv_db = pa_library::open_in_memory().unwrap();
    let mut phone = Replica::new("phone").unwrap();
    let mut tv = Replica::new("tv").unwrap();
    let mut follow = |db: &mut pa_library::rusqlite::Connection, on: bool, at: i64| {
        twitch::set_favorite(db, &mut phone, 1, CHANNEL, FavoriteSource::Local, on, at).unwrap()
    };

    // A local favourite, no Twitch login: one `twitch_follow.set` op; the
    // same state again syncs nothing.
    assert!(follow(&mut phone_db, true, T0));
    assert!(!follow(&mut phone_db, true, T0 + 1));
    assert_eq!(
        queued_ops(&phone_db),
        [OpBody::TwitchFollow {
            profile: "default".into(),
            channel_id: CHANNEL.into(),
            followed: true,
        }]
    );
    for p in outbox::due(&phone_db, i64::MAX, 100).unwrap() {
        assert!(tv.receive_into(&tv_db, &p.op, T0 + 10).unwrap());
        outbox::ack(&phone_db, p.op.op_id).unwrap();
    }
    let favs = twitch::favorites(&tv_db, 1).unwrap();
    assert_eq!(favs.len(), 1);
    assert_eq!(favs[0].user_id, CHANNEL);
    assert_eq!(favs[0].source, FavoriteSource::Local);

    // Unfollow on the phone removes it on the TV.
    assert!(follow(&mut phone_db, false, T0 + 20));
    for p in outbox::due(&phone_db, i64::MAX, 100).unwrap() {
        assert!(tv.receive_into(&tv_db, &p.op, T0 + 30).unwrap());
    }
    assert!(twitch::favorites(&tv_db, 1).unwrap().is_empty());
    assert_eq!(outbox::len(&tv_db).unwrap(), 0);
}
