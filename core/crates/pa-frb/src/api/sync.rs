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
//! Every method may block on the network or the secret store, so only the
//! plain getters are `#[frb(sync)]`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use flutter_rust_bridge::frb;
use pa_catalog::Catalog;
use pa_catalog::pa_sync::lan::service::{
    DeviceStatus, ServiceConfig, ServiceError, ServiceEvent, SyncLibrary, SyncService,
};
use pa_catalog::pa_sync::{Op, OpId};
use pa_discovery::DeviceClass;

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
    /// What another device plays now (`payload`: JSON of `pb.update`);
    /// hook for the handoff banner of SYNC-003.
    Playback,
    /// Another device started a title (`payload`: JSON of `pb.takeover`).
    Takeover,
}

/// An event of the sync service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSyncEvent {
    pub kind: CoreSyncEventKind,
    pub device_id: Option<String>,
    pub name: Option<String>,
    pub message: Option<String>,
    pub payload: Option<String>,
}

impl CoreSyncEvent {
    fn of(kind: CoreSyncEventKind) -> Self {
        Self {
            kind,
            device_id: None,
            name: None,
            message: None,
            payload: None,
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
        let store: Arc<dyn pa_secrets::SecretStore> =
            match (own_store, crate::api::secrets::opened_store()) {
                (Some(own), _) => Arc::new(own),
                (None, Some(opened)) => opened.store,
                (None, None) => {
                    crate::api::secrets::secrets_open_default(Some(config.data_dir.clone()))
                        .map_err(|e| CoreSyncError {
                            code: "secrets".into(),
                            message: e.message,
                        })?;
                    crate::api::secrets::opened_store()
                        .map(|o| o.store)
                        .ok_or_else(|| CoreSyncError {
                            code: "secrets".into(),
                            message: "the secret store did not open".into(),
                        })?
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
        Ok(Self { service })
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
    /// stays open.
    pub fn events(&self, sink: StreamSink<CoreSyncEvent>) {
        let rx = self.service.subscribe();
        std::thread::Builder::new()
            .name("pa-sync-events".into())
            .spawn(move || {
                for event in rx {
                    if sink.add(event.into()).is_err() {
                        return;
                    }
                }
            })
            .map(drop)
            .unwrap_or_default();
    }
}
