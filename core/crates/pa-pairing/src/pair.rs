// SPDX-License-Identifier: MPL-2.0
//! Pairing a new device into the family, sans-IO.
//!
//! A member (the *inviter*: the server, a desktop, a TV) shows a 6-digit
//! code or a QR code; the new device (the *joiner*) types the code or scans
//! the QR. Both run SPAKE2 (RustCrypto `spake2`, Ed25519 group) with the
//! code as the password over any channel in the LAN, so a passive listener
//! learns nothing and an active attacker gets one guess per attempt; the
//! inviter allows [`MAX_ATTEMPTS`] attempts within [`INVITE_TTL_MS`].
//!
//! ```text
//! joiner                                   inviter
//!   pair.hello{spake}         ⇄            pair.hello{spake}      (any order)
//!   pair.intro{sealed: device, keys, proof}  →
//!                                          ←  pair.welcome{sealed: roster, family key, proof}
//! ```
//!
//! `pair.intro` and `pair.welcome` are ChaCha20-Poly1305 boxes under keys
//! derived (HKDF-SHA256) from the SPAKE2 key and the transcript; a wrong
//! code shows up as an intro that does not decrypt. Each side also signs
//! the transcript hash with its device key, which binds the keys it
//! presents to this session. A QR code carries the inviter's signing key
//! too, and the joiner checks it.

use std::fmt;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use zeroize::Zeroizing;

use crate::codec::{self, Transcript, hex_array};
use crate::family::{Family, FamilyKey};
use crate::identity::{self, DeviceIdentity, PublicKeys};
use crate::roster::{self, DeviceKind, Member, Roster};
use crate::{Error, Result};

/// Version of the pairing messages.
pub const PAIR_VERSION: u16 = 1;
/// How long an invitation is valid.
pub const INVITE_TTL_MS: u64 = 5 * 60 * 1000;
/// Pairing attempts per invitation (each attempt is one guess of the code).
pub const MAX_ATTEMPTS: u32 = 5;
/// Largest pairing frame.
pub const MAX_FRAME_LEN: usize = 256 * 1024;
/// Scheme and path of the QR payload.
pub const QR_PREFIX: &str = "projectalpha://pair?";

/// Error codes of [`PairMessage::Error`].
pub mod code {
    /// The code was wrong.
    pub const WRONG_CODE: &str = "pair.code";
    /// No valid invitation (none, expired or used up).
    pub const EXPIRED: &str = "pair.expired";
    /// The inviter refused the device.
    pub const DENIED: &str = "pair.denied";
    /// A frame that does not decode or comes out of order.
    pub const BAD_MESSAGE: &str = "bad_message";
}

const ID_JOINER: &[u8] = b"kinolune.pair.joiner";
const ID_INVITER: &[u8] = b"kinolune.pair.inviter";

/// A pairing code: 6 digits to type or 32 hex characters in a QR code.
/// `Debug` hides it; zeroed on drop.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingCode(Zeroizing<String>);

impl PairingCode {
    /// A random 6-digit code (uniform).
    pub fn digits() -> Result<Self> {
        loop {
            let n = u32::from_be_bytes(codec::random::<4>()?);
            // Reject the top of the range so `% 1_000_000` is uniform.
            if n < 4_294_000_000 {
                return Ok(Self(Zeroizing::new(format!("{:06}", n % 1_000_000))));
            }
        }
    }

    /// A random 128-bit code for a QR code.
    pub fn long() -> Result<Self> {
        Ok(Self(Zeroizing::new(codec::to_hex(&codec::random::<16>()?))))
    }

    /// Parses what a person typed: spaces and dashes are ignored.
    pub fn parse(text: &str) -> Result<Self> {
        let code: String = text
            .chars()
            .filter(|c| !matches!(c, ' ' | '-'))
            .flat_map(char::to_lowercase)
            .collect();
        let digits = code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
        let long = code.len() == 32 && codec::from_hex(&code, 16).is_ok();
        if digits || long {
            Ok(Self(Zeroizing::new(code)))
        } else {
            Err(Error::Invalid("pairing code"))
        }
    }

    /// The code text, to show on screen. Do not log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingCode(***)")
    }
}

/// An open invitation of the inviter.
#[derive(Debug, Clone)]
pub struct Invitation {
    code: PairingCode,
    expires_ms: u64,
    attempts_left: u32,
}

impl Invitation {
    /// An invitation with a new 6-digit code.
    pub fn digits(now_ms: u64) -> Result<Self> {
        Ok(Self::with_code(PairingCode::digits()?, now_ms))
    }

    /// An invitation with a long code for a QR code.
    pub fn long(now_ms: u64) -> Result<Self> {
        Ok(Self::with_code(PairingCode::long()?, now_ms))
    }

    /// An invitation with `code`.
    pub fn with_code(code: PairingCode, now_ms: u64) -> Self {
        Self {
            code,
            expires_ms: now_ms.saturating_add(INVITE_TTL_MS),
            attempts_left: MAX_ATTEMPTS,
        }
    }

    /// The code to show.
    pub fn code(&self) -> &PairingCode {
        &self.code
    }

    /// When it expires, Unix ms.
    pub fn expires_ms(&self) -> u64 {
        self.expires_ms
    }

    /// Attempts left.
    pub fn attempts_left(&self) -> u32 {
        self.attempts_left
    }

    /// Whether another attempt may start at `now_ms`.
    pub fn is_open(&self, now_ms: u64) -> bool {
        self.attempts_left > 0 && now_ms < self.expires_ms
    }

    /// The QR payload for this invitation: see [`QrInvite`].
    pub fn qr(&self, inviter: &PublicKeys, addr: Option<&str>) -> String {
        QrInvite {
            code: self.code.clone(),
            inviter: inviter.sign,
            addr: addr.map(str::to_owned),
        }
        .to_uri()
    }

    fn take_attempt(&mut self, now_ms: u64) -> Result<()> {
        if !self.is_open(now_ms) {
            return Err(Error::Expired);
        }
        self.attempts_left -= 1;
        Ok(())
    }
}

/// What a QR code tells the joiner:
/// `projectalpha://pair?v=1&c=<code>&k=<inviter key>[&a=<host:port>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrInvite {
    pub code: PairingCode,
    /// The inviter's signing key; the joiner refuses another one.
    pub inviter: [u8; 32],
    /// Where the inviter listens, if it is reachable over the network.
    pub addr: Option<String>,
}

fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b':') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode_component(s: &str) -> Result<String> {
    let raw = s.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or(Error::Invalid("qr"))?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| Error::Invalid("qr"))?);
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::Invalid("qr"))
}

impl QrInvite {
    /// The URI to put into the QR code.
    pub fn to_uri(&self) -> String {
        let mut uri = format!(
            "{QR_PREFIX}v={PAIR_VERSION}&c={}&k={}",
            self.code.expose(),
            codec::to_hex(&self.inviter)
        );
        if let Some(addr) = &self.addr {
            uri.push_str("&a=");
            uri.push_str(&encode_component(addr));
        }
        uri
    }

    /// Parses a scanned QR code. Unknown parameters are ignored.
    pub fn parse(uri: &str) -> Result<Self> {
        let query = uri.strip_prefix(QR_PREFIX).ok_or(Error::Invalid("qr"))?;
        let (mut v, mut code, mut key, mut addr) = (None, None, None, None);
        for pair in query.split('&') {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            match name {
                "v" => v = Some(value),
                "c" => code = Some(PairingCode::parse(value)?),
                "k" => key = Some(codec::from_hex_array::<32>(value)?),
                "a" => addr = Some(decode_component(value)?),
                _ => {}
            }
        }
        if v != Some("1") {
            return Err(Error::Invalid("qr version"));
        }
        Ok(Self {
            code: code.ok_or(Error::Invalid("qr"))?,
            inviter: key.ok_or(Error::Invalid("qr"))?,
            addr,
        })
    }
}

/// A pairing message; one JSON object per frame, `t` names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum PairMessage {
    /// SPAKE2 message (hex).
    #[serde(rename = "pair.hello")]
    Hello { v: u16, spake: String },
    /// The joiner's device, sealed (hex).
    #[serde(rename = "pair.intro")]
    Intro { sealed: String },
    /// The roster and the family key, sealed (hex).
    #[serde(rename = "pair.welcome")]
    Welcome { sealed: String },
    /// See [`code`].
    #[serde(rename = "pair.error")]
    Error {
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

impl PairMessage {
    /// An error message.
    pub fn error(code: &str, message: Option<&str>) -> Self {
        Self::Error {
            code: code.to_owned(),
            message: message.map(str::to_owned),
        }
    }

    /// One frame.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("pair message serializes")
    }

    /// Parses one frame.
    pub fn decode(frame: &str) -> Result<Self> {
        if frame.len() > MAX_FRAME_LEN {
            return Err(Error::Invalid("frame size"));
        }
        Ok(serde_json::from_str(frame)?)
    }
}

/// What the joiner tells about itself (inside `pair.intro`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRequest {
    pub device_id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub keys: PublicKeys,
}

#[derive(Serialize, Deserialize)]
struct Intro {
    #[serde(flatten)]
    device: JoinRequest,
    #[serde(with = "hex_array")]
    proof: [u8; 64],
}

#[derive(Serialize, Deserialize)]
struct Welcome {
    roster: Roster,
    epoch: u32,
    key: String,
    #[serde(with = "hex_array")]
    inviter: [u8; 32],
    #[serde(with = "hex_array")]
    proof: [u8; 64],
}

/// Session keys after SPAKE2.
struct Session {
    hash: [u8; 32],
    joiner: Zeroizing<[u8; 32]>,
    inviter: Zeroizing<[u8; 32]>,
}

impl Session {
    fn derive(spake_key: &[u8], joiner_msg: &[u8], inviter_msg: &[u8]) -> Self {
        let mut t = Transcript::new("kinolune.pair.v1");
        t.put(joiner_msg).put(inviter_msg);
        let hash = codec::sha256(t.bytes());
        let hk = Hkdf::<Sha256>::new(Some(&hash), spake_key);
        let mut joiner = Zeroizing::new([0u8; 32]);
        let mut inviter = Zeroizing::new([0u8; 32]);
        hk.expand(b"kinolune.pair.joiner", joiner.as_mut())
            .expect("valid HKDF length");
        hk.expand(b"kinolune.pair.inviter", inviter.as_mut())
            .expect("valid HKDF length");
        Self {
            hash,
            joiner,
            inviter,
        }
    }

    fn proof_bytes(&self, what: &str) -> Vec<u8> {
        let mut t = Transcript::new(what);
        t.put(&self.hash);
        t.bytes().to_vec()
    }
}

// Each key seals exactly one message, so a fixed nonce is safe.
fn seal(key: &[u8; 32], hash: &[u8; 32], plain: &[u8]) -> String {
    let sealed = ChaCha20Poly1305::new_from_slice(key)
        .expect("32-byte key")
        .encrypt(
            &Nonce::default(),
            Payload {
                msg: plain,
                aad: hash,
            },
        )
        .expect("encryption of a small message");
    codec::to_hex(&sealed)
}

fn open(key: &[u8; 32], hash: &[u8; 32], sealed: &str) -> Result<Zeroizing<Vec<u8>>> {
    let sealed = codec::from_hex(sealed, MAX_FRAME_LEN / 2)?;
    ChaCha20Poly1305::new_from_slice(key)
        .expect("32-byte key")
        .decrypt(
            &Nonce::default(),
            Payload {
                msg: &sealed,
                aad: hash,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::WrongCode)
}

fn spake_of(msg: &PairMessage) -> Result<Vec<u8>> {
    match msg {
        PairMessage::Hello { v, spake } => {
            if *v != PAIR_VERSION {
                return Err(Error::Invalid("pairing version"));
            }
            codec::from_hex(spake, 64)
        }
        PairMessage::Error { code, .. } => Err(refused(code)),
        _ => Err(Error::Unexpected),
    }
}

fn refused(code: &str) -> Error {
    match code {
        code::WRONG_CODE => Error::WrongCode,
        code::EXPIRED => Error::Expired,
        other => Error::Refused(other.to_owned()),
    }
}

fn hello(msg: &[u8]) -> PairMessage {
    PairMessage::Hello {
        v: PAIR_VERSION,
        spake: codec::to_hex(msg),
    }
}

/// The new device's side.
pub struct Joiner {
    spake: Option<Spake2<Ed25519Group>>,
    msg: Vec<u8>,
    request: JoinRequest,
    expect_inviter: Option<[u8; 32]>,
    session: Option<Session>,
}

/// What the joiner gets: the family to save.
#[derive(Debug)]
pub struct Joined {
    pub roster: Roster,
    pub key: FamilyKey,
    /// Device id of the inviter.
    pub inviter: String,
}

impl Joined {
    /// The family of this device.
    pub fn into_family(self, identity: DeviceIdentity) -> Result<Family> {
        Family::new(identity, self.roster, self.key)
    }
}

impl Joiner {
    /// Starts pairing with `code`. `inviter` is the key from a QR code, if
    /// the code came from one. Send the returned `pair.hello`.
    pub fn start(
        identity: &DeviceIdentity,
        name: &str,
        kind: DeviceKind,
        code: &PairingCode,
        inviter: Option<[u8; 32]>,
    ) -> Result<(Self, PairMessage)> {
        roster::check_name(name)?;
        let (spake, msg) = Spake2::<Ed25519Group>::start_a(
            &Password::new(code.expose()),
            &Identity::new(ID_JOINER),
            &Identity::new(ID_INVITER),
        );
        let out = hello(&msg);
        Ok((
            Self {
                spake: Some(spake),
                msg,
                request: JoinRequest {
                    device_id: identity.device_id().to_owned(),
                    name: name.to_owned(),
                    kind,
                    keys: identity.public(),
                },
                expect_inviter: inviter,
                session: None,
            },
            out,
        ))
    }

    /// Takes the inviter's `pair.hello`; returns the `pair.intro` to send.
    pub fn intro(&mut self, identity: &DeviceIdentity, msg: &PairMessage) -> Result<PairMessage> {
        let theirs = spake_of(msg)?;
        let spake = self.spake.take().ok_or(Error::Unexpected)?;
        let key = Zeroizing::new(
            spake
                .finish(&theirs)
                .map_err(|_| Error::Invalid("spake2"))?,
        );
        let session = Session::derive(&key, &self.msg, &theirs);
        let intro = Intro {
            device: self.request.clone(),
            proof: identity.sign(&session.proof_bytes("kinolune.pair.intro.v1")),
        };
        let plain = Zeroizing::new(serde_json::to_vec(&intro)?);
        let sealed = seal(&session.joiner, &session.hash, &plain);
        self.session = Some(session);
        Ok(PairMessage::Intro { sealed })
    }

    /// Takes the `pair.welcome` (or a `pair.error`) and checks it.
    pub fn finish(self, msg: &PairMessage) -> Result<Joined> {
        let sealed = match msg {
            PairMessage::Welcome { sealed } => sealed,
            PairMessage::Error { code, .. } => return Err(refused(code)),
            _ => return Err(Error::Unexpected),
        };
        let session = self.session.ok_or(Error::Unexpected)?;
        let plain = open(&session.inviter, &session.hash, sealed)?;
        let Welcome {
            roster,
            epoch,
            key,
            inviter,
            proof,
        } = serde_json::from_slice(&plain)?;
        let key_hex = Zeroizing::new(key);
        if let Some(expected) = self.expect_inviter
            && expected != inviter
        {
            return Err(Error::KeyMismatch);
        }
        identity::verify(
            &inviter,
            &session.proof_bytes("kinolune.pair.welcome.v1"),
            &proof,
            "welcome",
        )?;
        let host = roster
            .member(&inviter)
            .filter(|_| roster.is_active(&inviter))
            .ok_or(Error::Unexpected)?
            .device_id
            .clone();
        let me = roster
            .member(&self.request.keys.sign)
            .ok_or(Error::Unexpected)?;
        if me.device_id != self.request.device_id || !roster.is_active(&me.keys.sign) {
            return Err(Error::Unexpected);
        }
        let key = FamilyKey::from_bytes(epoch, codec::from_hex_array::<32>(&key_hex)?);
        Ok(Joined {
            roster,
            key,
            inviter: host,
        })
    }
}

/// The inviter's side of one attempt.
pub struct Inviter {
    spake: Option<Spake2<Ed25519Group>>,
    msg: Vec<u8>,
    session: Option<Session>,
}

impl Inviter {
    /// Starts an attempt on `invitation` (uses one attempt). Send the
    /// returned `pair.hello`.
    pub fn start(invitation: &mut Invitation, now_ms: u64) -> Result<(Self, PairMessage)> {
        invitation.take_attempt(now_ms)?;
        let (spake, msg) = Spake2::<Ed25519Group>::start_b(
            &Password::new(invitation.code.expose()),
            &Identity::new(ID_JOINER),
            &Identity::new(ID_INVITER),
        );
        let out = hello(&msg);
        Ok((
            Self {
                spake: Some(spake),
                msg,
                session: None,
            },
            out,
        ))
    }

    /// Takes the joiner's `pair.hello`.
    pub fn receive_hello(&mut self, msg: &PairMessage) -> Result<()> {
        let theirs = spake_of(msg)?;
        let spake = self.spake.take().ok_or(Error::Unexpected)?;
        let key = Zeroizing::new(
            spake
                .finish(&theirs)
                .map_err(|_| Error::Invalid("spake2"))?,
        );
        self.session = Some(Session::derive(&key, &theirs, &self.msg));
        Ok(())
    }

    /// Opens the joiner's `pair.intro`. [`Error::WrongCode`] if the code
    /// did not match: answer `pair.error{pair.code}`.
    pub fn open_intro(&self, msg: &PairMessage) -> Result<JoinRequest> {
        let PairMessage::Intro { sealed } = msg else {
            return Err(Error::Unexpected);
        };
        let session = self.session.as_ref().ok_or(Error::Unexpected)?;
        let plain = open(&session.joiner, &session.hash, sealed)?;
        let intro: Intro = serde_json::from_slice(&plain)?;
        if !codec::is_valid_device_id(&intro.device.device_id) {
            return Err(Error::Invalid("device id"));
        }
        roster::check_name(&intro.device.name)?;
        intro.device.keys.verify(
            &session.proof_bytes("kinolune.pair.intro.v1"),
            &intro.proof,
            "intro",
        )?;
        Ok(intro.device)
    }

    /// Admits the device into `family` and returns its record and the
    /// `pair.welcome` to send. Save the family afterwards.
    pub fn welcome(
        self,
        family: &mut Family,
        request: &JoinRequest,
        now_ms: u64,
    ) -> Result<(Member, PairMessage)> {
        let session = self.session.ok_or(Error::Unexpected)?;
        let member = family.admit(
            &request.device_id,
            &request.name,
            request.kind,
            request.keys,
            now_ms,
        )?;
        let key_hex = Zeroizing::new(codec::to_hex(family.key().expose()));
        let welcome = Welcome {
            roster: family.roster().clone(),
            epoch: family.key().epoch(),
            key: key_hex.to_string(),
            inviter: family.identity().public().sign,
            proof: family
                .identity()
                .sign(&session.proof_bytes("kinolune.pair.welcome.v1")),
        };
        let plain = Zeroizing::new(serde_json::to_vec(&welcome)?);
        let sealed = seal(&session.inviter, &session.hash, &plain);
        Ok((member, PairMessage::Welcome { sealed }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(msg: &PairMessage) -> PairMessage {
        PairMessage::decode(&msg.encode()).unwrap()
    }

    fn server() -> Family {
        Family::found(
            DeviceIdentity::generate("server").unwrap(),
            "Server",
            DeviceKind::Server,
            1,
        )
        .unwrap()
    }

    #[test]
    fn codes() {
        for _ in 0..50 {
            let c = PairingCode::digits().unwrap();
            assert_eq!(c.expose().len(), 6);
            assert!(c.expose().bytes().all(|b| b.is_ascii_digit()));
        }
        assert_eq!(PairingCode::parse("123 456").unwrap().expose(), "123456");
        assert_eq!(PairingCode::long().unwrap().expose().len(), 32);
        assert!(PairingCode::parse("12345").is_err());
        assert!(PairingCode::parse("12345x").is_err());
        assert_eq!(
            format!("{:?}", PairingCode::parse("123456").unwrap()),
            "PairingCode(***)"
        );
    }

    #[test]
    fn qr_round_trip() {
        let fam = server();
        let inv = Invitation::long(0).unwrap();
        let uri = inv.qr(&fam.identity().public(), Some("[fe80::1%eth0]:8096"));
        assert!(uri.starts_with(QR_PREFIX));
        let qr = QrInvite::parse(&uri).unwrap();
        assert_eq!(qr.code, *inv.code());
        assert_eq!(qr.inviter, fam.identity().public().sign);
        assert_eq!(qr.addr.as_deref(), Some("[fe80::1%eth0]:8096"));
        assert!(QrInvite::parse("projectalpha://pair?v=2&c=123456&k=00").is_err());
        assert!(QrInvite::parse("https://example.org/?v=1").is_err());
    }

    #[test]
    fn invitation_limits() {
        let mut inv = Invitation::digits(0).unwrap();
        assert!(Inviter::start(&mut inv, INVITE_TTL_MS).is_err());
        for _ in 0..MAX_ATTEMPTS {
            Inviter::start(&mut inv, 1).unwrap();
        }
        assert!(matches!(Inviter::start(&mut inv, 1), Err(Error::Expired)));
    }

    #[test]
    fn wrong_code_is_detected_by_the_inviter() {
        let fam = server();
        let mut inv = Invitation::with_code(PairingCode::parse("111111").unwrap(), 0);
        let phone = DeviceIdentity::generate("phone").unwrap();
        let (mut joiner, j_hello) = Joiner::start(
            &phone,
            "Phone",
            DeviceKind::Phone,
            &PairingCode::parse("111112").unwrap(),
            None,
        )
        .unwrap();
        let (mut inviter, i_hello) = Inviter::start(&mut inv, 1).unwrap();
        inviter.receive_hello(&wire(&j_hello)).unwrap();
        let intro = joiner.intro(&phone, &wire(&i_hello)).unwrap();
        assert!(matches!(
            inviter.open_intro(&wire(&intro)),
            Err(Error::WrongCode)
        ));
        let err = PairMessage::error(code::WRONG_CODE, None);
        assert!(matches!(joiner.finish(&wire(&err)), Err(Error::WrongCode)));
        assert_eq!(fam.roster().active().count(), 1);
        assert_eq!(inv.attempts_left(), MAX_ATTEMPTS - 1);
    }

    #[test]
    fn qr_key_must_match() {
        let mut fam = server();
        let mut inv = Invitation::long(0).unwrap();
        let phone = DeviceIdentity::generate("phone").unwrap();
        let (mut joiner, j_hello) = Joiner::start(
            &phone,
            "Phone",
            DeviceKind::Phone,
            inv.code(),
            Some([7; 32]),
        )
        .unwrap();
        let (mut inviter, i_hello) = Inviter::start(&mut inv, 1).unwrap();
        inviter.receive_hello(&j_hello).unwrap();
        let intro = joiner.intro(&phone, &i_hello).unwrap();
        let req = inviter.open_intro(&intro).unwrap();
        let (_, welcome) = inviter.welcome(&mut fam, &req, 2).unwrap();
        assert!(matches!(joiner.finish(&welcome), Err(Error::KeyMismatch)));
    }
}
