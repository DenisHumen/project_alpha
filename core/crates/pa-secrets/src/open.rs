// SPDX-License-Identifier: MPL-2.0
//! Choosing the backend: platform callback → OS keychain → encrypted file.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    Backend, EncryptedFileStore, PlatformKeystore, PlatformStore, Result, SecretError, SecretKey,
    SecretStore, SecretString, forget_known, register_known,
};

/// Keychain service name of the app: the items show up under this name in
/// Keychain Access, Credential Manager and Seahorse.
pub const DEFAULT_SERVICE: &str = "Kinolune";

/// Whether this build has a desktop OS keychain backend
/// ([`NativeStore`](crate::NativeStore)).
pub const NATIVE_AVAILABLE: bool = cfg!(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "macos",
    windows
));

/// What [`open`] may use.
#[derive(Clone)]
pub struct OpenOptions {
    /// Keychain service name, [`DEFAULT_SERVICE`] by default. Tests use a
    /// unique name.
    pub service: String,
    /// Secure storage of the app (mobile, tvOS). Takes precedence over the
    /// OS keychain.
    pub platform: Option<Arc<dyn PlatformKeystore>>,
    /// Try the desktop OS keychain. Default: [`NATIVE_AVAILABLE`].
    pub native: bool,
    /// Directory of the encrypted fallback file (the data directory of the
    /// core). Also checked for a previous fallback file to migrate.
    pub fallback_dir: Option<PathBuf>,
    /// Use the encrypted file when no secure backend works. Default: true on
    /// desktop, false on mobile (there the platform callback is required).
    pub allow_fallback: bool,
}

impl OpenOptions {
    /// Defaults for this platform with the given fallback directory.
    pub fn new(fallback_dir: Option<PathBuf>) -> Self {
        Self {
            service: DEFAULT_SERVICE.to_owned(),
            platform: None,
            native: NATIVE_AVAILABLE,
            fallback_dir,
            allow_fallback: NATIVE_AVAILABLE,
        }
    }

    /// Sets the platform keystore.
    #[must_use]
    pub fn with_platform(mut self, platform: Arc<dyn PlatformKeystore>) -> Self {
        self.platform = Some(platform);
        self
    }
}

impl fmt::Debug for OpenOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenOptions")
            .field("service", &self.service)
            .field("platform", &self.platform.is_some())
            .field("native", &self.native)
            .field("fallback_dir", &self.fallback_dir)
            .field("allow_fallback", &self.allow_fallback)
            .finish()
    }
}

/// Result of [`open`].
#[derive(Debug, Clone)]
pub struct OpenedStore {
    /// The store. Values passing through it are registered for
    /// [`redact`](crate::redact).
    pub store: Arc<dyn SecretStore>,
    /// Why the secure backends were not used, when
    /// [`Backend::EncryptedFile`] was chosen. The app shows a warning.
    pub fallback_reason: Option<String>,
    /// Secrets moved from an old fallback file into the secure backend.
    pub migrated: usize,
    /// The old fallback file could not be read and was moved aside
    /// (`secrets.enc.corrupt`): secrets in it must be entered again.
    pub discarded_corrupted: bool,
}

impl OpenedStore {
    /// Backend of [`store`](Self::store).
    pub fn backend(&self) -> Backend {
        self.store.backend()
    }
}

/// Opens the best available secret store.
///
/// Order: [`OpenOptions::platform`] → OS keychain → encrypted file in
/// [`OpenOptions::fallback_dir`]. When a secure backend is chosen and the
/// fallback directory still has a secrets file from an earlier run, its
/// secrets are moved to the secure backend (existing values there win) and
/// the file is deleted.
pub fn open(options: OpenOptions) -> Result<OpenedStore> {
    let mut reasons = Vec::new();
    let mut secure: Option<Arc<dyn SecretStore>> = None;

    if let Some(platform) = &options.platform {
        secure = Some(Arc::new(PlatformStore::new(platform.clone())));
    } else if options.native {
        match open_native(&options.service) {
            Ok(store) => secure = Some(store),
            Err(e) => reasons.push(format!("OS keychain: {e}")),
        }
    } else {
        reasons.push("no OS keychain on this platform".to_owned());
    }

    let mut discarded_corrupted = false;
    if let Some(store) = secure {
        let mut migrated = 0;
        if let Some(dir) = &options.fallback_dir
            && EncryptedFileStore::exists(dir)
        {
            match EncryptedFileStore::open(dir) {
                Ok(file) => migrated = migrate(&file, store.as_ref())?,
                Err(SecretError::Corrupted) => {
                    set_aside_corrupted(dir)?;
                    discarded_corrupted = true;
                }
                Err(e) => return Err(e),
            }
        }
        return Ok(OpenedStore {
            store: Arc::new(Tracked(store)),
            fallback_reason: None,
            migrated,
            discarded_corrupted,
        });
    }

    let reason = reasons.join("; ");
    let Some(dir) = options.fallback_dir.filter(|_| options.allow_fallback) else {
        return Err(SecretError::Unavailable(reason));
    };
    let file = match EncryptedFileStore::open(&dir) {
        Err(SecretError::Corrupted) => {
            set_aside_corrupted(&dir)?;
            discarded_corrupted = true;
            EncryptedFileStore::open(&dir)?
        }
        other => other?,
    };
    Ok(OpenedStore {
        store: Arc::new(Tracked(Arc::new(file))),
        fallback_reason: Some(reason),
        migrated: 0,
        discarded_corrupted,
    })
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "macos",
    windows
))]
fn open_native(service: &str) -> Result<Arc<dyn SecretStore>> {
    Ok(Arc::new(crate::NativeStore::open(service)?))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "macos",
    windows
)))]
fn open_native(_service: &str) -> Result<Arc<dyn SecretStore>> {
    Err(SecretError::Unavailable(
        "no OS keychain backend in this build".into(),
    ))
}

/// Copies every secret of `file` that `target` does not have yet, then
/// deletes the file. Returns the number of copied secrets.
fn migrate(file: &EncryptedFileStore, target: &dyn SecretStore) -> Result<usize> {
    let mut copied = 0;
    for key in file.keys() {
        if target.get(&key)?.is_none()
            && let Some(value) = file.get(&key)?
        {
            target.set(&key, &value)?;
            copied += 1;
        }
    }
    EncryptedFileStore::remove_files(file.dir())?;
    Ok(copied)
}

fn set_aside_corrupted(dir: &std::path::Path) -> Result<()> {
    let path = dir.join(crate::SECRETS_FILE);
    std::fs::rename(&path, path.with_extension("enc.corrupt"))?;
    // The key file may be the reason; a fresh one is made on next open.
    match std::fs::remove_file(dir.join(crate::KEY_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Registers every value it sees for log redaction.
#[derive(Debug)]
struct Tracked(Arc<dyn SecretStore>);

impl SecretStore for Tracked {
    fn get(&self, key: &SecretKey) -> Result<Option<SecretString>> {
        let value = self.0.get(key)?;
        if let Some(value) = &value {
            register_known(value);
        }
        Ok(value)
    }

    fn set(&self, key: &SecretKey, value: &SecretString) -> Result<()> {
        register_known(value);
        self.0.set(key, value)
    }

    fn delete(&self, key: &SecretKey) -> Result<bool> {
        if let Ok(Some(old)) = self.0.get(key) {
            forget_known(&old);
        }
        self.0.delete(key)
    }

    fn backend(&self) -> Backend {
        self.0.backend()
    }
}
