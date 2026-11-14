// SPDX-License-Identifier: MPL-2.0
//! SYNC-003 on two processes: a phone plays a movie and sends heartbeats;
//! a TV (a second process of this test binary, each with its own catalog,
//! keys and LAN node on loopback) asks where to continue and gets the
//! phone's position within 2 s. Starting it on the TV sends a takeover;
//! the phone pauses and its final heartbeat is what the TV resumes from.
//!
//! The child is this binary run again with `HANDOFF_CHILD_QR` set; the two
//! talk over the child's stdin/stdout (one command per line).

use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use pa_catalog::pa_sync::handoff::{self, PULL_TIMEOUT_MS, ResumePoint};
use pa_catalog::pa_sync::hlc::system_ms;
use pa_catalog::pa_sync::lan::service::{ServiceConfig, ServiceEvent, SyncLibrary, SyncService};
use pa_catalog::pa_sync::proto::PlayState;
use pa_catalog::pa_sync::{Op, OpId};
use pa_catalog::{Catalog, ItemRef};
use pa_discovery::DeviceClass;
use pa_scanner::CancelToken;
use pa_secrets::MemoryStore;

const CHILD_ENV: &str = "HANDOFF_CHILD_QR";
const DURATION_MS: u64 = 8_160_000;

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

struct Device {
    catalog: Arc<Catalog>,
    service: SyncService,
    _dir: tempfile::TempDir,
}

fn device(name: &str, class: DeviceClass) -> Device {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    // The same file on both devices: same fingerprint, same `file:` key.
    let path = root.join("Movies/The Matrix 1999.mkv");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..150_000u32)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(2))
        .collect();
    std::fs::write(&path, data).unwrap();
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
    cat.movies()
        .unwrap()
        .into_iter()
        .find(|m| m.title.contains("Matrix"))
        .unwrap()
        .id
}

fn wait(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What the app does before playing (`CoreSync::resume_hint`): a short
/// pull, then the newest point of stored progress and the others' last
/// heartbeat, moved on to now.
fn resume(d: &Device, item: ItemRef) -> (u64, String) {
    d.service.refresh(Duration::from_millis(PULL_TIMEOUT_MS));
    let profile = d.catalog.sync_profile().unwrap();
    let key = d.catalog.sync_key(item).unwrap().unwrap();
    let local = d.catalog.resume_point(item).unwrap();
    let remote = d
        .service
        .remote_playback(&profile, &key)
        .map(|u| ResumePoint::from_update(&u));
    let point = handoff::newest(local.into_iter().chain(remote)).expect("a resume point");
    (point.position_at(system_ms()), point.device().to_owned())
}

/// The TV: joins the phone's family, then answers commands.
fn child(qr: &str) {
    let tv = device("TV", DeviceClass::Tv);
    tv.service.join(qr).unwrap();
    wait("tv running", || tv.service.is_running());
    let item = matrix(&tv.catalog);
    println!(
        "READY {} {}",
        tv.service.device_id(),
        tv.service.sync_addr().unwrap()
    );
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["PEER", id, addr] => {
                tv.service
                    .peer_up(id, DeviceClass::Mobile, addr.parse().unwrap());
                println!("OK");
            }
            ["RESUME"] => {
                let (pos, device) = resume(&tv, item);
                println!("HINT {pos} {} {device}", system_ms());
            }
            ["PLAY"] => {
                let profile = tv.catalog.sync_profile().unwrap();
                let key = tv.catalog.sync_key(item).unwrap().unwrap();
                tv.service.start_playing(&profile, &key).unwrap();
                println!("STARTED");
            }
            ["QUIT"] => return,
            _ => panic!("unknown command {line}"),
        }
        std::io::stdout().flush().unwrap();
    }
}

/// Entry of the child process; a no-op in a normal test run.
#[test]
fn handoff_child_process() {
    if let Ok(qr) = std::env::var(CHILD_ENV) {
        child(&qr);
    }
}

struct Tv {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: Receiver<String>,
}

impl Tv {
    fn spawn(qr: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "handoff_child_process",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_ENV, qr)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
        }
    }

    /// The words from `tag` on of the next line of the child with `tag`.
    fn expect(&self, tag: &str) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("child did not say {tag}"));
            // libtest prints `test <name> ... ` before the test's output.
            if let Some(at) = line.find(tag) {
                return line[at..].split_whitespace().map(str::to_owned).collect();
            }
        }
    }

    fn send(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }
}

#[test]
fn resume_on_another_process_within_two_seconds_and_takeover() {
    let phone = device("Phone", DeviceClass::Mobile);
    let item = matrix(&phone.catalog);
    let profile = phone.catalog.sync_profile().unwrap();
    let key = phone.catalog.sync_key(item).unwrap().unwrap();
    let invite = phone.service.invite().unwrap();
    let mut tv = Tv::spawn(&invite.qr);
    let ready = tv.expect("READY");
    let (tv_id, tv_addr) = (ready[1].clone(), ready[2].parse().unwrap());
    wait("phone running", || phone.service.is_running());
    phone.service.peer_up(&tv_id, DeviceClass::Tv, tv_addr);
    let phone_addr = phone.service.sync_addr().unwrap();
    tv.send(&format!("PEER {} {phone_addr}", phone.service.device_id()));
    tv.expect("OK");
    // The TV outranks a phone: it is the hub.
    wait("phone links to the TV", || {
        phone.service.hub().as_deref() == Some(tv_id.as_str())
    });
    let events = phone.service.subscribe();

    // The phone plays from 10:00 and sends a heartbeat every 5 s.
    let start_pos = 600_000;
    let t0 = system_ms();
    let position = |at: u64| start_pos + at.saturating_sub(t0);
    phone.service.start_playing(&profile, &key).unwrap();
    let beat = |state| {
        phone
            .service
            .heartbeat(
                &profile,
                &key,
                position(system_ms()),
                Some(DURATION_MS),
                state,
            )
            .unwrap();
    };
    beat(PlayState::Playing);
    std::thread::sleep(Duration::from_millis(handoff::HEARTBEAT_INTERVAL_MS));
    beat(PlayState::Playing);
    // Between heartbeats the TV is asked where to continue.
    std::thread::sleep(Duration::from_millis(3_000));
    tv.send("RESUME");
    let hint = tv.expect("HINT");
    let (pos, at): (u64, u64) = (hint[1].parse().unwrap(), hint[2].parse().unwrap());
    assert_eq!(hint[3], phone.service.device_id(), "the phone's heartbeat");
    let error = pos.abs_diff(position(at));
    eprintln!(
        "resume on the TV: {pos} ms, phone at {} ms, off by {error} ms",
        position(at)
    );
    assert!(error <= 2_000, "resume off by {error} ms");

    // Takeover: the TV starts the title, the phone pauses and says where.
    tv.send("PLAY");
    tv.expect("STARTED");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events
            .recv_timeout(left)
            .expect("takeover reaches the phone")
        {
            ServiceEvent::Takeover(t) => {
                assert_eq!(t.key, key);
                assert_eq!(t.profile, profile);
                assert_eq!(t.hlc.device, tv_id);
                break;
            }
            _ => continue,
        }
    }
    let paused_at = position(system_ms());
    phone
        .service
        .heartbeat(
            &profile,
            &key,
            paused_at,
            Some(DURATION_MS),
            PlayState::Paused,
        )
        .unwrap();
    // A pause also saves durable progress.
    let now = i64::try_from(system_ms()).unwrap();
    let paused_ms = i64::try_from(paused_at).unwrap();
    phone
        .catalog
        .save_progress(item, paused_ms, DURATION_MS as i64, now, false)
        .unwrap();
    phone.service.flush();
    // Wall time passes; a paused title does not move on.
    std::thread::sleep(Duration::from_millis(1_500));
    tv.send("RESUME");
    let hint = tv.expect("HINT");
    let pos: u64 = hint[1].parse().unwrap();
    eprintln!("after takeover: {pos} ms, paused at {paused_at} ms");
    assert!(
        pos.abs_diff(paused_at) <= 2_000,
        "resume {pos} vs {paused_at}"
    );
    assert_eq!(hint[3], phone.service.device_id());

    tv.send("QUIT");
    let status = tv.child.wait().unwrap();
    assert!(status.success(), "child failed: {status}");
}

#[test]
fn heartbeats_are_not_stored() {
    let d = device("Solo", DeviceClass::Desktop);
    let item = matrix(&d.catalog);
    let key = d.catalog.sync_key(item).unwrap().unwrap();
    let profile = d.catalog.sync_profile().unwrap();
    assert_eq!(profile, "default");
    // Alone: no node runs; a heartbeat is a no-op and leaves no trace.
    d.service
        .heartbeat(&profile, &key, 1_000, None, PlayState::Playing)
        .unwrap();
    d.service.start_playing(&profile, &key).unwrap();
    assert!(d.catalog.pending_ops(10).unwrap().is_empty());
    assert!(d.catalog.progress(item).unwrap().is_none());
    assert_eq!(d.catalog.item_for_key(&key).unwrap(), Some(item));
    assert!(d.catalog.resume_point(item).unwrap().is_none());
}
