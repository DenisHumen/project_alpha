// SPDX-License-Identifier: MPL-2.0
//! Desktop OS keychains through `keyring-core`.

use std::fmt;
use std::sync::Arc;

use keyring_core::{CredentialStore, Error as KeyringError};

use crate::{Backend, Result, SecretError, SecretKey, SecretStore, SecretString};

/// Account name used by [`NativeStore::open`] to check that the keychain
/// answers. Never written.
const PROBE_ACCOUNT: &str = "pa-secrets.probe";

/// The keychain of the desktop OS: Keychain on macOS, Credential Manager on
/// Windows, Secret Service on Linux and BSD.
///
/// Each secret is one keychain item with service = the app service name
/// (see [`DEFAULT_SERVICE`](crate::DEFAULT_SERVICE)) and account = the
/// [`SecretKey`].
pub struct NativeStore {
    store: Arc<CredentialStore>,
    service: String,
    backend: Backend,
}

impl NativeStore {
    /// Connects to the keychain and checks that it answers.
    ///
    /// Fails with [`SecretError::Unavailable`] when the OS has no keychain
    /// service (no D-Bus session or no Secret Service provider on Linux) and
    /// with [`SecretError::Locked`] when access is denied.
    pub fn open(service: &str) -> Result<Self> {
        if service.is_empty() {
            return Err(SecretError::InvalidKey(service.to_owned()));
        }
        let (store, backend) = connect().map_err(|e| match map_err(e) {
            SecretError::Backend(details) => SecretError::Unavailable(details),
            other => other,
        })?;
        let this = Self {
            store,
            service: service.to_owned(),
            backend,
        };
        this.get_raw(PROBE_ACCOUNT)?;
        Ok(this)
    }

    fn entry(&self, account: &str) -> Result<keyring_core::Entry> {
        self.store
            .build(&self.service, account, None)
            .map_err(map_err)
    }

    fn get_raw(&self, account: &str) -> Result<Option<SecretString>> {
        match self.entry(account)?.get_password() {
            Ok(value) => Ok(Some(SecretString::new(value))),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }
}

impl fmt::Debug for NativeStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeStore")
            .field("backend", &self.backend)
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}

impl SecretStore for NativeStore {
    fn get(&self, key: &SecretKey) -> Result<Option<SecretString>> {
        self.get_raw(key.as_str())
    }

    fn set(&self, key: &SecretKey, value: &SecretString) -> Result<()> {
        value.check_len()?;
        self.entry(key.as_str())?
            .set_password(value.expose())
            .map_err(map_err)
    }

    fn delete(&self, key: &SecretKey) -> Result<bool> {
        match self.entry(key.as_str())?.delete_credential() {
            Ok(()) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(e) => Err(map_err(e)),
        }
    }

    fn backend(&self) -> Backend {
        self.backend
    }
}

#[cfg(target_os = "macos")]
fn connect() -> keyring_core::Result<(Arc<CredentialStore>, Backend)> {
    let store: Arc<CredentialStore> = apple_native_keyring_store::keychain::Store::new()?;
    Ok((store, Backend::Keychain))
}

#[cfg(windows)]
fn connect() -> keyring_core::Result<(Arc<CredentialStore>, Backend)> {
    let store: Arc<CredentialStore> = windows_native_keyring_store::Store::new()?;
    Ok((store, Backend::CredentialManager))
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn connect() -> keyring_core::Result<(Arc<CredentialStore>, Backend)> {
    let store: Arc<CredentialStore> = zbus_secret_service_keyring_store::Store::new()?;
    Ok((store, Backend::SecretService))
}

/// Maps keyring errors without their payloads: `BadEncoding` and
/// `BadDataFormat` carry the raw secret bytes, which must not reach a log.
fn map_err(err: KeyringError) -> SecretError {
    match err {
        KeyringError::NoStorageAccess(e) => SecretError::Locked(e.to_string()),
        KeyringError::PlatformFailure(e) => SecretError::Backend(e.to_string()),
        KeyringError::BadEncoding(_) => SecretError::Backend("stored value is not UTF-8".into()),
        KeyringError::BadDataFormat(_, _) => {
            SecretError::Backend("stored value has an unexpected format".into())
        }
        KeyringError::TooLong(attr, max) => {
            SecretError::Backend(format!("{attr} is longer than the keychain limit {max}"))
        }
        KeyringError::Ambiguous(entries) => SecretError::Backend(format!(
            "{} keychain items match one key",
            entries.len()
        )),
        KeyringError::NoEntry => SecretError::Backend("no entry".into()),
        other => SecretError::Backend(other.to_string()),
    }
}
