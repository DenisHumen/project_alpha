// SPDX-License-Identifier: MPL-2.0
//! UI-019: two catalogs on two "devices" with the same files pair over the
//! LAN service, and a position played on one shows up in "Continue
//! watching" of the other; "watched" follows.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pa_catalog::pa_sync::lan::service::{ServiceConfig, SyncLibrary, SyncService};
use pa_catalog::pa_sync::{Op, OpId};
use pa_catalog::{Catalog, ItemRef};
use pa_discovery::DeviceClass;
use pa_scanner::CancelToken;
use pa_secrets::MemoryStore;

struct Lib(Arc<Catalog>);

impl SyncLibrary for Lib {
    fn enable(&self, device_id: &str) -> Result<(), String> {
        self.0.enable_sync(device_id).map_err(|e| e.to_string())
    }
    fn disable(&self) -> Result<(), String> {
        self.0.disable_sync().map_err(|e| e.to_string())
    }
    fn pending(&self) -> Result<Vec<Op>, String> {
        self.0.pending_ops(512).map_err(|e| e.to_string())
    }
    fn ack(&self, op_id: OpId) -> Result<(), String> {
        self.0.ack_op(op_id).map(drop).map_err(|e| e.to_string())
    }
    fn apply(&self, op: &Op) -> Result<bool, String> {
        self.0.apply_remote(op).map_err(|e| e.to_string())
    }
}

fn media(root: &Path, rel: &str, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..150_000u32)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

struct Device {
    catalog: Arc<Catalog>,
    service: SyncService,
    _dir: tempfile::TempDir,
}

fn device(name: &str, class: DeviceClass) -> Device {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    // The same files on both devices: same fingerprints, same `file:` keys.
    media(&root, "Movies/The Matrix 1999.mkv", 2);
    media(
        &root,
        "Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv",
        3,
    );
    let catalog = Arc::new(Catalog::open(dir.path().join("db/library.db")).unwrap());
    catalog
        .add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    catalog
        .scan(None, &CancelToken::new(), &mut |_| {})
        .unwrap();
    let config = ServiceConfig {
        bind_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        mdns: None,
        flush_interval: Duration::from_millis(200),
        ..ServiceConfig::new(dir.path().join("sync"), name, class)
    };
    let service = SyncService::open(
        config,
        Arc::new(MemoryStore::new()),
        Arc::new(Lib(Arc::clone(&catalog))),
    )
    .unwrap();
    Device {
        catalog,
        service,
        _dir: dir,
    }
}

fn matrix(cat: &Catalog) -> ItemRef {
    let movies = cat.movies().unwrap();
    let m = movies.iter().find(|m| m.title.contains("Matrix")).unwrap();
    m.id
}

fn wait(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn progress_on_one_device_continues_on_the_other() {
    let pc = device("Desktop", DeviceClass::Desktop);
    let phone = device("Phone", DeviceClass::Mobile);

    // Local progress before pairing stays local (sync is off).
    let movie_pc = matrix(&pc.catalog);
    assert!(
        pc.catalog
            .save_progress(movie_pc, 60_000, 8_000_000, 1_000, false)
            .unwrap()
    );
    assert_eq!(pc.catalog.pending_ops(10).unwrap().len(), 0);

    let invite = pc.service.invite().unwrap();
    phone.service.join(&invite.qr).unwrap();
    wait("desktop running", || pc.service.is_running());
    let (a, b) = (
        pc.service.sync_addr().unwrap(),
        phone.service.sync_addr().unwrap(),
    );
    pc.service
        .peer_up(phone.service.device_id(), DeviceClass::Mobile, b);
    phone
        .service
        .peer_up(pc.service.device_id(), DeviceClass::Desktop, a);
    wait("phone links to the desktop", || {
        phone.service.hub().as_deref() == Some(pc.service.device_id())
    });

    let now = pa_catalog::pa_sync::hlc::system_ms() as i64;
    assert!(
        pc.catalog
            .save_progress(movie_pc, 2_471_200, 8_160_000, now, false)
            .unwrap()
    );
    pc.service.flush();
    let movie_phone = matrix(&phone.catalog);
    wait("position reaches the phone", || {
        phone
            .catalog
            .progress(movie_phone)
            .unwrap()
            .is_some_and(|p| p.position_ms == 2_471_200)
    });
    let cont = phone.catalog.continue_watching(10).unwrap();
    assert_eq!(cont.len(), 1);
    assert_eq!(cont[0].item.id, movie_phone);

    // Finished on the phone: "watched" goes back to the desktop.
    assert!(
        phone
            .catalog
            .save_progress(movie_phone, 8_000_000, 8_160_000, now + 10_000, true)
            .unwrap()
    );
    phone.service.flush();
    wait("watched reaches the desktop", || {
        pc.catalog
            .progress(movie_pc)
            .unwrap()
            .is_some_and(|p| p.watched)
    });
    assert!(pc.catalog.continue_watching(10).unwrap().is_empty());
}
