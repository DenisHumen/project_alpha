// SPDX-License-Identifier: MPL-2.0
//! Pairing and LAN sync for Dart (UI-019): [`CoreSync`] wraps the
//! `pa-sync` [`SyncService`] of one app over its [`CoreLibrary`].
//!
//! The device keys and the family key live in the secret store opened by
//! `secrets_open_platform` / `secrets_open_default` (opened here with the
//! default backends if the app has not done it). Progress and "watched"
//! changes the library records while paired go to the other devices; their
//! changes come back as [`CoreSyncEventKind::LibraryChanged`].
//!
//! The playback session (SYNC-003): [`CoreSync::heartbeat`] while a title
//! plays, [`CoreSync::start_playing`] (takeover), [`CoreSync::resume_hint`]
//! before it starts and [`CoreSync::playing_elsewhere`] for the "Continue
//! on this device" banner. Titles are item ids of the library; the core
//! maps them to the synced media keys.
//!
//! Every method may block on the network or the secret store, so only the
//! plain getters are `#[frb(sync)]`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flutter_rust_bridge::frb;
use pa_catalog::Catalog;
use pa_catalog::pa_sync::lan::service::{
    DeviceStatus, ServiceConfig, ServiceError, ServiceEvent, SyncLibrary, SyncService,
};
use pa_catalog::pa_sync::handoff::{self, ResumePoint};
use pa_catalog::pa_sync::hlc::system_ms;
use pa_catalog::pa_sync::proto::{PlayState, PlaybackUpdate};
use pa_catalog::pa_sync::{Op, OpId};
use pa_catalog::{ItemRef, LOCAL_DEVICE_ID};
use pa_discovery::DeviceClass;
use pa_model::MediaKey;

use crate::api::library::CoreLibrary;
use crate::frb_generated::StreamSink;

/// Error of a sync call; Dart receives it as an exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSyncError {
    /// `invalid_code`, `wrong_code`, `expired`, `not_found`,
    /// `already_paired`, `refused`, `unknown_device`, `io`, `pairing`,
    /// `secrets`.
    pub code: String,
    /// Human-readable message (no secrets, no codes).
    pub message: String,
}

impl From<ServiceError> for CoreSyncError {
    fn from(e: ServiceError) -> Self {
        Self {
            code: e.code.to_owned(),
            message: e.message,
        }
    }
}

type SyncResult<T> = std::result::Result<T, CoreSyncError>;

/// What kind of device this app runs on (orders the hub election: desktop,
/// TV, phone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreDeviceClass {
    Desktop,
    Tv,
    Mobile,
}

impl From<CoreDeviceClass> for DeviceClass {
    fn from(c: CoreDeviceClass) -> Self {
        match c {
            CoreDeviceClass::Desktop => Self::Desktop,
            CoreDeviceClass::Tv => Self::Tv,
            CoreDeviceClass::Mobile => Self::Mobile,
        }
    }
}

/// Settings of [`CoreSync::open`].
#[derive(Debug, Clone)]
pub struct CoreSyncConfig {
    /// Folder for the family roster (`<app data>/sync`).
    pub data_dir: String,
    /// Name of this device shown on the others.
    pub device_name: String,
    pub device_class: CoreDeviceClass,
    /// Keep the device keys in an encrypted file store in this folder
    /// instead of the app's secret store (tests; two instances in one
    /// process need two identities).
    pub secrets_dir: Option<String>,
    /// Listen on 127.0.0.1 without mDNS (tests: devices are introduced
    /// with [`CoreSync::peer_up`]).
    pub loopback: bool,
}

/// A device of the family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSyncDevice {
    pub device_id: String,
    pub name: String,
    /// `phone`, `tablet`, `tv`, `desktop`, `server`.
    pub kind: String,
    /// Fingerprint of the device key, to compare on both screens.
    pub fingerprint: String,
    pub added_ms: i64,
    pub this_device: bool,
    /// Linked or visible on the LAN now.
    pub online: bool,
}

impl From<DeviceStatus> for CoreSyncDevice {
    fn from(d: DeviceStatus) -> Self {
        Self {
            device_id: d.device_id,
            name: d.name,
            kind: d.kind.as_str().to_owned(),
            fingerprint: d.fingerprint,
            added_ms: i64::try_from(d.added_ms).unwrap_or(i64::MAX),
            this_device: d.this_device,
            online: d.online,
        }
    }
}

/// An open invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSyncInvite {
    /// 6 digits to type on the other device.
    pub code: String,
    /// Content of the QR code (`projectalpha://pair?…`).
    pub qr: String,
    /// Unix ms when the invitation closes.
    pub expires_ms: i64,
}

/// Kind of a [`CoreSyncEvent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSyncEventKind {
    /// Changes from another device are in the library: refresh the views.
    LibraryChanged,
    /// The device list or online states changed.
    DevicesChanged,
    /// A device joined through this device's invitation (`device_id`,
    /// `name`).
    Paired,
    /// An attempt on the invitation failed (`message`); it stays open while
    /// attempts are left.
    PairingFailed,
    /// The invitation closed.
    InviteClosed,
    /// What another device plays now (`payload`: JSON of `pb.update`,
    /// `item_id`: the title in this library, if it has it).
    Playback,
    /// Another device started a title (`payload`: JSON of `pb.takeover`,
    /// `item_id` as for `Playback`): pause it here if the setting allows.
    Takeover,
}

/// Player state in a heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePlayState {
    Playing,
    Paused,
    Stopped,
}

impl From<CorePlayState> for PlayState {
    fn from(s: CorePlayState) -> Self {
        match s {
            CorePlayState::Playing => Self::Playing,
            CorePlayState::Paused => Self::Paused,
            CorePlayState::Stopped => Self::Stopped,
        }
    }
}

/// Where a title continues (SYNC-003): the newest point by HLC of stored
/// progress and of the last heartbeat of another device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreResumeHint {
    /// Position now: a title another device still plays moved on since its
    /// last heartbeat.
    pub position_ms: i64,
    /// 0 if unknown.
    pub duration_ms: i64,
    /// Device that set the point.
    pub device_id: String,
    /// Its name in the family, if known.
    pub device_name: Option<String>,
    /// Set by another device (not by this one).
    pub remote: bool,
    /// That device was playing it at its last heartbeat.
    pub playing: bool,
    /// HLC of the point (wall ms, counter): later points order after it.
    pub stamp_ms: i64,
    pub stamp_counter: u32,
}

/// A title another device plays right now (the handoff banner).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreRemotePlayback {
    /// The title in this library; `None` if this library does not have it.
    pub item_id: Option<String>,
    pub device_id: String,
    pub device_name: Option<String>,
    /// Position now (moved on since the heartbeat).
    pub position_ms: i64,
    /// 0 if unknown.
    pub duration_ms: i64,
    pub stamp_ms: i64,
}

/// An event of the sync service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSyncEvent {
    pub kind: CoreSyncEventKind,
    pub device_id: Option<String>,
    pub name: Option<String>,
    pub message: Option<String>,
    pub payload: Option<String>,
    /// `Playback`/`Takeover`: the title in this library, if it has it.
    pub item_id: Option<String>,
}

impl CoreSyncEvent {
    fn of(kind: CoreSyncEventKind) -> Self {
        Self {
            kind,
            device_id: None,
            name: None,
            message: None,
            payload: None,
            item_id: None,
        }
    }
}

impl From<ServiceEvent> for CoreSyncEvent {
    fn from(e: ServiceEvent) -> Self {
        use CoreSyncEventKind as K;
        match e {
            ServiceEvent::LibraryChanged => Self::of(K::LibraryChanged),
            ServiceEvent::DevicesChanged => Self::of(K::DevicesChanged),
            ServiceEvent::Paired { device_id, name } => Self {
                device_id: Some(device_id),
                name: Some(name),
                ..Self::of(K::Paired)
            },
            ServiceEvent::PairingFailed { reason } => Self {
                message: Some(reason),
                ..Self::of(K::PairingFailed)
            },
            ServiceEvent::InviteClosed => Self::of(K::InviteClosed),
            ServiceEvent::Playback(u) => Self {
                device_id: Some(u.hlc.device.clone()),
                payload: serde_json::to_string(&u).ok(),
                ..Self::of(K::Playback)
            },
            ServiceEvent::Takeover(t) => Self {
                device_id: Some(t.hlc.device.clone()),
                payload: serde_json::to_string(&t).ok(),
                ..Self::of(K::Takeover)
            },
        }
    }
}

/// The catalog as the library of the sync service.
#[frb(ignore)]
pub(crate) struct CatalogSync(pub(crate) Arc<Catalog>);

impl SyncLibrary for CatalogSync {
    fn enable(&self, device_id: &str) -> std::result::Result<(), String> {
        self.0.enable_sync(device_id).map_err(|e| e.to_string())
    }

    fn disable(&self) -> std::result::Result<(), String> {
        self.0.disable_sync().map_err(|e| e.to_string())
    }

    fn pending(&self) -> std::result::Result<Vec<Op>, String> {
        self.0.pending_ops(512).map_err(|e| e.to_string())
    }

    fn ack(&self, op_id: OpId) -> std::result::Result<(), String> {
        self.0.ack_op(op_id).map(drop).map_err(|e| e.to_string())
    }

    fn apply(&self, op: &Op) -> std::result::Result<bool, String> {
        self.0.apply_remote(op).map_err(|e| e.to_string())
    }
}

/// Pairing and LAN sync of this app.
#[frb(opaque)]
pub struct CoreSync {
    service: Arc<SyncService>,
    catalog: Arc<Catalog>,
}

fn io(e: impl std::fmt::Display) -> CoreSyncError {
    CoreSyncError {
        code: "io".into(),
        message: e.to_string(),
    }
}

fn ms(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

impl CoreSync {
    /// Opens the sync service over `library`. Nothing runs yet: call
    /// [`start`](Self::start) (syncs if paired), [`invite`](Self::invite)
    /// or [`join`](Self::join).
    pub fn open(library: &CoreLibrary, config: CoreSyncConfig) -> SyncResult<CoreSync> {
        let own_store = match &config.secrets_dir {
            Some(dir) => {
                Some(
                    pa_secrets::EncryptedFileStore::open(dir).map_err(|e| CoreSyncError {
                        code: "secrets".into(),
                        message: e.to_string(),
                    })?,
                )
            }
            None => None,
        };
        // The app's store (or the shared default one, like the source
        // manager's); its fallback file sits next to the roster's folder.
        let store: Arc<dyn pa_secrets::SecretStore> = match own_store {
            Some(own) => Arc::new(own),
            None => {
                let dir = PathBuf::from(&config.data_dir);
                let base = dir.parent().map_or(dir.clone(), Path::to_path_buf);
                crate::shared::secret_store(&base)
                    .map_err(|e| CoreSyncError {
                        code: "secrets".into(),
                        message: e.to_string(),
                    })?
                    .store
            }
        };
        let class: DeviceClass = config.device_class.into();
        let mut service_config = ServiceConfig::new(
            PathBuf::from(&config.data_dir),
            config.device_name.trim(),
            class,
        );
        if config.loopback {
            service_config.bind_ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
            service_config.mdns = None;
        }
        let service = Arc::new(SyncService::open(
            service_config,
            store,
            Arc::new(CatalogSync(library.catalog())),
        )?);
        library.set_sync_hook(Arc::downgrade(&service));
        Ok(Self {
            service,
            catalog: library.catalog(),
        })
    }

    /// This device's id.
    #[frb(sync)]
    pub fn device_id(&self) -> String {
        self.service.device_id().to_owned()
    }

    /// Fingerprint of this device's key.
    #[frb(sync)]
    pub fn fingerprint(&self) -> String {
        self.service.fingerprint()
    }

    /// Whether other devices are paired.
    #[frb(sync)]
    pub fn is_paired(&self) -> bool {
        self.service.is_paired()
    }

    /// Whether sync runs.
    #[frb(sync)]
    pub fn is_running(&self) -> bool {
        self.service.is_running()
    }

    /// The family, this device first; empty before the first pairing.
    #[frb(sync)]
    pub fn devices(&self) -> Vec<CoreSyncDevice> {
        self.service.devices().into_iter().map(Into::into).collect()
    }

    /// Starts sync if paired; returns whether it runs.
    pub fn start(&self) -> SyncResult<bool> {
        Ok(self.service.start()?)
    }

    /// Stops sync (until [`start`](Self::start)).
    pub fn stop(&self) {
        self.service.stop();
    }

    /// `host:port` of the running sync node, if any.
    #[frb(sync)]
    pub fn sync_addr(&self) -> Option<String> {
        self.service.sync_addr().map(|a| a.to_string())
    }

    /// A device at a known `host:port` (when mDNS cannot see it; tests).
    #[frb(sync)]
    pub fn peer_up(
        &self,
        device_id: String,
        device_class: CoreDeviceClass,
        addr: String,
    ) -> SyncResult<()> {
        let addr: SocketAddr = addr.parse().map_err(|_| CoreSyncError {
            code: "invalid".into(),
            message: "address is not host:port".into(),
        })?;
        self.service.peer_up(&device_id, device_class.into(), addr);
        Ok(())
    }

    /// Sends queued changes now.
    #[frb(sync)]
    pub fn flush(&self) {
        self.service.flush();
    }

    /// Opens an invitation (5 minutes, 5 attempts) for another device.
    pub fn invite(&self) -> SyncResult<CoreSyncInvite> {
        let invite = self.service.invite()?;
        Ok(CoreSyncInvite {
            code: invite.code,
            qr: invite.qr,
            expires_ms: i64::try_from(invite.expires_ms).unwrap_or(i64::MAX),
        })
    }

    /// Closes the open invitation.
    pub fn cancel_invite(&self) {
        self.service.cancel_invite();
    }

    /// Joins the family of the device that shows `input`: a scanned QR
    /// code or the typed 6-digit code. Returns that device.
    pub fn join(&self, input: String) -> SyncResult<CoreSyncDevice> {
        Ok(self.service.join(&input)?.into())
    }

    /// Removes a device from the family; this device's id leaves the
    /// family.
    pub fn unpair(&self, device_id: String) -> SyncResult<()> {
        Ok(self.service.unpair(&device_id)?)
    }

    /// Events until the service is dropped. Returns at once; the stream
    /// stays open. `pb.*` of other profiles are left out.
    pub fn events(&self, sink: StreamSink<CoreSyncEvent>) {
        let rx = self.service.subscribe();
        let catalog = Arc::clone(&self.catalog);
        std::thread::Builder::new()
            .name("pa-sync-events".into())
            .spawn(move || {
                let profile = catalog.sync_profile().unwrap_or_default();
                for event in rx {
                    let target = match &event {
                        ServiceEvent::Playback(u) => Some((&u.profile, &u.key)),
                        ServiceEvent::Takeover(t) => Some((&t.profile, &t.key)),
                        _ => None,
                    };
                    let item_id = match target {
                        Some((p, _)) if *p != profile => continue,
                        Some((_, key)) => catalog
                            .item_for_key(key)
                            .ok()
                            .flatten()
                            .map(|i| i.to_string()),
                        None => None,
                    };
                    let event = CoreSyncEvent {
                        item_id,
                        ..event.into()
                    };
                    if sink.add(event).is_err() {
                        return;
                    }
                }
            })
            .map(drop)
            .unwrap_or_default();
    }

    /// Heartbeat of the playback session (SYNC-003): every 5 s while
    /// playing and at once on pause, seek, stop and going to the
    /// background. Ephemeral: nothing is stored. `false` if the item has no
    /// synced key yet or sync does not run.
    pub fn heartbeat(
        &self,
        item_id: String,
        position_ms: i64,
        duration_ms: i64,
        state: CorePlayState,
    ) -> SyncResult<bool> {
        if !self.service.is_running() {
            return Ok(false);
        }
        let Some((profile, key)) = self.key(&item_id)? else {
            return Ok(false);
        };
        self.service.heartbeat(
            &profile,
            &key,
            u64::try_from(position_ms).unwrap_or(0),
            u64::try_from(duration_ms).ok().filter(|d| *d > 0),
            state.into(),
        )?;
        Ok(true)
    }

    /// This device starts playing the item: the other devices of the
    /// profile pause it (`pb.takeover`, if their setting allows).
    pub fn start_playing(&self, item_id: String) -> SyncResult<bool> {
        if !self.service.is_running() {
            return Ok(false);
        }
        let Some((profile, key)) = self.key(&item_id)? else {
            return Ok(false);
        };
        self.service.start_playing(&profile, &key)?;
        Ok(true)
    }

    /// Where the item continues: first pulls from the hub for at most
    /// `timeout_ms` (500–800 ms before playing; 0 skips the pull), then
    /// takes the newest point by HLC of the stored progress (with the
    /// others' `progress.upsert`) and the last heartbeat of another device.
    /// `None` if there is neither (or the item was finished last).
    pub fn resume_hint(
        &self,
        item_id: String,
        timeout_ms: u32,
    ) -> SyncResult<Option<CoreResumeHint>> {
        let Ok(item) = item_id.parse::<ItemRef>() else {
            return Ok(None);
        };
        if timeout_ms > 0 && self.service.is_running() {
            self.service
                .refresh(std::time::Duration::from_millis(u64::from(timeout_ms)));
        }
        let local = self.catalog.resume_point(item).map_err(io)?;
        let remote = match self.key(&item_id)? {
            Some((profile, key)) => self
                .service
                .remote_playback(&profile, &key)
                .map(|u| ResumePoint::from_update(&u)),
            None => None,
        };
        let Some(point) = handoff::newest(local.into_iter().chain(remote)) else {
            return Ok(None);
        };
        let me = self.service.device_id();
        let device_id = point.device().to_owned();
        Ok(Some(CoreResumeHint {
            position_ms: ms(point.position_at(system_ms())),
            duration_ms: point.duration_ms.map_or(0, ms),
            device_name: self.name_of(&device_id),
            remote: device_id != me && device_id != LOCAL_DEVICE_ID,
            playing: point.state == Some(PlayState::Playing),
            stamp_ms: ms(point.stamp.wall_ms),
            stamp_counter: point.stamp.counter,
            device_id,
        }))
    }

    /// Titles other devices of the family play right now, newest first
    /// (the "Continue on this device" banner).
    pub fn playing_elsewhere(&self) -> SyncResult<Vec<CoreRemotePlayback>> {
        let profile = self.catalog.sync_profile().map_err(io)?;
        let now = system_ms();
        Ok(self
            .service
            .playing_elsewhere()
            .into_iter()
            .filter(|u| u.profile == profile)
            .map(|u| self.remote(&u, now))
            .collect())
    }

    fn remote(&self, u: &PlaybackUpdate, now: u64) -> CoreRemotePlayback {
        let point = ResumePoint::from_update(u);
        CoreRemotePlayback {
            item_id: self
                .catalog
                .item_for_key(&u.key)
                .ok()
                .flatten()
                .map(|i| i.to_string()),
            device_name: self.name_of(&u.hlc.device),
            device_id: u.hlc.device.clone(),
            position_ms: ms(point.position_at(now)),
            duration_ms: u.duration_ms.map_or(0, ms),
            stamp_ms: ms(u.hlc.wall_ms),
        }
    }

    fn name_of(&self, device_id: &str) -> Option<String> {
        self.service
            .devices()
            .into_iter()
            .find(|d| d.device_id == device_id)
            .map(|d| d.name)
    }

    /// Profile and synced key of a library item; `None` while it has no
    /// stable key yet (file not fingerprinted).
    fn key(&self, item_id: &str) -> SyncResult<Option<(String, MediaKey)>> {
        let Ok(item) = item_id.parse::<ItemRef>() else {
            return Ok(None);
        };
        let Some(key) = self.catalog.sync_key(item).map_err(io)? else {
            return Ok(None);
        };
        Ok(Some((self.catalog.sync_profile().map_err(io)?, key)))
    }
}
