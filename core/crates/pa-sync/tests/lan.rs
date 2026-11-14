// SPDX-License-Identifier: MPL-2.0
//! LAN P2P transport (SYNC-006) end to end on loopback: three devices of one
//! family converge, a lost hub is replaced in under 10 s, a returning device
//! catches up, strangers and revoked devices are refused, and the whole
//! thing also works with real mDNS on a private port.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pa_discovery::{
    Advertiser, DeviceClass, Discovery, Interfaces, MdnsBackend, MdnsConfig, ServiceKind,
};
use pa_library::rusqlite::Connection;
use pa_model::{MediaKey, MediaKind};
use pa_pairing::{DeviceIdentity, DeviceKind, Family, FamilyKey, Roster};
use pa_secrets::MemoryStore;
use pa_sync::lan::{LanConfig, LanEvent, LanNode};
use pa_sync::proto::{PlayState, PlaybackUpdate};
use pa_sync::{OpBody, OpId, Replica};
use tokio::sync::mpsc::UnboundedReceiver;

const T0: i64 = 1_763_000_000_000;

fn now_ms() -> i64 {
    T0 + i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            % 1_000_000,
    )
    .unwrap()
}

fn title(n: u32) -> MediaKey {
    format!("tmdb:movie:{n}").parse().unwrap()
}

fn progress(n: u32, position_ms: u64) -> OpBody {
    OpBody::ProgressUpsert {
        profile: "default".into(),
        key: title(n),
        kind: MediaKind::Movie,
        position_ms,
        duration_ms: Some(8_160_000),
    }
}

/// Keys of the devices, kept in stores so a device can restart.
struct Keys {
    roster: Roster,
    stores: Vec<(String, DeviceClass, MemoryStore)>,
}

impl Keys {
    /// A family of `devices`; the first one founds it.
    fn new(devices: &[(&str, DeviceClass)]) -> Self {
        let kind = |c: DeviceClass| match c {
            DeviceClass::Server => DeviceKind::Server,
            DeviceClass::Desktop => DeviceKind::Desktop,
            DeviceClass::Tv => DeviceKind::Tv,
            _ => DeviceKind::Phone,
        };
        let ids: Vec<DeviceIdentity> = devices
            .iter()
            .map(|(id, _)| DeviceIdentity::generate(*id).unwrap())
            .collect();
        let mut roster = Roster::found(&ids[0], devices[0].0, kind(devices[0].1), 1).unwrap();
        for (i, (id, class)) in devices.iter().enumerate().skip(1) {
            roster
                .admit(&ids[0], id, id, kind(*class), ids[i].public(), 2)
                .unwrap();
        }
        let stores = devices
            .iter()
            .zip(ids)
            .map(|((id, class), identity)| {
                let store = MemoryStore::new();
                identity.save(&store).unwrap();
                ((*id).to_owned(), *class, store)
            })
            .collect();
        Self { roster, stores }
    }

    fn family(&self, id: &str) -> Family {
        let (_, _, store) = self.stores.iter().find(|(d, _, _)| d == id).unwrap();
        let identity = DeviceIdentity::load(store).unwrap().unwrap();
        Family::new(
            identity,
            self.roster.clone(),
            FamilyKey::generate(1).unwrap(),
        )
        .unwrap()
    }

    fn class(&self, id: &str) -> DeviceClass {
        self.stores.iter().find(|(d, _, _)| d == id).unwrap().1
    }
}

/// One app: its node, its replica and library, and what it was told.
struct Dev {
    id: String,
    node: LanNode,
    rx: UnboundedReceiver<LanEvent>,
    replica: Replica,
    conn: Connection,
    acked: HashSet<OpId>,
    playback: Vec<PlaybackUpdate>,
}

fn config(class: DeviceClass) -> LanConfig {
    LanConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        ..LanConfig::new(class)
    }
}

impl Dev {
    async fn start(keys: &Keys, id: &str) -> Self {
        let (node, rx) = LanNode::start(config(keys.class(id)), keys.family(id))
            .await
            .unwrap();
        Self {
            id: id.to_owned(),
            node,
            rx,
            replica: Replica::new(id).unwrap(),
            conn: pa_library::open_in_memory().unwrap(),
            acked: HashSet::new(),
            playback: Vec::new(),
        }
    }

    fn sees(&self, other: &Dev, class: DeviceClass) {
        self.node.peer_up(&other.id, class, other.node.local_addr());
    }

    fn record(&mut self, body: OpBody) -> OpId {
        let op = self.replica.record(&self.conn, body, now_ms()).unwrap();
        self.node.publish(&op).unwrap();
        op.op_id
    }

    fn pump(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LanEvent::Op(op) => {
                    self.replica.receive(&op, now_ms()).unwrap();
                }
                LanEvent::Acked(id) => {
                    self.acked.insert(id);
                }
                LanEvent::Playback(u) => self.playback.push(u),
                _ => {}
            }
        }
    }
}

/// Wires every pair, as mDNS would.
fn mesh(keys: &Keys, devs: &[&Dev]) {
    for a in devs {
        for b in devs {
            if a.id != b.id {
                a.sees(b, keys.class(&b.id));
            }
        }
    }
}

async fn wait_until(devs: &mut [&mut Dev], what: &str, cond: impl Fn(&[&mut Dev]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        for d in devs.iter_mut() {
            d.pump();
        }
        if cond(devs) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn same_state(devs: &[&mut Dev], titles: u32) -> bool {
    let first = devs[0].replica.state();
    first.progress.len() == titles as usize && devs.iter().all(|d| d.replica.state() == first)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_devices_converge_and_replace_a_lost_hub() {
    let keys = Keys::new(&[
        ("pc", DeviceClass::Desktop),
        ("tv", DeviceClass::Tv),
        ("phone", DeviceClass::Mobile),
    ]);
    let mut pc = Dev::start(&keys, "pc").await;
    let mut tv = Dev::start(&keys, "tv").await;
    let mut phone = Dev::start(&keys, "phone").await;
    mesh(&keys, &[&pc, &tv, &phone]);

    // The desktop wins the election on every device.
    wait_until(&mut [&mut pc, &mut tv, &mut phone], "hub = pc", |d| {
        d[1].node.hub().as_deref() == Some("pc") && d[2].node.hub().as_deref() == Some("pc")
    })
    .await;
    assert_eq!(pc.node.hub(), None);
    assert_eq!(pc.node.links(), ["phone", "tv"]);

    // Each device writes; everyone converges, the same title included.
    pc.record(progress(1, 1_000));
    let tv_op = tv.record(progress(2, 2_000));
    let phone_op = phone.record(progress(3, 3_000));
    phone.record(progress(1, 41_000));
    wait_until(
        &mut [&mut pc, &mut tv, &mut phone],
        "3 devices converge",
        |d| same_state(d, 3) && d[1].acked.contains(&tv_op) && d[2].acked.contains(&phone_op),
    )
    .await;
    let pos = &pc.replica.state().progress[&("default".to_owned(), title(1))];
    assert_eq!(pos.value.position_ms, 41_000);

    // Instant resume: what the phone plays reaches the others at once.
    let update = PlaybackUpdate {
        profile: "default".into(),
        key: title(1),
        position_ms: 41_500,
        duration_ms: Some(8_160_000),
        rate: 1.0,
        state: PlayState::Paused,
        hlc: phone
            .replica
            .clock_mut()
            .tick(u64::try_from(now_ms()).unwrap()),
    };
    phone.node.playback(update.clone()).unwrap();
    wait_until(&mut [&mut pc, &mut tv], "pb.update fan-out", |d| {
        d.iter().all(|d| d.playback.last() == Some(&update))
    })
    .await;

    // The hub goes away; the TV takes over well within 10 s.
    let lost = Instant::now();
    drop(pc);
    wait_until(&mut [&mut tv, &mut phone], "hub = tv", |d| {
        d[0].node.hub().is_none() && d[1].node.hub().as_deref() == Some("tv")
    })
    .await;
    let failover = lost.elapsed();
    assert!(
        failover < Duration::from_secs(10),
        "failover took {failover:?}"
    );
    eprintln!("closed hub replaced in {failover:?}");

    // Writes keep flowing through the new hub.
    let late = phone.record(progress(4, 4_000));
    tv.record(progress(2, 22_000));
    wait_until(&mut [&mut tv, &mut phone], "2 devices converge", |d| {
        same_state(d, 4) && d[1].acked.contains(&late)
    })
    .await;

    // The desktop restarts with nothing: it becomes the hub again and the
    // others push everything it lacks.
    let mut pc = Dev::start(&keys, "pc").await;
    mesh(&keys, &[&pc, &tv, &phone]);
    wait_until(
        &mut [&mut pc, &mut tv, &mut phone],
        "restarted pc catches up",
        |d| {
            d[1].node.hub().as_deref() == Some("pc")
                && d[2].node.hub().as_deref() == Some("pc")
                && same_state(d, 4)
        },
    )
    .await;
    let pos = &pc.replica.state().progress[&("default".to_owned(), title(2))];
    assert_eq!(pos.value.position_ms, 22_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strangers_and_revoked_devices_are_refused() {
    let keys = Keys::new(&[("pc", DeviceClass::Desktop), ("phone", DeviceClass::Mobile)]);
    let other = Keys::new(&[("pc", DeviceClass::Desktop), ("evil", DeviceClass::Mobile)]);
    let mut pc = Dev::start(&keys, "pc").await;
    let mut evil = Dev::start(&other, "evil").await;
    // The stranger claims to see our desktop; its own "pc" has other keys,
    // and our desktop does not know it.
    evil.sees(&pc, DeviceClass::Desktop);
    evil.record(progress(9, 9_000));
    tokio::time::sleep(Duration::from_secs(2)).await;
    pc.pump();
    evil.pump();
    assert_eq!(evil.node.hub(), None);
    assert!(pc.node.links().is_empty());
    assert!(pc.replica.state().progress.is_empty());

    // A revoked phone cannot connect either.
    let revoked = Keys::new(&[("pc", DeviceClass::Desktop), ("phone", DeviceClass::Mobile)]);
    let phone_family = revoked.family("phone");
    let mut pc_family = revoked.family("pc");
    pc_family.revoke("phone", 3).unwrap();
    let (pc2, _rx) = LanNode::start(config(DeviceClass::Desktop), pc_family)
        .await
        .unwrap();
    let (phone2, _rx2) = LanNode::start(config(DeviceClass::Mobile), phone_family)
        .await
        .unwrap();
    phone2.peer_up("pc", DeviceClass::Desktop, pc2.local_addr());
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(phone2.hub(), None);
    assert!(pc2.links().is_empty());
}

fn private_port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let n = nanos.wrapping_add(NEXT.fetch_add(7919, Ordering::Relaxed)) ^ std::process::id();
    30_000 + u16::try_from(n % 20_000).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn devices_find_each_other_over_mdns() {
    let keys = Keys::new(&[
        ("pc", DeviceClass::Desktop),
        ("tv", DeviceClass::Tv),
        ("phone", DeviceClass::Mobile),
    ]);
    let mdns = MdnsConfig {
        port: Some(private_port()),
        interfaces: Interfaces::LoopbackOnly,
        multicast_lock: None,
    };
    let mut devs = Vec::new();
    let mut keep = Vec::new();
    for id in ["pc", "tv", "phone"] {
        let dev = Dev::start(&keys, id).await;
        let advertiser = Advertiser::with_config(&mdns).unwrap();
        let registration = advertiser.announce(&dev.node.announcement(id)).unwrap();
        let discovery = Discovery::new(MdnsBackend::with_config(mdns.clone()).unwrap(), [id]);
        let watcher = dev.node.watch_discovery(discovery.subscribe());
        discovery.browse([ServiceKind::SyncPeer]).unwrap();
        keep.push((advertiser, registration, discovery, watcher));
        devs.push(dev);
    }
    let [pc, tv, phone] = devs.as_mut_slice() else {
        unreachable!()
    };
    wait_until(&mut [pc, tv, phone], "hub = pc over mDNS", |d| {
        d[1].node.hub().as_deref() == Some("pc") && d[2].node.hub().as_deref() == Some("pc")
    })
    .await;
    phone.record(progress(7, 7_000));
    tv.record(progress(8, 8_000));
    wait_until(&mut [pc, tv, phone], "converge over mDNS", |d| {
        same_state(d, 2)
    })
    .await;
    assert_eq!(
        pc.node.peer_txt().fingerprint,
        Some(keys.family("pc").identity().public().fingerprint())
    );
}

/// A TCP relay that can go silent without closing anything: what a hub that
/// lost power or Wi-Fi looks like to its peers.
async fn freezable_relay(target: SocketAddr) -> (SocketAddr, Arc<AtomicBool>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let frozen = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&frozen);
    tokio::spawn(async move {
        while let Ok((inbound, _)) = listener.accept().await {
            let Ok(outbound) = tokio::net::TcpStream::connect(target).await else {
                continue;
            };
            let (ri, wi) = inbound.into_split();
            let (ro, wo) = outbound.into_split();
            tokio::spawn(pipe(ri, wo, Arc::clone(&flag)));
            tokio::spawn(pipe(ro, wi, Arc::clone(&flag)));
        }
    });
    (addr, frozen)
}

async fn pipe(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    frozen: Arc<AtomicBool>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        if frozen.load(Ordering::SeqCst) {
            // Hold both sockets open and never forward again.
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
        let n = match tokio::time::timeout(Duration::from_millis(50), from.read(&mut buf)).await {
            Err(_) => continue,
            Ok(Ok(0) | Err(_)) => return,
            Ok(Ok(n)) => n,
        };
        if !frozen.load(Ordering::SeqCst) && to.write_all(&buf[..n]).await.is_err() {
            return;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn silent_hub_is_replaced_within_10_s() {
    let keys = Keys::new(&[
        ("pc", DeviceClass::Desktop),
        ("tv", DeviceClass::Tv),
        ("phone", DeviceClass::Mobile),
    ]);
    let mut pc = Dev::start(&keys, "pc").await;
    let mut tv = Dev::start(&keys, "tv").await;
    let mut phone = Dev::start(&keys, "phone").await;
    // The others reach the desktop only through the relay.
    let (relay, frozen) = freezable_relay(pc.node.local_addr()).await;
    for d in [&tv, &phone] {
        d.node.peer_up("pc", DeviceClass::Desktop, relay);
    }
    tv.sees(&phone, DeviceClass::Mobile);
    phone.sees(&tv, DeviceClass::Tv);
    wait_until(&mut [&mut pc, &mut tv, &mut phone], "hub = pc", |d| {
        d[1].node.hub().as_deref() == Some("pc") && d[2].node.hub().as_deref() == Some("pc")
    })
    .await;

    // No FIN, no RST: the links just go quiet.
    let lost = Instant::now();
    frozen.store(true, Ordering::SeqCst);
    wait_until(&mut [&mut tv, &mut phone], "hub = tv", |d| {
        d[0].node.hub().is_none() && d[1].node.hub().as_deref() == Some("tv")
    })
    .await;
    let failover = lost.elapsed();
    assert!(
        failover < Duration::from_secs(10),
        "failover took {failover:?}"
    );
    eprintln!("silent hub replaced in {failover:?}");

    phone.record(progress(5, 5_000));
    wait_until(&mut [&mut tv, &mut phone], "converge after failover", |d| {
        same_state(d, 1)
    })
    .await;
}
