// SPDX-License-Identifier: MPL-2.0
//! The discovery facade: one backend, one registry, many subscribers.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use crate::kind::ServiceKind;
use crate::registry::{DiscoveryEvent, RawEvent, Registry};
use crate::service::DiscoveredService;

/// Discovery error.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    /// The mDNS daemon failed (socket, command queue, invalid name).
    #[error("mDNS: {0}")]
    Mdns(String),
    /// The platform browser (NWBrowser, NsdManager) refused to start.
    #[error("platform browser: {0}")]
    Platform(String),
    /// An announcement is malformed (name too long, TXT entry too big).
    #[error("invalid announcement: {0}")]
    InvalidAnnouncement(String),
    /// The backend or the facade was shut down.
    #[error("discovery is shut down")]
    Shutdown,
}

/// Where a backend reports what it finds. Cheap to clone, `Send + Sync`.
#[derive(Debug, Clone)]
pub struct EventSink {
    tx: Sender<Msg>,
    generation: u64,
}

impl EventSink {
    /// An instance was resolved (first time or again).
    pub fn resolved(&self, service: DiscoveredService) {
        let _ = self
            .tx
            .send(Msg::Raw(self.generation, RawEvent::Resolved(service)));
    }

    /// An instance went away.
    pub fn removed(&self, kind: ServiceKind, instance: impl Into<String>) {
        let _ = self.tx.send(Msg::Raw(
            self.generation,
            RawEvent::Removed {
                kind,
                instance: instance.into(),
            },
        ));
    }
}

/// A source of raw discovery reports: the pure-Rust mDNS browser
/// ([`MdnsBackend`](crate::MdnsBackend)), a platform browser
/// ([`PlatformBackend`](crate::PlatformBackend)) or a test double.
pub trait Backend: Send + Sync + 'static {
    /// Starts browsing `kind`, reporting into `sink` until [`Backend::stop`].
    /// Called at most once per kind between stops.
    fn start(&self, kind: ServiceKind, sink: EventSink) -> Result<(), DiscoveryError>;
    /// Stops browsing `kind`. Reports that arrive afterwards are dropped.
    fn stop(&self, kind: ServiceKind);
    /// Releases everything (sockets, threads, multicast locks).
    fn shutdown(&self) {}
}

enum Msg {
    /// A report tagged with the browse generation of the sink it came from.
    Raw(u64, RawEvent),
    Stopped(ServiceKind),
    Subscribe(Sender<DiscoveryEvent>),
    Quit,
}

struct Shared {
    registry: Registry,
    /// Browsed kinds and the generation of their current browse; reports
    /// from an older generation (a stopped browse) are dropped.
    browsing: BTreeMap<ServiceKind, u64>,
    next_generation: u64,
}

/// Finds services in the LAN and keeps the current picture.
///
/// Events are delivered on a background thread to every subscriber's
/// channel; [`Discovery::services`] gives a snapshot at any time.
pub struct Discovery {
    backend: Box<dyn Backend>,
    shared: Arc<Mutex<Shared>>,
    tx: Sender<Msg>,
    worker: Option<JoinHandle<()>>,
}

impl Discovery {
    /// Wraps a backend. `own_ids` are TXT ids of this server or device, so
    /// it never "finds" itself.
    pub fn new<I: Into<String>>(
        backend: impl Backend,
        own_ids: impl IntoIterator<Item = I>,
    ) -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            registry: Registry::new().with_ignored_ids(own_ids),
            browsing: BTreeMap::new(),
            next_generation: 0,
        }));
        let (tx, rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("pa-discovery".into())
            .spawn(move || run(&rx, &worker_shared))
            .expect("spawn discovery thread");
        Self {
            backend: Box::new(backend),
            shared,
            tx,
            worker: Some(worker),
        }
    }

    /// Starts browsing `kinds` (already browsed kinds are skipped).
    pub fn browse(
        &self,
        kinds: impl IntoIterator<Item = ServiceKind>,
    ) -> Result<(), DiscoveryError> {
        for kind in kinds {
            let generation = {
                let mut s = lock(&self.shared);
                if s.browsing.contains_key(&kind) {
                    continue;
                }
                s.next_generation += 1;
                let generation = s.next_generation;
                s.browsing.insert(kind, generation);
                generation
            };
            let sink = EventSink {
                tx: self.tx.clone(),
                generation,
            };
            if let Err(e) = self.backend.start(kind, sink) {
                lock(&self.shared).browsing.remove(&kind);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Stops browsing `kind`; its instances are reported lost.
    pub fn stop(&self, kind: ServiceKind) {
        if lock(&self.shared).browsing.remove(&kind).is_some() {
            self.backend.stop(kind);
            let _ = self.tx.send(Msg::Stopped(kind));
        }
    }

    /// Kinds being browsed.
    pub fn browsing(&self) -> Vec<ServiceKind> {
        lock(&self.shared).browsing.keys().copied().collect()
    }

    /// New subscription. It first receives [`DiscoveryEvent::Found`] for
    /// every instance already visible, then live changes. Dropping the
    /// receiver unsubscribes.
    pub fn subscribe(&self) -> Receiver<DiscoveryEvent> {
        let (tx, rx) = mpsc::channel();
        let _ = self.tx.send(Msg::Subscribe(tx));
        rx
    }

    /// Snapshot of the visible services.
    pub fn services(&self) -> Vec<DiscoveredService> {
        lock(&self.shared).registry.services().cloned().collect()
    }

    /// Snapshot of the visible services of one kind.
    pub fn services_of(&self, kind: ServiceKind) -> Vec<DiscoveredService> {
        lock(&self.shared)
            .registry
            .services_of(kind)
            .cloned()
            .collect()
    }

    /// Our server with TXT `id`, if visible (reconnect by identity, not IP).
    pub fn server_by_id(&self, id: &str) -> Option<DiscoveredService> {
        lock(&self.shared).registry.server_by_id(id).cloned()
    }

    /// Stops all browsing and the backend. Also done on drop.
    pub fn shutdown(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        let kinds: Vec<_> = std::mem::take(&mut lock(&self.shared).browsing)
            .into_keys()
            .collect();
        for kind in kinds {
            self.backend.stop(kind);
        }
        self.backend.shutdown();
        let _ = self.tx.send(Msg::Quit);
        let _ = worker.join();
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn run(rx: &Receiver<Msg>, shared: &Mutex<Shared>) {
    let mut subscribers: Vec<Sender<DiscoveryEvent>> = Vec::new();
    while let Ok(msg) = rx.recv() {
        let events = match msg {
            Msg::Quit => break,
            Msg::Raw(generation, raw) => {
                let mut s = lock(shared);
                let kind = match &raw {
                    RawEvent::Resolved(svc) => svc.kind,
                    RawEvent::Removed { kind, .. } => *kind,
                };
                // Late reports of a stopped browse would resurrect stale
                // entries; a backend reporting another kind is a bug.
                if s.browsing.get(&kind) != Some(&generation) {
                    continue;
                }
                s.registry.apply(raw).into_iter().collect::<Vec<_>>()
            }
            // The channel is FIFO: everything of the stopped browse was sent
            // before this, everything of a restarted browse after it.
            Msg::Stopped(kind) => lock(shared).registry.clear_kind(kind),
            Msg::Subscribe(tx) => {
                let snapshot: Vec<_> = lock(shared).registry.services().cloned().collect();
                if snapshot
                    .into_iter()
                    .all(|svc| tx.send(DiscoveryEvent::Found(svc)).is_ok())
                {
                    subscribers.push(tx);
                }
                continue;
            }
        };
        for event in events {
            subscribers.retain(|sub| sub.send(event.clone()).is_ok());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::Txt;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    /// Test backend: hands the sinks out so the test plays the network.
    #[derive(Clone, Default)]
    struct Mock(Arc<StdMutex<MockState>>);

    #[derive(Default)]
    struct MockState {
        sinks: Vec<(ServiceKind, EventSink)>,
        stopped: Vec<ServiceKind>,
        shutdown: bool,
        fail: Option<ServiceKind>,
    }

    impl Backend for Mock {
        fn start(&self, kind: ServiceKind, sink: EventSink) -> Result<(), DiscoveryError> {
            let mut s = self.0.lock().unwrap();
            if s.fail == Some(kind) {
                return Err(DiscoveryError::Platform("denied".into()));
            }
            s.sinks.push((kind, sink));
            Ok(())
        }
        fn stop(&self, kind: ServiceKind) {
            self.0.lock().unwrap().stopped.push(kind);
        }
        fn shutdown(&self) {
            self.0.lock().unwrap().shutdown = true;
        }
    }

    impl Mock {
        fn sink(&self, kind: ServiceKind) -> EventSink {
            let s = self.0.lock().unwrap();
            s.sinks
                .iter()
                .rev()
                .find(|(k, _)| *k == kind)
                .unwrap()
                .1
                .clone()
        }
    }

    fn smb(name: &str) -> DiscoveredService {
        DiscoveredService::new(
            ServiceKind::Smb,
            name,
            "nas.local.",
            445,
            ["192.168.1.2".parse().unwrap()],
            Txt::new(),
        )
    }

    fn next(rx: &Receiver<DiscoveryEvent>) -> DiscoveryEvent {
        rx.recv_timeout(Duration::from_secs(5)).expect("event")
    }

    #[test]
    fn events_flow_to_subscribers_and_snapshot() {
        let mock = Mock::default();
        let d = Discovery::new(mock.clone(), ["self-id"]);
        let rx = d.subscribe();
        d.browse([ServiceKind::Smb, ServiceKind::SyncPeer]).unwrap();
        d.browse([ServiceKind::Smb]).unwrap(); // idempotent
        assert_eq!(d.browsing(), [ServiceKind::SyncPeer, ServiceKind::Smb]);
        assert_eq!(mock.0.lock().unwrap().sinks.len(), 2);

        let sink = mock.sink(ServiceKind::Smb);
        sink.resolved(smb("NAS"));
        sink.resolved(smb("NAS"));
        assert_eq!(next(&rx), DiscoveryEvent::Found(smb("NAS")));

        // Our own announcement is ignored.
        let me = DiscoveredService::new(
            ServiceKind::SyncPeer,
            "Me",
            "",
            7000,
            [],
            Txt::from_pairs([("id", "self-id"), ("v", "1")]),
        );
        mock.sink(ServiceKind::SyncPeer).resolved(me);

        sink.removed(ServiceKind::Smb, "NAS");
        assert_eq!(
            next(&rx),
            DiscoveryEvent::Lost {
                kind: ServiceKind::Smb,
                instance: "NAS".into()
            }
        );
        assert!(d.services().is_empty());
    }

    #[test]
    fn late_subscriber_gets_snapshot_first() {
        let mock = Mock::default();
        let d = Discovery::new(mock.clone(), Vec::<String>::new());
        d.browse([ServiceKind::Smb]).unwrap();
        let first = d.subscribe();
        mock.sink(ServiceKind::Smb).resolved(smb("A"));
        assert!(matches!(next(&first), DiscoveryEvent::Found(_)));
        let late = d.subscribe();
        assert_eq!(next(&late), DiscoveryEvent::Found(smb("A")));
        assert_eq!(d.services_of(ServiceKind::Smb), [smb("A")]);
    }

    #[test]
    fn stop_reports_lost_and_drops_late_reports() {
        let mock = Mock::default();
        let d = Discovery::new(mock.clone(), Vec::<String>::new());
        let rx = d.subscribe();
        d.browse([ServiceKind::Smb]).unwrap();
        let sink = mock.sink(ServiceKind::Smb);
        sink.resolved(smb("A"));
        assert!(matches!(next(&rx), DiscoveryEvent::Found(_)));
        d.stop(ServiceKind::Smb);
        assert!(matches!(next(&rx), DiscoveryEvent::Lost { .. }));
        assert_eq!(mock.0.lock().unwrap().stopped, [ServiceKind::Smb]);
        // A report racing with stop is dropped.
        sink.resolved(smb("B"));
        d.browse([ServiceKind::Smb]).unwrap();
        mock.sink(ServiceKind::Smb).resolved(smb("C"));
        assert_eq!(next(&rx), DiscoveryEvent::Found(smb("C")));
        assert_eq!(d.services_of(ServiceKind::Smb), [smb("C")]);
    }

    #[test]
    fn failed_start_is_not_marked_browsing() {
        let mock = Mock::default();
        mock.0.lock().unwrap().fail = Some(ServiceKind::Nfs);
        let d = Discovery::new(mock.clone(), Vec::<String>::new());
        assert!(matches!(
            d.browse([ServiceKind::Nfs]),
            Err(DiscoveryError::Platform(_))
        ));
        assert!(d.browsing().is_empty());
    }

    #[test]
    fn drop_shuts_backend_down() {
        let mock = Mock::default();
        {
            let d = Discovery::new(mock.clone(), Vec::<String>::new());
            d.browse([ServiceKind::Server]).unwrap();
        }
        let s = mock.0.lock().unwrap();
        assert!(s.shutdown);
        assert_eq!(s.stopped, [ServiceKind::Server]);
    }
}
