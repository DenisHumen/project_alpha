// SPDX-License-Identifier: MPL-2.0
//! The real mDNS stack end to end, kept off the network: daemons bind only
//! the IPv4 loopback and a private UDP port, so nothing leaves the host and
//! the system responder (Avahi, Bonjour) is not involved.
//!
//! `lan_smoke` is the exception: it uses the real LAN and is ignored by
//! default (`cargo test -p pa-discovery -- --ignored lan_smoke --nocapture`).
#![cfg(feature = "mdns")]

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pa_discovery::{
    Advertiser, Announcement, DeviceClass, Discovery, DiscoveryEvent, Interfaces, MdnsBackend,
    MdnsConfig, MulticastLock, PeerTxt, ServerTxt, ServiceKind,
};

fn private_port() -> u16 {
    // Distinct per test and per run; 5353 is never used here.
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let n = nanos.wrapping_add(NEXT.fetch_add(7919, Ordering::Relaxed)) ^ std::process::id();
    30_000 + u16::try_from(n % 20_000).unwrap()
}

fn loopback(port: u16) -> MdnsConfig {
    MdnsConfig {
        port: Some(port),
        interfaces: Interfaces::LoopbackOnly,
        multicast_lock: None,
    }
}

fn wait_for(
    rx: &Receiver<DiscoveryEvent>,
    pred: impl Fn(&DiscoveryEvent) -> bool,
) -> DiscoveryEvent {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let ev = rx
            .recv_timeout(left)
            .expect("expected discovery event in time");
        if pred(&ev) {
            return ev;
        }
    }
}

#[test]
fn server_and_share_found_and_lost_on_loopback() {
    let port = private_port();
    let advertiser = Advertiser::with_config(&loopback(port)).unwrap();
    let server_txt = ServerTxt {
        id: "0192f1d0-0000-7000-8000-00000000abcd".into(),
        api_version: 1,
        name: Some("Home server".into()),
        fingerprint: Some("sha256:00ff".into()),
    };
    let server = advertiser
        .announce(&Announcement::server("Home server", 8096, &server_txt))
        .unwrap();
    let share = advertiser
        .announce(&Announcement {
            kind: ServiceKind::Smb,
            instance: "Living room NAS".into(),
            host: "pa-testnas.local.".into(),
            port: 445,
            txt: vec![],
        })
        .unwrap();

    let backend = MdnsBackend::with_config(loopback(port)).unwrap();
    let discovery = Discovery::new(backend, Vec::<String>::new());
    let rx = discovery.subscribe();
    discovery
        .browse([ServiceKind::Server, ServiceKind::Smb])
        .unwrap();

    // Both arrive in any order.
    let mut found = Vec::new();
    while found.len() < 2 {
        if let DiscoveryEvent::Found(s) = wait_for(&rx, |e| matches!(e, DiscoveryEvent::Found(_))) {
            found.push(s);
        }
    }
    let svc = found
        .iter()
        .find(|s| s.kind == ServiceKind::Server)
        .expect("server found");
    assert_eq!(svc.instance, "Home server");
    assert_eq!(svc.port, 8096);
    assert_eq!(svc.server_txt().unwrap(), server_txt);
    assert!(
        svc.addresses.iter().any(std::net::IpAddr::is_loopback),
        "{:?}",
        svc.addresses
    );

    let smb = discovery.services_of(ServiceKind::Smb);
    assert_eq!(smb.len(), 1);
    assert_eq!(smb[0].instance, "Living room NAS");
    assert_eq!(smb[0].share_url().as_deref(), Some("smb://127.0.0.1/"));
    assert_eq!(discovery.server_by_id(&server_txt.id).unwrap().port, 8096);

    // Withdrawing sends a goodbye; the browser reports the loss.
    drop(share);
    let lost = wait_for(&rx, |e| matches!(e, DiscoveryEvent::Lost { .. }));
    assert_eq!(
        lost,
        DiscoveryEvent::Lost {
            kind: ServiceKind::Smb,
            instance: "Living room NAS".into()
        }
    );
    drop(server);
    advertiser.shutdown();
}

#[test]
fn own_peer_announcement_is_ignored() {
    let port = private_port();
    let backend = MdnsBackend::with_config(loopback(port)).unwrap();
    let advertiser = backend.advertiser();
    let me = PeerTxt {
        id: "device-me".into(),
        protocol_version: 1,
        device_class: DeviceClass::Desktop,
        name: None,
        fingerprint: None,
    };
    let other = PeerTxt {
        id: "device-tv".into(),
        device_class: DeviceClass::Tv,
        ..me.clone()
    };
    let _me = advertiser
        .announce(&Announcement::peer("My desktop", 7100, &me))
        .unwrap();
    let _tv = advertiser
        .announce(&Announcement::peer("Living room TV", 7101, &other))
        .unwrap();

    let discovery = Discovery::new(backend, ["device-me"]);
    let rx = discovery.subscribe();
    discovery.browse([ServiceKind::SyncPeer]).unwrap();
    let ev = wait_for(&rx, |e| matches!(e, DiscoveryEvent::Found(_)));
    let DiscoveryEvent::Found(svc) = ev else {
        unreachable!()
    };
    assert_eq!(svc.peer_txt().unwrap().id, "device-tv");
    // Give "me" time to be resolved too; it must stay filtered out.
    std::thread::sleep(Duration::from_millis(1500));
    let peers = discovery.services_of(ServiceKind::SyncPeer);
    assert_eq!(peers.len(), 1, "{peers:?}");
}

#[derive(Default)]
struct CountingLock {
    held: AtomicI32,
    acquired: AtomicU32,
}

impl MulticastLock for CountingLock {
    fn acquire(&self) {
        self.held.fetch_add(1, Ordering::SeqCst);
        self.acquired.fetch_add(1, Ordering::SeqCst);
    }
    fn release(&self) {
        self.held.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn multicast_lock_held_while_browsing() {
    let lock = Arc::new(CountingLock::default());
    let config = MdnsConfig {
        multicast_lock: Some(lock.clone()),
        ..loopback(private_port())
    };
    let mut discovery = Discovery::new(
        MdnsBackend::with_config(config).unwrap(),
        Vec::<String>::new(),
    );
    assert_eq!(lock.held.load(Ordering::SeqCst), 0);
    discovery
        .browse([ServiceKind::Smb, ServiceKind::Nfs])
        .unwrap();
    assert_eq!(lock.held.load(Ordering::SeqCst), 1);
    discovery.stop(ServiceKind::Smb);
    assert_eq!(lock.held.load(Ordering::SeqCst), 1);
    discovery.stop(ServiceKind::Nfs);
    assert_eq!(lock.held.load(Ordering::SeqCst), 0);
    discovery.browse([ServiceKind::Server]).unwrap();
    discovery.shutdown();
    assert_eq!(lock.held.load(Ordering::SeqCst), 0);
    assert_eq!(lock.acquired.load(Ordering::SeqCst), 2);
}

/// Real LAN, standard port: announces a test SMB service and lists every
/// share and server visible for a few seconds. Expect the test announcement
/// plus any Samba/Avahi hosts on the network.
#[test]
#[ignore = "uses the real network (multicast on 5353)"]
fn lan_smoke() {
    let backend = MdnsBackend::new().unwrap();
    let advertiser = backend.advertiser();
    let _ann = advertiser
        .announce(&Announcement {
            kind: ServiceKind::Smb,
            instance: "pa-discovery smoke".into(),
            host: "pa-smoketest.local.".into(),
            port: 445,
            txt: vec![],
        })
        .unwrap();
    let discovery = Discovery::new(backend, Vec::<String>::new());
    let rx = discovery.subscribe();
    discovery.browse(ServiceKind::ALL).unwrap();
    wait_for(
        &rx,
        |e| matches!(e, DiscoveryEvent::Found(s) if s.instance == "pa-discovery smoke"),
    );
    std::thread::sleep(Duration::from_secs(5));
    for s in discovery.services() {
        println!(
            "{} {:?} {}:{} {:?} {:?}",
            s.kind,
            s.instance,
            s.host,
            s.port,
            s.addresses,
            s.share_url()
        );
    }
}
