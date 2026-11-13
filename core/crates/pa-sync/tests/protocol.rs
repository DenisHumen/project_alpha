// SPDX-License-Identifier: MPL-2.0
//! Message codec, version compatibility and op exchange (SYNC-002).

use pa_model::{MediaKey, MediaKind, SCHEMA_VERSION};
use pa_sync::proto::{
    self, Ack, Auth, Change, Cursor, ErrorFrame, Family, Hello, Journal, Message, OpFrame, Payload,
    PlayState, PlaybackUpdate, ProtoRange, Pull, PullEnd, Role, Takeover,
};
use pa_sync::{Error, Hlc, Op, OpBody, OpId, Replica, outbox};
use uuid::Uuid;

const T0: i64 = 1_763_000_000_000;

fn movie() -> MediaKey {
    "tmdb:movie:603".parse().unwrap()
}

fn hlc(device: &str) -> Hlc {
    Hlc {
        wall_ms: 1_763_000_000_000,
        counter: 2,
        device: device.into(),
    }
}

fn op(n: u128) -> Op {
    Op {
        op_id: OpId(Uuid::from_u128(n)),
        hlc: hlc("phone"),
        body: OpBody::Watched {
            profile: "default".into(),
            key: movie(),
        },
    }
}

fn cursor(seq: u64) -> Cursor {
    Cursor {
        log: "j1".into(),
        seq,
    }
}

fn update(state: PlayState, position_ms: u64) -> PlaybackUpdate {
    PlaybackUpdate {
        profile: "default".into(),
        key: movie(),
        position_ms,
        duration_ms: Some(8_160_000),
        rate: 1.0,
        state,
        hlc: hlc("phone"),
    }
}

/// Exact frames of protocol v1. Changing any of them breaks older peers.
fn golden() -> Vec<(Message, String)> {
    let h = r#""hlc":{"wall_ms":1763000000000,"counter":2,"device":"phone"}"#;
    let id = "00000000-0000-0000-0000-000000000007";
    let payload = serde_json::to_string(&outbox::encode(&op(7)).unwrap()).unwrap();
    vec![
        (
            Message::Hello(Hello {
                proto: ProtoRange { min: 1, max: 1 },
                device_id: "phone".into(),
                role: Role::Device,
                cursor: Some(cursor(41)),
                head: None,
                nonce: None,
            }),
            r#"{"t":"hello","proto":{"min":1,"max":1},"device_id":"phone","role":"device","cursor":{"log":"j1","seq":41}}"#.into(),
        ),
        (
            Message::Hello(Hello {
                proto: ProtoRange { min: 1, max: 1 },
                device_id: "tv".into(),
                role: Role::Device,
                cursor: None,
                head: None,
                nonce: Some("0a1b".into()),
            }),
            r#"{"t":"hello","proto":{"min":1,"max":1},"device_id":"tv","role":"device","nonce":"0a1b"}"#.into(),
        ),
        (
            Message::Auth(Auth { sig: "c0ffee".into() }),
            r#"{"t":"auth","sig":"c0ffee"}"#.into(),
        ),
        (
            Message::Family(Family {
                body: r#"{"roster":{}}"#.into(),
            }),
            r#"{"t":"family","body":"{\"roster\":{}}"}"#.into(),
        ),
        (
            Message::PbUpdate(update(PlayState::Playing, 2_471_200)),
            format!(
                r#"{{"t":"pb.update","profile":"default","key":"tmdb:movie:603","position_ms":2471200,"duration_ms":8160000,"state":"playing",{h}}}"#
            ),
        ),
        (
            Message::PbTakeover(Takeover {
                profile: "default".into(),
                key: movie(),
                hlc: hlc("phone"),
            }),
            format!(r#"{{"t":"pb.takeover","profile":"default","key":"tmdb:movie:603",{h}}}"#),
        ),
        (
            Message::Op(OpFrame::plain(&op(7)).unwrap()),
            format!(r#"{{"t":"op","op_id":"{id}","payload":{{"enc":"plain","body":{payload}}}}}"#),
        ),
        (
            Message::Ack(Ack {
                op_id: op(7).op_id,
                seq: Some(42),
            }),
            format!(r#"{{"t":"ack","op_id":"{id}","seq":42}}"#),
        ),
        (
            Message::Change(Change { head: cursor(42) }),
            r#"{"t":"change","head":{"log":"j1","seq":42}}"#.into(),
        ),
        (
            Message::Pull(Pull {
                cursor: Some(cursor(41)),
                limit: 500,
            }),
            r#"{"t":"pull","cursor":{"log":"j1","seq":41},"limit":500}"#.into(),
        ),
        (
            Message::PullEnd(PullEnd {
                cursor: cursor(42),
                more: false,
            }),
            r#"{"t":"pull.end","cursor":{"log":"j1","seq":42},"more":false}"#.into(),
        ),
        (
            Message::Error(ErrorFrame {
                code: proto::code::PROTO_UNSUPPORTED.into(),
                message: None,
                op_id: None,
            }),
            r#"{"t":"error","code":"proto.unsupported"}"#.into(),
        ),
    ]
}

#[test]
fn every_message_matches_its_golden_frame() {
    for (msg, frame) in golden() {
        assert_eq!(proto::encode(&msg).unwrap(), frame);
        assert_eq!(proto::decode(&frame).unwrap(), msg);
    }
}

#[test]
fn plain_payload_opens_to_the_same_op() {
    let frame = OpFrame::plain(&op(7)).unwrap();
    assert_eq!(frame.open().unwrap(), op(7));
    let body: serde_json::Value = serde_json::from_str(&frame.payload.body).unwrap();
    assert_eq!(body["schema_version"], SCHEMA_VERSION);
}

#[test]
fn versions_one_apart_are_compatible() {
    for a in 1..=6u16 {
        for b in 1..=6u16 {
            let got = proto::negotiate(ProtoRange::for_version(a), ProtoRange::for_version(b));
            if a.abs_diff(b) <= 1 {
                assert_eq!(got.unwrap(), a.min(b), "v{a} with v{b}");
            } else {
                assert!(
                    matches!(got, Err(Error::Incompatible { .. })),
                    "v{a} with v{b}"
                );
            }
        }
    }
    assert_eq!(ProtoRange::LOCAL.max, proto::PROTOCOL_VERSION);
    assert_eq!(ProtoRange::LOCAL.min, proto::MIN_PROTOCOL_VERSION);
}

#[test]
fn frames_of_the_next_version_still_decode() {
    // v2 adds a message type, fields and enum values; v1 ignores them.
    let unknown = r#"{"t":"pb.queue","items":["tmdb:movie:603"]}"#;
    assert_eq!(proto::decode(unknown).unwrap(), Message::Unknown);

    let hello = r#"{"t":"hello","proto":{"min":1,"max":2},"device_id":"tv","role":"relay","caps":["zstd"]}"#;
    let Message::Hello(h) = proto::decode(hello).unwrap() else {
        panic!("not a hello");
    };
    assert_eq!(h.role, Role::Other);
    assert_eq!(proto::negotiate(ProtoRange::LOCAL, h.proto).unwrap(), 1);

    let pb = r#"{"t":"pb.update","profile":"default","key":"tmdb:movie:603","position_ms":5,"state":"buffering","audio":"eng","hlc":{"wall_ms":1,"counter":0,"device":"tv"}}"#;
    let Message::PbUpdate(u) = proto::decode(pb).unwrap() else {
        panic!("not a pb.update");
    };
    assert_eq!(
        (u.state, u.rate, u.duration_ms),
        (PlayState::Other, 1.0, None)
    );
}

#[test]
fn ops_of_a_newer_schema_are_relayed_unread() {
    let mut frame = OpFrame::plain(&op(1)).unwrap();
    frame.payload.body = frame.payload.body.replacen(
        &format!("\"schema_version\":{SCHEMA_VERSION}"),
        &format!("\"schema_version\":{}", SCHEMA_VERSION + 1),
        1,
    );
    assert!(matches!(
        frame.open(),
        Err(Error::Invalid("schema version"))
    ));
    let sealed = OpFrame {
        payload: Payload {
            enc: "secretbox.v1".into(),
            body: "bm9uY2UrY2lwaGVydGV4dA==".into(),
        },
        ..frame.clone()
    };
    assert!(matches!(
        sealed.open(),
        Err(Error::Invalid("payload encryption"))
    ));

    // The hub stores both without reading them.
    let mut hub = Journal::new("j1").unwrap();
    let wire = proto::encode(&Message::Op(sealed.clone())).unwrap();
    let Message::Op(got) = proto::decode(&wire).unwrap() else {
        panic!("not an op");
    };
    assert_eq!(hub.append(&got), (1, true));
}

#[test]
fn bad_frames_are_rejected() {
    let bad = [
        r#"{"t":"hello","proto":{"min":2,"max":1},"device_id":"tv","role":"device"}"#,
        r#"{"t":"hello","proto":{"min":1,"max":1},"device_id":"bad id","role":"device"}"#,
        r#"{"t":"pb.update","profile":"default","key":"tmdb:movie:603","position_ms":5,"rate":0,"state":"playing","hlc":{"wall_ms":1,"counter":0,"device":"tv"}}"#,
        r#"{"t":"pb.takeover","profile":"","key":"tmdb:movie:603","hlc":{"wall_ms":1,"counter":0,"device":"tv"}}"#,
        r#"{"t":"pull","limit":0}"#,
        r#"{"t":"pull","limit":1001}"#,
        r#"{"t":"change","head":{"log":"","seq":1}}"#,
        r#"{"t":"op","op_id":"x","payload":{"enc":"plain","body":"{}"}}"#,
        r#"{"t":"error","code":""}"#,
        r#"{"t":"hello","proto":{"min":1,"max":1},"device_id":"tv","role":"device","nonce":"zz"}"#,
        r#"{"t":"auth","sig":""}"#,
        r#"{"t":"auth","sig":"not hex"}"#,
        r#"{"t":"family","body":""}"#,
        r#"{"proto":{"min":1,"max":1}}"#,
        "not json",
    ];
    for frame in bad {
        assert!(proto::decode(frame).is_err(), "{frame}");
    }
    let huge = format!(
        r#"{{"t":"error","code":"x","pad":"{}"}}"#,
        "a".repeat(proto::MAX_FRAME_LEN)
    );
    assert!(proto::decode(&huge).is_err());
    assert!(proto::encode(&Message::Unknown).is_err());

    let mut frame = OpFrame::plain(&op(1)).unwrap();
    frame.op_id = op(2).op_id;
    assert!(matches!(frame.open(), Err(Error::Invalid("op id"))));
}

#[test]
fn journal_dedups_and_pages_by_cursor() {
    let mut j = Journal::new("j1").unwrap();
    for n in 1..=5 {
        assert_eq!(j.append(&OpFrame::plain(&op(n)).unwrap()), (n as u64, true));
    }
    assert_eq!(j.append(&OpFrame::plain(&op(3)).unwrap()), (3, false));
    assert_eq!(j.head(), cursor(5));

    let (frames, end) = j.pull(&Pull {
        cursor: Some(cursor(1)),
        limit: 3,
    });
    let seqs: Vec<_> = frames.iter().map(|f| f.seq.unwrap()).collect();
    assert_eq!(seqs, [2, 3, 4]);
    assert_eq!(
        end,
        PullEnd {
            cursor: cursor(4),
            more: true
        }
    );

    let (frames, end) = j.pull(&Pull {
        cursor: Some(end.cursor),
        limit: 3,
    });
    assert_eq!(frames.len(), 1);
    assert_eq!(
        end,
        PullEnd {
            cursor: cursor(5),
            more: false
        }
    );

    // A cursor of another journal (new hub) starts from the beginning.
    let (frames, _) = j.pull(&Pull {
        cursor: Some(Cursor {
            log: "old-hub".into(),
            seq: 4,
        }),
        limit: 10,
    });
    assert_eq!(frames.len(), 5);
    assert!(Journal::new("").is_err());
}

/// Sends `msg` over a pretend wire: encode, then decode on the other side.
fn wire(msg: Message) -> Message {
    proto::decode(&proto::encode(&msg).unwrap()).unwrap()
}

#[test]
fn phone_hands_playback_over_to_tv_through_a_hub() {
    let phone_db = pa_library::open_in_memory().unwrap();
    let mut phone = Replica::new("phone").unwrap();
    let mut tv = Replica::new("tv").unwrap();
    let mut hub = Journal::new("j1").unwrap();

    // Offline on the phone: the user pauses at 41:11.
    let progress = OpBody::ProgressUpsert {
        profile: "default".into(),
        key: movie(),
        kind: MediaKind::Movie,
        position_ms: 2_471_200,
        duration_ms: Some(8_160_000),
    };
    let recorded = phone.record(&phone_db, progress, T0).unwrap();

    // Phone connects: both sides say hello and agree on a version.
    let Message::Hello(from_hub) = wire(Message::Hello(Hello {
        proto: ProtoRange::LOCAL,
        device_id: "server".into(),
        role: Role::Hub,
        cursor: None,
        head: Some(hub.head()),
        nonce: None,
    })) else {
        panic!("not a hello");
    };
    assert_eq!(
        proto::negotiate(ProtoRange::LOCAL, from_hub.proto).unwrap(),
        1
    );

    // Outbox first: every due op goes out, the hub acks, the outbox drains.
    for pending in outbox::due(&phone_db, T0, 100).unwrap() {
        let Message::Op(frame) = wire(Message::Op(OpFrame::plain(&pending.op).unwrap())) else {
            panic!("not an op");
        };
        let (seq, _) = hub.append(&frame);
        let Message::Ack(ack) = wire(Message::Ack(Ack {
            op_id: frame.op_id,
            seq: Some(seq),
        })) else {
            panic!("not an ack");
        };
        assert!(outbox::ack(&phone_db, ack.op_id).unwrap());
    }
    assert_eq!(outbox::len(&phone_db).unwrap(), 0);

    // Phone resends after a lost ack: the hub keeps one copy, same seq.
    assert_eq!(hub.append(&OpFrame::plain(&recorded).unwrap()), (1, false));

    // The TV wakes up later with no cursor and pulls.
    let Message::Pull(pull) = wire(Message::Pull(Pull {
        cursor: None,
        limit: proto::MAX_PULL_LIMIT,
    })) else {
        panic!("not a pull");
    };
    let (frames, end) = hub.pull(&pull);
    for frame in frames {
        let Message::Op(frame) = wire(Message::Op(frame)) else {
            panic!("not an op");
        };
        assert!(tv.receive(&frame.open().unwrap(), T0 + 50).unwrap());
    }
    assert!(!end.more);
    let pos = &tv.state().progress[&("default".to_owned(), movie())];
    assert_eq!(pos.value.position_ms, 2_471_200);

    // The TV starts playing and tells the phone (same profile) to pause.
    let start = Takeover {
        profile: "default".into(),
        key: movie(),
        hlc: tv.clock_mut().tick(u64::try_from(T0).unwrap() + 100),
    };
    let Message::PbTakeover(got) = wire(Message::PbTakeover(start.clone())) else {
        panic!("not a takeover");
    };
    assert_eq!(got, start);
    assert!(got.hlc > recorded.hlc);
}
