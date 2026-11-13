// SPDX-License-Identifier: MPL-2.0
//! The server's place in the device family (SYNC-005, `pa-pairing`).
//!
//! On first start the server founds a family: its device id is the hub id,
//! its keys and the family key go into an encrypted secrets file in the
//! data directory (`pa_secrets::EncryptedFileStore`; a headless server has no
//! OS keychain), the roster into [`ROSTER_FILE`].
//!
//! - **Pairing:** an administrator asks for a code on the loopback API
//!   (`POST /api/v1/pair/invite`); a new device connects to the WebSocket
//!   [`PAIR_PATH`] and runs SPAKE2 with the code. The invitation allows
//!   [`pa_pairing::pair::MAX_ATTEMPTS`] attempts within five minutes and is
//!   consumed by a successful pairing.
//! - **Authentication:** the sync hub checks `hello.nonce` + `auth` of every
//!   device against the roster ([`Pairing::verify`]).
//! - **Revocation:** `DELETE /api/v1/devices/{id}` (loopback) or a `family`
//!   update from another member revokes a device: its connections are closed
//!   with `unauthorized`, the family key rotates, and connected members get
//!   the new roster and key.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::extract::ws::{Message as WsMessage, WebSocket};
use pa_pairing::pair::{Invitation, Inviter, PairMessage, code};
use pa_pairing::{Device, DeviceIdentity, DeviceKind, Family, FamilyUpdate};
use pa_secrets::{EncryptedFileStore, SecretStore};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::Error;

/// Roster (trust store) file inside the data directory.
pub const ROSTER_FILE: &str = "devices.json";
/// WebSocket endpoint for pairing a new device.
pub const PAIR_PATH: &str = "/api/v1/pair";
/// Name of the server in device lists.
pub const SERVER_NAME: &str = "Kinolune server";

/// How long a pairing peer may take for each step.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// A change of the family for live sync connections.
#[derive(Debug, Clone)]
pub(crate) enum FamilyEvent {
    /// Devices revoked: their connections close.
    Revoked(Vec<String>),
    /// The roster or the key changed: members get a fresh `family` update.
    Changed,
}

/// What `POST /api/v1/pair/invite` returns.
#[derive(Debug, Clone, Serialize)]
pub struct Invite {
    /// The code to type on the new device.
    pub code: String,
    /// QR payload (`projectalpha://pair?…`) with the same code.
    pub qr: String,
    /// When the invitation expires, Unix ms.
    pub expires_ms: u64,
}

struct Inner {
    store: Arc<dyn SecretStore>,
    roster_path: PathBuf,
    family: Mutex<Family>,
    invitation: Mutex<Option<Invitation>>,
    /// Where devices reach the server, for the QR code.
    addr: Option<String>,
    events: broadcast::Sender<FamilyEvent>,
}

/// The family of the server; cheap to clone.
#[derive(Clone)]
pub struct Pairing {
    inner: Arc<Inner>,
}

fn now_ms() -> u64 {
    u64::try_from(crate::sync::now_ms()).unwrap_or(0)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Pairing {
    /// Loads the family from `data_dir` or founds a new one with the server
    /// as `device_id`. `addr` goes into QR codes. Blocking.
    pub fn open(data_dir: &Path, device_id: &str, addr: Option<String>) -> Result<Self, Error> {
        let err = |source| Error::Pairing {
            path: data_dir.to_owned(),
            source,
        };
        let store: Arc<dyn SecretStore> = Arc::new(
            EncryptedFileStore::open(data_dir).map_err(|e| err(pa_pairing::Error::from(e)))?,
        );
        let roster_path = data_dir.join(ROSTER_FILE);
        let family = match Family::load(store.as_ref(), &roster_path).map_err(err)? {
            Some(family) => family,
            None => {
                let identity =
                    DeviceIdentity::load_or_create(store.as_ref(), device_id).map_err(err)?;
                let family = Family::found(identity, SERVER_NAME, DeviceKind::Server, now_ms())
                    .map_err(err)?;
                family.save(store.as_ref(), &roster_path).map_err(err)?;
                tracing::info!(family = family.family_id(), "founded a device family");
                family
            }
        };
        let (events, _) = broadcast::channel(64);
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                roster_path,
                family: Mutex::new(family),
                invitation: Mutex::new(None),
                addr,
                events,
            }),
        })
    }

    /// Family id.
    pub fn family_id(&self) -> String {
        self.family().family_id().to_owned()
    }

    /// Fingerprint of the server's key, to compare on the new device.
    pub fn fingerprint(&self) -> String {
        self.family().identity().public().fingerprint()
    }

    fn family(&self) -> MutexGuard<'_, Family> {
        lock(&self.inner.family)
    }

    pub(crate) fn events(&self) -> broadcast::Receiver<FamilyEvent> {
        self.inner.events.subscribe()
    }

    fn save(&self, family: &Family) {
        if let Err(e) = family.save(self.inner.store.as_ref(), &self.inner.roster_path) {
            tracing::error!(error = %e, "pairing: family not saved");
        }
    }

    /// A new invitation, replacing an open one: a 6-digit code, or with
    /// `long` a 128-bit code for a QR code only.
    pub fn invite(&self, long: bool) -> Result<Invite, pa_pairing::Error> {
        let now = now_ms();
        let invitation = if long {
            Invitation::long(now)?
        } else {
            Invitation::digits(now)?
        };
        let keys = self.family().identity().public();
        let invite = Invite {
            code: invitation.code().expose().to_owned(),
            qr: invitation.qr(&keys, self.inner.addr.as_deref()),
            expires_ms: invitation.expires_ms(),
        };
        *lock(&self.inner.invitation) = Some(invitation);
        tracing::info!("pairing: invitation opened");
        Ok(invite)
    }

    /// The device list, newest first.
    pub fn devices(&self) -> Vec<Device> {
        self.family().roster().devices()
    }

    /// Revokes `device_id`, rotates the family key, closes the device's
    /// connections and sends the others the new key. Blocking (saves).
    pub fn revoke(&self, device_id: &str) -> Result<(), pa_pairing::Error> {
        {
            let mut family = self.family();
            family.revoke(device_id, now_ms())?;
            self.save(&family);
        }
        tracing::info!(device = device_id, "pairing: device revoked");
        let _ = self
            .inner
            .events
            .send(FamilyEvent::Revoked(vec![device_id.to_owned()]));
        let _ = self.inner.events.send(FamilyEvent::Changed);
        Ok(())
    }

    /// Our `hello`'s answer: `auth.sig` for the device `peer` that sent
    /// `peer_nonce`.
    pub(crate) fn prove(&self, peer: &str, own_nonce: &str, peer_nonce: &str) -> String {
        pa_pairing::auth::prove(self.family().identity(), peer, own_nonce, peer_nonce)
    }

    /// Checks the `auth.sig` of `device`.
    pub(crate) fn verify(
        &self,
        device: &str,
        device_nonce: &str,
        own_nonce: &str,
        sig: &str,
    ) -> Result<(), pa_pairing::Error> {
        let family = self.family();
        let me = family.identity().device_id().to_owned();
        pa_pairing::auth::verify(family.roster(), device, &me, device_nonce, own_nonce, sig)
            .map(|_| ())
    }

    /// The `family` update for a paired `device`, as JSON text.
    pub(crate) fn update_for(&self, device: &str) -> Option<String> {
        match self.family().update_for(device) {
            Ok(update) => Some(update.to_json()),
            Err(e) => {
                tracing::debug!(device, error = %e, "pairing: no family update");
                None
            }
        }
    }

    /// Takes over a `family` update from a member. Blocking (saves).
    pub(crate) fn apply_update(&self, from: &str, body: &str) {
        let update = match FamilyUpdate::from_json(body) {
            Ok(update) => update,
            Err(e) => {
                tracing::info!(device = from, error = %e, "pairing: bad family update");
                return;
            }
        };
        let revoked_ids = |family: &Family| -> BTreeSet<String> {
            family
                .roster()
                .devices()
                .into_iter()
                .filter(|d| d.revoked_ms.is_some() && family.authorize(&d.device_id).is_err())
                .map(|d| d.device_id)
                .collect()
        };
        let (newly_revoked, changed) = {
            let mut family = self.family();
            let before = revoked_ids(&family);
            match family.apply_update(&update) {
                Ok(report) => {
                    let after = revoked_ids(&family);
                    let changed = report.roster.changed() || report.key_rotated;
                    if changed {
                        self.save(&family);
                    }
                    // The revoking member rotated the key and sealed it to
                    // us in the same update.
                    (
                        after.difference(&before).cloned().collect::<Vec<_>>(),
                        changed,
                    )
                }
                Err(e) => {
                    tracing::warn!(device = from, error = %e, "pairing: family update rejected");
                    return;
                }
            }
        };
        if !newly_revoked.is_empty() {
            tracing::info!(devices = ?newly_revoked, by = from, "pairing: devices revoked by a member");
            let _ = self.inner.events.send(FamilyEvent::Revoked(newly_revoked));
        }
        if changed {
            let _ = self.inner.events.send(FamilyEvent::Changed);
        }
    }

    /// Runs one pairing attempt as the inviter over `socket`.
    pub(crate) async fn serve(self, mut socket: WebSocket) {
        match self.pair(&mut socket).await {
            Ok(device) => tracing::info!(device, "pairing: device paired"),
            Err(reason) => tracing::info!(reason, "pairing: attempt failed"),
        }
        let _ = socket.send(WsMessage::Close(None)).await;
    }

    async fn pair(&self, socket: &mut WebSocket) -> Result<String, &'static str> {
        let started = {
            let mut invitation = lock(&self.inner.invitation);
            match invitation.as_mut() {
                Some(inv) => Inviter::start(inv, now_ms()).ok(),
                None => None,
            }
        };
        let Some((mut inviter, hello)) = started else {
            send(socket, &PairMessage::error(code::EXPIRED, None)).await;
            return Err("no open invitation");
        };
        send(socket, &hello).await;

        let msg = recv(socket).await.ok_or("no hello")?;
        if inviter.receive_hello(&msg).is_err() {
            send(socket, &PairMessage::error(code::BAD_MESSAGE, None)).await;
            return Err("bad hello");
        }
        let msg = recv(socket).await.ok_or("no intro")?;
        let request = match inviter.open_intro(&msg) {
            Ok(request) => request,
            Err(pa_pairing::Error::WrongCode) => {
                send(socket, &PairMessage::error(code::WRONG_CODE, None)).await;
                return Err("wrong code");
            }
            Err(_) => {
                send(socket, &PairMessage::error(code::BAD_MESSAGE, None)).await;
                return Err("bad intro");
            }
        };
        let pairing = self.clone();
        let device = request.device_id.clone();
        let welcome = tokio::task::spawn_blocking(move || {
            let mut family = pairing.family();
            let (_, welcome) = inviter.welcome(&mut family, &request, now_ms())?;
            pairing.save(&family);
            Ok::<_, pa_pairing::Error>(welcome)
        })
        .await
        .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
        let welcome = match welcome {
            Ok(welcome) => welcome,
            Err(e) => {
                tracing::info!(device, error = %e, "pairing: device refused");
                send(socket, &PairMessage::error(code::DENIED, None)).await;
                return Err("refused");
            }
        };
        *lock(&self.inner.invitation) = None;
        send(socket, &welcome).await;
        let _ = self.inner.events.send(FamilyEvent::Changed);
        Ok(device)
    }
}

async fn send(socket: &mut WebSocket, msg: &PairMessage) {
    let _ = socket.send(WsMessage::Text(msg.encode().into())).await;
}

async fn recv(socket: &mut WebSocket) -> Option<PairMessage> {
    loop {
        let msg = tokio::time::timeout(STEP_TIMEOUT, socket.recv())
            .await
            .ok()??
            .ok()?;
        match msg {
            WsMessage::Text(text) => return PairMessage::decode(text.as_str()).ok(),
            WsMessage::Close(_) => return None,
            _ => {}
        }
    }
}
