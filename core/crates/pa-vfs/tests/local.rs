// SPDX-License-Identifier: MPL-2.0
//! `LocalFs` behavior beyond the shared contract.

mod common;

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use common::TempDir;
use pa_vfs::{ByteSource, EntryKind, ErrorKind, LocalFs, SourceReader, Vfs, VfsPath};

fn p(s: &str) -> VfsPath {
    VfsPath::parse(s).unwrap()
}

#[test]
fn new_requires_existing_directory() {
    let dir = TempDir::new();
    let err = LocalFs::new(dir.path().join("missing")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);
    fs::write(dir.path().join("f"), b"x").unwrap();
    let err = LocalFs::new(dir.path().join("f")).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotADirectory);
    let fs = LocalFs::new(dir.path()).unwrap();
    assert_eq!(fs.root(), dir.path());
}

#[test]
fn paths_stay_under_root() {
    let dir = TempDir::new();
    let vfs = LocalFs::new(dir.path()).unwrap();
    assert_eq!(vfs.host_path(&VfsPath::root()).unwrap(), dir.path());
    assert_eq!(
        vfs.host_path(&p("/a/b.mkv")).unwrap(),
        dir.path().join("a").join("b.mkv")
    );
    // `..` cannot even be expressed.
    assert!(VfsPath::parse("/../etc").is_err());
}

#[test]
fn listing_is_sorted_and_includes_hidden() {
    let dir = TempDir::new();
    for name in ["b.mkv", ".hidden", "a.mkv"] {
        fs::write(dir.path().join(name), b"").unwrap();
    }
    let vfs = LocalFs::new(dir.path()).unwrap();
    let names: Vec<_> = vfs
        .list(&VfsPath::root())
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, [".hidden", "a.mkv", "b.mkv"]);
}

#[test]
fn size_is_fixed_at_open() {
    let dir = TempDir::new();
    let file = dir.path().join("growing.ts");
    fs::write(&file, b"0123456789").unwrap();
    let vfs = LocalFs::new(dir.path()).unwrap();
    let src = vfs.open(&p("/growing.ts")).unwrap();
    // The file grows after open (a recording in progress).
    fs::write(&file, b"0123456789abcdef").unwrap();
    assert_eq!(src.size(), 10);
    let mut buf = [0u8; 32];
    let n = src.read_at(5, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"56789");
    assert_eq!(src.read_at(10, &mut buf).unwrap(), 0);
    // A new open sees the new size.
    assert_eq!(vfs.open(&p("/growing.ts")).unwrap().size(), 16);
}

#[test]
fn source_is_shareable_and_readable_as_stream() {
    let dir = TempDir::new();
    fs::write(dir.path().join("s.bin"), (0u8..=255).collect::<Vec<_>>()).unwrap();
    let vfs: Arc<dyn Vfs> = Arc::new(LocalFs::new(dir.path()).unwrap());
    let src: Arc<dyn ByteSource> = Arc::from(vfs.open(&p("s.bin")).unwrap());
    let mut r = SourceReader::new(Arc::clone(&src));
    r.seek(SeekFrom::Start(250)).unwrap();
    let mut tail = Vec::new();
    r.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, [250, 251, 252, 253, 254, 255]);
    assert_eq!(src.size(), 256);
}

#[cfg(unix)]
#[test]
fn symlinks_are_followed_and_dangling_ones_are_other() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new();
    fs::create_dir(dir.path().join("real")).unwrap();
    fs::write(dir.path().join("real").join("f.mkv"), b"abc").unwrap();
    symlink("real", dir.path().join("link-dir")).unwrap();
    symlink("real/f.mkv", dir.path().join("link-file.mkv")).unwrap();
    symlink("nowhere", dir.path().join("dangling")).unwrap();

    let vfs = LocalFs::new(dir.path()).unwrap();
    let kinds: Vec<_> = vfs
        .list(&VfsPath::root())
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.metadata.kind, e.metadata.size))
        .collect();
    assert_eq!(
        kinds,
        [
            ("dangling".to_owned(), EntryKind::Other, 0),
            ("link-dir".to_owned(), EntryKind::Dir, 0),
            ("link-file.mkv".to_owned(), EntryKind::File, 3),
            ("real".to_owned(), EntryKind::Dir, 0),
        ]
    );
    assert_eq!(vfs.list(&p("/link-dir")).unwrap()[0].name, "f.mkv");
    assert_eq!(vfs.stat(&p("/dangling")).unwrap().kind, EntryKind::Other);
    assert_eq!(
        vfs.open(&p("/dangling")).unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[test]
fn fifo_is_not_opened() {
    let dir = TempDir::new();
    let fifo = dir.path().join("pipe");
    let ok = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        return; // mkfifo unavailable; nothing to check.
    }
    let vfs = LocalFs::new(dir.path()).unwrap();
    assert_eq!(vfs.stat(&p("/pipe")).unwrap().kind, EntryKind::Other);
    assert_eq!(
        vfs.open(&p("/pipe")).unwrap_err().kind(),
        ErrorKind::Unsupported
    );
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_names_are_skipped() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let dir = TempDir::new();
    fs::write(dir.path().join(OsStr::from_bytes(b"bad-\xff.mkv")), b"").unwrap();
    fs::write(dir.path().join("good.mkv"), b"").unwrap();
    let vfs = LocalFs::new(dir.path()).unwrap();
    let names: Vec<_> = vfs
        .list(&VfsPath::root())
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["good.mkv"]);
}

#[cfg(windows)]
#[test]
fn windows_rejects_unrepresentable_names() {
    let dir = TempDir::new();
    let vfs = LocalFs::new(dir.path()).unwrap();
    for bad in ["/a:b", "/CON", "/x\\y"] {
        assert_eq!(
            vfs.stat(&p(bad)).unwrap_err().kind(),
            ErrorKind::InvalidPath,
            "{bad}"
        );
    }
}
