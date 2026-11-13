// SPDX-License-Identifier: MPL-2.0
//! The store trait and backend kinds.

use std::fmt;

use crate::{Result, SecretKey, SecretString};

/// Where a store keeps secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Backend {
    /// Implemented by the app through [`PlatformKeystore`](crate::PlatformKeystore):
    /// Android Keystore, iOS/tvOS Keychain.
    Platform,
    /// macOS Keychain.
    Keychain,
    /// Windows Credential Manager (DPAPI).
    CredentialManager,
    /// freedesktop Secret Service (GNOME Keyring, KWallet, KeePassXC).
    SecretService,
    /// Fallback: a file encrypted with a key from this machine. Not secure
    /// against software running as the same user; the app warns the user.
    EncryptedFile,
    /// In memory, lost on exit. Tests only.
    Memory,
}

impl Backend {
    /// Whether the backend is an OS-protected storage.
    pub fn is_secure(self) -> bool {
        !matches!(self, Self::EncryptedFile | Self::Memory)
    }

    /// Whether secrets survive a restart of the app.
    pub fn is_persistent(self) -> bool {
        !matches!(self, Self::Memory)
    }

    /// Stable lowercase name for logs and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Keychain => "keychain",
            Self::CredentialManager => "credential-manager",
            Self::SecretService => "secret-service",
            Self::EncryptedFile => "encrypted-file",
            Self::Memory => "memory",
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A place to keep secrets.
///
/// Calls are blocking (a keychain may show an unlock prompt): from async code
/// call them through `spawn_blocking`.
pub trait SecretStore: Send + Sync + fmt::Debug {
    /// The secret, `None` if it is not set.
    fn get(&self, key: &SecretKey) -> Result<Option<SecretString>>;

    /// Stores or replaces the secret.
    fn set(&self, key: &SecretKey, value: &SecretString) -> Result<()>;

    /// Deletes the secret; returns whether it existed.
    fn delete(&self, key: &SecretKey) -> Result<bool>;

    /// Which backend keeps the secrets.
    fn backend(&self) -> Backend;
}
