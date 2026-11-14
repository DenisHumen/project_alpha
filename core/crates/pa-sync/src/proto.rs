// SPDX-License-Identifier: MPL-2.0
//! Wire protocol of sync (SYNC-002); the spec is `docs/api/sync-protocol.md`.
//!
//! One protocol for every transport (LAN P2P, the user's own server, iroh):
//! a connection carries [`Message`]s, one JSON object per frame (WebSocket
//! text frame, QUIC stream message or line). The code here is sans-IO: it
//! encodes, decodes, validates and negotiates; transports only move strings.
//!
//! - [`Message::Hello`] opens a connection; both sides send it and agree on a
//!   version with [`negotiate`] (this build talks to builds ±1 version).
//! - `pb.update` / `pb.takeover` carry the ephemeral "now playing" state for
//!   instant resume; they are fanned out and never stored in a journal.
//! - `op` carries a durable [`Op`] as an opaque, versioned [`Payload`], so a
//!   hub stores and forwards ops it cannot read (newer schema, E2E later).
//! - `ack`, `change`, `pull`, `pull.end` move ops between an outbox and a
//!   hub [`Journal`] with per-journal cursors.
//! - `hello.nonce` and [`Message::Auth`] authenticate paired devices
//!   (SYNC-005, `pa-pairing`); [`Message::Family`] carries roster updates
//!   and the rotated family key between members.

use std::collections::HashMap;

use pa_model::MediaKey;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::hlc::{Hlc, is_valid_device_id};
use crate::op::{Op, OpId, check_ref};
use crate::outbox;

/// Protocol version spoken by this build.
pub const PROTOCOL_VERSION: u16 = 1;
/// Oldest version this build still speaks: one below [`PROTOCOL_VERSION`].
pub const MIN_PROTOCOL_VERSION: u16 = if PROTOCOL_VERSION > 1 {
    PROTOCOL_VERSION - 1
} else {
    1
};
/// Largest frame accepted by [`decode`] and produced by [`encode`].
pub const MAX_FRAME_LEN: usize = 256 * 1024;
/// Largest [`Payload::body`].
pub const MAX_PAYLOAD_LEN: usize = 64 * 1024;
/// Largest `limit` of a [`Pull`].
pub const MAX_PULL_LIMIT: u32 = 1_000;
/// Fastest playback rate accepted in a [`PlaybackUpdate`].
pub const MAX_RATE: f64 = 16.0;
/// [`Payload::enc`] of a plain (not encrypted) payload.
pub const ENC_PLAIN: &str = "plain";
/// Longest `hello.nonce` and `auth.sig`, in characters (hex).
pub const MAX_AUTH_LEN: usize = 256;
/// Largest [`Family::body`].
pub const MAX_FAMILY_LEN: usize = 192 * 1024;

/// Error codes of [`ErrorFrame::code`].
pub mod code {
    /// No common protocol version; the connection is closed.
    pub const PROTO_UNSUPPORTED: &str = "proto.unsupported";
    /// A message other than `hello` before the handshake finished.
    pub const HANDSHAKE: &str = "proto.handshake";
    /// A frame that does not decode or validate.
    pub const BAD_MESSAGE: &str = "bad_message";
    /// The peer is not paired or was revoked (SYNC-005).
    pub const UNAUTHORIZED: &str = "unauthorized";
    /// An op the receiver rejects for good; the sender may drop it.
    pub const OP_REJECTED: &str = "op.rejected";
}

/// Range of protocol versions a side speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtoRange {
    pub min: u16,
    pub max: u16,
}

impl ProtoRange {
    /// The range of this build.
    pub const LOCAL: Self = Self::for_version(PROTOCOL_VERSION);

    /// The range a build of `version` advertises: `version - 1 ..= version`.
    pub const fn for_version(version: u16) -> Self {
        Self {
            min: if version > 1 { version - 1 } else { 1 },
            max: version,
        }
    }

    fn is_valid(self) -> bool {
        self.min >= 1 && self.min <= self.max
    }
}

/// The version both sides use: the highest one in both ranges.
pub fn negotiate(local: ProtoRange, remote: ProtoRange) -> Result<u16, Error> {
    let version = local.max.min(remote.max);
    if local.is_valid() && remote.is_valid() && version >= local.min.max(remote.min) {
        Ok(version)
    } else {
        Err(Error::Incompatible {
            local: (local.min, local.max),
            remote: (remote.min, remote.max),
        })
    }
}

/// What a side of a connection is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// An app; may also serve a journal.
    Device,
    /// Keeps a [`Journal`] and fans out messages to other devices.
    Hub,
    /// A role of a newer version; treated as `device`.
    #[serde(other)]
    Other,
}

/// Position in a hub journal: the last `seq` seen of journal `log`. A
/// cursor of another `log` (new hub, reset journal) means "from the start".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub log: String,
    pub seq: u64,
}

impl Cursor {
    fn validate(&self) -> Result<(), Error> {
        is_valid_device_id(&self.log)
            .then_some(())
            .ok_or(Error::Invalid("journal id"))
    }
}

/// First message of each side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub proto: ProtoRange,
    pub device_id: String,
    pub role: Role,
    /// What the sender has seen of the receiver's journal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Head of the sender's own journal, if it keeps one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<Cursor>,
    /// Fresh random challenge (hex) for the peer's [`Auth`]; set by paired
    /// devices and hubs (SYNC-005).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

/// `auth`: proof that the sender holds the key of its paired device: an
/// Ed25519 signature (hex) over both device ids and both `hello.nonce`s
/// (`pa_pairing::auth`). Sent right after receiving the peer's `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Auth {
    pub sig: String,
}

/// `family`: a roster update and the family key sealed to the receiver
/// (`pa_pairing::FamilyUpdate` as JSON text), sent by a member after the
/// handshake and after pairing or revoking a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Family {
    pub body: String,
}

/// Playback state in a [`PlaybackUpdate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
    /// A state of a newer version; treated as `paused`.
    #[serde(other)]
    Other,
}

fn one() -> f64 {
    1.0
}

fn is_one(rate: &f64) -> bool {
    (*rate - 1.0).abs() < f64::EPSILON
}

/// `pb.update`: what a device is playing now. Sent every 5 s while playing
/// and at once on pause, seek, stop and going to background (SYNC-003).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackUpdate {
    pub profile: String,
    pub key: MediaKey,
    pub position_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub rate: f64,
    pub state: PlayState,
    /// When the state was sampled; `hlc.device` is the playing device.
    pub hlc: Hlc,
}

/// `pb.takeover`: `hlc.device` started playing `key`; other devices of the
/// same profile pause it (if the setting allows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Takeover {
    pub profile: String,
    pub key: MediaKey,
    pub hlc: Hlc,
}

/// Opaque, versioned body of an op. With `enc: "plain"` the body is the
/// [`outbox::encode`] JSON (`{"schema_version":…,"data":{op}}`); other values
/// are reserved for encryption (E2E, SYNC-010) and are relayed unread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    pub enc: String,
    pub body: String,
}

impl Payload {
    /// A plain payload of `op`.
    pub fn plain(op: &Op) -> Result<Self, Error> {
        Ok(Self {
            enc: ENC_PLAIN.to_owned(),
            body: outbox::encode(op)?,
        })
    }

    /// The op inside. Fails on an unknown `enc` or schema version.
    pub fn open(&self) -> Result<Op, Error> {
        if self.enc != ENC_PLAIN {
            return Err(Error::Invalid("payload encryption"));
        }
        outbox::decode(&self.body)
    }
}

/// `op`: one durable op. `seq` is set by a hub when the op comes from its
/// journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpFrame {
    pub op_id: OpId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    pub payload: Payload,
}

impl OpFrame {
    /// A frame with a plain payload of `op`.
    pub fn plain(op: &Op) -> Result<Self, Error> {
        Ok(Self {
            op_id: op.op_id,
            seq: None,
            payload: Payload::plain(op)?,
        })
    }

    /// The op inside; its id must match `op_id`.
    pub fn open(&self) -> Result<Op, Error> {
        let op = self.payload.open()?;
        if op.op_id != self.op_id {
            return Err(Error::Invalid("op id"));
        }
        Ok(op)
    }
}

/// `ack`: the receiver stored the op durably; the sender removes it from
/// its outbox ([`outbox::ack`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    pub op_id: OpId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

/// `change`: the hub journal grew to `head`; pull if behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub head: Cursor,
}

/// `pull`: send ops after `cursor` (all ops if `None` or another journal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pull {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

/// `pull.end`: the reply to a pull is complete; `cursor` is the new cursor,
/// `more` asks for another pull.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullEnd {
    pub cursor: Cursor,
    pub more: bool,
}

/// `error`: see [`code`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorFrame {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_id: Option<OpId>,
}

/// One protocol message; the `t` field names it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Message {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "pb.update")]
    PbUpdate(PlaybackUpdate),
    #[serde(rename = "pb.takeover")]
    PbTakeover(Takeover),
    #[serde(rename = "op")]
    Op(OpFrame),
    #[serde(rename = "ack")]
    Ack(Ack),
    #[serde(rename = "change")]
    Change(Change),
    #[serde(rename = "pull")]
    Pull(Pull),
    #[serde(rename = "pull.end")]
    PullEnd(PullEnd),
    #[serde(rename = "error")]
    Error(ErrorFrame),
    #[serde(rename = "auth")]
    Auth(Auth),
    #[serde(rename = "family")]
    Family(Family),
    /// A message of a newer version; receivers ignore it. Never encoded.
    #[serde(other)]
    Unknown,
}

fn check_hlc(hlc: &Hlc) -> Result<(), Error> {
    is_valid_device_id(&hlc.device)
        .then_some(())
        .ok_or(Error::Invalid("device id"))
}

fn check_auth_text(text: &str, what: &'static str) -> Result<(), Error> {
    let ok = !text.is_empty()
        && text.len() <= MAX_AUTH_LEN
        && text.bytes().all(|b| b.is_ascii_hexdigit());
    ok.then_some(()).ok_or(Error::Invalid(what))
}

fn check_key(key: &MediaKey) -> Result<(), Error> {
    key.is_valid()
        .then_some(())
        .ok_or(Error::Invalid("media key"))
}

impl Message {
    /// Checks ids, ranges and sizes. [`encode`] and [`decode`] call it.
    pub fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Hello(h) => {
                if !h.proto.is_valid() {
                    return Err(Error::Invalid("protocol range"));
                }
                if !is_valid_device_id(&h.device_id) {
                    return Err(Error::Invalid("device id"));
                }
                if let Some(nonce) = &h.nonce {
                    check_auth_text(nonce, "nonce")?;
                }
                h.cursor
                    .iter()
                    .chain(&h.head)
                    .try_for_each(Cursor::validate)
            }
            Self::PbUpdate(u) => {
                check_ref(&u.profile, "profile")?;
                check_key(&u.key)?;
                check_hlc(&u.hlc)?;
                if !(u.rate.is_finite() && u.rate > 0.0 && u.rate <= MAX_RATE) {
                    return Err(Error::Invalid("rate"));
                }
                Ok(())
            }
            Self::PbTakeover(t) => {
                check_ref(&t.profile, "profile")?;
                check_key(&t.key)?;
                check_hlc(&t.hlc)
            }
            Self::Op(f) => {
                let p = &f.payload;
                if p.enc.is_empty() || p.enc.len() > 32 || p.body.len() > MAX_PAYLOAD_LEN {
                    return Err(Error::Invalid("payload"));
                }
                Ok(())
            }
            Self::Ack(_) | Self::Unknown => Ok(()),
            Self::Change(c) => c.head.validate(),
            Self::Pull(p) => {
                if !(1..=MAX_PULL_LIMIT).contains(&p.limit) {
                    return Err(Error::Invalid("pull limit"));
                }
                p.cursor.as_ref().map_or(Ok(()), Cursor::validate)
            }
            Self::PullEnd(e) => e.cursor.validate(),
            Self::Auth(a) => check_auth_text(&a.sig, "auth signature"),
            Self::Family(f) => {
                if f.body.is_empty() || f.body.len() > MAX_FAMILY_LEN {
                    return Err(Error::Invalid("family update"));
                }
                Ok(())
            }
            Self::Error(e) => {
                if e.code.is_empty() || e.code.len() > 64 {
                    return Err(Error::Invalid("error code"));
                }
                match &e.message {
                    Some(m) if m.len() > 1024 => Err(Error::Invalid("error message")),
                    _ => Ok(()),
                }
            }
        }
    }
}

/// Encodes a message as one frame.
pub fn encode(msg: &Message) -> Result<String, Error> {
    if matches!(msg, Message::Unknown) {
        return Err(Error::Invalid("message type"));
    }
    msg.validate()?;
    let frame = serde_json::to_string(msg)?;
    if frame.len() > MAX_FRAME_LEN {
        return Err(Error::Invalid("frame size"));
    }
    Ok(frame)
}

/// Decodes and validates one frame. Unknown message types give
/// [`Message::Unknown`], unknown fields are ignored.
pub fn decode(frame: &str) -> Result<Message, Error> {
    if frame.len() > MAX_FRAME_LEN {
        return Err(Error::Invalid("frame size"));
    }
    let msg: Message = serde_json::from_str(frame)?;
    msg.validate()?;
    Ok(msg)
}

/// In-memory hub journal: ops in arrival order with `seq` from 1, deduplicated
/// by `op_id`. The reference for hub implementations (SYNC-006, SYNC-008),
/// which persist the same structure.
#[derive(Debug, Clone)]
pub struct Journal {
    log: String,
    frames: Vec<OpFrame>,
    seqs: HashMap<OpId, u64>,
}

impl Journal {
    /// An empty journal with id `log` (a fresh UUID per journal).
    pub fn new(log: impl Into<String>) -> Result<Self, Error> {
        let head = Cursor {
            log: log.into(),
            seq: 0,
        };
        head.validate()?;
        Ok(Self {
            log: head.log,
            frames: Vec::new(),
            seqs: HashMap::new(),
        })
    }

    /// The cursor after the last op.
    pub fn head(&self) -> Cursor {
        Cursor {
            log: self.log.clone(),
            seq: self.frames.len() as u64,
        }
    }

    /// Journal id.
    pub fn log(&self) -> &str {
        &self.log
    }

    /// Stored frames in `seq` order.
    pub fn iter(&self) -> impl Iterator<Item = &OpFrame> {
        self.frames.iter()
    }

    /// Stores an op frame. Returns its `seq` and whether it is new; a
    /// duplicate keeps its first `seq` (so the ack is the same).
    pub fn append(&mut self, frame: &OpFrame) -> (u64, bool) {
        if let Some(&seq) = self.seqs.get(&frame.op_id) {
            return (seq, false);
        }
        let seq = self.frames.len() as u64 + 1;
        self.frames.push(OpFrame {
            seq: Some(seq),
            ..frame.clone()
        });
        self.seqs.insert(frame.op_id, seq);
        (seq, true)
    }

    /// Answers a [`Pull`]: up to `limit` frames after the cursor and the
    /// closing [`PullEnd`].
    pub fn pull(&self, pull: &Pull) -> (Vec<OpFrame>, PullEnd) {
        let after = match &pull.cursor {
            Some(c) if c.log == self.log => usize::try_from(c.seq).unwrap_or(usize::MAX),
            _ => 0,
        };
        let limit = pull.limit.clamp(1, MAX_PULL_LIMIT) as usize;
        let frames: Vec<OpFrame> = self
            .frames
            .iter()
            .skip(after)
            .take(limit)
            .cloned()
            .collect();
        let seq = frames
            .last()
            .and_then(|f| f.seq)
            .unwrap_or_else(|| (after as u64).min(self.frames.len() as u64));
        let more = seq < self.frames.len() as u64;
        let end = PullEnd {
            cursor: Cursor {
                log: self.log.clone(),
                seq,
            },
            more,
        };
        (frames, end)
    }
}
