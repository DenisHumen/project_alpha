// SPDX-License-Identifier: MPL-2.0
//! Device keys: an Ed25519 key to sign and an X25519 key to agree on
//! secrets, kept in the [`SecretStore`] of the device.

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use pa_secrets::{SecretKey, SecretStore, SecretString};
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey as DhPublic, StaticSecret};
use zeroize::Zeroizing;

use crate::codec::{self, hex_array};
use crate::{Error, Result};

/// Name of the device identity in the secret store.
pub const IDENTITY_SECRET: &str = "pairing.identity";

/// Public keys of a device: what the family knows about it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PublicKeys {
    /// Ed25519 verifying key; it also names the device record.
    #[serde(with = "hex_array")]
    pub sign: [u8; 32],
    /// X25519 public key for sealing the family key to this device.
    #[serde(with = "hex_array")]
    pub dh: [u8; 32],
}

impl PublicKeys {
    /// Short fingerprint for people to compare on two screens:
    /// `xxxx-xxxx-xxxx-xxxx-xxxx` (80 bits of SHA-256 of the signing key).
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.sign)
    }

    /// Verifies an Ed25519 signature by this device (strict: no malleable
    /// or small-order forms).
    pub(crate) fn verify(&self, msg: &[u8], sig: &[u8; 64], what: &'static str) -> Result<()> {
        verify(&self.sign, msg, sig, what)
    }
}

impl fmt::Debug for PublicKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKeys({})", self.fingerprint())
    }
}

/// Fingerprint of a signing key, see [`PublicKeys::fingerprint`].
pub fn fingerprint(sign: &[u8; 32]) -> String {
    let hex = codec::to_hex(&codec::sha256(sign)[..10]);
    hex.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).expect("hex is ascii"))
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) fn verify(key: &[u8; 32], msg: &[u8], sig: &[u8; 64], what: &'static str) -> Result<()> {
    let key = VerifyingKey::from_bytes(key).map_err(|_| Error::Invalid("public key"))?;
    key.verify_strict(msg, &Signature::from_bytes(sig))
        .map_err(|_| Error::BadSignature(what))
}

#[derive(Serialize, Deserialize)]
struct Stored {
    v: u8,
    device_id: String,
    sign: String,
    dh: String,
}

/// The private keys of this device. `Debug` shows only the fingerprint;
/// the keys are zeroed on drop.
pub struct DeviceIdentity {
    device_id: String,
    sign: SigningKey,
    dh: StaticSecret,
}

impl DeviceIdentity {
    /// Fresh random keys for `device_id`.
    pub fn generate(device_id: impl Into<String>) -> Result<Self> {
        let sign = Zeroizing::new(codec::random::<32>()?);
        let dh = Zeroizing::new(codec::random::<32>()?);
        Self::from_seeds(device_id.into(), &sign, &dh)
    }

    fn from_seeds(device_id: String, sign: &[u8; 32], dh: &[u8; 32]) -> Result<Self> {
        if !codec::is_valid_device_id(&device_id) {
            return Err(Error::Invalid("device id"));
        }
        Ok(Self {
            device_id,
            sign: SigningKey::from_bytes(sign),
            dh: StaticSecret::from(*dh),
        })
    }

    /// The identity saved in `store`, if any.
    pub fn load(store: &dyn SecretStore) -> Result<Option<Self>> {
        let Some(secret) = store.get(&secret_key())? else {
            return Ok(None);
        };
        let Stored {
            v,
            device_id,
            sign,
            dh,
        } = serde_json::from_str(secret.expose())?;
        let (sign_hex, dh_hex) = (Zeroizing::new(sign), Zeroizing::new(dh));
        if v != 1 {
            return Err(Error::Invalid("identity version"));
        }
        let sign = Zeroizing::new(codec::from_hex_array::<32>(&sign_hex)?);
        let dh = Zeroizing::new(codec::from_hex_array::<32>(&dh_hex)?);
        Self::from_seeds(device_id, &sign, &dh).map(Some)
    }

    /// Saves the private keys into `store` (replacing older ones).
    pub fn save(&self, store: &dyn SecretStore) -> Result<()> {
        let sign = Zeroizing::new(codec::to_hex(self.sign.as_bytes()));
        let dh = Zeroizing::new(codec::to_hex(self.dh.as_bytes()));
        let json = Zeroizing::new(serde_json::to_string(&Stored {
            v: 1,
            device_id: self.device_id.clone(),
            sign: sign.to_string(),
            dh: dh.to_string(),
        })?);
        store.set(&secret_key(), &SecretString::new(json.as_str()))?;
        Ok(())
    }

    /// The saved identity of `device_id`, or new keys saved into `store`.
    /// Fails if the store holds an identity of another device id.
    pub fn load_or_create(store: &dyn SecretStore, device_id: &str) -> Result<Self> {
        match Self::load(store)? {
            Some(identity) if identity.device_id == device_id => Ok(identity),
            Some(identity) => Err(Error::IdentityMismatch {
                stored: identity.device_id,
                expected: device_id.to_owned(),
            }),
            None => {
                let identity = Self::generate(device_id)?;
                identity.save(store)?;
                Ok(identity)
            }
        }
    }

    /// Deletes the saved identity (leaving the family for good).
    pub fn delete(store: &dyn SecretStore) -> Result<bool> {
        Ok(store.delete(&secret_key())?)
    }

    /// Device id the keys belong to.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Public keys.
    pub fn public(&self) -> PublicKeys {
        PublicKeys {
            sign: self.sign.verifying_key().to_bytes(),
            dh: DhPublic::from(&self.dh).to_bytes(),
        }
    }

    /// Ed25519 signature of `msg`.
    pub(crate) fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.sign.sign(msg).to_bytes()
    }

    /// X25519 agreement with `peer`. Fails on a low-order peer key.
    pub(crate) fn agree(&self, peer: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>> {
        let shared = self.dh.diffie_hellman(&DhPublic::from(*peer));
        if !shared.was_contributory() {
            return Err(Error::Invalid("peer key"));
        }
        Ok(Zeroizing::new(shared.to_bytes()))
    }
}

impl fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceIdentity")
            .field("device_id", &self.device_id)
            .field("fingerprint", &self.public().fingerprint())
            .finish()
    }
}

fn secret_key() -> SecretKey {
    SecretKey::new(IDENTITY_SECRET).expect("valid secret key")
}

#[cfg(test)]
mod tests {
    use pa_secrets::MemoryStore;

    use super::*;

    #[test]
    fn identity_survives_the_store() {
        let store = MemoryStore::new();
        let a = DeviceIdentity::load_or_create(&store, "phone").unwrap();
        let b = DeviceIdentity::load_or_create(&store, "phone").unwrap();
        assert_eq!(a.public(), b.public());
        assert!(matches!(
            DeviceIdentity::load_or_create(&store, "tv"),
            Err(Error::IdentityMismatch { .. })
        ));
        assert!(DeviceIdentity::delete(&store).unwrap());
        let c = DeviceIdentity::load_or_create(&store, "tv").unwrap();
        assert_ne!(a.public(), c.public());
    }

    #[test]
    fn debug_shows_no_secret() {
        let id = DeviceIdentity::generate("phone").unwrap();
        let text = format!("{id:?}");
        assert!(text.contains("phone"));
        assert!(!text.contains(&codec::to_hex(id.sign.as_bytes())));
        assert_eq!(id.public().fingerprint().len(), 24);
    }

    #[test]
    fn signatures_and_agreement() {
        let a = DeviceIdentity::generate("a").unwrap();
        let b = DeviceIdentity::generate("b").unwrap();
        let sig = a.sign(b"hello");
        a.public().verify(b"hello", &sig, "test").unwrap();
        assert!(b.public().verify(b"hello", &sig, "test").is_err());
        assert!(a.public().verify(b"hellO", &sig, "test").is_err());
        assert_eq!(
            *a.agree(&b.public().dh).unwrap(),
            *b.agree(&a.public().dh).unwrap()
        );
        assert!(a.agree(&[0u8; 32]).is_err());
        assert!(DeviceIdentity::generate("bad id").is_err());
    }
}
