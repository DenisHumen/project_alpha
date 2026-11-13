// SPDX-License-Identifier: MPL-2.0
//! State of a LAN node shared by its links: the journal, the links, cursors
//! and the routing of ops and playback messages between them.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use pa_discovery::DeviceClass;
use pa_model::MediaKey;
use pa_pairing::{Family, FamilyUpdate};
use tokio::sync::mpsc;

use super::election::Election;
use super::{LanConfig, LanEvent};
use crate::hlc::Hlc;
use crate::proto::{
    Ack, Change, Cursor, ErrorFrame, Hello, MAX_PULL_LIMIT, Message, OpFrame, ProtoRange, Pull,
    Role, code, encode,
};
use crate::OpId;

/// Frames to a link's writer; `None` closes the link.
pub(crate) type LinkTx = mpsc::UnboundedSender<Option<String>>;

/// Where a message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    App,
    Link(u64),
}

/// Which way a link goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Side {
    /// We dialed the hub `hub_id`.
    Up { hub_id: String },
    /// A device dialed us.
    Down,
}

struct Link {
    device_id: String,
    side: Side,
    tx: LinkTx,
    /// Journal id of the hub (upstream links only).
    log: Option<String>,
}

pub(crate) struct Shared {
    pub(crate) config: LanConfig,
    pub(crate) device_id: String,
    pub(crate) tls_server: Arc<rustls::ServerConfig>,
    pub(crate) tls_client: Arc<rustls::ClientConfig>,
    state: Mutex<State>,
}

pub(crate) struct State {
    pub(crate) family: Family,
    pub(crate) election: Election,
    pub(crate) hub: Option<String>,
    revoked: bool,
    playback_ttl: std::time::Duration,
    events: mpsc::UnboundedSender<LanEvent>,
    /// Every op this node has seen; also what it serves as a hub.
    journal: crate::proto::Journal,
    links: HashMap<u64, Link>,
    next_link: u64,
    /// Our cursor in each hub's journal, by hub device id.
    cursors: HashMap<String, Cursor>,
    /// Ops each hub journal (by log id) is known to hold.
    acked: HashMap<String, HashSet<OpId>>,
    /// Last `pb.update` frame per title, replayed to new links.
    playback: HashMap<(String, MediaKey), (Instant, String)>,
    /// Newest stamp per message type and title: drops repeats and loops.
    seen: HashMap<(bool, String, MediaKey), Hlc>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn frame(msg: &Message) -> Option<String> {
    encode(msg).ok()
}

impl Shared {
    pub(crate) fn new(
        config: LanConfig,
        family: Family,
        events: mpsc::UnboundedSender<LanEvent>,
    ) -> std::io::Result<Self> {
        let device_id = family.identity().device_id().to_owned();
        let log = uuid::Uuid::now_v7().to_string();
        let journal = crate::proto::Journal::new(log).map_err(super::tls::io_err)?;
        let state = State {
            election: Election::new(device_id.clone(), config.class),
            family,
            hub: None,
            revoked: false,
            playback_ttl: config.playback_ttl,
            events,
            journal,
            links: HashMap::new(),
            next_link: 1,
            cursors: HashMap::new(),
            acked: HashMap::new(),
            playback: HashMap::new(),
            seen: HashMap::new(),
        };
        Ok(Self {
            tls_server: Arc::new(super::tls::server_config()?),
            tls_client: Arc::new(super::tls::client_config()?),
            config,
            device_id,
            state: Mutex::new(state),
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    pub(crate) fn with_family<R>(&self, f: impl FnOnce(&Family) -> R) -> R {
        f(&self.lock().family)
    }

    pub(crate) fn peer_up(&self, device_id: &str, class: DeviceClass, addr: SocketAddr) {
        self.lock().election.upsert(device_id, class, addr);
    }

    pub(crate) fn peer_down(&self, device_id: &str) {
        self.lock().election.remove(device_id);
    }
}

impl State {
    fn emit(&self, event: LanEvent) {
        let _ = self.events.send(event);
    }

    pub(crate) fn set_hub(&mut self, hub: Option<String>) {
        if self.hub != hub {
            self.hub.clone_from(&hub);
            self.emit(LanEvent::HubChanged(hub));
        }
    }

    pub(crate) fn link_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.links.values().map(|l| l.device_id.clone()).collect();
        ids.sort();
        ids
    }

    /// Our `hello` on a link of `side`.
    pub(crate) fn hello(&self, side: &Side, nonce: String) -> Hello {
        let (role, cursor) = match side {
            Side::Up { hub_id } => (Role::Device, self.cursors.get(hub_id).cloned()),
            Side::Down => (Role::Hub, None),
        };
        Hello {
            proto: ProtoRange::LOCAL,
            device_id: self.family.identity().device_id().to_owned(),
            role,
            cursor,
            head: Some(self.journal.head()),
            nonce: Some(nonce),
        }
    }

    /// Checks a peer's `auth` against the roster.
    pub(crate) fn verify(
        &self,
        peer: &str,
        peer_nonce: &str,
        own_nonce: &str,
        sig: &str,
    ) -> Result<(), pa_pairing::Error> {
        if self.revoked {
            return Err(pa_pairing::Error::Revoked(peer.to_owned()));
        }
        let me = self.family.identity().device_id();
        pa_pairing::auth::verify(self.family.roster(), peer, me, peer_nonce, own_nonce, sig)
            .map(|_| ())
    }

    /// Adds a link that passed the handshake and sends what it needs first.
    pub(crate) fn register(&mut self, side: Side, peer: &Hello, tx: LinkTx) -> u64 {
        let id = self.next_link;
        self.next_link += 1;
        let log = match &side {
            Side::Up { .. } => peer.head.as_ref().map(|h| h.log.clone()),
            Side::Down => None,
        };
        let link = Link {
            device_id: peer.device_id.clone(),
            side: side.clone(),
            tx,
            log,
        };
        match &side {
            Side::Up { hub_id } => {
                // Push what this hub journal lacks, then pull what we lack.
                let acked = link.log.as_ref().and_then(|l| self.acked.get(l));
                for f in self.journal.iter() {
                    if acked.is_none_or(|a| !a.contains(&f.op_id)) {
                        let up = OpFrame {
                            seq: None,
                            ..f.clone()
                        };
                        Self::send(&link.tx, &Message::Op(up));
                    }
                }
                let cursor = self
                    .cursors
                    .get(hub_id)
                    .filter(|c| Some(&c.log) == link.log.as_ref())
                    .cloned();
                Self::send(
                    &link.tx,
                    &Message::Pull(Pull {
                        cursor,
                        limit: MAX_PULL_LIMIT,
                    }),
                );
                let hub = hub_id.clone();
                self.links.insert(id, link);
                self.set_hub(Some(hub));
            }
            Side::Down => {
                if let Ok(update) = self.family.update_for(&peer.device_id) {
                    Self::send(
                        &link.tx,
                        &Message::Family(crate::proto::Family {
                            body: update.to_json(),
                        }),
                    );
                }
                let ttl = self.playback_ttl;
                self.playback.retain(|_, (at, _)| at.elapsed() < ttl);
                for (_, raw) in self.playback.values() {
                    let _ = link.tx.send(Some(raw.clone()));
                }
                let head = self.journal.head();
                if peer.cursor.as_ref() != Some(&head) {
                    Self::send(&link.tx, &Message::Change(Change { head }));
                }
                self.links.insert(id, link);
            }
        }
        id
    }

    pub(crate) fn unregister(&mut self, id: u64) {
        if let Some(link) = self.links.remove(&id)
            && let Side::Up { hub_id } = link.side
            && self.hub.as_ref() == Some(&hub_id)
        {
            self.set_hub(None);
        }
    }

    fn send(tx: &LinkTx, msg: &Message) {
        if let Some(text) = frame(msg) {
            let _ = tx.send(Some(text));
        }
    }

    fn send_error(tx: &LinkTx, code: &str, op_id: Option<OpId>) {
        Self::send(
            tx,
            &Message::Error(ErrorFrame {
                code: code.to_owned(),
                message: None,
                op_id,
            }),
        );
    }

    /// Stores an op and routes it to every other link. Returns its `seq`
    /// in our journal.
    pub(crate) fn on_op(&mut self, frame: OpFrame, source: Source) -> u64 {
        let frame = OpFrame { seq: None, ..frame };
        let (seq, new) = self.journal.append(&frame);
        if !new {
            return seq;
        }
        if source != Source::App
            && let Ok(op) = frame.open()
        {
            self.emit(LanEvent::Op(op));
        }
        for (id, link) in &self.links {
            if Source::Link(*id) == source {
                continue;
            }
            let out = match link.side {
                Side::Down => OpFrame {
                    seq: Some(seq),
                    ..frame.clone()
                },
                Side::Up { .. } => frame.clone(),
            };
            Self::send(&link.tx, &Message::Op(out));
        }
        seq
    }

    /// Routes `pb.update` / `pb.takeover` (as the original `raw` frame) to
    /// every other link, once per stamp.
    pub(crate) fn on_playback(&mut self, raw: &str, msg: Message, source: Source) {
        let (is_update, profile, key, hlc) = match &msg {
            Message::PbUpdate(u) => (true, &u.profile, &u.key, &u.hlc),
            Message::PbTakeover(t) => (false, &t.profile, &t.key, &t.hlc),
            _ => return,
        };
        let slot = (is_update, profile.clone(), key.clone());
        if self.seen.get(&slot).is_some_and(|seen| seen >= hlc) {
            return;
        }
        self.seen.insert(slot, hlc.clone());
        if is_update {
            self.playback
                .insert((profile.clone(), key.clone()), (Instant::now(), raw.to_owned()));
        }
        for (id, link) in &self.links {
            if Source::Link(*id) != source {
                let _ = link.tx.send(Some(raw.to_owned()));
            }
        }
        if source != Source::App {
            self.emit(match msg {
                Message::PbUpdate(u) => LanEvent::Playback(u),
                Message::PbTakeover(t) => LanEvent::Takeover(t),
                _ => unreachable!("matched above"),
            });
        }
    }

    /// Handles one message of link `id`.
    pub(crate) fn on_message(&mut self, id: u64, raw: &str, msg: Message) {
        let Some(link) = self.links.get(&id) else {
            return;
        };
        let (tx, side, log) = (link.tx.clone(), link.side.clone(), link.log.clone());
        match msg {
            Message::Op(f) => {
                if let (Side::Up { hub_id }, Some(log), Some(seq)) = (&side, &log, f.seq) {
                    self.acked.entry(log.clone()).or_default().insert(f.op_id);
                    let cursor = self.cursors.entry(hub_id.clone()).or_insert(Cursor {
                        log: log.clone(),
                        seq: 0,
                    });
                    if cursor.log == *log && cursor.seq + 1 == seq {
                        cursor.seq = seq;
                    }
                }
                let op_id = f.op_id;
                let seq = self.on_op(f, Source::Link(id));
                if side == Side::Down {
                    Self::send(&tx, &Message::Ack(Ack { op_id, seq: Some(seq) }));
                }
            }
            Message::Ack(a) => {
                if let Some(log) = log {
                    self.acked.entry(log).or_default().insert(a.op_id);
                    self.emit(LanEvent::Acked(a.op_id));
                }
            }
            Message::Change(c) => {
                if let Side::Up { hub_id } = &side {
                    let cursor = self.cursors.get(hub_id).filter(|k| k.log == c.head.log);
                    if cursor.is_none_or(|k| k.seq < c.head.seq) {
                        Self::send(
                            &tx,
                            &Message::Pull(Pull {
                                cursor: cursor.cloned(),
                                limit: MAX_PULL_LIMIT,
                            }),
                        );
                    }
                }
            }
            Message::Pull(p) => {
                let (frames, end) = self.journal.pull(&p);
                for f in frames {
                    Self::send(&tx, &Message::Op(f));
                }
                Self::send(&tx, &Message::PullEnd(end));
            }
            Message::PullEnd(end) => {
                if let Side::Up { hub_id } = &side {
                    let more = end.more;
                    let cursor = end.cursor;
                    self.cursors.insert(hub_id.clone(), cursor.clone());
                    if more {
                        Self::send(
                            &tx,
                            &Message::Pull(Pull {
                                cursor: Some(cursor),
                                limit: MAX_PULL_LIMIT,
                            }),
                        );
                    }
                }
            }
            msg @ (Message::PbUpdate(_) | Message::PbTakeover(_)) => {
                self.on_playback(raw, msg, Source::Link(id));
            }
            Message::Family(f) => self.on_family(id, &f.body),
            Message::Error(e) if e.code == code::UNAUTHORIZED => {
                let _ = tx.send(None);
            }
            _ => {}
        }
    }

    fn on_family(&mut self, from: u64, body: &str) {
        let Ok(update) = FamilyUpdate::from_json(body) else {
            return;
        };
        match self.family.apply_update(&update) {
            Ok(report) if report.roster.changed() || report.key_rotated => {
                self.emit(LanEvent::FamilyChanged);
                // Spread the change and drop links of revoked devices.
                for (id, link) in &self.links {
                    match self.family.update_for(&link.device_id) {
                        Ok(update) if *id != from => Self::send(
                            &link.tx,
                            &Message::Family(crate::proto::Family {
                                body: update.to_json(),
                            }),
                        ),
                        Ok(_) => {}
                        Err(_) => {
                            Self::send_error(&link.tx, code::UNAUTHORIZED, None);
                            let _ = link.tx.send(None);
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(pa_pairing::Error::Revoked(_)) => {
                self.revoked = true;
                self.emit(LanEvent::Revoked);
                for link in self.links.values() {
                    let _ = link.tx.send(None);
                }
            }
            Err(e) => tracing::debug!(error = %e, "lan sync: family update rejected"),
        }
    }
}
