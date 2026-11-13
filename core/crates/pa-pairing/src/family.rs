// SPDX-License-Identifier: MPL-2.0
//! The family key and the [`Family`] of this device.
//!
//! The family key is a 256-bit symmetric key shared by all active members:
//! the E2E key of synced secrets (`SYNC-010`) and of the shared-folder
//! journal. It has an epoch; revoking a device rotates it, and each
//! remaining member gets the new key sealed to its X25519 key in a
//! [`KeyEnvelope`] (static-static X25519, HKDF-SHA256,
//! ChaCha20-Poly1305), signed by the sender.

use std::fmt;
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use pa_secrets::{SecretKey, SecretStore, SecretString};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::codec::{self, Transcript, hex_array};
use crate::identity::{self, DeviceIdentity};
use crate::roster::{DeviceKind, Member, MergeReport, Revocation, Roster};
use crate::{Error, Result};

/// Name of the family key in the secret store.
pub const FAMILY_KEY_SECRET: &str = "pairing.family_key";

/// The symmetric key of the family at an epoch. `Debug` hides the key; the
/// memory is zeroed on drop.
#[derive(Clone)]
pub struct FamilyKey {
    epoch: u32,
    key: Zeroizing<[u8; 32]>,
}

impl FamilyKey {
    /// A random key for `epoch`.
    pub fn generate(epoch: u32) -> Result<Self> {
        Ok(Self {
            epoch,
            key: Zeroizing::new(codec::random::<32>()?),
        })
    }

    pub(crate) fn from_bytes(epoch: u32, key: [u8; 32]) -> Self {
        Self {
            epoch,
            key: Zeroizing::new(key),
        }
    }

    /// Epoch, from 1; grows on every rotation.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// The key bytes, for encrypting synced secrets. Do not log them.
    pub fn expose(&self) -> &[u8; 32] {
        &self.key
    }

    /// A fresh key of the next epoch.
    pub fn next(&self) -> Result<Self> {
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or(Error::Invalid("key epoch"))?;
        Self::generate(epoch)
    }
}

impl fmt::Debug for FamilyKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FamilyKey(epoch {}, ***)", self.epoch)
    }
}

/// The family key sealed by one member to another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEnvelope {
    pub epoch: u32,
    /// Signing key of the sender.
    #[serde(with = "hex_array")]
    pub from: [u8; 32],
    /// Signing key of the receiver.
    #[serde(with = "hex_array")]
    pub to: [u8; 32],
    #[serde(with = "hex_array")]
    pub nonce: [u8; 12],
    /// ChaCha20-Poly1305 ciphertext of the key, hex.
    pub sealed: String,
    #[serde(with = "hex_array")]
    pub sig: [u8; 64],
}

impl KeyEnvelope {
    fn header(family: &str, epoch: u32, from: &[u8; 32], to: &[u8; 32]) -> Transcript {
        let mut t = Transcript::new("kinolune.family-key.v1");
        t.put_str(family).put_u64(u64::from(epoch)).put(from).put(to);
        t
    }

    fn signed(&self, family: &str) -> Transcript {
        let mut t = Self::header(family, self.epoch, &self.from, &self.to);
        t.put(&self.nonce).put_str(&self.sealed);
        t
    }

    fn cipher(family: &str, shared: &[u8; 32], from: &[u8; 32], to: &[u8; 32]) -> ChaCha20Poly1305 {
        let mut info = Transcript::new("kinolune.family-key.kdf.v1");
        info.put(from).put(to);
        let mut key = Zeroizing::new([0u8; 32]);
        Hkdf::<Sha256>::new(Some(family.as_bytes()), shared)
            .expand(info.bytes(), key.as_mut())
            .expect("32 bytes is a valid HKDF length");
        ChaCha20Poly1305::new_from_slice(key.as_ref()).expect("32-byte key")
    }

    /// Seals `key` from `from` to member `to`.
    pub fn seal(family: &str, from: &DeviceIdentity, to: &Member, key: &FamilyKey) -> Result<Self> {
        let sender = from.public().sign;
        let shared = from.agree(&to.keys.dh)?;
        let nonce = codec::random::<12>()?;
        let header = Self::header(family, key.epoch, &sender, &to.keys.sign);
        let sealed = Self::cipher(family, &shared, &sender, &to.keys.sign)
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: key.expose(),
                    aad: header.bytes(),
                },
            )
            .map_err(|_| Error::Invalid("encryption"))?;
        let mut env = Self {
            epoch: key.epoch,
            from: sender,
            to: to.keys.sign,
            nonce,
            sealed: codec::to_hex(&sealed),
            sig: [0; 64],
        };
        env.sig = from.sign(env.signed(family).bytes());
        Ok(env)
    }

    /// Opens an envelope addressed to `me` from an active member of
    /// `roster`.
    pub fn open(&self, roster: &Roster, me: &DeviceIdentity) -> Result<FamilyKey> {
        if self.to != me.public().sign {
            return Err(Error::Invalid("envelope receiver"));
        }
        let sender = roster
            .member(&self.from)
            .ok_or_else(|| Error::Unknown(codec::to_hex(&self.from)))?;
        if !roster.is_active(&self.from) {
            return Err(Error::Revoked(sender.device_id.clone()));
        }
        let family = roster.family_id();
        identity::verify(&self.from, self.signed(family).bytes(), &self.sig, "key envelope")?;
        let shared = me.agree(&sender.keys.dh)?;
        let sealed = codec::from_hex(&self.sealed, 64)?;
        let header = Self::header(family, self.epoch, &self.from, &self.to);
        let plain = Zeroizing::new(
            Self::cipher(family, &shared, &self.from, &self.to)
                .decrypt(
                    &Nonce::from(self.nonce),
                    Payload {
                        msg: &sealed,
                        aad: header.bytes(),
                    },
                )
                .map_err(|_| Error::BadSignature("key envelope"))?,
        );
        let key: [u8; 32] = plain
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("family key"))?;
        Ok(FamilyKey::from_bytes(self.epoch, key))
    }
}

/// What a member sends to another after a change (or on connect): the
/// roster and the current family key sealed to the receiver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilyUpdate {
    pub roster: Roster,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<KeyEnvelope>,
}

impl FamilyUpdate {
    /// JSON text.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("update serializes")
    }

    /// Parses and verifies the roster in it.
    pub fn from_json(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }
}

/// Result of [`Family::apply_update`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdateReport {
    pub roster: MergeReport,
    /// A newer family key was taken over.
    pub key_rotated: bool,
}

#[derive(Serialize, Deserialize)]
struct StoredKey {
    v: u8,
    family: String,
    epoch: u32,
    key: String,
}

/// This device's membership: its identity, the roster and the family key.
#[derive(Debug)]
pub struct Family {
    identity: DeviceIdentity,
    roster: Roster,
    key: FamilyKey,
}

impl Family {
    /// A new family with this device as the founder.
    pub fn found(identity: DeviceIdentity, name: &str, kind: DeviceKind, now_ms: u64) -> Result<Self> {
        let roster = Roster::found(&identity, name, kind, now_ms)?;
        Ok(Self {
            identity,
            roster,
            key: FamilyKey::generate(1)?,
        })
    }

    /// Joins parts; `identity` must be an active member of `roster`.
    pub fn new(identity: DeviceIdentity, roster: Roster, key: FamilyKey) -> Result<Self> {
        let me = identity.public().sign;
        if !roster.is_active(&me) {
            return Err(if roster.member(&me).is_some() {
                Error::Revoked(identity.device_id().to_owned())
            } else {
                Error::Unknown(identity.device_id().to_owned())
            });
        }
        Ok(Self {
            identity,
            roster,
            key,
        })
    }

    /// This device's keys.
    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    /// The trust store.
    pub fn roster(&self) -> &Roster {
        &self.roster
    }

    /// The current family key.
    pub fn key(&self) -> &FamilyKey {
        &self.key
    }

    /// Family id.
    pub fn family_id(&self) -> &str {
        self.roster.family_id()
    }

    /// Active records of `device_id`; see [`Roster::authorize`].
    pub fn authorize(&self, device_id: &str) -> Result<Vec<&Member>> {
        self.roster.authorize(device_id)
    }

    /// Admits a device (after pairing).
    pub fn admit(
        &mut self,
        device_id: &str,
        name: &str,
        kind: DeviceKind,
        keys: crate::PublicKeys,
        now_ms: u64,
    ) -> Result<Member> {
        self.roster
            .admit(&self.identity, device_id, name, kind, keys, now_ms)
    }

    /// Revokes `device_id` and rotates the family key, so the revoked
    /// device cannot read anything encrypted from now on. Send
    /// [`update_for`](Self::update_for) to the other members.
    pub fn revoke(&mut self, device_id: &str, now_ms: u64) -> Result<Vec<Revocation>> {
        if device_id == self.identity.device_id() {
            return Err(Error::Invalid("revoking this device"));
        }
        let revs = self.roster.revoke(&self.identity, device_id, now_ms)?;
        self.key = self.key.next()?;
        Ok(revs)
    }

    /// The update for `device_id`: the roster and the key sealed to its
    /// newest active record.
    pub fn update_for(&self, device_id: &str) -> Result<FamilyUpdate> {
        let records = self.roster.authorize(device_id)?;
        let to = records
            .into_iter()
            .max_by_key(|m| m.added_ms)
            .expect("authorize returns at least one record");
        Ok(FamilyUpdate {
            roster: self.roster.clone(),
            key: Some(KeyEnvelope::seal(
                self.family_id(),
                &self.identity,
                to,
                &self.key,
            )?),
        })
    }

    /// Takes over an update from another member: merges the roster and
    /// adopts a newer family key. Fails with [`Error::Revoked`] when this
    /// device was revoked: the app then forgets the family.
    pub fn apply_update(&mut self, update: &FamilyUpdate) -> Result<UpdateReport> {
        let roster = self.roster.merge(&update.roster)?;
        if !self.roster.is_active(&self.identity.public().sign) {
            return Err(Error::Revoked(self.identity.device_id().to_owned()));
        }
        let mut report = UpdateReport {
            roster,
            key_rotated: false,
        };
        if let Some(env) = &update.key
            && env.epoch >= self.key.epoch
        {
            let key = env.open(&self.roster, &self.identity)?;
            // Two members may rotate at once and make different keys of the
            // same epoch; everyone keeps the one with the larger hash, so the
            // family converges as updates spread.
            let newer = key.epoch > self.key.epoch
                || (key.epoch == self.key.epoch && key_rank(&key) > key_rank(&self.key));
            if newer {
                self.key = key;
                report.key_rotated = true;
            }
        }
        Ok(report)
    }

    /// Loads the family of the identity in `store` with the roster at
    /// `roster_path`; `None` if this device has no family yet.
    pub fn load(store: &dyn SecretStore, roster_path: &Path) -> Result<Option<Self>> {
        let Some(identity) = DeviceIdentity::load(store)? else {
            return Ok(None);
        };
        let Some(roster) = Roster::load(roster_path)? else {
            return Ok(None);
        };
        let Some(secret) = store.get(&family_key_secret())? else {
            return Ok(None);
        };
        let StoredKey {
            v,
            family,
            epoch,
            key,
        } = serde_json::from_str(secret.expose())?;
        let key_hex = Zeroizing::new(key);
        if v != 1 || family != roster.family_id() {
            return Err(Error::OtherFamily);
        }
        let key = FamilyKey::from_bytes(epoch, codec::from_hex_array::<32>(&key_hex)?);
        Self::new(identity, roster, key).map(Some)
    }

    /// Saves the identity and the key into `store` and the roster to
    /// `roster_path`.
    pub fn save(&self, store: &dyn SecretStore, roster_path: &Path) -> Result<()> {
        self.identity.save(store)?;
        let key_hex = Zeroizing::new(codec::to_hex(self.key.expose()));
        let json = Zeroizing::new(serde_json::to_string(&StoredKey {
            v: 1,
            family: self.family_id().to_owned(),
            epoch: self.key.epoch,
            key: key_hex.to_string(),
        })?);
        store.set(&family_key_secret(), &SecretString::new(json.as_str()))?;
        self.roster.save(roster_path)
    }

    /// Forgets the family (after this device was revoked or left): deletes
    /// the key and the roster file; the device identity stays.
    pub fn forget(store: &dyn SecretStore, roster_path: &Path) -> Result<()> {
        store.delete(&family_key_secret())?;
        match std::fs::remove_file(roster_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn key_rank(key: &FamilyKey) -> [u8; 32] {
    let mut t = Transcript::new("kinolune.family-key.rank.v1");
    t.put(key.expose());
    codec::sha256(t.bytes())
}

fn family_key_secret() -> SecretKey {
    SecretKey::new(FAMILY_KEY_SECRET).expect("valid secret key")
}

#[cfg(test)]
mod tests {
    use pa_secrets::MemoryStore;

    use super::*;

    fn id(name: &str) -> DeviceIdentity {
        DeviceIdentity::generate(name).unwrap()
    }

    #[test]
    fn envelope_opens_only_for_its_receiver() {
        let mut fam = Family::found(id("phone"), "Phone", DeviceKind::Phone, 1).unwrap();
        let tv = id("tv");
        let pc = id("pc");
        fam.admit("tv", "TV", DeviceKind::Tv, tv.public(), 2).unwrap();
        fam.admit("pc", "PC", DeviceKind::Desktop, pc.public(), 3)
            .unwrap();
        let update = fam.update_for("tv").unwrap();
        let env = update.key.clone().unwrap();
        let key = env.open(fam.roster(), &tv).unwrap();
        assert_eq!(key.expose(), fam.key().expose());
        assert!(env.open(fam.roster(), &pc).is_err());
        let mut bad = env.clone();
        bad.sealed.replace_range(0..2, if &bad.sealed[0..2] == "00" { "01" } else { "00" });
        assert!(bad.open(fam.roster(), &tv).is_err());
        assert!(format!("{key:?}").contains("***"));
    }

    #[test]
    fn revoke_rotates_the_key_for_the_others_only() {
        let mut phone = Family::found(id("phone"), "Phone", DeviceKind::Phone, 1).unwrap();
        let tv_id = id("tv");
        let pc_id = id("pc");
        phone
            .admit("tv", "TV", DeviceKind::Tv, tv_id.public(), 2)
            .unwrap();
        phone
            .admit("pc", "PC", DeviceKind::Desktop, pc_id.public(), 3)
            .unwrap();
        let mut tv = Family::new(tv_id, phone.roster().clone(), phone.key().clone()).unwrap();
        let mut pc = Family::new(pc_id, phone.roster().clone(), phone.key().clone()).unwrap();

        phone.revoke("pc", 4).unwrap();
        assert_eq!(phone.key().epoch(), 2);
        assert!(phone.update_for("pc").is_err());
        let report = tv.apply_update(&phone.update_for("tv").unwrap()).unwrap();
        assert!(report.key_rotated);
        assert_eq!(tv.key().expose(), phone.key().expose());
        assert!(matches!(tv.authorize("pc"), Err(Error::Revoked(_))));
        // The revoked device learns it was revoked and gets no key.
        let roster_only = FamilyUpdate {
            roster: phone.roster().clone(),
            key: None,
        };
        assert!(matches!(pc.apply_update(&roster_only), Err(Error::Revoked(_))));
        assert!(phone.revoke("phone", 5).is_err());
    }

    #[test]
    fn concurrent_rotations_converge() {
        let mut a = Family::found(id("a"), "A", DeviceKind::Phone, 1).unwrap();
        let b_id = id("b");
        a.admit("b", "B", DeviceKind::Tv, b_id.public(), 2).unwrap();
        for (n, name) in ["c", "d"].into_iter().enumerate() {
            let other = id(name);
            a.admit(name, name, DeviceKind::Tv, other.public(), 3 + n as u64)
                .unwrap();
        }
        let mut b = Family::new(b_id, a.roster().clone(), a.key().clone()).unwrap();
        // A revokes C while B revokes D: two different keys of epoch 2.
        a.revoke("c", 10).unwrap();
        b.revoke("d", 10).unwrap();
        assert_eq!(a.key().epoch(), b.key().epoch());
        assert_ne!(a.key().expose(), b.key().expose());
        let (to_b, to_a) = (a.update_for("b").unwrap(), b.update_for("a").unwrap());
        a.apply_update(&to_a).unwrap();
        b.apply_update(&to_b).unwrap();
        assert_eq!(a.key().expose(), b.key().expose());
        for fam in [&a, &b] {
            assert!(fam.authorize("c").is_err());
            assert!(fam.authorize("d").is_err());
        }
    }

    #[test]
    fn family_survives_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("family.json");
        let store = MemoryStore::new();
        assert!(Family::load(&store, &path).unwrap().is_none());
        let fam = Family::found(id("server"), "Server", DeviceKind::Server, 1).unwrap();
        fam.save(&store, &path).unwrap();
        let back = Family::load(&store, &path).unwrap().unwrap();
        assert_eq!(back.roster(), fam.roster());
        assert_eq!(back.key().expose(), fam.key().expose());
        assert_eq!(back.identity().public(), fam.identity().public());
        Family::forget(&store, &path).unwrap();
        assert!(Family::load(&store, &path).unwrap().is_none());
        assert!(DeviceIdentity::load(&store).unwrap().is_some());
    }
}
