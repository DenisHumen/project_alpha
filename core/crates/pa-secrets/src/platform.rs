// SPDX-License-Identifier: MPL-2.0
//! Secure storage implemented by the app (Android, iOS, tvOS).

use std::fmt;
use std::sync::Arc;

use crate::{Backend, Result, SecretKey, SecretStore, SecretString};

/// Secure storage implemented on the platform side.
///
/// - Android: Android Keystore key + Tink AEAD over a private
///   `SharedPreferences`/DataStore file.
/// - iOS, iPadOS, tvOS: Keychain generic passwords with
///   `kSecAttrAccessibleAfterFirstUnlock`, so background downloads can read
///   them.
///
/// The FFI facade adapts a UniFFI callback interface to this trait. Calls are
/// blocking and may come from any core thread.
pub trait PlatformKeystore: Send + Sync {
    /// The value, `None` if missing.
    fn load(&self, key: &str) -> Result<Option<String>>;
    /// Stores or replaces the value.
    fn save(&self, key: &str, value: &str) -> Result<()>;
    /// Deletes the value; returns whether it existed.
    fn remove(&self, key: &str) -> Result<bool>;
}

/// [`SecretStore`] over a [`PlatformKeystore`].
#[derive(Clone)]
pub struct PlatformStore(Arc<dyn PlatformKeystore>);

impl PlatformStore {
    /// Wraps the platform implementation.
    pub fn new(keystore: Arc<dyn PlatformKeystore>) -> Self {
        Self(keystore)
    }
}

impl fmt::Debug for PlatformStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PlatformStore")
    }
}

impl SecretStore for PlatformStore {
    fn get(&self, key: &SecretKey) -> Result<Option<SecretString>> {
        Ok(self.0.load(key.as_str())?.map(SecretString::new))
    }

    fn set(&self, key: &SecretKey, value: &SecretString) -> Result<()> {
        value.check_len()?;
        self.0.save(key.as_str(), value.expose())
    }

    fn delete(&self, key: &SecretKey) -> Result<bool> {
        self.0.remove(key.as_str())
    }

    fn backend(&self) -> Backend {
        Backend::Platform
    }
}
