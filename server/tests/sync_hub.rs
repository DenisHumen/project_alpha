// SPDX-License-Identifier: MPL-2.0
//! SYNC-008: the sync hub over WebSocket (`docs/api/sync-protocol.md`).
//!
//! Protocol behaviour (handshake, journal, fan-out, instant resume,
//! persistence) and the "done when" load test: 50 devices, 10 messages per
//! second each, p95 latency under 200 ms.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use pa_model::{MediaKey, MediaKind};
use pa_server::{Config, Server};
use pa_sync::proto::{
    self, Cursor, Hello, Message, OpFrame, Payload, PlayState, PlaybackUpdate, ProtoRange, Pull,
    Role, Takeover, code,
};
use pa_sync::{Hlc, Op, OpBody, OpId};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const WAIT: Duration = Duration::from_secs(5);

struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), pa_server::Error>>,
    hub_id: String,
}

impl Running {
    async fn start(data: &Path) -> Self {
        let config = Config {
            listen: "127.0.0.1:0".parse().unwrap(),
            // Plain HTTP: these tests speak raw HTTP/WebSocket (TLS: tls_mdns.rs).
            tls: pa_server::config::TlsConfig {
                enabled: false,
                ..Default::default()
            },
            ..Config::default()
        };
        let server = Server::bind(&config, data).await.unwrap();
        let addr = server.local_addr().unwrap();
        let hub_id = server.hub().hub_id().to_owned();
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(server.run(async {
            let _ = stopped.await;
        }));
        Self {
            addr,
            stop,
            task,
            hub_id,
        }
    }

    async fn stop(self) {
        self.stop.send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }
}

async fn open(addr: SocketAddr) -> Ws {
    let url = format!("ws://{addr}{}", pa_server::sync::PATH);
    tokio_tungstenite::connect_async(url).await.unwrap().0
}

async fn send(ws: &mut Ws, msg: &Message) {
    let frame = proto::encode(msg).unwrap();
    ws.send(WsMessage::Text(frame.into())).await.unwrap();
}

/// Next protocol message; `None` if the hub closed the connection.
async fn recv(ws: &mut Ws) -> Option<Message> {
    loop {
        let msg = tokio::time::timeout(WAIT, ws.next())
            .await
            .expect("hub answers in time")?;
        match msg {
            Ok(WsMessage::Text(text)) => return Some(proto::decode(text.as_str()).unwrap()),
            Ok(WsMessage::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

/// Nothing arrives for a while.
async fn silent(ws: &mut Ws) {
    let next = tokio::time::timeout(Duration::from_millis(300), ws.next()).await;
    assert!(next.is_err(), "unexpected frame {next:?}");
}

fn hello(device: &str, cursor: Option<Cursor>) -> Message {
    Message::Hello(Hello {
        proto: ProtoRange::LOCAL,
        device_id: device.to_owned(),
        role: Role::Device,
        cursor,
        head: None,
    })
}

/// A device after the handshake and the hub's `hello`.
async fn connect(addr: SocketAddr, device: &str, cursor: Option<Cursor>) -> (Ws, Hello) {
    let mut ws = open(addr).await;
    send(&mut ws, &hello(device, cursor)).await;
    match recv(&mut ws).await {
        Some(Message::Hello(h)) => (ws, h),
        other => panic!("expected hello, got {other:?}"),
    }
}

fn key(id: u32) -> MediaKey {
    format!("tmdb:movie:{id}").parse().unwrap()
}

fn hlc(device: &str, wall_ms: u64) -> Hlc {
    Hlc {
        wall_ms,
        counter: 0,
        device: device.to_owned(),
    }
}

fn progress_op(device: &str, id: u32, position_ms: u64, wall_ms: u64) -> Op {
    Op {
        op_id: OpId::new_v7(),
        hlc: hlc(device, wall_ms),
        body: OpBody::ProgressUpsert {
            profile: "default".to_owned(),
            key: key(id),
            kind: MediaKind::Movie,
            position_ms,
            duration_ms: Some(8_160_000),
        },
    }
}

fn pb(device: &str, id: u32, position_ms: u64, wall_ms: u64) -> PlaybackUpdate {
    PlaybackUpdate {
        profile: "default".to_owned(),
        key: key(id),
        position_ms,
        duration_ms: None,
        rate: 1.0,
        state: PlayState::Playing,
        hlc: hlc(device, wall_ms),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handshake_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let hub = Running::start(tmp.path()).await;

    // The hub says hello first, as a hub, with its empty journal.
    let mut ws = open(hub.addr).await;
    let Some(Message::Hello(h)) = recv(&mut ws).await else {
        panic!("no hello")
    };
    assert_eq!(h.role, Role::Hub);
    assert_eq!(h.device_id, hub.hub_id);
    assert_eq!(h.head.unwrap().seq, 0);
    // Anything but hello before the handshake closes the connection.
    send(&mut ws, &Message::PbUpdate(pb("phone", 1, 0, 1))).await;
    let Some(Message::Error(e)) = recv(&mut ws).await else {
        panic!("no error")
    };
    assert_eq!(e.code, code::HANDSHAKE);
    assert!(recv(&mut ws).await.is_none());

    // Two versions ahead: no common version.
    let mut ws = open(hub.addr).await;
    recv(&mut ws).await.unwrap();
    send(
        &mut ws,
        &Message::Hello(Hello {
            proto: ProtoRange::for_version(proto::PROTOCOL_VERSION + 2),
            device_id: "future".to_owned(),
            role: Role::Device,
            cursor: None,
            head: None,
        }),
    )
    .await;
    let Some(Message::Error(e)) = recv(&mut ws).await else {
        panic!("no error")
    };
    assert_eq!(e.code, code::PROTO_UNSUPPORTED);
    assert!(recv(&mut ws).await.is_none());

    // A garbage frame after the handshake is skipped, not fatal.
    let (mut ws, _) = connect(hub.addr, "phone", None).await;
    ws.send(WsMessage::Text("{\"t\":\"op\"}".into()))
        .await
        .unwrap();
    let Some(Message::Error(e)) = recv(&mut ws).await else {
        panic!("no error")
    };
    assert_eq!(e.code, code::BAD_MESSAGE);
    // A message type of a newer version is ignored.
    ws.send(WsMessage::Text("{\"t\":\"future.thing\"}".into()))
        .await
        .unwrap();
    silent(&mut ws).await;

    hub.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ops_are_journaled_acked_fanned_out_and_pulled() {
    let tmp = tempfile::tempdir().unwrap();
    let hub = Running::start(tmp.path()).await;
    let (mut phone, _) = connect(hub.addr, "phone", None).await;
    let (mut tv, _) = connect(hub.addr, "tv", None).await;

    let op = progress_op("phone", 603, 2_471_200, 1_763_000_000_000);
    let frame = OpFrame::plain(&op).unwrap();
    send(&mut phone, &Message::Op(frame.clone())).await;
    let Some(Message::Ack(ack)) = recv(&mut phone).await else {
        panic!("no ack")
    };
    assert_eq!((ack.op_id, ack.seq), (op.op_id, Some(1)));
    let Some(Message::Op(fanned)) = recv(&mut tv).await else {
        panic!("no fan-out")
    };
    assert_eq!(fanned.seq, Some(1));
    assert_eq!(fanned.open().unwrap(), op);

    // A resend (no ack seen, or the same op also came over P2P): same seq,
    // no second fan-out.
    send(&mut phone, &Message::Op(frame.clone())).await;
    let Some(Message::Ack(ack)) = recv(&mut phone).await else {
        panic!("no ack")
    };
    assert_eq!(ack.seq, Some(1));
    silent(&mut tv).await;

    // An encrypted payload the hub cannot read is stored and relayed.
    let sealed = OpFrame {
        op_id: OpId::new_v7(),
        seq: None,
        payload: Payload {
            enc: "secretbox.v1".to_owned(),
            body: "c2VhbGVk".to_owned(),
        },
    };
    send(&mut tv, &Message::Op(sealed.clone())).await;
    let Some(Message::Ack(ack)) = recv(&mut tv).await else {
        panic!("no ack")
    };
    assert_eq!(ack.seq, Some(2));
    let Some(Message::Op(fanned)) = recv(&mut phone).await else {
        panic!("no fan-out")
    };
    assert_eq!(fanned.payload, sealed.payload);

    // A new device is told to pull and pages through the journal.
    let (mut laptop, h) = connect(hub.addr, "laptop", None).await;
    let head = h.head.unwrap();
    assert_eq!(head.seq, 2);
    let Some(Message::Change(change)) = recv(&mut laptop).await else {
        panic!("no change")
    };
    assert_eq!(change.head, head);
    let mut cursor = None;
    let mut got = Vec::new();
    loop {
        send(
            &mut laptop,
            &Message::Pull(Pull {
                cursor: cursor.clone(),
                limit: 1,
            }),
        )
        .await;
        loop {
            match recv(&mut laptop).await.unwrap() {
                Message::Op(f) => got.push(f),
                Message::PullEnd(end) => {
                    cursor = Some(end.cursor);
                    if end.more {
                        break;
                    }
                    assert_eq!(cursor.as_ref(), Some(&head));
                    let ids: Vec<_> = got.iter().map(|f| (f.op_id, f.seq)).collect();
                    assert_eq!(ids, [(op.op_id, Some(1)), (sealed.op_id, Some(2))]);
                    // Up to date: no change after a reconnect with the cursor.
                    let (mut again, _) = connect(hub.addr, "laptop", cursor).await;
                    silent(&mut again).await;
                    hub.stop().await;
                    return;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn playback_is_forwarded_and_replayed_to_new_devices() {
    let tmp = tempfile::tempdir().unwrap();
    let hub = Running::start(tmp.path()).await;
    let (mut phone, _) = connect(hub.addr, "phone", None).await;
    let (mut tv, _) = connect(hub.addr, "tv", None).await;

    let older = pb("phone", 603, 2_400_000, 1_763_000_000_000);
    let newer = pb("phone", 603, 2_471_200, 1_763_000_005_000);
    for update in [&older, &newer] {
        send(&mut phone, &Message::PbUpdate(update.clone())).await;
        assert_eq!(recv(&mut tv).await, Some(Message::PbUpdate(update.clone())));
    }
    // The sender does not get its own frame back.
    silent(&mut phone).await;

    // A device that wakes up learns the fresh position right after hello.
    let (mut tablet, _) = connect(hub.addr, "tablet", None).await;
    assert_eq!(recv(&mut tablet).await, Some(Message::PbUpdate(newer)));
    silent(&mut tablet).await;

    // The TV takes over: the phone and the tablet hear about it.
    let takeover = Takeover {
        profile: "default".to_owned(),
        key: key(603),
        hlc: hlc("tv", 1_763_000_006_000),
    };
    send(&mut tv, &Message::PbTakeover(takeover.clone())).await;
    for ws in [&mut phone, &mut tablet] {
        assert_eq!(recv(ws).await, Some(Message::PbTakeover(takeover.clone())));
    }
    hub.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn journal_and_progress_survive_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let op = progress_op("phone", 603, 2_471_200, 1_763_000_000_000);

    let hub = Running::start(tmp.path()).await;
    let hub_id = hub.hub_id.clone();
    let (mut phone, _) = connect(hub.addr, "phone", None).await;
    send(&mut phone, &Message::Op(OpFrame::plain(&op).unwrap())).await;
    assert!(matches!(recv(&mut phone).await, Some(Message::Ack(_))));
    // The hub closes device connections on shutdown.
    let stopping = tokio::spawn(hub.stop());
    assert!(recv(&mut phone).await.is_none());
    stopping.await.unwrap();

    // The server wrote the position into its own library.
    {
        let conn = pa_library::open(tmp.path().join(pa_server::config::LIBRARY_FILE)).unwrap();
        let item = pa_library::progress::get(&conn, 1, &key(603))
            .unwrap()
            .unwrap();
        assert_eq!(item.position_ms, 2_471_200);
        assert_eq!(item.device_id, "phone");
    }

    let hub = Running::start(tmp.path()).await;
    assert_eq!(hub.hub_id, hub_id);
    let (mut tv, h) = connect(hub.addr, "tv", None).await;
    assert_eq!(h.head.as_ref().unwrap().seq, 1);
    assert!(matches!(recv(&mut tv).await, Some(Message::Change(_))));
    send(
        &mut tv,
        &Message::Pull(Pull {
            cursor: None,
            limit: 100,
        }),
    )
    .await;
    let Some(Message::Op(frame)) = recv(&mut tv).await else {
        panic!("no op")
    };
    assert_eq!(frame.open().unwrap(), op);
    assert!(matches!(
        recv(&mut tv).await,
        Some(Message::PullEnd(end)) if !end.more
    ));
    hub.stop().await;
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let rank = ((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// "Done when" of SYNC-008: 50 devices send 10 messages per second each
/// (nine `pb.update` and one `progress.upsert` op); every message reaches
/// the other 49 devices with p95 latency under 200 ms, acks too.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn load_50_devices_10_messages_per_second() {
    const DEVICES: usize = 50;
    const SECONDS: u64 = 3;
    const PER_SECOND: u64 = 10;

    let tmp = tempfile::tempdir().unwrap();
    let hub = Running::start(tmp.path()).await;
    let epoch = Instant::now();
    let micros = move || epoch.elapsed().as_micros() as u64;
    let sent_ops: Arc<Mutex<HashMap<OpId, Instant>>> = Arc::default();

    let mut devices = Vec::new();
    for i in 0..DEVICES {
        devices.push(connect(hub.addr, &format!("dev{i}"), None).await.0);
    }
    let start = tokio::time::Instant::now() + Duration::from_millis(200);
    let stop_reading = start + Duration::from_secs(SECONDS) + Duration::from_secs(2);

    let mut tasks = Vec::new();
    for (i, ws) in devices.into_iter().enumerate() {
        let (mut tx, mut rx) = ws.split();
        let ops_sent = Arc::clone(&sent_ops);
        let device = format!("dev{i}");
        let writer = tokio::spawn(async move {
            // Spread the devices over the 100 ms tick.
            let first = start + Duration::from_millis(i as u64 * 100 / DEVICES as u64);
            let mut tick =
                tokio::time::interval_at(first, Duration::from_millis(1000 / PER_SECOND));
            let (mut pbs, mut ops) = (0u64, 0u64);
            for n in 0..SECONDS * PER_SECOND {
                tick.tick().await;
                let msg = if n % PER_SECOND == PER_SECOND - 1 {
                    let op = progress_op(&device, i as u32 + 1, n * 1000, 1_763_000_000_000 + n);
                    ops_sent.lock().unwrap().insert(op.op_id, Instant::now());
                    ops += 1;
                    Message::Op(OpFrame::plain(&op).unwrap())
                } else {
                    // `position_ms` carries the send time for the receivers.
                    pbs += 1;
                    Message::PbUpdate(pb(&device, i as u32 + 1, micros(), 1_763_000_000_000 + n))
                };
                let frame = proto::encode(&msg).unwrap();
                tx.send(WsMessage::Text(frame.into())).await.unwrap();
            }
            (tx, pbs, ops)
        });
        let sent_ops = Arc::clone(&sent_ops);
        let reader = tokio::spawn(async move {
            let (mut fanout, mut acks) = (Vec::new(), Vec::new());
            while let Ok(Some(Ok(msg))) = tokio::time::timeout_at(stop_reading, rx.next()).await {
                let WsMessage::Text(text) = msg else { continue };
                match proto::decode(text.as_str()).unwrap() {
                    Message::PbUpdate(u) => {
                        fanout.push(Duration::from_micros(micros() - u.position_ms));
                    }
                    Message::Op(f) => {
                        let sent = sent_ops.lock().unwrap()[&f.op_id];
                        fanout.push(sent.elapsed());
                    }
                    Message::Ack(a) => {
                        let sent = sent_ops.lock().unwrap()[&a.op_id];
                        acks.push(sent.elapsed());
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
            (fanout, acks)
        });
        tasks.push((writer, reader));
    }

    let (mut fanout, mut acks) = (Vec::new(), Vec::new());
    let (mut sent_pbs, mut sent_op_count) = (0, 0);
    for (writer, reader) in tasks {
        let (_tx, pbs, ops) = writer.await.unwrap();
        sent_pbs += pbs;
        sent_op_count += ops;
        let (f, a) = reader.await.unwrap();
        fanout.extend(f);
        acks.extend(a);
    }
    fanout.sort();
    acks.sort();

    let expected = (sent_pbs + sent_op_count) as usize * (DEVICES - 1);
    let p95 = percentile(&fanout, 0.95);
    let ack_p95 = percentile(&acks, 0.95);
    eprintln!(
        "load: {} fan-out frames (expected {expected}), p50 {:?}, p95 {p95:?}, max {:?}; \
         {} acks, p95 {ack_p95:?}",
        fanout.len(),
        percentile(&fanout, 0.5),
        fanout.last().unwrap(),
        acks.len(),
    );
    assert_eq!(
        fanout.len(),
        expected,
        "every message reaches every other device"
    );
    assert_eq!(acks.len() as u64, sent_op_count, "every op is acked");
    assert!(p95 < Duration::from_millis(200), "fan-out p95 {p95:?}");
    assert!(ack_p95 < Duration::from_millis(200), "ack p95 {ack_p95:?}");
    hub.stop().await;
}
