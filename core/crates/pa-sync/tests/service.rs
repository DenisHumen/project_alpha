// SPDX-License-Identifier: MPL-2.0
//! UI-019 end to end in one process: two apps pair by QR code and by a
//! typed code (found over mDNS on a private port), then a progress update
//! and a "watched" mark on one appear in the other's library; unpairing
//! stops it.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pa_discovery::{DeviceClass, Interfaces, MdnsConfig};
use pa_library::progress::{self, PositionReport, Thresholds};
use pa_library::rusqlite::Connection;
use pa_model::{MediaKey, MediaKind};
use pa_secrets::MemoryStore;
use pa_sync::lan::service::{ServiceConfig, ServiceEvent, SyncLibrary, SyncService};
use pa_sync::{Op, OpId, Replica, outbox};

/// A library like the app's catalog: one database and a replica while
/// sync is on.
struct TestLibrary {
    conn: Mutex<Connection>,
    replica: Mutex<Option<Replica>>,
}

impl TestLibrary {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            conn: Mutex::new(pa_library::open_in_memory().unwrap()),
            replica: Mutex::new(None),
        })
    }

    /// The player reports a position (what `Catalog::save_progress` does).
    fn play(&self, key: &MediaKey, position_ms: u64) {
        let mut conn = self.conn.lock().unwrap();
        let mut replica = self.replica.lock().unwrap();
        let replica = replica.as_mut().expect("sync is on");
        let report = PositionReport {
            profile_id: 1,
            key: key.clone(),
            kind: MediaKind::Movie,
            position_ms,
            duration_ms: Some(6_000_000),
        };
        progress::report_position(
            &mut conn,
            replica,
            &Thresholds::default(),
            &report,
            now_ms(),
        )
        .unwrap();
    }

    fn position(&self, key: &MediaKey) -> Option<(u64, bool)> {
        let conn = self.conn.lock().unwrap();
        progress::get(&conn, 1, key)
            .unwrap()
            .map(|p| (p.position_ms, p.watched))
    }
}

impl SyncLibrary for TestLibrary {
    fn enable(&self, device_id: &str) -> Result<(), String> {
        let mut r = self.replica.lock().unwrap();
        if r.as_ref().is_none_or(|r| r.device_id() != device_id) {
            *r = Some(Replica::new(device_id).map_err(|e| e.to_string())?);
        }
        Ok(())
    }

    fn disable(&self) -> Result<(), String> {
        *self.replica.lock().unwrap() = None;
        Ok(())
    }

    fn pending(&self) -> Result<Vec<Op>, String> {
        let conn = self.conn.lock().unwrap();
        outbox::compact(&conn).map_err(|e| e.to_string())?;
        Ok(outbox::due(&conn, i64::MAX, 256)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|p| p.op)
            .collect())
    }

    fn ack(&self, op_id: OpId) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        outbox::ack(&conn, op_id).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn apply(&self, op: &Op) -> Result<bool, String> {
        let conn = self.conn.lock().unwrap();
        let mut r = self.replica.lock().unwrap();
        let Some(r) = r.as_mut() else {
            return Ok(false);
        };
        r.receive_into(&conn, op, now_ms())
            .map_err(|e| e.to_string())
    }
}

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
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

struct App {
    service: SyncService,
    library: Arc<TestLibrary>,
    _dir: tempfile::TempDir,
}

fn app(name: &str, class: DeviceClass, mdns: Option<MdnsConfig>) -> App {
    let dir = tempfile::tempdir().unwrap();
    let library = TestLibrary::new();
    let config = ServiceConfig {
        bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        mdns,
        flush_interval: Duration::from_millis(200),
        find_timeout: Duration::from_secs(5),
        ..ServiceConfig::new(dir.path(), name, class)
    };
    let service = SyncService::open(
        config,
        Arc::new(MemoryStore::new()),
        Arc::clone(&library) as Arc<dyn SyncLibrary>,
    )
    .unwrap();
    App {
        service,
        library,
        _dir: dir,
    }
}

/// Without mDNS the devices learn each other's sync address by hand.
fn introduce(a: &App, b: &App) {
    let (pa, pb) = (
        a.service.sync_addr().unwrap(),
        b.service.sync_addr().unwrap(),
    );
    a.service
        .peer_up(b.service.device_id(), DeviceClass::Mobile, pb);
    b.service
        .peer_up(a.service.device_id(), DeviceClass::Desktop, pa);
}

fn wait(what: &str, limit: Duration, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < limit, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn movie(n: u32) -> MediaKey {
    format!("tmdb:movie:{n}").parse().unwrap()
}

#[test]
fn pair_by_qr_then_progress_and_watched_sync() {
    let pc = app("Desktop", DeviceClass::Desktop, None);
    let phone = app("Phone", DeviceClass::Mobile, None);
    let pc_events = pc.service.subscribe();
    assert!(pc.service.devices().is_empty());
    assert!(!pc.service.start().unwrap(), "nothing to sync alone");

    // A wrong code costs an attempt but keeps the invitation.
    let invite = pc.service.invite().unwrap();
    assert_eq!(invite.code.len(), 6);
    assert!(invite.qr.starts_with("projectalpha://pair?"));
    let wrong = if invite.code == "000000" {
        "111111"
    } else {
        "000000"
    };
    let qr_wrong = invite
        .qr
        .replace(&format!("c={}", invite.code), &format!("c={wrong}"));
    assert_eq!(
        phone.service.join(&qr_wrong).unwrap_err().code,
        "wrong_code"
    );
    assert!(pc.service.invite_addr().is_some());

    let inviter = phone.service.join(&invite.qr).unwrap();
    assert_eq!(inviter.device_id, pc.service.device_id());
    assert_eq!(inviter.name, "Desktop");
    assert!(phone.service.is_paired());
    wait("pc sees the pairing", Duration::from_secs(5), || {
        pc.service.is_paired() && pc.service.is_running()
    });
    assert!(phone.service.is_running());
    let paired = pc_events
        .try_iter()
        .any(|e| matches!(&e, ServiceEvent::Paired { device_id, .. } if device_id == phone.service.device_id()));
    assert!(paired, "Paired event");

    let names: Vec<String> = pc.service.devices().into_iter().map(|d| d.name).collect();
    assert_eq!(names, ["Desktop", "Phone"]);
    assert!(pc.service.devices()[0].this_device);
    assert_eq!(
        phone.service.devices()[1].fingerprint,
        pc.service.fingerprint()
    );

    introduce(&pc, &phone);
    wait(
        "phone links to the desktop hub",
        Duration::from_secs(10),
        || phone.service.hub().as_deref() == Some(pc.service.device_id()),
    );
    wait("both online", Duration::from_secs(5), || {
        pc.service.devices().iter().all(|d| d.online)
            && phone.service.devices().iter().all(|d| d.online)
    });

    // Progress on the desktop shows up on the phone…
    let phone_events = phone.service.subscribe();
    pc.library.play(&movie(603), 1_200_000);
    pc.service.flush();
    wait(
        "progress reaches the phone",
        Duration::from_secs(10),
        || phone.library.position(&movie(603)) == Some((1_200_000, false)),
    );
    assert!(
        phone_events
            .try_iter()
            .any(|e| e == ServiceEvent::LibraryChanged)
    );
    // …and the other way round, with "watched" past 95 %.
    phone.library.play(&movie(604), 5_900_000);
    phone.service.flush();
    wait(
        "watched reaches the desktop",
        Duration::from_secs(10),
        || pc.library.position(&movie(604)) == Some((5_900_000, true)),
    );
    // The phone's ops were acknowledged by its hub.
    wait("outbox drained", Duration::from_secs(5), || {
        let conn = phone.library.conn.lock().unwrap();
        outbox::len(&conn).unwrap() == 0
    });

    // Unpairing the phone on the desktop: the desktop is alone, stops, and
    // its later changes stay local.
    pc.service.unpair(phone.service.device_id()).unwrap();
    assert_eq!(pc.service.devices().len(), 1);
    assert!(!pc.service.is_paired());
    assert!(!pc.service.is_running());
}

#[test]
fn pair_by_typed_code_over_mdns() {
    let mdns = MdnsConfig {
        port: Some(private_port()),
        interfaces: Interfaces::LoopbackOnly,
        multicast_lock: None,
    };
    let tv = app("TV", DeviceClass::Tv, Some(mdns.clone()));
    let phone = app("Phone", DeviceClass::Mobile, Some(mdns));
    let invite = tv.service.invite().unwrap();
    assert_eq!(
        phone.service.join("12345").unwrap_err().code,
        "invalid_code"
    );
    let inviter = phone.service.join(&invite.code).unwrap();
    assert_eq!(inviter.device_id, tv.service.device_id());
    wait("tv paired", Duration::from_secs(5), || {
        tv.service.is_running()
    });
    // The two find each other over mDNS and sync.
    wait("phone links to the TV", Duration::from_secs(15), || {
        phone.service.hub().as_deref() == Some(tv.service.device_id())
    });
    phone.library.play(&movie(1), 600_000);
    phone.service.flush();
    wait("progress reaches the TV", Duration::from_secs(10), || {
        tv.library.position(&movie(1)) == Some((600_000, false))
    });
    // A paired device cannot join another family before it unpairs.
    let other = app("Other", DeviceClass::Desktop, None);
    let qr = other.service.invite().unwrap().qr;
    assert_eq!(phone.service.join(&qr).unwrap_err().code, "already_paired");
    // Leaving: the phone forgets the family.
    phone.service.unpair(phone.service.device_id()).unwrap();
    assert!(phone.service.devices().is_empty());
    assert!(!phone.service.is_running());
}
