// SPDX-License-Identifier: MPL-2.0
//! LAN P2P transport of sync (SYNC-006, feature `lan`): devices of one
//! family find each other over mDNS (`_alpha-sync._tcp`, ADR-0019), elect a
//! hub and speak [`proto`](crate::proto) over WebSocket on TLS 1.3 bound to
//! the pairing keys ([`tls`]). No project servers: the hub is whichever
//! device of the user is on (server, desktop, TV, phone — [`election`]).
//!
//! Every [`LanNode`] listens and keeps a [`Journal`](crate::proto::Journal)
//! of every op it has seen, and dials the elected hub. If devices disagree
//! about the hub for a moment (one has not seen a peer yet), ops still flow:
//! each node relays between its upstream and its downstream links, and
//! `op_id` / HLC deduplication stops loops. When the hub's link breaks
//! (closed socket, or no frame for [`LanConfig::dead_after`]) the device
//! marks it dead and dials the next best one; on a new hub it pushes every
//! op the hub has not acknowledged and pulls what it lacks, so the family
//! converges whatever the order of failures.
//!
//! The node does not own the library: it reports [`LanEvent`]s, and the app
//! applies received ops ([`Replica::receive`](crate::Replica::receive),
//! [`library::apply`](crate::library)), acks its outbox on
//! [`LanEvent::Acked`], and shows "continue on this device" from
//! [`LanEvent::Playback`]. On start the app publishes its outbox
//! ([`outbox::due`](crate::outbox::due)) with [`LanNode::publish`].
//!
//! [`pair`] runs pairing by QR code or 6-digit code between two apps, and
//! [`service::SyncService`] ties identity, family, pairing and the node
//! together for one app (UI-019).

pub mod election;
pub mod pair;
pub mod service;
pub mod tls;

mod conn;
mod state;

use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use pa_discovery::{Announcement, DeviceClass, DiscoveryEvent, PeerTxt, ServiceKind};
use pa_pairing::Family;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

use crate::proto::{Message, PROTOCOL_VERSION, PlaybackUpdate, Takeover};
use crate::{Error, Op, OpId};
use election::Hub;
use state::{Shared, Source};

/// Most inbound links (handshakes included) a node keeps at once.
pub const MAX_LINKS: usize = 256;

/// Settings of a [`LanNode`].
#[derive(Debug, Clone)]
pub struct LanConfig {
    /// Device class for the hub election and TXT `dev`.
    pub class: DeviceClass,
    /// Display name for TXT `name`.
    pub name: Option<String>,
    /// Where to listen; port 0 picks a free one (announced over mDNS).
    pub bind: SocketAddr,
    /// A WebSocket ping is sent this often on every link.
    pub ping_interval: Duration,
    /// A link with no frame for this long is dead.
    pub dead_after: Duration,
    /// A dead hub is not dialed again for this long (unless it re-announces
    /// with a new address).
    pub dead_cooldown: Duration,
    /// Limit for TCP connect, TLS, WebSocket upgrade and `hello`/`auth`.
    pub handshake_timeout: Duration,
    /// How long the last `pb.update` per title is replayed to devices that
    /// connect (docs/api/sync-protocol.md §4, item 5).
    pub playback_ttl: Duration,
    /// How often the election is re-evaluated.
    pub tick: Duration,
}

impl LanConfig {
    /// Defaults for a device of `class`: a free port on every interface,
    /// a 2 s ping and a 6 s dead timeout, so a lost hub is replaced in
    /// well under 10 s.
    pub fn new(class: DeviceClass) -> Self {
        Self {
            class,
            name: None,
            bind: SocketAddr::from(([0, 0, 0, 0], 0)),
            ping_interval: Duration::from_secs(2),
            dead_after: Duration::from_secs(6),
            dead_cooldown: Duration::from_secs(20),
            handshake_timeout: Duration::from_secs(5),
            playback_ttl: Duration::from_secs(600),
            tick: Duration::from_millis(250),
        }
    }
}

/// What the node reports to the app.
#[derive(Debug, Clone, PartialEq)]
pub enum LanEvent {
    /// A new op from another device: apply it.
    Op(Op),
    /// The hub holds our op: remove it from the outbox. A LAN hub keeps its
    /// journal in memory, so an op acked here is lost only if the hub and
    /// every device it reached restart before passing it on.
    Acked(OpId),
    /// What another device is playing now (instant resume).
    Playback(PlaybackUpdate),
    /// Another device started playing a title: pause it here if allowed.
    Takeover(Takeover),
    /// The hub changed: `Some(id)` is connected, `None` means this device
    /// is the hub (or alone).
    HubChanged(Option<String>),
    /// The roster or the family key changed: save the family.
    FamilyChanged,
    /// This device was revoked: forget the family; the node stops talking.
    Revoked,
}

/// One device in LAN sync. Dropping it closes every link.
pub struct LanNode {
    shared: Arc<Shared>,
    local_addr: SocketAddr,
    _listener: AbortOnDrop,
    _supervisor: AbortOnDrop,
}

impl LanNode {
    /// Starts listening and electing. Events go to the returned receiver.
    pub async fn start(
        config: LanConfig,
        family: Family,
    ) -> std::io::Result<(Self, mpsc::UnboundedReceiver<LanEvent>)> {
        let listener = TcpListener::bind(config.bind).await?;
        let local_addr = listener.local_addr()?;
        let (events, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared::new(config, family, events)?);
        let listener = AbortOnDrop(tokio::spawn(accept_loop(Arc::clone(&shared), listener)));
        let supervisor = AbortOnDrop(tokio::spawn(supervise(Arc::downgrade(&shared))));
        Ok((
            Self {
                shared,
                local_addr,
                _listener: listener,
                _supervisor: supervisor,
            },
            rx,
        ))
    }

    /// Listening address (the port to announce).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// This device's id.
    pub fn device_id(&self) -> &str {
        &self.shared.device_id
    }

    /// TXT of this device's `_alpha-sync._tcp` announcement; `fp` is the
    /// pairing fingerprint of its signing key.
    pub fn peer_txt(&self) -> PeerTxt {
        PeerTxt {
            id: self.shared.device_id.clone(),
            protocol_version: u32::from(PROTOCOL_VERSION),
            device_class: self.shared.config.class,
            name: self.shared.config.name.clone(),
            fingerprint: Some(
                self.shared
                    .with_family(|f| f.identity().public().fingerprint()),
            ),
        }
    }

    /// The mDNS announcement of this device (publish it with
    /// `pa_discovery::Advertiser`).
    pub fn announcement(&self, instance: impl Into<String>) -> Announcement {
        Announcement::peer(instance, self.local_addr.port(), &self.peer_txt())
    }

    /// A peer is visible at `addr` (from discovery or configured by hand).
    pub fn peer_up(&self, device_id: &str, class: DeviceClass, addr: SocketAddr) {
        self.shared.peer_up(device_id, class, addr);
    }

    /// A peer went away.
    pub fn peer_down(&self, device_id: &str) {
        self.shared.peer_down(device_id);
    }

    /// Feeds `_alpha-sync._tcp` results of a `pa_discovery::Discovery`
    /// subscription into the election, on a thread that ends with the
    /// subscription or the node.
    pub fn watch_discovery(
        &self,
        events: std::sync::mpsc::Receiver<DiscoveryEvent>,
    ) -> std::thread::JoinHandle<()> {
        let weak = Arc::downgrade(&self.shared);
        std::thread::spawn(move || watch(&weak, &events))
    }

    /// Sends a locally recorded op ([`Replica::record`](crate::Replica::record)).
    pub fn publish(&self, op: &Op) -> Result<(), Error> {
        let frame = crate::proto::OpFrame::plain(op)?;
        self.shared.lock().on_op(frame, Source::App);
        Ok(())
    }

    /// Sends what this device is playing now (every 5 s and on pause,
    /// seek, stop, background).
    pub fn playback(&self, update: PlaybackUpdate) -> Result<(), Error> {
        let raw = crate::proto::encode(&Message::PbUpdate(update.clone()))?;
        self.shared
            .lock()
            .on_playback(&raw, Message::PbUpdate(update), Source::App);
        Ok(())
    }

    /// Announces that this device started playing a title.
    pub fn takeover(&self, takeover: Takeover) -> Result<(), Error> {
        let raw = crate::proto::encode(&Message::PbTakeover(takeover.clone()))?;
        self.shared
            .lock()
            .on_playback(&raw, Message::PbTakeover(takeover), Source::App);
        Ok(())
    }

    /// The connected hub; `None` if this device is the hub or alone.
    pub fn hub(&self) -> Option<String> {
        self.shared.lock().hub.clone()
    }

    /// Device ids of the live links (upstream and downstream).
    pub fn links(&self) -> Vec<String> {
        self.shared.lock().link_ids()
    }

    /// Reads the family (to save it after [`LanEvent::FamilyChanged`]).
    pub fn with_family<R>(&self, f: impl FnOnce(&Family) -> R) -> R {
        self.shared.with_family(f)
    }
}

/// Aborts a task when dropped.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn accept_loop(shared: Arc<Shared>, listener: TcpListener) {
    // Dropping the set (the node is gone) aborts every accepted link.
    let mut links = JoinSet::new();
    loop {
        while links.try_join_next().is_some() {}
        let Ok((tcp, _)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        if links.len() >= MAX_LINKS {
            continue;
        }
        let shared = Arc::clone(&shared);
        links.spawn(async move {
            if let Err(e) = conn::accept(shared, tcp).await {
                tracing::debug!(error = %e, "lan sync: inbound link closed");
            }
        });
    }
}

/// Keeps one upstream link to the elected hub.
async fn supervise(weak: Weak<Shared>) {
    let mut upstream: Option<(String, AbortOnDrop)> = None;
    loop {
        let Some(shared) = weak.upgrade() else { return };
        let tick = shared.config.tick;
        let now = Instant::now();
        if let Some((id, task)) = &upstream
            && task.0.is_finished()
        {
            shared
                .lock()
                .election
                .mark_dead(id, now + shared.config.dead_cooldown);
            upstream = None;
        }
        let want = shared.lock().election.hub(now);
        match want {
            Hub::Me => {
                upstream = None;
                shared.lock().set_hub(None);
            }
            Hub::Peer { device_id, addr } => {
                if upstream.as_ref().is_none_or(|(id, _)| *id != device_id) {
                    let task =
                        tokio::spawn(conn::dial(Arc::clone(&shared), device_id.clone(), addr));
                    upstream = Some((device_id, AbortOnDrop(task)));
                }
            }
        }
        drop(shared);
        tokio::time::sleep(tick).await;
    }
}

fn watch(weak: &Weak<Shared>, events: &std::sync::mpsc::Receiver<DiscoveryEvent>) {
    use std::collections::HashMap;
    use std::sync::mpsc::RecvTimeoutError;
    // Lost events carry only the instance name.
    let mut ids: HashMap<String, String> = HashMap::new();
    loop {
        let event = match events.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => {
                if weak.strong_count() == 0 {
                    return;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let Some(shared) = weak.upgrade() else { return };
        match event {
            DiscoveryEvent::Found(s) | DiscoveryEvent::Updated(s)
                if s.kind == ServiceKind::SyncPeer =>
            {
                let Ok(txt) = s.peer_txt() else { continue };
                let compatible = txt.protocol_version.abs_diff(u32::from(PROTOCOL_VERSION)) <= 1;
                if let (true, Some(ip)) = (compatible, s.primary_address()) {
                    ids.insert(s.instance.to_lowercase(), txt.id.clone());
                    shared.peer_up(&txt.id, txt.device_class, SocketAddr::new(ip, s.port));
                }
            }
            DiscoveryEvent::Lost {
                kind: ServiceKind::SyncPeer,
                instance,
            } => {
                if let Some(id) = ids.remove(&instance.to_lowercase()) {
                    shared.peer_down(&id);
                }
            }
            _ => {}
        }
    }
}
