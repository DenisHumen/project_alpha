// SPDX-License-Identifier: MPL-2.0
//! State shared by the facades of one process: the secret store.
//!
//! The source manager ([`CoreSources`](crate::api::sources::CoreSources))
//! writes passwords and the library ([`CoreLibrary`](crate::api::library::CoreLibrary))
//! reads them to reach network sources (LIB-018). Both must use the same
//! store object: the encrypted fallback file caches its contents, so two
//! instances over one file would not see each other's writes.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pa_secrets::{OpenOptions, OpenedStore, SecretError};

static STORES: Mutex<Vec<(PathBuf, OpenedStore)>> = Mutex::new(Vec::new());

/// The secret store the app opened (`secrets_open_platform` or
/// `secrets_open_default`, CORE-015), or else the default store with its
/// fallback file in `data_dir`, opened once per process and directory.
pub(crate) fn secret_store(data_dir: &Path) -> Result<OpenedStore, SecretError> {
    if let Some(opened) = crate::api::secrets::opened_store() {
        return Ok(opened);
    }
    let mut stores = STORES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, opened)) = stores.iter().find(|(dir, _)| dir == data_dir) {
        return Ok(opened.clone());
    }
    let opened = pa_secrets::open(OpenOptions::new(Some(data_dir.to_path_buf())))?;
    stores.push((data_dir.to_path_buf(), opened.clone()));
    Ok(opened)
}
