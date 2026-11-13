// SPDX-License-Identifier: MPL-2.0
//! Secret storage of project_alpha (CORE-007).
//!
//! Passwords of network sources (SMB, WebDAV), OAuth tokens (Twitch, Trakt)
//! and user API keys (TMDB) never go to the settings database or to logs.
//! They live in the secure storage of the OS:
//!
//! | Platform | Backend |
//! |---|---|
//! | macOS | Keychain (`keyring-core` + `apple-native-keyring-store`) |
//! | Windows | Credential Manager, DPAPI-protected (`windows-native-keyring-store`) |
//! | Linux, BSD | Secret Service: GNOME Keyring, KWallet, KeePassXC (`zbus-secret-service-keyring-store`) |
//! | Android | Keystore + Tink, through the [`PlatformKeystore`] callback |
//! | iOS, iPadOS, tvOS | Keychain (`AfterFirstUnlock`), through the [`PlatformKeystore`] callback |
//!
//! When a desktop has no keychain (a headless Linux box, a window manager
//! without Secret Service) [`open`] falls back to an [`EncryptedFileStore`]
//! and reports it in [`OpenedStore::fallback_reason`], so the app can warn
//! the user. The fallback protects against casual reading and leaks of the
//! data directory, not against malware running as the same user. Once a
//! keychain appears, the next [`open`] moves the secrets there and deletes
//! the file.
//!
//! Values are [`SecretString`]: zeroed on drop, never printed by `Debug`.
//! Everything that passes through a store opened with [`open`] is also
//! registered for [`redact`], which masks known secrets and common
//! credential patterns (`password=…`, `Bearer …`, `smb://user:pass@…`) in
//! log lines.

mod error;
mod file;
mod key;
mod memory;
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "macos",
    windows
))]
mod native;
mod open;
mod platform;
mod redact;
mod store;
mod value;

pub use error::SecretError;
pub use file::{EncryptedFileStore, KEY_FILE, SECRETS_FILE};
pub use key::{MAX_KEY_LEN, SecretKey};
pub use memory::MemoryStore;
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "macos",
    windows
))]
pub use native::NativeStore;
pub use open::{DEFAULT_SERVICE, NATIVE_AVAILABLE, OpenOptions, OpenedStore, open};
pub use platform::{PlatformKeystore, PlatformStore};
pub use redact::{forget_known, redact, register_known};
pub use store::{Backend, SecretStore};
pub use value::{MAX_SECRET_LEN, SecretString};

/// Shorthand for results of this crate.
pub type Result<T> = std::result::Result<T, SecretError>;
