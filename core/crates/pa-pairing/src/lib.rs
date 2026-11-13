// SPDX-License-Identifier: MPL-2.0
//! The device family of Kinolune (SYNC-005, ADR-NNNN in
//! `docs/adr/0026-device-family-pairing.md`): pairing devices without a
//! cloud account.
//!
//! - [`DeviceIdentity`]: the keys of this device (Ed25519 to sign, X25519
//!   to receive the family key), kept in the [`SecretStore`] of
//!   `pa-secrets`.
//! - [`Roster`]: the trust store. Signed device records and revocations;
//!   every member verifies it on its own and rosters merge from any member.
//! - [`pair`]: pairing by a 6-digit code or a QR code (SPAKE2), sans-IO.
//! - [`Family`]: identity + roster + the symmetric [`FamilyKey`]; revoking a
//!   device rotates the key, and the others get it in [`KeyEnvelope`]s.
//! - [`auth`]: mutual authentication of sync connections (`hello.nonce`
//!   and `auth`); unknown and revoked devices get `unauthorized`.
//!
//! Crypto is audited pure-Rust code from the dalek and RustCrypto projects:
//! `ed25519-dalek`, `x25519-dalek`, `chacha20poly1305`, `hkdf`, `sha2`, and
//! `spake2` (RustCrypto PAKEs on `curve25519-dalek`). No OpenSSL.
//!
//! ```
//! use pa_pairing::pair::{Invitation, Inviter, Joiner};
//! use pa_pairing::{DeviceIdentity, DeviceKind, Family};
//!
//! // The server founds a family and shows a 6-digit code.
//! let server = DeviceIdentity::generate("server").unwrap();
//! let mut family = Family::found(server, "NAS", DeviceKind::Server, 1).unwrap();
//! let mut invitation = Invitation::digits(1).unwrap();
//!
//! // The phone types the code; the messages go over any LAN channel.
//! let phone = DeviceIdentity::generate("phone").unwrap();
//! let (mut joiner, j_hello) =
//!     Joiner::start(&phone, "Phone", DeviceKind::Phone, invitation.code(), None).unwrap();
//! let (mut inviter, i_hello) = Inviter::start(&mut invitation, 2).unwrap();
//! inviter.receive_hello(&j_hello).unwrap();
//! let intro = joiner.intro(&phone, &i_hello).unwrap();
//! let request = inviter.open_intro(&intro).unwrap();
//! let (_, welcome) = inviter.welcome(&mut family, &request, 3).unwrap();
//! let joined = joiner.finish(&welcome).unwrap();
//! let phone_family = joined.into_family(phone).unwrap();
//!
//! assert_eq!(phone_family.key().expose(), family.key().expose());
//! assert!(family.authorize("phone").is_ok());
//! family.revoke("phone", 4).unwrap();
//! assert!(family.authorize("phone").is_err());
//! ```
//!
//! [`SecretStore`]: pa_secrets::SecretStore

pub mod auth;
mod codec;
mod error;
pub mod family;
mod identity;
pub mod pair;
mod roster;

pub use codec::is_valid_device_id;
pub use error::{Error, Result};
pub use family::{Family, FamilyKey, FamilyUpdate, KeyEnvelope, UpdateReport};
pub use identity::{DeviceIdentity, IDENTITY_SECRET, PublicKeys, fingerprint};
pub use roster::{
    Device, DeviceKind, MAX_NAME_LEN, MAX_RECORDS, Member, MergeReport, Revocation, Roster,
    check_name, family_id_of,
};
