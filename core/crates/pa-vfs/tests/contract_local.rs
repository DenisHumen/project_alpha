// SPDX-License-Identifier: MPL-2.0
//! The Vfs contract suite against `LocalFs` (NET-001 "done when").

mod common;

use common::TempDir;
use pa_vfs::contract::{self, Harness};
use pa_vfs::{LocalFs, VfsPath};

/// Fixture at the source root.
fn seeded_root() -> Harness {
    let dir = TempDir::new();
    contract::write_fixture(dir.path()).expect("write fixture");
    let vfs = LocalFs::new(dir.path()).expect("LocalFs::new");
    Harness::new(vfs, VfsPath::root()).with_guard(dir)
}

pa_vfs::vfs_contract_tests!(seeded_root);

/// Fixture under a nested base, next to unrelated data, as on a shared
/// test server.
mod nested_base {
    use super::*;

    fn seeded_nested() -> Harness {
        let dir = TempDir::new();
        let base = dir.path().join("media").join("контракт");
        std::fs::create_dir_all(&base).expect("create base");
        std::fs::write(dir.path().join("media").join("other.txt"), b"x").expect("write");
        contract::write_fixture(&base).expect("write fixture");
        let vfs = LocalFs::new(dir.path()).expect("LocalFs::new");
        let base = VfsPath::parse("/media/контракт").expect("base path");
        Harness::new(vfs, base).with_guard(dir)
    }

    pa_vfs::vfs_contract_tests!(seeded_nested);
}

#[test]
fn run_all_passes() {
    let h = seeded_root();
    contract::run_all(&*h.vfs, &h.base);
}

/// The suite is not vacuous: a backend that loses bytes at the end fails it.
#[test]
#[should_panic(expected = "read_whole_file")]
fn broken_backend_fails() {
    use pa_vfs::{ByteSource, DirEntry, Metadata, Result, Vfs};

    #[derive(Debug)]
    struct Truncating(LocalFs);
    #[derive(Debug)]
    struct Short(Box<dyn ByteSource>);

    impl ByteSource for Short {
        fn size(&self) -> u64 {
            self.0.size()
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
            // Drops the last byte of the file.
            if offset + 1 >= self.0.size() {
                return Ok(0);
            }
            let max = usize::try_from(self.0.size() - 1 - offset).unwrap_or(usize::MAX);
            let len = buf.len().min(max);
            self.0.read_at(offset, &mut buf[..len])
        }
    }

    impl Vfs for Truncating {
        fn list(&self, dir: &VfsPath) -> Result<Vec<DirEntry>> {
            self.0.list(dir)
        }
        fn stat(&self, path: &VfsPath) -> Result<Metadata> {
            self.0.stat(path)
        }
        fn open(&self, path: &VfsPath) -> Result<Box<dyn ByteSource>> {
            Ok(Box::new(Short(self.0.open(path)?)))
        }
    }

    let dir = TempDir::new();
    contract::write_fixture(dir.path()).expect("write fixture");
    let vfs = Truncating(LocalFs::new(dir.path()).expect("LocalFs::new"));
    contract::run_all(&vfs, &VfsPath::root());
}
