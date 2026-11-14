// SPDX-License-Identifier: MPL-2.0
//! One app's sync service (UI-019): the device identity and family, pairing
//! by QR code or 6-digit code ([`pair`](super::pair)), and the
//! [`LanNode`] that moves the library's ops while paired devices exist.
//!
//! The service owns a small tokio runtime, so its API is blocking and can be
//! called from any thread that is not inside a runtime (FFI worker threads,
//! tests). The library stays outside: the app hands in a [`SyncLibrary`]
//! (the catalog database), and the service reports [`ServiceEvent`]s.
//!
//! Files: `roster.json` in [`ServiceConfig::dir`] (not secret); the device
//! keys and the family key are in the [`SecretStore`].

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use pa_discovery::{
    Advertiser, Announcement, DeviceClass, Discovery, DiscoveryEvent, MdnsBackend, MdnsConfig,
    Registration, ServiceKind, host_for_id,
};
use pa_pairing::pair::{Invitation, PairingCode, QR_PREFIX, QrInvite};
use pa_model::MediaKey;
use pa_pairing::{DeviceIdentity, DeviceKind, Family};
use pa_secrets::SecretStore;
use tokio::runtime::Runtime;
use tokio::sync::Notify;

use super::pair::{self, Admit, HostEvent, JoinAs, JoinError, PairHost};
use super::{AbortOnDrop, LanConfig, LanEvent, LanNode};
use crate::handoff;
use crate::hlc::HlcClock;
use crate::proto::{PlayState, PlaybackUpdate, Takeover};
use crate::{Op, OpId};

/// The library the service syncs: the app's catalog database.
pub trait SyncLibrary: Send + Sync + 'static {
    /// Starts recording local changes as ops of `device_id`.
    fn enable(&self, device_id: &str) -> std::result::Result<(), String>;
    /// Stops recording (the device left its family); drops queued ops.
    fn disable(&self) -> std::result::Result<(), String>;
    /// Queued local ops.
    fn pending(&self) -> std::result::Result<Vec<Op>, String>;
    /// Another device holds `op_id` now.
    fn ack(&self, op_id: OpId) -> std::result::Result<(), String>;
    /// Writes an op of another device; `true` if the library changed.
    fn apply(&self, op: &Op) -> std::result::Result<bool, String>;
}

/// Settings of a [`SyncService`].
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Folder for `roster.json`.
    pub dir: PathBuf,
    /// Name of this device shown to the others (`Living room TV`).
    pub name: String,
    /// Device class: orders the hub election.
    pub class: DeviceClass,
    /// Address to listen on: every interface in the app, loopback in tests.
    pub bind_ip: IpAddr,
    /// Address to put into the QR code; `None` finds the address of the
    /// default route.
    pub advertise_ip: Option<IpAddr>,
    /// mDNS settings; `None` turns mDNS off (devices are then found only
    /// through [`SyncService::peer_up`] and the QR address).
    pub mdns: Option<MdnsConfig>,
    /// Timings of the LAN node.
    pub lan: LanConfig,
    /// How often queued ops are checked when nothing calls
    /// [`SyncService::flush`].
    pub flush_interval: Duration,
    /// How long a code-only join looks for inviters on mDNS.
    pub find_timeout: Duration,
}

impl ServiceConfig {
    /// App defaults for a device of `class` named `name` keeping its files
    /// in `dir`.
    pub fn new(dir: impl Into<PathBuf>, name: impl Into<String>, class: DeviceClass) -> Self {
        Self {
            dir: dir.into(),
            name: name.into(),
            class,
            bind_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            advertise_ip: None,
            mdns: Some(MdnsConfig::default()),
            lan: LanConfig::new(class),
            flush_interval: Duration::from_secs(2),
            find_timeout: Duration::from_secs(4),
        }
    }

    fn roster_path(&self) -> PathBuf {
        self.dir.join("roster.json")
    }
}

/// A device of the family as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceStatus {
    pub device_id: String,
    pub name: String,
    pub kind: DeviceKind,
    /// Fingerprint of its signing key (to compare on both screens).
    pub fingerprint: String,
    pub added_ms: u64,
    /// This device.
    pub this_device: bool,
    /// Linked or visible on the LAN now (always true for this device).
    pub online: bool,
}

/// An open invitation: show [`Invite::code`] and the QR code of
/// [`Invite::qr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    /// 6 digits.
    pub code: String,
    /// `projectalpha://pair?…` with the address and the inviter's key.
    pub qr: String,
    /// Unix ms when the invitation closes.
    pub expires_ms: u64,
    /// Where the inviter listens.
    pub addr: SocketAddr,
}

/// What the service tells the app.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceEvent {
    /// Ops of another device changed the library: refresh the views.
    LibraryChanged,
    /// Devices, their online state or the hub changed.
    DevicesChanged,
    /// A device joined through this device's invitation.
    Paired { device_id: String, name: String },
    /// An attempt on this device's invitation failed (it stays open while
    /// attempts are left).
    PairingFailed { reason: String },
    /// This device's invitation closed (expired, used up, or used).
    InviteClosed,
    /// What another device plays now (`pb.update`, SYNC-003).
    Playback(PlaybackUpdate),
    /// Another device started a title (`pb.takeover`): pause it here if
    /// it plays in the same profile and the user allows it.
    Takeover(Takeover),
}

/// Why a call failed. `code` is stable for the UI.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ServiceError {
    /// `invalid_code`, `wrong_code`, `expired`, `not_found`,
    /// `already_paired`, `refused`, `unknown_device`, `io`, `pairing`.
    pub code: &'static str,
    pub message: String,
}

impl ServiceError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<pa_pairing::Error> for ServiceError {
    fn from(e: pa_pairing::Error) -> Self {
        let code = match &e {
            pa_pairing::Error::WrongCode => "wrong_code",
            pa_pairing::Error::Expired => "expired",
            pa_pairing::Error::Refused(_) | pa_pairing::Error::KeyMismatch => "refused",
            pa_pairing::Error::Unknown(_) => "unknown_device",
            _ => "pairing",
        };
        Self::new(code, e.to_string())
    }
}

impl From<std::io::Error> for ServiceError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}

type Result<T> = std::result::Result<T, ServiceError>;

fn now_ms() -> u64 {
    crate::hlc::system_ms()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Device kind in the roster for a device class.
pub fn kind_of(class: DeviceClass) -> DeviceKind {
    match class {
        DeviceClass::Server => DeviceKind::Server,
        DeviceClass::Desktop => DeviceKind::Desktop,
        DeviceClass::Tv => DeviceKind::Tv,
        DeviceClass::Mobile | DeviceClass::Other => DeviceKind::Phone,
    }
}

/// The running LAN node and what lives with it.
struct Running {
    node: Arc<LanNode>,
    _tasks: Vec<AbortOnDrop>,
    _announcement: Option<Registration>,
}

struct HostState {
    host: PairHost,
    _announcement: Option<Registration>,
    _task: AbortOnDrop,
}

#[derive(Default)]
struct State {
    family: Option<Family>,
    running: Option<Running>,
    host: Option<HostState>,
    /// Port of the last node, reused on restart so peers keep the address.
    port: u16,
    /// Sync peers visible on mDNS.
    visible: HashSet<String>,
    /// Ops handed to the node in this run.
    published: HashSet<OpId>,
}

struct Mdns {
    discovery: Discovery,
    advertiser: Advertiser,
}

struct Inner {
    config: ServiceConfig,
    store: Arc<dyn SecretStore>,
    library: Arc<dyn SyncLibrary>,
    identity: DeviceIdentity,
    state: Mutex<State>,
    mdns: Mutex<Option<Arc<Mdns>>>,
    subscribers: Mutex<Vec<std_mpsc::Sender<ServiceEvent>>>,
    flush: Notify,
    /// Stamps of this device's `pb.*` messages; observes the others'.
    clock: Mutex<HlcClock>,
    /// Full pull answers applied by the event loop, and its signal.
    pulled: AtomicU64,
    pulled_notify: Notify,
}

/// The sync service of one app. Dropping it stops everything.
pub struct SyncService {
    inner: Arc<Inner>,
    runtime: Option<Runtime>,
}

impl Drop for SyncService {
    fn drop(&mut self) {
        {
            let mut state = lock(&self.inner.state);
            state.running = None;
            state.host = None;
        }
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

impl SyncService {
    /// Loads (or creates) this device's keys from `store` and its family
    /// from `config.dir`. Nothing runs until [`start`](Self::start),
    /// [`invite`](Self::invite) or [`join`](Self::join).
    pub fn open(
        config: ServiceConfig,
        store: Arc<dyn SecretStore>,
        library: Arc<dyn SyncLibrary>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&config.dir)?;
        let identity = match DeviceIdentity::load(store.as_ref())? {
            Some(identity) => identity,
            None => {
                let id = format!("k{}", uuid::Uuid::now_v7().simple());
                let identity = DeviceIdentity::generate(id)?;
                identity.save(store.as_ref())?;
                identity
            }
        };
        // Another family's key or a broken roster: start without one.
        let family = Family::load(store.as_ref(), &config.roster_path()).unwrap_or_default();
        let clock = HlcClock::new(identity.device_id())
            .map_err(|e| ServiceError::new("pairing", e.to_string()))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("pa-sync")
            .enable_all()
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                store,
                library,
                identity,
                state: Mutex::new(State {
                    family,
                    ..State::default()
                }),
                mdns: Mutex::new(None),
                subscribers: Mutex::new(Vec::new()),
                flush: Notify::new(),
                clock: Mutex::new(clock),
                pulled: AtomicU64::new(0),
                pulled_notify: Notify::new(),
            }),
            runtime: Some(runtime),
        })
    }

    fn rt(&self) -> &Runtime {
        self.runtime.as_ref().expect("runtime lives until drop")
    }

    /// This device's id (also the HLC device of its ops).
    pub fn device_id(&self) -> &str {
        self.inner.identity.device_id()
    }

    /// Fingerprint of this device's signing key.
    pub fn fingerprint(&self) -> String {
        self.inner.identity.public().fingerprint()
    }

    /// Events from now on. Every call is a new subscription; dropping the
    /// receiver ends it.
    pub fn subscribe(&self) -> std_mpsc::Receiver<ServiceEvent> {
        let (tx, rx) = std_mpsc::channel();
        lock(&self.inner.subscribers).push(tx);
        rx
    }

    /// Whether this device has other active devices in its family.
    pub fn is_paired(&self) -> bool {
        lock(&self.inner.state)
            .family
            .as_ref()
            .is_some_and(|f| f.roster().active().count() > 1)
    }

    /// Whether the LAN node runs.
    pub fn is_running(&self) -> bool {
        lock(&self.inner.state).running.is_some()
    }

    /// The device this one is connected to as its hub; `None` when this
    /// device is the hub, alone, or not running.
    pub fn hub(&self) -> Option<String> {
        lock(&self.inner.state)
            .running
            .as_ref()
            .and_then(|r| r.node.hub())
    }

    /// Port of the LAN node, if it runs.
    pub fn sync_addr(&self) -> Option<SocketAddr> {
        lock(&self.inner.state)
            .running
            .as_ref()
            .map(|r| r.node.local_addr())
    }

    /// The devices of the family (this one first), without revoked ones.
    /// Empty while this device has no family.
    pub fn devices(&self) -> Vec<DeviceStatus> {
        self.inner.devices()
    }

    /// Starts the LAN node if this device is paired. Returns whether it
    /// runs. Safe to call again.
    pub fn start(&self) -> Result<bool> {
        self.rt().block_on(Inner::start(&self.inner))
    }

    /// Stops the LAN node (the app goes to the background or quits).
    pub fn stop(&self) {
        lock(&self.inner.state).running = None;
    }

    /// Tells the service that the library queued new ops: send them now.
    pub fn flush(&self) {
        self.inner.flush.notify_one();
    }

    /// A sync peer at a known address (tests, or an address remembered
    /// from pairing when mDNS is not available).
    pub fn peer_up(&self, device_id: &str, class: DeviceClass, addr: SocketAddr) {
        if let Some(r) = &lock(&self.inner.state).running {
            r.node.peer_up(device_id, class, addr);
        }
    }

    /// Opens an invitation: founds a family first if this device has none.
    /// Replaces an open invitation.
    pub fn invite(&self) -> Result<Invite> {
        self.rt().block_on(Inner::invite(&self.inner))
    }

    /// Closes the open invitation.
    pub fn cancel_invite(&self) {
        lock(&self.inner.state).host = None;
    }

    /// The open invitation's address, if any.
    pub fn invite_addr(&self) -> Option<SocketAddr> {
        lock(&self.inner.state)
            .host
            .as_ref()
            .filter(|h| h.host.is_open())
            .map(|h| h.host.local_addr())
    }

    /// Joins the family of another device: `input` is a scanned QR code
    /// (`projectalpha://pair?…`) or a typed 6-digit code (the inviter is
    /// then looked up on mDNS). Returns the inviter. Fails with
    /// `already_paired` while this device has other devices.
    pub fn join(&self, input: &str) -> Result<DeviceStatus> {
        self.rt().block_on(Inner::join(&self.inner, input))
    }

    /// Removes a device from the family. For another device: revokes it
    /// (the family key rotates; the others learn it on their next link).
    /// For this device: leaves the family (forgets it and stops syncing).
    pub fn unpair(&self, device_id: &str) -> Result<()> {
        self.rt().block_on(Inner::unpair(&self.inner, device_id))
    }

    /// Heartbeat of the playback session (SYNC-003): what this device plays
    /// now, stamped with its clock and the TTL of `state`. Call it every
    /// [`handoff::HEARTBEAT_INTERVAL_MS`] while playing and at once on
    /// pause, seek, stop and going to the background. Ephemeral: nothing is
    /// stored. A no-op while sync does not run.
    pub fn heartbeat(
        &self,
        profile: &str,
        key: &MediaKey,
        position_ms: u64,
        duration_ms: Option<u64>,
        state: PlayState,
    ) -> Result<()> {
        if !self.is_running() {
            return Ok(());
        }
        let hlc = lock(&self.inner.clock).now();
        self.playback(PlaybackUpdate {
            profile: profile.to_owned(),
            key: key.clone(),
            position_ms,
            duration_ms: duration_ms.filter(|d| *d > 0),
            rate: 1.0,
            state,
            hlc,
            ttl_ms: Some(handoff::ttl_for(state)),
        })
    }

    /// This device starts playing `key` (SYNC-003): other devices of the
    /// profile get `pb.takeover` and pause it if their setting allows.
    pub fn start_playing(&self, profile: &str, key: &MediaKey) -> Result<()> {
        if !self.is_running() {
            return Ok(());
        }
        let hlc = lock(&self.inner.clock).now();
        self.takeover(Takeover {
            profile: profile.to_owned(),
            key: key.clone(),
            hlc,
        })
    }

    /// Pulls what the hub has and this device lacks, waiting at most
    /// `timeout` ([`handoff::PULL_TIMEOUT_MS`] before playing). Ops that
    /// arrive are applied to the library before this returns. `true` when
    /// the hub answered in time or there is none; `false` on timeout or
    /// while sync does not run.
    pub fn refresh(&self, timeout: Duration) -> bool {
        let node = lock(&self.inner.state)
            .running
            .as_ref()
            .map(|r| Arc::clone(&r.node));
        let Some(node) = node else { return false };
        let started = std::time::Instant::now();
        let asked = node.hub().is_some();
        let before = self.inner.pulled.load(Ordering::Acquire);
        let answered = self.rt().block_on(node.refresh(timeout));
        if answered && asked {
            // The answer's ops reach the library through the event loop:
            // wait until it has passed the end of the answer (within what
            // is left of `timeout`, plus a little for the last ops).
            let left = timeout
                .saturating_sub(started.elapsed())
                .max(Duration::from_millis(50));
            self.rt().block_on(self.inner.applied_after(before, left));
        }
        answered
    }

    /// The last `pb.update` of another device for `key` in `profile`, if
    /// one is known (it may be old: see [`handoff::is_current`]).
    pub fn remote_playback(&self, profile: &str, key: &MediaKey) -> Option<PlaybackUpdate> {
        self.playback_updates()
            .into_iter()
            .filter(|u| u.profile == profile && u.key == *key)
            .max_by(|a, b| a.hlc.cmp(&b.hlc))
    }

    /// What other devices play right now ([`handoff::is_playing_now`]),
    /// newest first: the "Continue on this device" banner.
    pub fn playing_elsewhere(&self) -> Vec<PlaybackUpdate> {
        let now = now_ms();
        let mut out: Vec<PlaybackUpdate> = self
            .playback_updates()
            .into_iter()
            .filter(|u| handoff::is_playing_now(u, now))
            .collect();
        out.sort_by(|a, b| b.hlc.cmp(&a.hlc));
        out
    }

    fn playback_updates(&self) -> Vec<PlaybackUpdate> {
        lock(&self.inner.state)
            .running
            .as_ref()
            .map(|r| r.node.playback_updates())
            .unwrap_or_default()
    }

    /// Sends a `pb.update` as is (stamped by the caller).
    pub fn playback(&self, update: PlaybackUpdate) -> Result<()> {
        if let Some(r) = &lock(&self.inner.state).running {
            r.node
                .playback(update)
                .map_err(|e| ServiceError::new("io", e.to_string()))?;
        }
        Ok(())
    }

    /// Sends a `pb.takeover` as is (stamped by the caller).
    pub fn takeover(&self, takeover: Takeover) -> Result<()> {
        if let Some(r) = &lock(&self.inner.state).running {
            r.node
                .takeover(takeover)
                .map_err(|e| ServiceError::new("io", e.to_string()))?;
        }
        Ok(())
    }
}

/// The address of the default route (no packet is sent), for the QR code.
fn default_route_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

/// A DNS-SD instance name: the device name and a short id, ≤ 63 bytes.
fn instance_name(name: &str, device_id: &str) -> String {
    let short: String = device_id
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let mut base: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '.')
        .collect();
    while base.len() > 50 {
        base.pop();
    }
    let base = base.trim();
    if base.is_empty() {
        format!("Kinolune {short}")
    } else {
        format!("{base} {short}")
    }
}

impl Inner {
    /// Orders this device's next `pb.*` after a stamp of another device.
    fn observe(&self, stamp: &crate::Hlc) {
        // A peer clock far ahead is ignored, not fatal.
        let _ = lock(&self.clock).observe(stamp, now_ms());
    }

    /// Waits (at most `limit`) until the event loop has applied a full
    /// pull answer counted after `before`.
    async fn applied_after(&self, before: u64, limit: Duration) {
        let wait = async {
            loop {
                let notified = self.pulled_notify.notified();
                if self.pulled.load(Ordering::Acquire) > before {
                    return;
                }
                notified.await;
            }
        };
        let _ = tokio::time::timeout(limit, wait).await;
    }

    fn emit(&self, event: &ServiceEvent) {
        lock(&self.subscribers).retain(|tx| tx.send(event.clone()).is_ok());
    }

    fn identity_copy(&self) -> Result<DeviceIdentity> {
        DeviceIdentity::load(self.store.as_ref())?
            .ok_or_else(|| ServiceError::new("pairing", "device keys are gone from the store"))
    }

    fn load_family(&self) -> Result<Option<Family>> {
        Ok(Family::load(
            self.store.as_ref(),
            &self.config.roster_path(),
        )?)
    }

    fn save_family(&self, family: &Family) -> Result<()> {
        Ok(family.save(self.store.as_ref(), &self.config.roster_path())?)
    }

    fn mdns(&self) -> Option<Arc<Mdns>> {
        let config = self.config.mdns.clone()?;
        let mut slot = lock(&self.mdns);
        if slot.is_none() {
            match MdnsBackend::with_config(config) {
                Ok(backend) => {
                    let advertiser = backend.advertiser();
                    let discovery = Discovery::new(backend, [self.identity.device_id()]);
                    *slot = Some(Arc::new(Mdns {
                        discovery,
                        advertiser,
                    }));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "sync: mDNS unavailable");
                    return None;
                }
            }
        }
        slot.clone()
    }

    fn devices(&self) -> Vec<DeviceStatus> {
        let state = lock(&self.state);
        let Some(family) = &state.family else {
            return Vec::new();
        };
        let me = self.identity.device_id();
        let (links, hub) = state
            .running
            .as_ref()
            .map(|r| (r.node.links(), r.node.hub()))
            .unwrap_or_default();
        let mut out: Vec<DeviceStatus> = family
            .roster()
            .devices()
            .into_iter()
            .filter(|d| d.revoked_ms.is_none())
            .map(|d| {
                let this_device = d.device_id == me;
                let online = this_device
                    || links.contains(&d.device_id)
                    || hub.as_deref() == Some(d.device_id.as_str())
                    || state.visible.contains(&d.device_id);
                DeviceStatus {
                    device_id: d.device_id,
                    name: d.name,
                    kind: d.kind,
                    fingerprint: d.fingerprint,
                    added_ms: d.added_ms,
                    this_device,
                    online,
                }
            })
            .collect();
        out.sort_by_key(|d| !d.this_device);
        out
    }

    fn paired(state: &State) -> bool {
        state
            .family
            .as_ref()
            .is_some_and(|f| f.roster().active().count() > 1)
    }

    async fn start(self: &Arc<Self>) -> Result<bool> {
        if lock(&self.state).running.is_some() {
            return Ok(true);
        }
        if !Self::paired(&lock(&self.state)) {
            return Ok(false);
        }
        let Some(family) = self.load_family()? else {
            return Ok(false);
        };
        self.library
            .enable(self.identity.device_id())
            .map_err(|e| ServiceError::new("io", e))?;
        let port = lock(&self.state).port;
        let mut lan = LanConfig {
            name: Some(self.config.name.clone()),
            bind: SocketAddr::new(self.config.bind_ip, port),
            ..self.config.lan.clone()
        };
        lan.class = self.config.class;
        let (node, events) = match LanNode::start(lan.clone(), family).await {
            Ok(started) => started,
            Err(_) if port != 0 => {
                let family = self.load_family()?.ok_or_else(|| {
                    ServiceError::new("pairing", "the family disappeared while starting")
                })?;
                let lan = LanConfig {
                    bind: SocketAddr::new(self.config.bind_ip, 0),
                    ..lan
                };
                LanNode::start(lan, family).await?
            }
            Err(e) => return Err(e.into()),
        };
        let node = Arc::new(node);
        let weak = Arc::downgrade(self);
        let tasks = vec![
            AbortOnDrop(tokio::spawn(event_loop(weak.clone(), events))),
            AbortOnDrop(tokio::spawn(flush_loop(
                weak.clone(),
                Arc::downgrade(&node),
            ))),
        ];
        let mut announcement = None;
        if let Some(mdns) = self.mdns() {
            let instance = instance_name(&self.config.name, self.identity.device_id());
            match mdns.advertiser.announce(&node.announcement(instance)) {
                Ok(reg) => announcement = Some(reg),
                Err(e) => tracing::warn!(error = %e, "sync: not announced"),
            }
            if let Err(e) = mdns.discovery.browse([ServiceKind::SyncPeer]) {
                tracing::warn!(error = %e, "sync: not browsing");
            }
            // The node's own election feed, and the "online" picture.
            let _feed = node.watch_discovery(mdns.discovery.subscribe());
            let visible = mdns.discovery.subscribe();
            let (weak, node_weak) = (weak.clone(), Arc::downgrade(&node));
            std::thread::spawn(move || watch_visible(&weak, &node_weak, &visible));
        }
        {
            let mut state = lock(&self.state);
            state.port = node.local_addr().port();
            state.published.clear();
            state.running = Some(Running {
                node,
                _tasks: tasks,
                _announcement: announcement,
            });
        }
        self.flush.notify_one();
        self.emit(&ServiceEvent::DevicesChanged);
        Ok(true)
    }

    /// Restarts the node with the saved family (after pairing or a
    /// revocation), or stops it when nobody is left.
    async fn restart(self: &Arc<Self>) -> Result<()> {
        lock(&self.state).running = None;
        self.start().await?;
        Ok(())
    }

    /// Hands queued ops to the node.
    fn publish_pending(&self, node: &LanNode) {
        let ops = match self.library.pending() {
            Ok(ops) => ops,
            Err(e) => {
                tracing::warn!(error = %e, "sync: outbox not readable");
                return;
            }
        };
        let fresh: Vec<Op> = {
            let mut state = lock(&self.state);
            ops.into_iter()
                .filter(|op| state.published.insert(op.op_id))
                .collect()
        };
        for op in fresh {
            if let Err(e) = node.publish(&op) {
                tracing::warn!(error = %e, "sync: op not sent");
            }
        }
    }

    async fn invite(self: &Arc<Self>) -> Result<Invite> {
        lock(&self.state).host = None;
        let founded = {
            let mut state = lock(&self.state);
            if state.family.is_none() {
                let family = Family::found(
                    self.identity_copy()?,
                    &self.config.name,
                    kind_of(self.config.class),
                    now_ms(),
                )?;
                self.save_family(&family)?;
                state.family = Some(family);
                true
            } else {
                false
            }
        };
        if founded {
            self.emit(&ServiceEvent::DevicesChanged);
        }
        let invitation = Invitation::digits(now_ms())?;
        let code = invitation.code().clone();
        let expires_ms = invitation.expires_ms();
        let weak = Arc::downgrade(self);
        let admit: Admit = Arc::new(move |inviter, request| {
            let inner = weak.upgrade().ok_or(pa_pairing::Error::Expired)?;
            let mut state = lock(&inner.state);
            let family = state.family.as_mut().ok_or(pa_pairing::Error::Expired)?;
            let (_, welcome) = inviter.welcome(family, request, now_ms())?;
            inner
                .save_family(family)
                .map_err(|e| pa_pairing::Error::Io(std::io::Error::other(e.message)))?;
            Ok(welcome)
        });
        let (host, events) =
            PairHost::start(SocketAddr::new(self.config.bind_ip, 0), invitation, admit).await?;
        let port = host.local_addr().port();
        let ip = self
            .config
            .advertise_ip
            .or_else(|| (!self.config.bind_ip.is_unspecified()).then_some(self.config.bind_ip))
            .or_else(default_route_ip);
        let addr = SocketAddr::new(ip.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)), port);
        let public = self.identity.public();
        let qr = QrInvite {
            code: code.clone(),
            inviter: public.sign,
            addr: ip.map(|_| addr.to_string()),
            fp: None,
        }
        .to_uri();
        let announcement = self.mdns().and_then(|mdns| {
            let a = Announcement {
                kind: ServiceKind::Pairing,
                instance: instance_name(&self.config.name, self.identity.device_id()),
                host: host_for_id(self.identity.device_id()),
                port,
                txt: vec![
                    ("txtvers".into(), "1".into()),
                    ("id".into(), self.identity.device_id().to_owned()),
                    ("k".into(), public.fingerprint()),
                    ("name".into(), self.config.name.clone()),
                ],
            };
            mdns.advertiser
                .announce(&a)
                .inspect_err(|e| tracing::warn!(error = %e, "pairing: not announced"))
                .ok()
        });
        let task = AbortOnDrop(tokio::spawn(host_loop(Arc::downgrade(self), events)));
        lock(&self.state).host = Some(HostState {
            host,
            _announcement: announcement,
            _task: task,
        });
        Ok(Invite {
            code: code.expose().to_owned(),
            qr,
            expires_ms,
            addr,
        })
    }

    async fn join(self: &Arc<Self>, input: &str) -> Result<DeviceStatus> {
        let input = input.trim();
        let (code, inviter_key, addr) = if input.starts_with(QR_PREFIX) {
            let qr = QrInvite::parse(input)
                .map_err(|_| ServiceError::new("invalid_code", "not a pairing QR code"))?;
            let addr = qr
                .addr
                .as_deref()
                .and_then(|a| a.parse::<SocketAddr>().ok());
            (qr.code, Some(qr.inviter), addr)
        } else {
            let code = PairingCode::parse(input)
                .map_err(|_| ServiceError::new("invalid_code", "the code has 6 digits"))?;
            (code, None, None)
        };
        if Self::paired(&lock(&self.state)) {
            return Err(ServiceError::new(
                "already_paired",
                "this device already has paired devices: unpair them first",
            ));
        }
        lock(&self.state).host = None;

        let mut candidates: Vec<SocketAddr> = addr.into_iter().collect();
        if let Some(mdns) = self.mdns() {
            let want = inviter_key.map(|k| pa_pairing::fingerprint(&k));
            for found in self.find_inviters(&mdns, want.as_deref()).await {
                if !candidates.contains(&found) {
                    candidates.push(found);
                }
            }
        }
        if candidates.is_empty() {
            return Err(ServiceError::new(
                "not_found",
                "no device with an open invitation was found",
            ));
        }

        let identity = self.identity_copy()?;
        let me = JoinAs {
            identity: &identity,
            name: &self.config.name,
            kind: kind_of(self.config.class),
        };
        let mut last = ServiceError::new("not_found", "no device answered");
        let mut joined = None;
        for addr in candidates {
            match pair::join(addr, me.clone(), &code, inviter_key).await {
                Ok(j) => {
                    joined = Some(j);
                    break;
                }
                Err(JoinError::Pairing(e)) => last = e.into(),
                Err(e) => {
                    if last.code == "not_found" {
                        last = ServiceError::new("not_found", e.to_string());
                    }
                }
            }
        }
        let joined = joined.ok_or(last)?;
        let inviter_id = joined.inviter.clone();
        let family = joined.into_family(self.identity_copy()?)?;
        {
            let mut state = lock(&self.state);
            if state.family.is_some() {
                // A family of this device alone (it showed a code before).
                Family::forget(self.store.as_ref(), &self.config.roster_path())?;
            }
            self.save_family(&family)?;
            state.family = Some(family);
        }
        self.restart().await?;
        self.emit(&ServiceEvent::DevicesChanged);
        self.devices()
            .into_iter()
            .find(|d| d.device_id == inviter_id)
            .ok_or_else(|| ServiceError::new("pairing", "the inviter is not in the family"))
    }

    /// `_alpha-pair._tcp` services, best match first.
    async fn find_inviters(&self, mdns: &Mdns, fingerprint: Option<&str>) -> Vec<SocketAddr> {
        if let Err(e) = mdns.discovery.browse([ServiceKind::Pairing]) {
            tracing::warn!(error = %e, "pairing: not browsing");
            return Vec::new();
        }
        let deadline = tokio::time::Instant::now() + self.config.find_timeout;
        let mut out = Vec::new();
        loop {
            for s in mdns.discovery.services_of(ServiceKind::Pairing) {
                if fingerprint.is_some_and(|fp| s.txt.get("k") != Some(fp)) {
                    continue;
                }
                if let Some(ip) = s.primary_address() {
                    let addr = SocketAddr::new(ip, s.port);
                    if !out.contains(&addr) {
                        out.push(addr);
                    }
                }
            }
            // A QR code names one inviter: stop at the first match. A typed
            // code waits a little for every inviter to answer.
            let enough = !out.is_empty()
                && (fingerprint.is_some()
                    || tokio::time::Instant::now() + self.config.find_timeout / 2 >= deadline);
            if enough || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        mdns.discovery.stop(ServiceKind::Pairing);
        out
    }

    async fn unpair(self: &Arc<Self>, device_id: &str) -> Result<()> {
        if device_id == self.identity.device_id() {
            {
                let mut state = lock(&self.state);
                state.running = None;
                state.host = None;
                state.family = None;
            }
            Family::forget(self.store.as_ref(), &self.config.roster_path())?;
            self.library
                .disable()
                .map_err(|e| ServiceError::new("io", e))?;
            self.emit(&ServiceEvent::DevicesChanged);
            return Ok(());
        }
        {
            let mut state = lock(&self.state);
            let family = state
                .family
                .as_mut()
                .ok_or_else(|| ServiceError::new("unknown_device", "no family"))?;
            family.revoke(device_id, now_ms())?;
            self.save_family(family)?;
        }
        let alone = !Self::paired(&lock(&self.state));
        if alone {
            lock(&self.state).running = None;
            self.library
                .disable()
                .map_err(|e| ServiceError::new("io", e))?;
        } else {
            self.restart().await?;
        }
        self.emit(&ServiceEvent::DevicesChanged);
        Ok(())
    }
}

async fn event_loop(weak: Weak<Inner>, mut events: tokio::sync::mpsc::UnboundedReceiver<LanEvent>) {
    while let Some(event) = events.recv().await {
        let Some(inner) = weak.upgrade() else { return };
        match event {
            LanEvent::Op(op) => match inner.library.apply(&op) {
                Ok(true) => inner.emit(&ServiceEvent::LibraryChanged),
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, "sync: op not applied"),
            },
            LanEvent::Acked(id) => {
                if let Err(e) = inner.library.ack(id) {
                    tracing::warn!(error = %e, "sync: ack not stored");
                }
            }
            LanEvent::Playback(u) => {
                inner.observe(&u.hlc);
                inner.emit(&ServiceEvent::Playback(u));
            }
            LanEvent::Takeover(t) => {
                inner.observe(&t.hlc);
                inner.emit(&ServiceEvent::Takeover(t));
            }
            LanEvent::Pulled => {
                inner.pulled.fetch_add(1, Ordering::AcqRel);
                inner.pulled_notify.notify_waiters();
            }
            LanEvent::HubChanged(_) => inner.emit(&ServiceEvent::DevicesChanged),
            LanEvent::FamilyChanged => {
                let saved = {
                    let state = lock(&inner.state);
                    state.running.as_ref().map(|r| {
                        r.node.with_family(|f| {
                            f.save(inner.store.as_ref(), &inner.config.roster_path())
                        })
                    })
                };
                if let Some(Err(e)) = saved {
                    tracing::warn!(error = %e, "sync: family not saved");
                }
                if let Ok(family) = inner.load_family() {
                    lock(&inner.state).family = family;
                }
                inner.emit(&ServiceEvent::DevicesChanged);
            }
            LanEvent::Revoked => {
                let _ = Family::forget(inner.store.as_ref(), &inner.config.roster_path());
                let _ = inner.library.disable();
                let running = {
                    let mut state = lock(&inner.state);
                    state.family = None;
                    state.running.take()
                };
                inner.emit(&ServiceEvent::DevicesChanged);
                // Dropping it aborts this very task: last thing to do.
                drop(running);
                return;
            }
        }
    }
}

async fn flush_loop(weak: Weak<Inner>, node: Weak<LanNode>) {
    loop {
        let (Some(inner), Some(node)) = (weak.upgrade(), node.upgrade()) else {
            return;
        };
        inner.publish_pending(&node);
        drop(node);
        let interval = inner.config.flush_interval;
        let _ = tokio::time::timeout(interval, inner.flush.notified()).await;
    }
}

async fn host_loop(weak: Weak<Inner>, mut events: tokio::sync::mpsc::UnboundedReceiver<HostEvent>) {
    while let Some(event) = events.recv().await {
        let Some(inner) = weak.upgrade() else { return };
        match event {
            HostEvent::Paired { device_id, name } => {
                inner.emit(&ServiceEvent::Paired { device_id, name });
                inner.emit(&ServiceEvent::InviteClosed);
                if let Err(e) = inner.restart().await {
                    tracing::warn!(error = %e, "sync: node not started after pairing");
                }
                inner.emit(&ServiceEvent::DevicesChanged);
                let host = lock(&inner.state).host.take();
                drop(host);
                return;
            }
            HostEvent::Failed { reason } => {
                inner.emit(&ServiceEvent::PairingFailed { reason });
            }
            HostEvent::Closed => {
                inner.emit(&ServiceEvent::InviteClosed);
                let host = lock(&inner.state).host.take();
                drop(host);
                return;
            }
        }
    }
}

/// Tracks which sync peers are visible, until the node stops.
fn watch_visible(
    weak: &Weak<Inner>,
    node: &Weak<LanNode>,
    events: &std_mpsc::Receiver<DiscoveryEvent>,
) {
    use std::collections::HashMap;
    let mut ids: HashMap<String, String> = HashMap::new();
    loop {
        let event = match events.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => event,
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                if weak.strong_count() == 0 || node.strong_count() == 0 {
                    return;
                }
                continue;
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if node.strong_count() == 0 {
            return;
        }
        let Some(inner) = weak.upgrade() else { return };
        let changed = match event {
            DiscoveryEvent::Found(s) | DiscoveryEvent::Updated(s)
                if s.kind == ServiceKind::SyncPeer =>
            {
                match s.id() {
                    Some(id) => {
                        ids.insert(s.instance.to_lowercase(), id.to_owned());
                        lock(&inner.state).visible.insert(id.to_owned())
                    }
                    None => false,
                }
            }
            DiscoveryEvent::Lost {
                kind: ServiceKind::SyncPeer,
                instance,
            } => match ids.remove(&instance.to_lowercase()) {
                Some(id) => lock(&inner.state).visible.remove(&id),
                None => false,
            },
            _ => false,
        };
        if changed {
            inner.emit(&ServiceEvent::DevicesChanged);
        }
    }
}
