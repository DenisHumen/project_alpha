// SPDX-License-Identifier: MPL-2.0
//! Contract test suite that every [`Vfs`] backend must pass.
//!
//! The suite reads a known tree, the *fixture*, through the backend and
//! checks listing, metadata, error kinds and positional reads. It is the same
//! for every backend; only the way the fixture gets onto the source differs:
//!
//! - local backend: [`write_fixture`] into a temporary directory;
//! - network backends (CI-012): the server's share is seeded with
//!   [`write_fixture`] before the server container starts, then the backend
//!   points at it.
//!
//! The fixture lives under a `base` directory, so it can sit next to other
//! data on a shared test server. Each check panics with a descriptive
//! message on failure.
//!
//! In a backend crate, generate one `#[test]` per case:
//!
//! ```ignore
//! pa_vfs::vfs_contract_tests!(|| {
//!     let dir = make_seeded_temp_dir();
//!     pa_vfs::contract::Harness::new(MyFs::connect(&dir), pa_vfs::VfsPath::root())
//!         .with_guard(dir)
//! });
//! ```
//!
//! or call [`run_all`] from a single test.

use std::any::Any;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::ErrorKind;
use crate::path::VfsPath;
use crate::source::ByteSource;
use crate::vfs::{EntryKind, Vfs};

/// Size of [`SAMPLE`]: larger than common buffer sizes (64 KiB, 256 KiB) and
/// odd, so block-aligned shortcuts in a backend do not hide bugs.
pub const SAMPLE_SIZE: usize = 300_001;

/// A large binary file at the fixture root.
pub const SAMPLE: &str = "sample.bin";
/// An empty file at the fixture root.
pub const EMPTY: &str = "empty.bin";
/// A directory with a file and an empty subdirectory.
pub const DIR: &str = "dir";
/// `dir/nested.txt`.
pub const NESTED: &str = "nested.txt";
/// `dir/sub`, an empty directory.
pub const SUB: &str = "sub";
/// Contents of `dir/nested.txt`.
pub const NESTED_CONTENT: &[u8] = b"nested file\n";
/// A directory with Cyrillic, spaces and brackets in its name. Letters have
/// no combining forms, so NFC/NFD normalization cannot rename them.
pub const UNICODE_DIR: &str = "Фильмы и сериалы";
/// A file inside [`UNICODE_DIR`].
pub const UNICODE_FILE: &str = "Дюна (2021) [1080p].mkv";
/// A file with characters that need escaping in URLs (WebDAV) and shells.
pub const SPECIAL_FILE: &str = "special #1 & 100% 'q'.txt";

/// Fixture entry: a directory or a file with its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureEntry {
    /// A directory, relative to the fixture base (`/`-separated).
    Dir(&'static str),
    /// A file, relative to the fixture base, and its bytes.
    File(String, Vec<u8>),
}

/// Deterministic bytes of [`SAMPLE`] (xorshift32, seed fixed).
#[must_use]
pub fn sample_bytes() -> Vec<u8> {
    let mut x: u32 = 0x9E37_79B9;
    let mut out = Vec::with_capacity(SAMPLE_SIZE);
    while out.len() < SAMPLE_SIZE {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        out.extend_from_slice(&x.to_le_bytes());
    }
    out.truncate(SAMPLE_SIZE);
    out
}

/// The whole fixture tree, parents before children.
#[must_use]
pub fn fixture() -> Vec<FixtureEntry> {
    vec![
        FixtureEntry::File(SAMPLE.to_owned(), sample_bytes()),
        FixtureEntry::File(EMPTY.to_owned(), Vec::new()),
        FixtureEntry::File(SPECIAL_FILE.to_owned(), b"special\n".to_vec()),
        FixtureEntry::Dir(DIR),
        FixtureEntry::File(format!("{DIR}/{NESTED}"), NESTED_CONTENT.to_vec()),
        FixtureEntry::Dir("dir/sub"),
        FixtureEntry::Dir(UNICODE_DIR),
        FixtureEntry::File(
            format!("{UNICODE_DIR}/{UNICODE_FILE}"),
            UNICODE_FILE.as_bytes().to_vec(),
        ),
    ]
}

/// Writes the fixture into the existing directory `base` on the host.
///
/// Used to seed a local temporary directory or a test server's share.
pub fn write_fixture(base: &Path) -> io::Result<()> {
    for entry in fixture() {
        match entry {
            FixtureEntry::Dir(rel) => fs::create_dir_all(host(base, rel))?,
            FixtureEntry::File(rel, bytes) => fs::write(host(base, &rel), bytes)?,
        }
    }
    Ok(())
}

fn host(base: &Path, rel: &str) -> std::path::PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, c| p.join(c))
}

/// A backend under test plus the fixture base path.
pub struct Harness {
    /// The backend.
    pub vfs: Box<dyn Vfs>,
    /// Directory that holds the fixture.
    pub base: VfsPath,
    guard: Option<Box<dyn Any>>,
}

impl Harness {
    /// Wraps a backend whose fixture lives under `base`.
    pub fn new(vfs: impl Vfs + 'static, base: VfsPath) -> Self {
        Self {
            vfs: Box::new(vfs),
            base,
            guard: None,
        }
    }

    /// Keeps `guard` (a temporary directory, a server container handle)
    /// alive until the harness is dropped.
    #[must_use]
    pub fn with_guard(mut self, guard: impl Any) -> Self {
        self.guard = Some(Box::new(guard));
        self
    }
}

impl std::fmt::Debug for Harness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Harness")
            .field("vfs", &self.vfs)
            .field("base", &self.base)
            .field("guard", &self.guard.is_some())
            .finish()
    }
}

/// One contract check.
pub type Case = fn(&dyn Vfs, &VfsPath);

/// All checks with their names, in a stable order.
pub const CASES: &[(&str, Case)] = &[
    ("list_base", list_base),
    ("list_nested", list_nested),
    ("list_errors", list_errors),
    ("stat_kinds_and_sizes", stat_kinds_and_sizes),
    ("stat_errors", stat_errors),
    ("open_errors", open_errors),
    ("read_whole_file", read_whole_file),
    ("read_empty_file", read_empty_file),
    ("read_random_ranges", read_random_ranges),
    ("read_bounds", read_bounds),
    ("read_concurrent", read_concurrent),
    ("open_twice_independent", open_twice_independent),
    ("unicode_and_special_names", unicode_and_special_names),
    ("modified_time_sane", modified_time_sane),
];

/// Runs every case against `vfs`, panicking on the first failure with the
/// case name.
pub fn run_all(vfs: &dyn Vfs, base: &VfsPath) {
    for (name, case) in CASES {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| case(vfs, base)));
        if let Err(panic) = result {
            let msg = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("panic");
            panic!("Vfs contract case `{name}` failed for {vfs:?}: {msg}");
        }
    }
}

fn at(base: &VfsPath, rel: &str) -> VfsPath {
    rel.split('/')
        .try_fold(base.clone(), |p, c| p.join(c))
        .unwrap_or_else(|e| panic!("bad fixture path {rel:?}: {e}"))
}

fn expect_err<T: std::fmt::Debug>(what: &str, r: crate::Result<T>, kind: ErrorKind) {
    match r {
        Ok(v) => panic!("{what}: expected {kind:?} error, got Ok({v:?})"),
        Err(e) => assert_eq!(e.kind(), kind, "{what}: wrong error kind ({e})"),
    }
}

fn open(vfs: &dyn Vfs, path: &VfsPath) -> Box<dyn ByteSource> {
    vfs.open(path)
        .unwrap_or_else(|e| panic!("open {path}: {e}"))
}

/// `list(base)` returns the fixture's top-level entries with correct paths,
/// kinds and sizes, consistent with `stat`.
pub fn list_base(vfs: &dyn Vfs, base: &VfsPath) {
    let entries = vfs
        .list(base)
        .unwrap_or_else(|e| panic!("list {base}: {e}"));
    let by_name: BTreeMap<_, _> = entries.iter().map(|e| (e.name.as_str(), e)).collect();
    assert_eq!(by_name.len(), entries.len(), "duplicate names in listing");
    let expected = [
        (SAMPLE, EntryKind::File, SAMPLE_SIZE as u64),
        (EMPTY, EntryKind::File, 0),
        (SPECIAL_FILE, EntryKind::File, 8),
        (DIR, EntryKind::Dir, 0),
        (UNICODE_DIR, EntryKind::Dir, 0),
    ];
    for (name, kind, size) in expected {
        let e = by_name
            .get(name)
            .unwrap_or_else(|| panic!("{name:?} missing from list {base}: {entries:?}"));
        assert_eq!(e.path, at(base, name), "{name}: path");
        assert_eq!(e.metadata.kind, kind, "{name}: kind");
        assert_eq!(e.metadata.size, size, "{name}: size");
        let st = vfs
            .stat(&e.path)
            .unwrap_or_else(|err| panic!("stat {}: {err}", e.path));
        assert_eq!(
            (st.kind, st.size),
            (e.metadata.kind, e.metadata.size),
            "{name}: list and stat disagree"
        );
    }
    for e in &entries {
        assert!(
            !matches!(e.name.as_str(), "" | "." | "..") && !e.name.contains('/'),
            "bad entry name {:?}",
            e.name
        );
    }
}

/// Nested directories list their children; an empty directory lists nothing.
pub fn list_nested(vfs: &dyn Vfs, base: &VfsPath) {
    let dir = at(base, DIR);
    let mut names: Vec<_> = vfs
        .list(&dir)
        .unwrap_or_else(|e| panic!("list {dir}: {e}"))
        .into_iter()
        .map(|e| (e.name, e.metadata.kind))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            (NESTED.to_owned(), EntryKind::File),
            (SUB.to_owned(), EntryKind::Dir)
        ]
    );
    let sub = at(base, "dir/sub");
    let children = vfs.list(&sub).unwrap_or_else(|e| panic!("list {sub}: {e}"));
    assert!(children.is_empty(), "{sub} should be empty: {children:?}");
}

/// Listing a missing path is `NotFound`; listing a file is `NotADirectory`.
pub fn list_errors(vfs: &dyn Vfs, base: &VfsPath) {
    expect_err(
        "list missing",
        vfs.list(&at(base, "no-such-dir")),
        ErrorKind::NotFound,
    );
    expect_err(
        "list missing/child",
        vfs.list(&at(base, "no-such-dir/child")),
        ErrorKind::NotFound,
    );
    expect_err(
        "list file",
        vfs.list(&at(base, SAMPLE)),
        ErrorKind::NotADirectory,
    );
}

/// `stat` reports kinds and sizes; the base and the source root are directories.
pub fn stat_kinds_and_sizes(vfs: &dyn Vfs, base: &VfsPath) {
    for dir in [
        VfsPath::root(),
        base.clone(),
        at(base, DIR),
        at(base, "dir/sub"),
    ] {
        let m = vfs.stat(&dir).unwrap_or_else(|e| panic!("stat {dir}: {e}"));
        assert!(m.is_dir(), "{dir} should be a directory: {m:?}");
        assert_eq!(m.size, 0, "{dir}: directory size must be 0");
    }
    for (rel, size) in [
        (SAMPLE.to_owned(), SAMPLE_SIZE as u64),
        (EMPTY.to_owned(), 0),
        (format!("{DIR}/{NESTED}"), NESTED_CONTENT.len() as u64),
    ] {
        let p = at(base, &rel);
        let m = vfs.stat(&p).unwrap_or_else(|e| panic!("stat {p}: {e}"));
        assert!(m.is_file(), "{p} should be a file: {m:?}");
        assert_eq!(m.size, size, "{p}: size");
    }
}

/// `stat` of a missing entry, or of an entry under a missing directory, is `NotFound`.
pub fn stat_errors(vfs: &dyn Vfs, base: &VfsPath) {
    expect_err(
        "stat missing",
        vfs.stat(&at(base, "missing.mkv")),
        ErrorKind::NotFound,
    );
    expect_err(
        "stat missing/child",
        vfs.stat(&at(base, "missing/child.mkv")),
        ErrorKind::NotFound,
    );
}

/// `open` of a missing file is `NotFound`; of a directory is `IsADirectory`.
pub fn open_errors(vfs: &dyn Vfs, base: &VfsPath) {
    expect_err(
        "open missing",
        vfs.open(&at(base, "missing.mkv")),
        ErrorKind::NotFound,
    );
    expect_err(
        "open dir",
        vfs.open(&at(base, DIR)),
        ErrorKind::IsADirectory,
    );
    expect_err("open base", vfs.open(base), ErrorKind::IsADirectory);
}

/// The whole sample file reads back byte for byte; `size()` matches `stat`.
pub fn read_whole_file(vfs: &dyn Vfs, base: &VfsPath) {
    let src = open(vfs, &at(base, SAMPLE));
    assert_eq!(src.size(), SAMPLE_SIZE as u64, "size()");
    let mut buf = vec![0u8; SAMPLE_SIZE];
    src.read_exact_at(0, &mut buf)
        .unwrap_or_else(|e| panic!("read sample: {e}"));
    assert!(buf == sample_bytes(), "sample content differs");

    let nested = open(vfs, &at(base, &format!("{DIR}/{NESTED}")));
    let mut buf = vec![0u8; NESTED_CONTENT.len()];
    nested.read_exact_at(0, &mut buf).expect("read nested");
    assert_eq!(buf, NESTED_CONTENT);
}

/// An empty file opens with size 0 and every read returns 0.
pub fn read_empty_file(vfs: &dyn Vfs, base: &VfsPath) {
    let src = open(vfs, &at(base, EMPTY));
    assert_eq!(src.size(), 0);
    let mut buf = [0u8; 16];
    assert_eq!(src.read_at(0, &mut buf).expect("read empty"), 0);
    assert_eq!(src.read_at(10, &mut buf).expect("read empty +10"), 0);
}

/// Reads at pseudo-random offsets and lengths return the right bytes, in
/// any order (seeks backwards and forwards).
pub fn read_random_ranges(vfs: &dyn Vfs, base: &VfsPath) {
    let src = open(vfs, &at(base, SAMPLE));
    let want = sample_bytes();
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..64 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let offset = (x % SAMPLE_SIZE as u64) as usize;
        let len = ((x >> 32) % 70_000) as usize + 1;
        let mut buf = vec![0u8; len];
        let end = (offset + len).min(SAMPLE_SIZE);
        src.read_exact_at(offset as u64, &mut buf[..end - offset])
            .unwrap_or_else(|e| panic!("read {offset}+{len}: {e}"));
        assert!(
            buf[..end - offset] == want[offset..end],
            "content differs at {offset}..{end}"
        );
    }
}

/// Reads at and past the end return 0; reads across the end are short and
/// never return bytes past `size()`; an empty buffer reads 0.
pub fn read_bounds(vfs: &dyn Vfs, base: &VfsPath) {
    let src = open(vfs, &at(base, SAMPLE));
    let size = src.size();
    let want = sample_bytes();
    let mut buf = [0u8; 64];
    assert_eq!(src.read_at(size, &mut buf).expect("read at size"), 0);
    assert_eq!(
        src.read_at(size + 1_000, &mut buf).expect("read past size"),
        0
    );
    assert_eq!(src.read_at(u64::MAX - 1, &mut buf).expect("read at max"), 0);
    assert_eq!(src.read_at(10, &mut []).expect("empty buffer"), 0);

    // Across the end: collect everything until 0 and compare.
    let start = size - 10;
    let mut got = Vec::new();
    loop {
        let n = src
            .read_at(start + got.len() as u64, &mut buf)
            .expect("read across end");
        if n == 0 {
            break;
        }
        assert!(n <= buf.len());
        got.extend_from_slice(&buf[..n]);
        assert!(got.len() <= 10, "read past size()");
    }
    assert_eq!(got, want[SAMPLE_SIZE - 10..]);
}

/// One opened source serves several threads at once.
pub fn read_concurrent(vfs: &dyn Vfs, base: &VfsPath) {
    let src = open(vfs, &at(base, SAMPLE));
    let want = sample_bytes();
    let chunk = SAMPLE_SIZE / 4 + 1;
    std::thread::scope(|s| {
        for t in 0..4usize {
            let src = &src;
            let want = &want;
            s.spawn(move || {
                for round in 0..8usize {
                    // Interleave regions so threads keep crossing each other.
                    let region = (t + round) % 4;
                    let start = region * chunk;
                    let end = (start + chunk).min(SAMPLE_SIZE);
                    let mut buf = vec![0u8; end - start];
                    src.read_exact_at(start as u64, &mut buf)
                        .unwrap_or_else(|e| panic!("thread {t}: {e}"));
                    assert!(buf == want[start..end], "thread {t}: content differs");
                }
            });
        }
    });
}

/// Two opens of the same file are independent sources.
pub fn open_twice_independent(vfs: &dyn Vfs, base: &VfsPath) {
    let path = at(base, SAMPLE);
    let a = open(vfs, &path);
    let b = open(vfs, &path);
    let want = sample_bytes();
    let mut x = [0u8; 100];
    let mut y = [0u8; 100];
    a.read_exact_at(200_000, &mut x).expect("read a");
    b.read_exact_at(5, &mut y).expect("read b");
    drop(a);
    b.read_exact_at(200_000, &mut x)
        .expect("read b after a dropped");
    assert_eq!(x[..], want[200_000..200_100]);
    assert_eq!(y[..], want[5..105]);
}

/// Non-ASCII names and names with URL/shell metacharacters list, stat and
/// open correctly.
pub fn unicode_and_special_names(vfs: &dyn Vfs, base: &VfsPath) {
    let dir = at(base, UNICODE_DIR);
    let entries = vfs.list(&dir).unwrap_or_else(|e| panic!("list {dir}: {e}"));
    assert_eq!(entries.len(), 1, "{dir}: {entries:?}");
    assert_eq!(entries[0].name, UNICODE_FILE);
    let file = at(base, &format!("{UNICODE_DIR}/{UNICODE_FILE}"));
    assert_eq!(entries[0].path, file);
    let src = open(vfs, &file);
    let mut buf = vec![0u8; UNICODE_FILE.len()];
    src.read_exact_at(0, &mut buf).expect("read unicode file");
    assert_eq!(buf, UNICODE_FILE.as_bytes());

    let special = at(base, SPECIAL_FILE);
    let m = vfs
        .stat(&special)
        .unwrap_or_else(|e| panic!("stat {special}: {e}"));
    assert!(m.is_file());
    let src = open(vfs, &special);
    let mut buf = [0u8; 8];
    src.read_exact_at(0, &mut buf).expect("read special file");
    assert_eq!(&buf, b"special\n");
}

/// If the backend reports modification times, they are plausible
/// (after 2000-01-01 and not in the future by more than a day).
pub fn modified_time_sane(vfs: &dyn Vfs, base: &VfsPath) {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let y2000_ms = 946_684_800_000;
    for rel in [SAMPLE, DIR] {
        let p = at(base, rel);
        let m = vfs.stat(&p).unwrap_or_else(|e| panic!("stat {p}: {e}"));
        if let Some(ms) = m.modified_ms {
            assert!(
                (y2000_ms..=now_ms + 86_400_000).contains(&ms),
                "{p}: implausible modified_ms {ms}"
            );
        }
    }
}

/// Generates one `#[test]` per contract case in a `vfs_contract` module.
///
/// The argument is a closure (or function) returning a
/// [`Harness`](crate::contract::Harness); it runs once per test, so each
/// test gets a fresh backend. Items of the calling module are visible in
/// the closure.
#[macro_export]
macro_rules! vfs_contract_tests {
    ($setup:expr) => {
        $crate::__vfs_contract_cases!(
            $setup;
            list_base,
            list_nested,
            list_errors,
            stat_kinds_and_sizes,
            stat_errors,
            open_errors,
            read_whole_file,
            read_empty_file,
            read_random_ranges,
            read_bounds,
            read_concurrent,
            open_twice_independent,
            unicode_and_special_names,
            modified_time_sane
        );
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __vfs_contract_cases {
    ($setup:expr; $($case:ident),+ $(,)?) => {
        #[allow(unused_imports)]
        mod vfs_contract {
            use super::*;

            fn harness() -> $crate::contract::Harness {
                let setup = $setup;
                setup()
            }

            $(
                #[test]
                fn $case() {
                    let h = harness();
                    $crate::contract::$case(&*h.vfs, &h.base);
                }
            )+

            /// Fails if a case was added to `CASES` but not to the macro.
            #[test]
            fn macro_covers_all_cases() {
                let names = [$(stringify!($case)),+];
                let all: ::std::vec::Vec<&str> =
                    $crate::contract::CASES.iter().map(|(n, _)| *n).collect();
                assert_eq!(names.as_slice(), all.as_slice());
            }
        }
    };
}
