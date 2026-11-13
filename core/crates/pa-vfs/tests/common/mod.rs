// SPDX-License-Identifier: MPL-2.0
//! Test helpers: a self-deleting temporary directory (std only).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A unique directory under the system temp dir, removed on drop.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("pa-vfs-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
