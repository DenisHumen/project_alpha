// SPDX-License-Identifier: MPL-2.0
//! Mutual authentication of a sync connection (`hello.nonce` + `auth`).
//!
//! Each side puts a fresh random [`nonce`] into its `hello`. Having the
//! peer's `hello`, a side answers `auth{sig}` with [`prove`]: an Ed25519
//! signature over both device ids and both nonces. The other side checks
//! it with [`verify`] against its roster: an unknown or revoked device, or
//! a bad signature, gets `error{unauthorized}`.
//!
//! This proves who is at the other end of the transport; it does not
//! encrypt the channel. Confidentiality and channel binding come from the
//! transport (TLS of the server, SRV-004; Noise for LAN P2P, SYNC-006).

use crate::codec::{self, Transcript};
use crate::identity::DeviceIdentity;
use crate::roster::{Member, Roster};
use crate::{Error, Result};

/// Length of a nonce in hex characters (32 random bytes).
pub const NONCE_LEN: usize = 64;

/// A fresh nonce for `hello`.
pub fn nonce() -> Result<String> {
    Ok(codec::to_hex(&codec::random::<32>()?))
}

fn transcript(
    prover: &str,
    verifier: &str,
    prover_nonce: &str,
    verifier_nonce: &str,
) -> Transcript {
    let mut t = Transcript::new("kinolune.sync.auth.v1");
    t.put_str(prover)
        .put_str(verifier)
        .put_str(prover_nonce)
        .put_str(verifier_nonce);
    t
}

/// The `auth.sig` of `identity` for a peer `verifier_id` that sent
/// `verifier_nonce`; `own_nonce` is the nonce of our own `hello`.
pub fn prove(
    identity: &DeviceIdentity,
    verifier_id: &str,
    own_nonce: &str,
    verifier_nonce: &str,
) -> String {
    let t = transcript(identity.device_id(), verifier_id, own_nonce, verifier_nonce);
    codec::to_hex(&identity.sign(t.bytes()))
}

/// Checks the `auth.sig` of `prover_id` (whose `hello` had `prover_nonce`)
/// for us (`verifier_id`, nonce `verifier_nonce`). Returns its record.
pub fn verify<'a>(
    roster: &'a Roster,
    prover_id: &str,
    verifier_id: &str,
    prover_nonce: &str,
    verifier_nonce: &str,
    sig: &str,
) -> Result<&'a Member> {
    let records = roster.authorize(prover_id)?;
    if prover_nonce.len() != NONCE_LEN || verifier_nonce.len() != NONCE_LEN {
        return Err(Error::Invalid("nonce"));
    }
    let sig = codec::from_hex_array::<64>(sig)?;
    let t = transcript(prover_id, verifier_id, prover_nonce, verifier_nonce);
    records
        .into_iter()
        .find(|m| m.keys.verify(t.bytes(), &sig, "auth").is_ok())
        .ok_or(Error::BadSignature("auth"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::DeviceKind;

    #[test]
    fn prove_and_verify() {
        let hub = DeviceIdentity::generate("hub").unwrap();
        let tv = DeviceIdentity::generate("tv").unwrap();
        let stranger = DeviceIdentity::generate("pc").unwrap();
        let mut roster = Roster::found(&hub, "Hub", DeviceKind::Server, 1).unwrap();
        roster
            .admit(&hub, "tv", "TV", DeviceKind::Tv, tv.public(), 2)
            .unwrap();
        let (n_hub, n_tv) = (nonce().unwrap(), nonce().unwrap());
        assert_ne!(n_hub, n_tv);

        let sig = prove(&tv, "hub", &n_tv, &n_hub);
        verify(&roster, "tv", "hub", &n_tv, &n_hub, &sig).unwrap();
        // Replay against a fresh nonce, a swapped role or another verifier fails.
        let fresh = nonce().unwrap();
        assert!(verify(&roster, "tv", "hub", &n_tv, &fresh, &sig).is_err());
        assert!(verify(&roster, "tv", "other", &n_tv, &n_hub, &sig).is_err());
        // The hub proves itself the same way.
        let sig = prove(&hub, "tv", &n_hub, &n_tv);
        verify(&roster, "hub", "tv", &n_hub, &n_tv, &sig).unwrap();

        let sig = prove(&stranger, "hub", &n_tv, &n_hub);
        assert!(matches!(
            verify(&roster, "pc", "hub", &n_tv, &n_hub, &sig),
            Err(Error::Unknown(_))
        ));
        // A stranger claiming the TV's id has the wrong key.
        let forged = DeviceIdentity::generate("tv").unwrap();
        let sig = prove(&forged, "hub", &n_tv, &n_hub);
        assert!(matches!(
            verify(&roster, "tv", "hub", &n_tv, &n_hub, &sig),
            Err(Error::BadSignature(_))
        ));

        roster.revoke(&hub, "tv", 3).unwrap();
        let sig = prove(&tv, "hub", &n_tv, &n_hub);
        assert!(matches!(
            verify(&roster, "tv", "hub", &n_tv, &n_hub, &sig),
            Err(Error::Revoked(_))
        ));
    }
}
