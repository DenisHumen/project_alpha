// SPDX-License-Identifier: MPL-2.0
//! Errors of pairing and authentication.

/// Error of pa-pairing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A field, id or encoding is not acceptable.
    #[error("invalid {0}")]
    Invalid(&'static str),
    /// A signature does not verify.
    #[error("bad signature of {0}")]
    BadSignature(&'static str),
    /// The device is not in the family.
    #[error("device {0} is not paired")]
    Unknown(String),
    /// The device was revoked.
    #[error("device {0} was revoked")]
    Revoked(String),
    /// The pairing code did not match (the SPAKE2 keys differ).
    #[error("wrong pairing code")]
    WrongCode,
    /// The invitation expired or has no attempts left.
    #[error("pairing invitation expired or used up")]
    Expired,
    /// A pairing message out of order.
    #[error("unexpected pairing message")]
    Unexpected,
    /// The peer refused pairing (`pair.error` with this code).
    #[error("pairing refused: {0}")]
    Refused(String),
    /// Records of another family.
    #[error("records of another family")]
    OtherFamily,
    /// The inviter's key does not match the key in the QR code.
    #[error("inviter key does not match the QR code")]
    KeyMismatch,
    /// The stored identity belongs to another device id.
    #[error("stored device identity is for {stored}, not {expected}")]
    IdentityMismatch { stored: String, expected: String },
    /// The OS random generator failed.
    #[error("random generator: {0}")]
    Random(String),
    #[error(transparent)]
    Secret(#[from] pa_secrets::SecretError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Shorthand for results of this crate.
pub type Result<T> = std::result::Result<T, Error>;
