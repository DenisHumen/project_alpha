// SPDX-License-Identifier: MPL-2.0
//! Errors of the secret stores.
//!
//! No variant carries a secret value: messages end up in logs.

use std::io;

/// Error of a secret store.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SecretError {
    /// The key is empty, too long or has characters outside
    /// `[A-Za-z0-9._:@-]`.
    #[error("invalid secret key {0:?}")]
    InvalidKey(String),
    /// The value is longer than [`MAX_SECRET_LEN`](crate::MAX_SECRET_LEN).
    #[error("secret value is too large: {len} bytes, at most {max}")]
    TooLarge {
        /// Length of the rejected value in bytes.
        len: usize,
        /// Allowed maximum.
        max: usize,
    },
    /// No secure storage on this device (no keychain service, no platform
    /// callback) and the fallback is not allowed.
    #[error("no secret storage is available: {0}")]
    Unavailable(String),
    /// The storage exists but is locked or access was denied (the user
    /// cancelled an unlock prompt, the keyring is locked).
    #[error("secret storage is locked or access was denied: {0}")]
    Locked(String),
    /// The storage failed (D-Bus error, Keychain error code, …).
    #[error("secret storage failed: {0}")]
    Backend(String),
    /// The encrypted fallback file cannot be decrypted: it is damaged or was
    /// made on another machine.
    #[error("the encrypted secrets file is damaged or belongs to another machine")]
    Corrupted,
    /// Reading or writing the fallback file failed.
    #[error("secrets file I/O error: {0}")]
    Io(#[from] io::Error),
}
