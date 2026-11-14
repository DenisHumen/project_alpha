// SPDX-License-Identifier: MPL-2.0
//! Error type of the catalog.

use pa_library::rusqlite;

/// Why a catalog operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Opening, migrating or querying the library database.
    #[error("library: {0}")]
    Library(#[from] pa_library::Error),
    /// The scan of a source failed as a whole.
    #[error("scan: {0}")]
    Scan(#[from] pa_scanner::ScanError),
    /// A file could not be read.
    #[error("io: {0}")]
    Io(String),
    /// No such source, item or file.
    #[error("not found: {0}")]
    NotFound(String),
    /// An argument is not acceptable.
    #[error("invalid: {0}")]
    Invalid(String),
    /// Sync: recording or applying an op (UI-019).
    #[error("sync: {0}")]
    Sync(String),
}

impl From<pa_sync::Error> for Error {
    fn from(e: pa_sync::Error) -> Self {
        Self::Sync(e.to_string())
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Library(pa_library::Error::Sqlite(e))
    }
}

impl From<pa_vfs::VfsError> for Error {
    fn from(e: pa_vfs::VfsError) -> Self {
        Self::Io(e.to_string())
    }
}
