// SPDX-License-Identifier: MPL-2.0
//! LAN discovery for Dart (NET-020, ADR-0019): [`CoreDiscovery`] wraps the
//! `pa-discovery` facade over the pure-Rust mDNS backend.
//!
//! The app browses file shares (`_smb._tcp`, `_webdav._tcp`,
//! `_webdavs._tcp`) and our own server (`_projectalpha._tcp`) while the
//! add-source flow is open, and gets the current list as a stream
//! ([`CoreDiscovery::watch`]).
//!
//! Android drops Wi-Fi multicast unless the app holds a
//! `WifiManager.MulticastLock`: the callback given to [`CoreDiscovery::open`]
//! is the `pa_discovery::MulticastLock` of the core. The core calls it with
//! `true` before the first browse and `false` after the last one; on other
//! platforms the callback does nothing.
//!
//! iOS and tvOS do not use this module: there `NWBrowser` reports into the
//! core through `pa-ffi` (`PlatformBrowser`), see `tools/bindings/PACore`.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use flutter_rust_bridge::{DartFnFuture, frb};
use pa_discovery::{
    Advertiser, Announcement, DiscoveredService, Discovery, DiscoveryError, DiscoveryEvent,
    Interfaces, MdnsBackend, MdnsConfig, MulticastLock, Registration, ServiceKind, host_for_id,
};

use crate::api::secrets::block_on_timeout;
use crate::frb_generated::StreamSink;

/// How long the core waits for the Dart multicast-lock callback.
const LOCK_CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Error of a discovery call; Dart receives it as an exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDiscoveryError {
    /// `mdns` (sockets, daemon), `platform`, `invalid` (announcement),
    /// `closed`.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

impl From<DiscoveryError> for CoreDiscoveryError {
    fn from(e: DiscoveryError) -> Self {
        let code = match &e {
            DiscoveryError::Mdns(_) => "mdns",
            DiscoveryError::Platform(_) => "platform",
            DiscoveryError::InvalidAnnouncement(_) => "invalid",
            _ => "closed",
        };
        Self {
            code: code.into(),
            message: e.to_string(),
        }
    }
}

fn closed() -> CoreDiscoveryError {
    DiscoveryError::Shutdown.into()
}

type DiscoveryResult<T> = std::result::Result<T, CoreDiscoveryError>;

/// A DNS-SD service type the core knows (`pa_discovery::ServiceKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CoreServiceKind {
    /// Our media server, `_projectalpha._tcp`.
    Server,
    /// Another app for LAN sync, `_alpha-sync._tcp`.
    SyncPeer,
    /// An app showing a pairing invitation, `_alpha-pair._tcp`.
    Pairing,
    /// SMB share, `_smb._tcp`.
    Smb,
    /// WebDAV over HTTP, `_webdav._tcp`.
    WebDav,
    /// WebDAV over HTTPS, `_webdavs._tcp`.
    WebDavs,
    /// NFS export, `_nfs._tcp`.
    Nfs,
}

impl From<CoreServiceKind> for ServiceKind {
    fn from(k: CoreServiceKind) -> Self {
        match k {
            CoreServiceKind::Server => Self::Server,
            CoreServiceKind::SyncPeer => Self::SyncPeer,
            CoreServiceKind::Pairing => Self::Pairing,
            CoreServiceKind::Smb => Self::Smb,
            CoreServiceKind::WebDav => Self::WebDav,
            CoreServiceKind::WebDavs => Self::WebDavs,
            CoreServiceKind::Nfs => Self::Nfs,
        }
    }
}

impl From<ServiceKind> for CoreServiceKind {
    fn from(k: ServiceKind) -> Self {
        match k {
            ServiceKind::Server => Self::Server,
            ServiceKind::SyncPeer => Self::SyncPeer,
            ServiceKind::Pairing => Self::Pairing,
            ServiceKind::Smb => Self::Smb,
            ServiceKind::WebDav => Self::WebDav,
            ServiceKind::WebDavs => Self::WebDavs,
            ServiceKind::Nfs => Self::Nfs,
        }
    }
}

/// Kinds the add-source flow browses: SMB, WebDAV and our server.
#[frb(sync)]
pub fn discovery_source_kinds() -> Vec<CoreServiceKind> {
    vec![
        CoreServiceKind::Smb,
        CoreServiceKind::WebDav,
        CoreServiceKind::WebDavs,
        CoreServiceKind::Server,
    ]
}

/// One service visible on the LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDiscoveredService {
    pub kind: CoreServiceKind,
    /// Instance name as announced ("Living room NAS").
    pub instance: String,
    /// Host from the SRV record without the trailing dot (`nas.local`);
    /// may be empty.
    pub host: String,
    /// TCP port from the SRV record.
    pub port: u16,
    /// Addresses, best first (IPv4 before IPv6, routable before link-local).
    pub addresses: Vec<String>,
    /// TXT `path` of WebDAV and NFS, normalized to start with `/`.
    pub path: Option<String>,
    /// Address of a file share for the source form, built from the primary
    /// address (`smb://192.168.1.2/`, `http://192.168.1.2:8080/dav`);
    /// `None` for our own service types or before an address is known.
    pub url: Option<String>,
    /// TXT `id` of our server or another app.
    pub id: Option<String>,
}

impl From<&DiscoveredService> for CoreDiscoveredService {
    fn from(s: &DiscoveredService) -> Self {
        Self {
            kind: s.kind.into(),
            instance: s.instance.clone(),
            host: s.host.trim_end_matches('.').to_owned(),
            port: s.port,
            addresses: s.addresses.iter().map(ToString::to_string).collect(),
            path: matches!(
                s.kind,
                ServiceKind::WebDav | ServiceKind::WebDavs | ServiceKind::Nfs
            )
            .then(|| s.path()),
            url: s.share_url(),
            id: s.id().map(str::to_owned),
        }
    }
}

/// A TXT entry of an announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTxtEntry {
    pub key: String,
    pub value: String,
}

/// Settings of [`CoreDiscovery::open`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreDiscoveryConfig {
    /// Tests: IPv4 loopback only on this private UDP port instead of every
    /// interface on 5353, so nothing reaches the network.
    pub loopback_port: Option<u16>,
    /// TXT ids of this app (its sync device id): never reported as found.
    pub own_ids: Vec<String>,
}

type LockFn = dyn Fn(bool) -> DartFnFuture<()> + Send + Sync + 'static;

/// `pa_discovery::MulticastLock` over the Dart callback. Calls are queued to
/// one thread so they reach the platform in order and never block the
/// caller (the core may release the lock while Dart drops the object).
struct DartMulticastLock {
    tx: Sender<bool>,
}

impl DartMulticastLock {
    fn new(callback: Box<LockFn>) -> Self {
        let (tx, rx) = mpsc::channel::<bool>();
        let spawned = thread::Builder::new()
            .name("pa-multicast-lock".into())
            .spawn(move || {
                for hold in rx {
                    // A platform that does not answer only costs the lock.
                    let _ = block_on_timeout(callback(hold), LOCK_CALL_TIMEOUT);
                }
            });
        if let Err(e) = spawned {
            eprintln!("pa-discovery: no multicast lock thread: {e}");
        }
        Self { tx }
    }
}

impl MulticastLock for DartMulticastLock {
    fn acquire(&self) {
        let _ = self.tx.send(true);
    }

    fn release(&self) {
        let _ = self.tx.send(false);
    }
}

struct Inner {
    discovery: Discovery,
    advertiser: Advertiser,
    announced: Vec<Registration>,
}

/// LAN discovery of this app.
#[frb(opaque)]
pub struct CoreDiscovery {
    inner: Mutex<Option<Inner>>,
}

impl CoreDiscovery {
    /// Starts the mDNS daemon. Nothing is browsed yet: call
    /// [`browse`](Self::browse). `multicast_lock` is called with `true`
    /// before the first browse and `false` after the last one (Android
    /// `WifiManager.MulticastLock`; a no-op elsewhere).
    pub fn open(
        config: CoreDiscoveryConfig,
        multicast_lock: impl Fn(bool) -> DartFnFuture<()> + Send + Sync + 'static,
    ) -> DiscoveryResult<CoreDiscovery> {
        let lock: Arc<dyn MulticastLock> = Arc::new(DartMulticastLock::new(Box::new(multicast_lock)));
        Self::with_lock(config, Some(lock))
    }

    fn with_lock(
        config: CoreDiscoveryConfig,
        lock: Option<Arc<dyn MulticastLock>>,
    ) -> DiscoveryResult<CoreDiscovery> {
        let mdns = MdnsConfig {
            port: config.loopback_port,
            interfaces: if config.loopback_port.is_some() {
                Interfaces::LoopbackOnly
            } else {
                Interfaces::All
            },
            multicast_lock: lock,
        };
        let backend = MdnsBackend::with_config(mdns)?;
        let advertiser = backend.advertiser();
        let discovery = Discovery::new(backend, config.own_ids);
        Ok(Self {
            inner: Mutex::new(Some(Inner {
                discovery,
                advertiser,
                announced: Vec::new(),
            })),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Option<Inner>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> DiscoveryResult<T> {
        self.lock().as_mut().map(f).ok_or_else(closed)
    }

    /// Starts browsing `kinds` (already browsed kinds are skipped).
    pub fn browse(&self, kinds: Vec<CoreServiceKind>) -> DiscoveryResult<()> {
        self.with(|i| i.discovery.browse(kinds.into_iter().map(Into::into)))?
            .map_err(Into::into)
    }

    /// Stops browsing `kinds`; their instances are reported lost.
    pub fn stop(&self, kinds: Vec<CoreServiceKind>) {
        let _ = self.with(|i| {
            for kind in kinds {
                i.discovery.stop(kind.into());
            }
        });
    }

    /// Kinds being browsed.
    #[frb(sync)]
    pub fn browsing(&self) -> Vec<CoreServiceKind> {
        self.with(|i| i.discovery.browsing().into_iter().map(Into::into).collect())
            .unwrap_or_default()
    }

    /// Snapshot of the visible services, sorted by kind and name.
    #[frb(sync)]
    pub fn services(&self) -> Vec<CoreDiscoveredService> {
        let mut list: Vec<CoreDiscoveredService> = self
            .with(|i| i.discovery.services().iter().map(Into::into).collect())
            .unwrap_or_default();
        sort(&mut list);
        list
    }

    /// The visible services: the current list at once, then the whole list
    /// again after every change. Returns at once; the stream ends on
    /// [`close`](Self::close).
    pub fn watch(&self, sink: StreamSink<Vec<CoreDiscoveredService>>) -> DiscoveryResult<()> {
        let rx = self.with(|i| i.discovery.subscribe())?;
        spawn_snapshots(rx, move |list| sink.add(list).is_ok());
        Ok(())
    }

    /// Announces a service from this app until [`close`](Self::close)
    /// (loopback tests and diagnostics; our own types are announced by the
    /// sync service and the server).
    pub fn announce(
        &self,
        kind: CoreServiceKind,
        instance: String,
        port: u16,
        txt: Vec<CoreTxtEntry>,
    ) -> DiscoveryResult<()> {
        self.with(|i| {
            let reg = i.advertiser.announce(&Announcement {
                kind: kind.into(),
                host: host_for_id(&instance),
                instance,
                port,
                txt: txt.into_iter().map(|e| (e.key, e.value)).collect(),
            })?;
            i.announced.push(reg);
            Ok(())
        })?
    }

    /// Stops browsing and announcing and releases the multicast lock; the
    /// [`watch`](Self::watch) streams end. Later calls fail with `closed`.
    pub fn close(&self) {
        let inner = self.lock().take();
        if let Some(mut inner) = inner {
            inner.announced.clear();
            inner.discovery.shutdown();
            inner.advertiser.shutdown();
        }
    }
}

fn sort(list: &mut [CoreDiscoveredService]) {
    list.sort_by_cached_key(|s| (s.kind, s.instance.to_lowercase()));
}

/// Turns discovery events into whole lists: one at once, then one per
/// batch of changes. Ends when the subscription ends or `emit` says no.
fn spawn_snapshots(
    rx: Receiver<DiscoveryEvent>,
    mut emit: impl FnMut(Vec<CoreDiscoveredService>) -> bool + Send + 'static,
) {
    let spawned = thread::Builder::new()
        .name("pa-discovery-watch".into())
        .spawn(move || {
            let mut visible: BTreeMap<(ServiceKind, String), CoreDiscoveredService> =
                BTreeMap::new();
            let mut next: Option<DiscoveryEvent> = None;
            let mut changed = true;
            loop {
                if let Some(ev) = next.take() {
                    apply(&mut visible, ev);
                    changed = true;
                }
                while let Ok(ev) = rx.try_recv() {
                    apply(&mut visible, ev);
                    changed = true;
                }
                if changed {
                    let mut list: Vec<_> = visible.values().cloned().collect();
                    sort(&mut list);
                    if !emit(list) {
                        return;
                    }
                    changed = false;
                }
                match rx.recv() {
                    Ok(ev) => next = Some(ev),
                    Err(_) => return,
                }
            }
        });
    if let Err(e) = spawned {
        eprintln!("pa-discovery: no watch thread: {e}");
    }
}


fn apply(visible: &mut BTreeMap<(ServiceKind, String), CoreDiscoveredService>, ev: DiscoveryEvent) {
    match ev {
        DiscoveryEvent::Found(s) | DiscoveryEvent::Updated(s) => {
            visible.insert(s.key(), (&s).into());
        }
        DiscoveryEvent::Lost { kind, instance } => {
            visible.remove(&(kind, instance.to_lowercase()));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    use super::*;

    fn private_port() -> u16 {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let n = nanos.wrapping_add(NEXT.fetch_add(7919, Ordering::Relaxed)) ^ std::process::id();
        30_000 + u16::try_from(n % 20_000).unwrap()
    }

    #[derive(Default)]
    struct CountingLock(Mutex<Vec<bool>>);

    impl MulticastLock for CountingLock {
        fn acquire(&self) {
            self.0.lock().unwrap().push(true);
        }
        fn release(&self) {
            self.0.lock().unwrap().push(false);
        }
    }

    fn until<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(Instant::now() < deadline, "timed out: {what}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn kinds_roundtrip() {
        for kind in ServiceKind::ALL {
            assert_eq!(ServiceKind::from(CoreServiceKind::from(kind)), kind);
        }
        let kinds: Vec<ServiceKind> = discovery_source_kinds()
            .into_iter()
            .map(Into::into)
            .collect();
        for ty in ["_smb._tcp", "_webdav._tcp", "_webdavs._tcp", "_projectalpha._tcp"] {
            assert!(kinds.iter().any(|k| k.service_type() == ty), "{ty}");
        }
    }

    #[test]
    fn service_maps_to_share_fields() {
        let s = DiscoveredService::new(
            ServiceKind::WebDav,
            "Files",
            "nas.local.",
            8080,
            ["192.168.1.7".parse().unwrap()],
            pa_discovery::Txt::from_pairs([("path", "dav")]),
        );
        let c = CoreDiscoveredService::from(&s);
        assert_eq!(c.kind, CoreServiceKind::WebDav);
        assert_eq!(c.host, "nas.local");
        assert_eq!(c.path.as_deref(), Some("/dav"));
        assert_eq!(c.url.as_deref(), Some("http://192.168.1.7:8080/dav"));
        assert_eq!(c.addresses, ["192.168.1.7"]);
        assert_eq!(c.id, None);

        let smb = DiscoveredService::new(
            ServiceKind::Smb,
            "NAS",
            "nas.local.",
            445,
            [],
            pa_discovery::Txt::new(),
        );
        let c = CoreDiscoveredService::from(&smb);
        assert_eq!((c.path, c.url), (None, None));
    }

    #[test]
    fn snapshots_follow_events() {
        let (tx, rx) = mpsc::channel();
        let (out_tx, out) = mpsc::channel();
        spawn_snapshots(rx, move |list| out_tx.send(list).is_ok());
        let next = || out.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(next().is_empty(), "the current (empty) list comes at once");
        let smb = |name: &str| {
            DiscoveredService::new(
                ServiceKind::Smb,
                name,
                "h.local.",
                445,
                ["10.0.0.9".parse().unwrap()],
                pa_discovery::Txt::new(),
            )
        };
        tx.send(DiscoveryEvent::Found(smb("b"))).unwrap();
        let list = next();
        assert_eq!(list.len(), 1);
        tx.send(DiscoveryEvent::Found(smb("A"))).unwrap();
        let mut list = next();
        while list.len() < 2 {
            list = next();
        }
        assert_eq!(
            list.iter().map(|s| s.instance.as_str()).collect::<Vec<_>>(),
            ["A", "b"]
        );
        tx.send(DiscoveryEvent::Lost {
            kind: ServiceKind::Smb,
            instance: "B".into(),
        })
        .unwrap();
        let list = next();
        assert_eq!(list.len(), 1);
        drop(tx);
        assert!(out.recv_timeout(Duration::from_secs(5)).is_err());
    }

    /// The real mDNS stack on loopback and a private port: a share
    /// announced by one daemon is found by another, with the lock held
    /// exactly while browsing.
    #[test]
    fn finds_a_share_on_loopback_and_balances_the_lock() {
        let port = private_port();
        let config = CoreDiscoveryConfig {
            loopback_port: Some(port),
            own_ids: vec![],
        };
        let announcer = CoreDiscovery::with_lock(config.clone(), None).unwrap();
        announcer
            .announce(
                CoreServiceKind::Smb,
                "Test NAS".into(),
                4450,
                vec![CoreTxtEntry {
                    key: "model".into(),
                    value: "Samba".into(),
                }],
            )
            .unwrap();
        announcer
            .announce(
                CoreServiceKind::WebDav,
                "Test DAV".into(),
                8080,
                vec![CoreTxtEntry {
                    key: "path".into(),
                    value: "/dav".into(),
                }],
            )
            .unwrap();

        let lock = Arc::new(CountingLock::default());
        let d = CoreDiscovery::with_lock(config, Some(lock.clone())).unwrap();
        let rx = d.with(|i| i.discovery.subscribe()).unwrap();
        let (out_tx, out) = mpsc::channel();
        spawn_snapshots(rx, move |list| out_tx.send(list).is_ok());
        d.browse(discovery_source_kinds()).unwrap();
        assert_eq!(*lock.0.lock().unwrap(), [true]);

        let list = until("share and WebDAV found", || {
            let list = out.recv_timeout(Duration::from_secs(1)).ok()?;
            (list.len() == 2 && list.iter().all(|s| s.url.is_some())).then_some(list)
        });
        let smb = &list[0];
        assert_eq!(smb.kind, CoreServiceKind::Smb);
        assert_eq!(smb.instance, "Test NAS");
        assert_eq!(smb.port, 4450);
        assert_eq!(smb.url.as_deref(), Some("smb://127.0.0.1:4450/"));
        let dav = &list[1];
        assert_eq!(dav.url.as_deref(), Some("http://127.0.0.1:8080/dav"));
        assert_eq!(d.services(), list);

        d.stop(vec![CoreServiceKind::WebDav]);
        until("WebDAV lost", || {
            (d.services().len() == 1).then_some(())
        });
        d.close();
        assert_eq!(*lock.0.lock().unwrap(), [true, false]);
        assert!(d.services().is_empty());
        assert_eq!(d.browse(vec![]).unwrap_err().code, "closed");
        // The watch stream ends with the discovery.
        until("watch ended", || match out.recv_timeout(Duration::from_secs(1)) {
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(()),
            _ => None,
        });
        announcer.close();
    }

    /// Real LAN, standard port: a Samba server announced by Avahi is found
    /// with an address to connect to. Run against the test container
    /// (core/crates/pa-discovery/tests/servers/README.md):
    /// `PA_LAN_SMB_INSTANCE='NET-020 Samba' cargo test -p pa-frb -- --ignored lan_samba`.
    #[test]
    #[ignore = "uses the real network (multicast on 5353) and a Samba/Avahi host"]
    fn lan_samba() {
        let want = std::env::var("PA_LAN_SMB_INSTANCE").unwrap_or_else(|_| "NET-020 Samba".into());
        let lock = Arc::new(CountingLock::default());
        let d = CoreDiscovery::with_lock(CoreDiscoveryConfig::default(), Some(lock.clone())).unwrap();
        d.browse(discovery_source_kinds()).unwrap();
        let found = until(&format!("{want:?} found"), || {
            d.services()
                .into_iter()
                .find(|s| s.instance == want && s.url.is_some())
        });
        println!("found: {found:?}");
        assert_eq!(found.kind, CoreServiceKind::Smb);
        assert!(found.url.as_deref().unwrap().starts_with("smb://"));
        d.close();
        assert_eq!(*lock.0.lock().unwrap(), [true, false]);
    }

    /// Calls reach the Dart callback in order, even when it answers late.
    #[test]
    fn dart_lock_calls_in_order() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let lock = DartMulticastLock::new(Box::new(move |hold| -> DartFnFuture<()> {
            let seen = Arc::clone(&seen);
            Box::pin(async move {
                seen.lock().unwrap().push(hold);
            })
        }));
        lock.acquire();
        lock.release();
        lock.acquire();
        until("three calls", || (calls.lock().unwrap().len() == 3).then_some(()));
        assert_eq!(*calls.lock().unwrap(), [true, false, true]);
    }
}
