// SPDX-License-Identifier: MPL-2.0
//! Secret storage over FFI (`pa-secrets`, CORE-007).
//!
//! Desktop: nothing to set up, the OS keychain is used (encrypted-file
//! fallback with a warning when there is none). Android, iOS and tvOS
//! implement [`PlatformSecretStore`] (Keystore + Tink, Keychain with
//! `AfterFirstUnlock`) and pass it to [`secrets_open`] right after
//! `core_init`. The fallback file lives in the data directory of the core.

use std::sync::{Arc, PoisonError, RwLock};

use pa_runtime::global;
use pa_secrets::{Backend, OpenOptions, PlatformKeystore, SecretKey, SecretString};

/// Secure storage implemented by the app (Kotlin or Swift).
///
/// Called synchronously on a core thread; may block (Keychain, Keystore).
#[uniffi::export(with_foreign)]
pub trait PlatformSecretStore: Send + Sync {
    /// The value, `None` if missing.
    fn load(&self, key: String) -> Result<Option<String>, SecretStoreError>;
    /// Stores or replaces the value.
    fn save(&self, key: String, value: String) -> Result<(), SecretStoreError>;
    /// Deletes the value; returns whether it existed.
    fn remove(&self, key: String) -> Result<bool, SecretStoreError>;
}

/// Where secrets are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SecretBackend {
    /// The app's [`PlatformSecretStore`].
    Platform,
    /// macOS Keychain.
    Keychain,
    /// Windows Credential Manager.
    CredentialManager,
    /// Linux Secret Service (GNOME Keyring, KWallet).
    SecretService,
    /// Encrypted file: no OS keychain, the app should warn the user.
    EncryptedFile,
    /// Memory only (tests).
    Memory,
}

impl From<Backend> for SecretBackend {
    fn from(b: Backend) -> Self {
        match b {
            Backend::Platform => Self::Platform,
            Backend::Keychain => Self::Keychain,
            Backend::CredentialManager => Self::CredentialManager,
            Backend::SecretService => Self::SecretService,
            Backend::EncryptedFile => Self::EncryptedFile,
            _ => Self::Memory,
        }
    }
}

/// The opened secret store.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SecretStoreInfo {
    /// Where secrets are kept.
    pub backend: SecretBackend,
    /// `false` for the encrypted-file fallback: show a warning.
    pub secure: bool,
    /// Why no secure backend was used (for the warning and logs).
    pub fallback_reason: Option<String>,
    /// Secrets moved from an old fallback file into the secure backend.
    pub migrated: u32,
    /// An unreadable fallback file was set aside; its secrets must be
    /// entered again.
    pub discarded_corrupted: bool,
}

/// Error of a secret call. Never contains a secret value.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SecretStoreError {
    /// The key is empty, too long or not `[A-Za-z0-9._:@-]`.
    #[error("invalid secret key {key:?}")]
    InvalidKey {
        /// The rejected key.
        key: String,
    },
    /// The value is too large.
    #[error("secret value is too large: {len} bytes, at most {max}")]
    TooLarge {
        /// Length in bytes.
        len: u32,
        /// Maximum in bytes.
        max: u32,
    },
    /// No secure storage and the fallback is not allowed (mobile without a
    /// [`PlatformSecretStore`]).
    #[error("no secret storage: {details}")]
    Unavailable {
        /// Details for logs.
        details: String,
    },
    /// The storage is locked or the user denied access.
    #[error("secret storage is locked: {details}")]
    Locked {
        /// Details for logs.
        details: String,
    },
    /// Any other failure.
    #[error("secret storage failed: {details}")]
    Failed {
        /// Details for logs.
        details: String,
    },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for SecretStoreError {
    fn from(err: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Failed {
            details: err.reason,
        }
    }
}

impl From<pa_secrets::SecretError> for SecretStoreError {
    fn from(err: pa_secrets::SecretError) -> Self {
        use pa_secrets::SecretError as E;
        match err {
            E::InvalidKey(key) => Self::InvalidKey { key },
            E::TooLarge { len, max } => Self::TooLarge {
                len: u32::try_from(len).unwrap_or(u32::MAX),
                max: u32::try_from(max).unwrap_or(u32::MAX),
            },
            E::Unavailable(details) => Self::Unavailable { details },
            E::Locked(details) => Self::Locked { details },
            other => Self::Failed {
                details: other.to_string(),
            },
        }
    }
}

impl From<SecretStoreError> for pa_secrets::SecretError {
    fn from(err: SecretStoreError) -> Self {
        match err {
            SecretStoreError::InvalidKey { key } => Self::InvalidKey(key),
            SecretStoreError::TooLarge { len, max } => Self::TooLarge {
                len: len as usize,
                max: max as usize,
            },
            SecretStoreError::Unavailable { details } => Self::Unavailable(details),
            SecretStoreError::Locked { details } => Self::Locked(details),
            SecretStoreError::Failed { details } => Self::Backend(details),
        }
    }
}

struct KeystoreAdapter(Arc<dyn PlatformSecretStore>);

impl PlatformKeystore for KeystoreAdapter {
    fn load(&self, key: &str) -> pa_secrets::Result<Option<String>> {
        Ok(self.0.load(key.to_owned())?)
    }
    fn save(&self, key: &str, value: &str) -> pa_secrets::Result<()> {
        Ok(self.0.save(key.to_owned(), value.to_owned())?)
    }
    fn remove(&self, key: &str) -> pa_secrets::Result<bool> {
        Ok(self.0.remove(key.to_owned())?)
    }
}

static STORE: RwLock<Option<pa_secrets::OpenedStore>> = RwLock::new(None);

fn info(opened: &pa_secrets::OpenedStore) -> SecretStoreInfo {
    SecretStoreInfo {
        backend: opened.backend().into(),
        secure: opened.backend().is_secure(),
        fallback_reason: opened.fallback_reason.clone(),
        migrated: u32::try_from(opened.migrated).unwrap_or(u32::MAX),
        discarded_corrupted: opened.discarded_corrupted,
    }
}

fn open_store(
    platform: Option<Arc<dyn PlatformSecretStore>>,
) -> Result<pa_secrets::OpenedStore, SecretStoreError> {
    let fallback_dir = global::current().ok().map(|rt| rt.dirs().data.clone());
    let mut options = OpenOptions::new(fallback_dir);
    if let Some(platform) = platform {
        options = options.with_platform(Arc::new(KeystoreAdapter(platform)));
    }
    Ok(pa_secrets::open(options)?)
}

fn store() -> Result<Arc<dyn pa_secrets::SecretStore>, SecretStoreError> {
    if let Some(opened) = STORE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        return Ok(opened.store.clone());
    }
    let mut slot = STORE.write().unwrap_or_else(PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(open_store(None)?);
    }
    Ok(slot.as_ref().expect("just set").store.clone())
}

fn key(key: String) -> Result<SecretKey, SecretStoreError> {
    Ok(SecretKey::new(key)?)
}

/// Opens the secret store, replacing the one opened before.
///
/// Mobile and tvOS pass their [`PlatformSecretStore`]; desktop may skip this
/// call (the first secret call opens the OS keychain). Call after
/// `core_init`, so that the fallback file and an old fallback file to
/// migrate are found in the data directory.
#[uniffi::export]
pub fn secrets_open(
    platform: Option<Arc<dyn PlatformSecretStore>>,
) -> Result<SecretStoreInfo, SecretStoreError> {
    let opened = open_store(platform)?;
    let result = info(&opened);
    *STORE.write().unwrap_or_else(PoisonError::into_inner) = Some(opened);
    Ok(result)
}

/// The opened store, `None` before the first secret call.
#[uniffi::export]
pub fn secrets_info() -> Option<SecretStoreInfo> {
    STORE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(info)
}

/// Closes the store; the next secret call opens it again.
#[uniffi::export]
pub fn secrets_close() {
    *STORE.write().unwrap_or_else(PoisonError::into_inner) = None;
}

/// The secret, `None` if it is not set.
#[uniffi::export]
pub fn secret_get(key: String) -> Result<Option<String>, SecretStoreError> {
    let value = store()?.get(&self::key(key)?)?;
    Ok(value.map(|v| v.expose().to_owned()))
}

/// Stores or replaces a secret.
#[uniffi::export]
pub fn secret_set(key: String, value: String) -> Result<(), SecretStoreError> {
    Ok(store()?.set(&self::key(key)?, &SecretString::new(value))?)
}

/// Deletes a secret; returns whether it existed.
#[uniffi::export]
pub fn secret_delete(key: String) -> Result<bool, SecretStoreError> {
    Ok(store()?.delete(&self::key(key)?)?)
}

/// Masks secrets in a log line (known secret values, `password=…`,
/// `Bearer …`, passwords in URLs). Platform loggers call it before writing.
#[uniffi::export]
pub fn redact_secrets(text: String) -> String {
    pa_secrets::redact(&text)
}
