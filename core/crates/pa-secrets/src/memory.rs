// SPDX-License-Identifier: MPL-2.0
//! In-memory store for tests.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use crate::{Backend, Result, SecretKey, SecretStore, SecretString};

/// Keeps secrets in memory only. For tests and previews.
#[derive(Debug, Default)]
pub struct MemoryStore {
    map: Mutex<BTreeMap<SecretKey, SecretString>>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keys of all stored secrets, sorted.
    pub fn keys(&self) -> Vec<SecretKey> {
        self.lock().keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<SecretKey, SecretString>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, key: &SecretKey) -> Result<Option<SecretString>> {
        Ok(self.lock().get(key).cloned())
    }

    fn set(&self, key: &SecretKey, value: &SecretString) -> Result<()> {
        value.check_len()?;
        self.lock().insert(key.clone(), value.clone());
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<bool> {
        Ok(self.lock().remove(key).is_some())
    }

    fn backend(&self) -> Backend {
        Backend::Memory
    }
}
