// SPDX-License-Identifier: MPL-2.0
//! The last valid copy on disk: one JSON file written atomically
//! (temp file + rename), re-verified every time it is loaded.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Error;

/// What is stored: the signed bytes exactly as downloaded, their signature
/// and the HTTP validator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CacheEntry {
    /// URL the body came from; the `ETag` is only valid for it.
    pub url: String,
    /// `ETag` of the body.
    pub etag: Option<String>,
    /// Signed document, UTF-8 text as downloaded.
    pub body: String,
    /// Text of the `.minisig` file.
    pub signature: String,
    /// Unix seconds of the last successful check (200 or 304).
    pub checked_at: u64,
}

pub(crate) struct DiskCache {
    path: PathBuf,
}

impl DiskCache {
    pub(crate) fn new(dir: &Path, file_name: &str) -> Self {
        Self {
            path: dir.join(format!("{file_name}.cache.json")),
        }
    }

    /// The stored entry, or None if there is none or it is unreadable.
    pub(crate) fn load(&self) -> Option<CacheEntry> {
        let bytes = fs::read(&self.path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub(crate) fn store(&self, entry: &CacheEntry) -> Result<(), Error> {
        let io = |e: std::io::Error| Error::Io(e.kind().to_string());
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(io)?;
        }
        let bytes = serde_json::to_vec(entry).map_err(|e| Error::Io(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(io)?;
            f.write_all(&bytes).map_err(io)?;
            f.sync_all().map_err(io)?;
        }
        fs::rename(&tmp, &self.path).map_err(io)
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}
