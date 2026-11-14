// SPDX-License-Identifier: MPL-2.0
//! Source manager of the project_alpha core (NET-011).
//!
//! A *source* is a place the library reads media from: a local folder, an
//! SMB share or a WebDAV collection. [`SourceManager`] keeps sources in the
//! `sources` table of the library database (`pa-library`) and their
//! passwords in the OS secret store (`pa-secrets`):
//!
//! - **CRUD:** [`SourceManager::create`], [`get`](SourceManager::get),
//!   [`list`](SourceManager::list), [`update`](SourceManager::update),
//!   [`delete`](SourceManager::delete).
//! - **Settings:** kind, address (a URI *without* credentials), user name,
//!   backend [`SourceOptions`], "scan into library" and auto-scan toggles.
//! - **Status** ([`SourceStatus`]): online, last check, last scan, last
//!   error, last speed test.
//! - **Connection:** [`SourceManager::connect`] builds the
//!   [`Vfs`](pa_vfs::Vfs) backend of the source kind;
//!   [`check`](SourceManager::check) and
//!   [`speed_test`](SourceManager::speed_test) use it and record the status.
//!
//! The password never reaches SQLite or logs: the row holds only
//! `credential_ref`, the key of the secret; addresses with `user:pass@`
//! are refused; `Debug` of every type hides the password; error texts saved
//! in the status or returned to the caller go through
//! [`pa_secrets::redact`] and have the password itself masked.
//!
//! ```
//! use std::sync::Arc;
//! use pa_secrets::{MemoryStore, SecretString};
//! use pa_sources::{NewSource, SourceKind, SourceManager};
//!
//! let manager = SourceManager::new(
//!     pa_library::open_in_memory().unwrap(),
//!     Arc::new(MemoryStore::new()),
//! );
//! let nas = manager
//!     .create(
//!         NewSource::new(SourceKind::Smb, "NAS", "smb://nas.local/media/Movies")
//!             .with_login("anna", SecretString::new("hunter2")),
//!     )
//!     .unwrap();
//! assert!(nas.has_password);
//! assert_eq!(manager.list().unwrap().len(), 1);
//! ```

mod address;
mod connect;
mod manager;
mod model;

pub use address::{SmbAddress, file_uri_to_path, path_to_file_uri};
pub use connect::{SpeedReport, SpeedTestOptions};
pub use manager::SourceManager;
pub use model::{
    NewSource, PasswordChange, SmbEncryption, SmbVersion, Source, SourceKind, SourceOptions,
    SourceStatus, SourceUpdate, SpeedSample,
};

/// Error of the source manager. No variant carries a password.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No source with this id.
    #[error("source {0} not found")]
    NotFound(i64),
    /// An argument is not acceptable (bad address, unknown option value).
    #[error("invalid source: {0}")]
    Invalid(String),
    /// The library database failed.
    #[error("library: {0}")]
    Library(#[from] pa_library::Error),
    /// The secret store failed (locked keychain, no storage).
    #[error("secret store: {0}")]
    Secrets(#[from] pa_secrets::SecretError),
    /// The source cannot be reached or read. `message` has secrets masked.
    #[error("source {id}: {kind}: {message}")]
    Source {
        /// Source id.
        id: i64,
        /// What went wrong, as the backend classified it.
        kind: pa_vfs::ErrorKind,
        /// Backend message with secrets masked.
        message: String,
    },
}

impl From<pa_library::rusqlite::Error> for Error {
    fn from(e: pa_library::rusqlite::Error) -> Self {
        Self::Library(pa_library::Error::Sqlite(e))
    }
}

impl Error {
    /// Kind of a [`Error::Source`] failure, `None` for other errors.
    pub fn source_kind(&self) -> Option<pa_vfs::ErrorKind> {
        match self {
            Self::Source { kind, .. } => Some(*kind),
            _ => None,
        }
    }
}

/// Shorthand for results of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Masks secrets in a text that may end up in the database or a log: the
/// known password of the source, then [`pa_secrets::redact`] patterns.
pub(crate) fn scrub(text: &str, password: Option<&pa_secrets::SecretString>) -> String {
    let text = match password {
        Some(p) if !p.is_empty() => text.replace(p.expose(), "***"),
        _ => text.to_owned(),
    };
    pa_secrets::redact(&text)
}

/// Wall clock in Unix milliseconds.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
