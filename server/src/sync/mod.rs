// SPDX-License-Identifier: MPL-2.0
//! Sync hub (SYNC-008): the sync protocol (`docs/api/sync-protocol.md`) over
//! WebSocket at [`PATH`].
//!
//! The server is the hub for devices that know it (ADR-0004, mode B):
//!
//! - **ops** go into a SQLite journal ([`journal`], `sync.sqlite3` in the
//!   data directory), are acked with their `seq` and fanned out to the other
//!   connected devices; duplicates (the same op also arriving over P2P or a
//!   resend from an outbox) are recognised by `op_id` and get the same `seq`.
//!   Plain progress ops are also written into the server library
//!   (`pa_library::progress::apply_remote`), so the server knows where each
//!   profile stopped in its own library.
//! - **`pb.update` / `pb.takeover`** are forwarded to the other devices at
//!   once and never stored on disk. The last `pb.update` per
//!   `(profile, key)` stays in memory for [`SyncConfig::playback_ttl_secs`]
//!   and is sent to each device right after the handshake (instant resume).
//!
//! There is no authentication yet: the server listens on loopback by default
//! and devices are trusted. Pairing and tokens come with SYNC-005 and
//! SRV-003, which will answer unknown devices with `error{unauthorized}`.

pub mod journal;
mod playback;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::ws::{Message as WsMessage, Utf8Bytes, WebSocket};
use pa_sync::proto::{
    self, Ack, Change, ErrorFrame, Hello, Message, OpFrame, ProtoRange, Role, code,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, watch};

pub use crate::config::SyncConfig;
use crate::{Error, Library};
use journal::{JournalError, SqliteJournal};
use playback::PlaybackCache;

/// WebSocket endpoint of the hub.
pub const PATH: &str = "/api/v1/sync";

/// Journal file inside the data directory.
pub const JOURNAL_FILE: &str = "sync.sqlite3";

/// How long a device has to send its `hello`.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// The hub pings idle connections this often…
const PING_INTERVAL: Duration = Duration::from_secs(30);
/// …and drops a connection that sent nothing (not even a pong) this long.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// Fan-out queue per connection; a device that falls further behind gets
/// `change{head}` and pulls the ops it missed.
const FANOUT_CAPACITY: usize = 4096;

/// One frame to fan out, already encoded once for every receiver.
#[derive(Clone)]
struct Event {
    /// Connection that sent it (it does not get its own frame back).
    from: u64,
    frame: Utf8Bytes,
}

struct Inner {
    journal: Mutex<SqliteJournal>,
    hub_id: String,
    library: Library,
    playback: Mutex<PlaybackCache>,
    events: broadcast::Sender<Event>,
    slots: Arc<Semaphore>,
    max_connections: u32,
    shutdown: watch::Sender<bool>,
    next_conn: std::sync::atomic::AtomicU64,
}

/// The sync hub; cheap to clone.
#[derive(Clone)]
pub struct Hub {
    inner: Arc<Inner>,
}

impl Hub {
    /// A hub with the journal at `journal` that writes progress into
    /// `library`. Blocking (opens SQLite).
    pub fn open(
        journal: &std::path::Path,
        library: Library,
        config: &SyncConfig,
    ) -> Result<Self, Error> {
        let journal = SqliteJournal::open(journal).map_err(|source| Error::Sync {
            path: journal.to_owned(),
            source,
        })?;
        let (events, _) = broadcast::channel(FANOUT_CAPACITY);
        let max_connections = config.max_connections.max(1);
        Ok(Self {
            inner: Arc::new(Inner {
                hub_id: journal.hub_id().to_owned(),
                journal: Mutex::new(journal),
                library,
                playback: Mutex::new(PlaybackCache::new(
                    Duration::from_secs(config.playback_ttl_secs),
                    config.playback_max_entries,
                )),
                events,
                slots: Arc::new(Semaphore::new(max_connections as usize)),
                max_connections,
                shutdown: watch::channel(false).0,
                next_conn: std::sync::atomic::AtomicU64::new(1),
            }),
        })
    }

    /// `device_id` of the hub.
    pub fn hub_id(&self) -> &str {
        &self.inner.hub_id
    }

    /// A connection slot, or `None` when [`SyncConfig::max_connections`]
    /// devices are connected or the hub is shutting down.
    pub(crate) fn try_slot(&self) -> Option<OwnedSemaphorePermit> {
        if *self.inner.shutdown.borrow() {
            return None;
        }
        Arc::clone(&self.inner.slots).try_acquire_owned().ok()
    }

    /// Tells every connection to close.
    pub(crate) fn close(&self) {
        self.inner.shutdown.send_replace(true);
    }

    /// Waits (up to `timeout`) for the connections to close, then
    /// checkpoints the journal.
    pub(crate) async fn drain(&self, timeout: Duration) {
        let all = Arc::clone(&self.inner.slots).acquire_many_owned(self.inner.max_connections);
        if tokio::time::timeout(timeout, all).await.is_err() {
            tracing::warn!("sync connections still open at shutdown");
        }
        let hub = self.clone();
        let done = tokio::task::spawn_blocking(move || hub.journal().checkpoint()).await;
        if let Ok(Err(e)) = done {
            tracing::warn!(error = %e, "sync journal checkpoint failed");
        }
    }

    fn journal(&self) -> std::sync::MutexGuard<'_, SqliteJournal> {
        self.inner.journal.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn blocking<T, F>(&self, f: F) -> Result<T, JournalError>
    where
        T: Send + 'static,
        F: FnOnce(&mut SqliteJournal) -> Result<T, JournalError> + Send + 'static,
    {
        let hub = self.clone();
        tokio::task::spawn_blocking(move || f(&mut hub.journal()))
            .await
            .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))
    }

    fn head(&self) -> proto::Cursor {
        self.journal().head()
    }

    fn hello(&self) -> Message {
        Message::Hello(Hello {
            proto: ProtoRange::LOCAL,
            device_id: self.inner.hub_id.clone(),
            role: Role::Hub,
            cursor: None,
            head: Some(self.head()),
        })
    }

    fn publish(&self, from: u64, frame: Utf8Bytes) {
        // No receivers is fine: nobody else is connected.
        let _ = self.inner.events.send(Event { from, frame });
    }

    /// Serves one device until it disconnects or the hub shuts down.
    pub(crate) async fn serve(self, socket: WebSocket, slot: OwnedSemaphorePermit) {
        let id = self
            .inner
            .next_conn
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut conn = Connection {
            hub: self,
            socket,
            id,
            device: None,
            last_seen: Instant::now(),
        };
        match conn.run().await {
            Ok(()) => tracing::debug!(conn = id, device = conn.device(), "sync: closed"),
            Err(e) => tracing::info!(conn = id, device = conn.device(), reason = %e, "sync: dropped"),
        }
        drop(slot);
    }
}

/// Why a connection ended early.
#[derive(Debug, thiserror::Error)]
enum ConnError {
    #[error("socket: {0}")]
    Socket(#[from] axum::Error),
    #[error("{0}")]
    Protocol(&'static str),
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn encode(msg: &Message) -> Utf8Bytes {
    // Messages built by the hub are always valid.
    proto::encode(msg)
        .expect("hub messages encode")
        .into()
}

fn error(code: &str, message: Option<&str>, op_id: Option<pa_sync::OpId>) -> Message {
    Message::Error(ErrorFrame {
        code: code.to_owned(),
        message: message.map(str::to_owned),
        op_id,
    })
}

struct Connection {
    hub: Hub,
    socket: WebSocket,
    id: u64,
    device: Option<String>,
    last_seen: Instant,
}

impl Connection {
    fn device(&self) -> &str {
        self.device.as_deref().unwrap_or("-")
    }

    async fn send(&mut self, msg: &Message) -> Result<(), ConnError> {
        self.send_raw(encode(msg)).await
    }

    async fn send_raw(&mut self, frame: Utf8Bytes) -> Result<(), ConnError> {
        Ok(self.socket.send(WsMessage::Text(frame)).await?)
    }

    /// Sends an error and closes the connection.
    async fn fail(&mut self, code: &'static str, message: &str) -> Result<(), ConnError> {
        self.send(&error(code, Some(message), None)).await?;
        let _ = self.socket.send(WsMessage::Close(None)).await;
        Err(ConnError::Protocol(code))
    }

    /// Next text frame; `None` when the peer closed. Pings, pongs and
    /// binary frames are handled here.
    async fn recv(&mut self) -> Result<Option<Utf8Bytes>, ConnError> {
        loop {
            let Some(msg) = self.socket.recv().await else {
                return Ok(None);
            };
            self.last_seen = Instant::now();
            match msg? {
                WsMessage::Text(text) => return Ok(Some(text)),
                WsMessage::Binary(_) => {
                    self.send(&error(code::BAD_MESSAGE, Some("text frames only"), None))
                        .await?;
                }
                WsMessage::Close(_) => return Ok(None),
                WsMessage::Ping(_) | WsMessage::Pong(_) => {}
            }
        }
    }

    async fn run(&mut self) -> Result<(), ConnError> {
        // Subscribe before the handshake so nothing published meanwhile is
        // lost: it is delivered right after the handshake.
        let mut events = self.hub.inner.events.subscribe();
        let mut shutdown = self.hub.inner.shutdown.subscribe();
        let hello = self.hub.hello();
        self.send(&hello).await?;

        // Handshake: both sides send `hello` first (spec §4.1).
        let remote = match tokio::time::timeout(HANDSHAKE_TIMEOUT, self.handshake()).await {
            Ok(Ok(Some(hello))) => hello,
            Ok(Ok(None)) => return Ok(()),
            Ok(Err(e)) => return Err(e),
            Err(_) => return self.fail(code::HANDSHAKE, "no hello").await,
        };
        let version = match proto::negotiate(ProtoRange::LOCAL, remote.proto) {
            Ok(v) => v,
            Err(e) => return self.fail(code::PROTO_UNSUPPORTED, &e.to_string()).await,
        };
        self.device = Some(remote.device_id.clone());
        tracing::info!(conn = self.id, device = %remote.device_id, version, "sync: connected");

        // Instant resume: what is playing right now on other devices.
        let recent = {
            let mut cache = self.hub.inner.playback.lock().unwrap_or_else(|e| e.into_inner());
            cache.fresh(Instant::now())
        };
        for frame in recent {
            self.send_raw(frame).await?;
        }
        // Tell a device that is behind to pull (it may pull anyway).
        let head = self.hub.head();
        let behind = match &remote.cursor {
            Some(c) => c.log != head.log || c.seq < head.seq,
            None => head.seq > 0,
        };
        if behind {
            self.send(&Message::Change(Change { head })).await?;
        }

        let mut ping = tokio::time::interval(PING_INTERVAL);
        ping.tick().await;
        loop {
            tokio::select! {
                incoming = self.recv() => {
                    let Some(text) = incoming? else { return Ok(()) };
                    self.handle(text).await?;
                }
                event = events.recv() => match event {
                    Ok(event) if event.from == self.id => {}
                    Ok(event) => self.send_raw(event.frame).await?,
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::debug!(conn = self.id, missed, "sync: fan-out lagged");
                        let head = self.hub.head();
                        self.send(&Message::Change(Change { head })).await?;
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                },
                _ = ping.tick() => {
                    if self.last_seen.elapsed() > IDLE_TIMEOUT {
                        return Err(ConnError::Protocol("idle"));
                    }
                    self.socket.send(WsMessage::Ping(Default::default())).await?;
                }
                _ = shutdown.wait_for(|stop| *stop) => {
                    let _ = self.socket.send(WsMessage::Close(None)).await;
                    return Ok(());
                }
            }
        }
    }

    /// Waits for the device's `hello`. `None`: the device closed.
    async fn handshake(&mut self) -> Result<Option<Hello>, ConnError> {
        loop {
            let Some(text) = self.recv().await? else {
                return Ok(None);
            };
            match proto::decode(text.as_str()) {
                Ok(Message::Hello(hello)) => return Ok(Some(hello)),
                Ok(_) => {
                    self.fail(code::HANDSHAKE, "hello expected").await?;
                }
                Err(e) => {
                    self.send(&error(code::BAD_MESSAGE, Some(&e.to_string()), None))
                        .await?;
                }
            }
        }
    }

    async fn handle(&mut self, text: Utf8Bytes) -> Result<(), ConnError> {
        let msg = match proto::decode(text.as_str()) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::debug!(conn = self.id, error = %e, "sync: bad frame");
                return self
                    .send(&error(code::BAD_MESSAGE, Some(&e.to_string()), None))
                    .await;
            }
        };
        match msg {
            Message::PbUpdate(update) => {
                {
                    let mut cache =
                        self.hub.inner.playback.lock().unwrap_or_else(|e| e.into_inner());
                    cache.put(&update, text.clone(), Instant::now());
                }
                // The original frame: unknown fields of newer clients survive.
                self.hub.publish(self.id, text);
            }
            Message::PbTakeover(_) => self.hub.publish(self.id, text),
            Message::Op(frame) => self.store(frame).await?,
            Message::Pull(pull) => {
                let reply = self.hub.blocking(move |j| j.pull(&pull)).await;
                match reply {
                    Ok((frames, end)) => {
                        for frame in frames {
                            self.send(&Message::Op(frame)).await?;
                        }
                        self.send(&Message::PullEnd(end)).await?;
                    }
                    Err(e) => tracing::error!(conn = self.id, error = %e, "sync: pull failed"),
                }
            }
            // A second hello, acks of fanned-out ops, frames meant for
            // devices and types of newer versions: nothing to do.
            Message::Hello(_)
            | Message::Ack(_)
            | Message::Change(_)
            | Message::PullEnd(_)
            | Message::Unknown => {}
            Message::Error(e) => {
                tracing::debug!(conn = self.id, code = %e.code, "sync: device reported an error");
            }
        }
        Ok(())
    }

    /// Journals an op, acks it and fans it out if it is new.
    async fn store(&mut self, frame: OpFrame) -> Result<(), ConnError> {
        let now = now_ms();
        let stored = {
            let frame = frame.clone();
            let library = self.hub.inner.library.clone();
            self.hub
                .blocking(move |j| {
                    let (seq, new) = j.append(&frame, now)?;
                    if new {
                        apply_to_library(&library, &frame);
                    }
                    Ok((seq, new))
                })
                .await
        };
        let (seq, new) = match stored {
            Ok(stored) => stored,
            Err(e) => {
                // No ack: the device keeps the op in its outbox and retries.
                tracing::error!(conn = self.id, error = %e, "sync: journal write failed");
                return Ok(());
            }
        };
        self.send(&Message::Ack(Ack {
            op_id: frame.op_id,
            seq: Some(seq),
        }))
        .await?;
        if new {
            let frame = OpFrame {
                seq: Some(seq),
                ..frame
            };
            self.hub.publish(self.id, encode(&Message::Op(frame)));
        }
        Ok(())
    }
}

/// The server is authoritative for its own library: progress ops of known
/// profiles go into its `progress` table (LWW by op time). Ops it cannot
/// read (encrypted, newer schema) are only journaled. Runs on a blocking
/// thread.
fn apply_to_library(library: &Library, frame: &OpFrame) {
    if frame.payload.enc != proto::ENC_PLAIN {
        return;
    }
    let Ok(op) = frame.open() else {
        return;
    };
    let Some(change) = pa_sync::library::progress_change(&op.body) else {
        return;
    };
    let Ok(at_ms) = i64::try_from(op.hlc.wall_ms) else {
        return;
    };
    let conn = library.conn.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = pa_library::progress::apply_remote(&conn, &change, at_ms, &op.hlc.device) {
        tracing::warn!(error = %e, "sync: progress not written to the library");
    }
}
