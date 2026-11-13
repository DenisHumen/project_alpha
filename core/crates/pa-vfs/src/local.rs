// SPDX-License-Identifier: MPL-2.0
//! Local file system backend: a folder on this device or a mounted share.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{ErrorKind, Result, VfsError};
use crate::path::VfsPath;
use crate::source::ByteSource;
use crate::vfs::{DirEntry, EntryKind, Metadata, Vfs};

/// [`Vfs`] over a local directory.
///
/// Every [`VfsPath`] resolves under `root`; `..` cannot be expressed, so a
/// path never leaves the root. Symbolic links are followed like on a
/// network share, including links that point outside the root: the user
/// chose the folder and its links.
///
/// On Windows, names that the platform cannot hold (`\`, `:`, `*`, `?`,
/// `"`, `<`, `>`, `|`, control characters, reserved device names such as
/// `CON` or `nul.txt`) are rejected with [`ErrorKind::InvalidPath`].
#[derive(Debug, Clone)]
pub struct LocalFs {
    root: PathBuf,
}

impl LocalFs {
    /// Opens a local source rooted at `root`, which must be an existing directory.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let meta = fs::metadata(&root).map_err(|e| VfsError::from_io(e, "source root"))?;
        if !meta.is_dir() {
            return Err(VfsError::new(
                ErrorKind::NotADirectory,
                "source root is not a directory",
            ));
        }
        Ok(Self { root })
    }

    /// The directory this source is rooted at.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Maps a [`VfsPath`] to a host path under the root.
    pub fn host_path(&self, path: &VfsPath) -> Result<PathBuf> {
        let mut out = self.root.clone();
        for name in path.components() {
            check_host_name(name)
                .map_err(|why| VfsError::new(ErrorKind::InvalidPath, format!("{path}: {why}")))?;
            out.push(name);
        }
        Ok(out)
    }
}

impl Vfs for LocalFs {
    fn list(&self, dir: &VfsPath) -> Result<Vec<DirEntry>> {
        let host = self.host_path(dir)?;
        let meta = fs::metadata(&host).map_err(|e| VfsError::from_io(e, dir))?;
        if !meta.is_dir() {
            return Err(VfsError::new(ErrorKind::NotADirectory, dir.to_string()));
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&host).map_err(|e| VfsError::from_io(e, dir))? {
            let entry = entry.map_err(|e| VfsError::from_io(e, dir))?;
            // Non-UTF-8 names cannot become a VfsPath; skip them (documented on `Vfs::list`).
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(path) = dir.join(&name) else {
                continue;
            };
            // Entries removed between read_dir and stat are skipped.
            let Some(metadata) = entry_metadata(&entry.path()) else {
                continue;
            };
            out.push(DirEntry {
                name,
                path,
                metadata,
            });
        }
        out.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn stat(&self, path: &VfsPath) -> Result<Metadata> {
        let host = self.host_path(path)?;
        match fs::metadata(&host) {
            Ok(m) => Ok(convert(&m)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => match fs::symlink_metadata(&host) {
                // A dangling symlink exists but cannot be followed.
                Ok(m) => Ok(Metadata {
                    kind: EntryKind::Other,
                    size: 0,
                    modified_ms: m.modified().ok().map(to_unix_ms),
                }),
                Err(_) => Err(VfsError::from_io(e, path)),
            },
            Err(e) => Err(VfsError::from_io(e, path)),
        }
    }

    fn open(&self, path: &VfsPath) -> Result<Box<dyn ByteSource>> {
        let host = self.host_path(path)?;
        // Check the type before opening: opening a FIFO would block.
        let meta = fs::metadata(&host).map_err(|e| VfsError::from_io(e, path))?;
        if meta.is_dir() {
            return Err(VfsError::new(ErrorKind::IsADirectory, path.to_string()));
        }
        if !meta.is_file() {
            return Err(VfsError::new(
                ErrorKind::Unsupported,
                format!("{path}: not a regular file"),
            ));
        }
        let file = File::open(&host).map_err(|e| VfsError::from_io(e, path))?;
        let size = file
            .metadata()
            .map_err(|e| VfsError::from_io(e, path))?
            .len();
        Ok(Box::new(LocalSource {
            file: imp::Handle::new(file),
            size,
            path: path.clone(),
        }))
    }
}

/// An opened local file.
#[derive(Debug)]
struct LocalSource {
    file: imp::Handle,
    size: u64,
    path: VfsPath,
}

impl ByteSource for LocalSource {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        if offset >= self.size || buf.is_empty() {
            return Ok(0);
        }
        // Never return bytes past the size fixed at open, even if the file grew.
        let left = usize::try_from(self.size - offset).unwrap_or(usize::MAX);
        let len = buf.len().min(left);
        loop {
            match self.file.read_at(&mut buf[..len], offset) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                r => return r.map_err(|e| VfsError::from_io(e, &self.path)),
            }
        }
    }
}

fn entry_metadata(host: &Path) -> Option<Metadata> {
    match fs::metadata(host) {
        Ok(m) => Some(convert(&m)),
        // Dangling link or an entry we may not stat: list it as Other.
        Err(_) => fs::symlink_metadata(host).ok().map(|m| Metadata {
            kind: EntryKind::Other,
            size: 0,
            modified_ms: m.modified().ok().map(to_unix_ms),
        }),
    }
}

fn convert(m: &fs::Metadata) -> Metadata {
    let kind = if m.is_file() {
        EntryKind::File
    } else if m.is_dir() {
        EntryKind::Dir
    } else {
        EntryKind::Other
    };
    Metadata {
        kind,
        size: if kind == EntryKind::File { m.len() } else { 0 },
        modified_ms: m.modified().ok().map(to_unix_ms),
    }
}

fn to_unix_ms(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => i64::try_from(e.duration().as_millis()).map_or(i64::MIN, |ms| -ms),
    }
}

#[cfg(windows)]
fn check_host_name(name: &str) -> std::result::Result<(), &'static str> {
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err("character not allowed on Windows");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let reserved = ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
        || ["COM", "LPT"].iter().any(|r| {
            stem.len() == 4
                && stem[..3].eq_ignore_ascii_case(r)
                && matches!(stem.as_bytes()[3], b'1'..=b'9')
        });
    if reserved {
        return Err("reserved device name on Windows");
    }
    Ok(())
}

#[cfg(not(windows))]
fn check_host_name(_name: &str) -> std::result::Result<(), &'static str> {
    Ok(())
}

#[cfg(unix)]
mod imp {
    use std::fs::File;
    use std::io;
    use std::os::unix::fs::FileExt;

    #[derive(Debug)]
    pub(super) struct Handle(File);

    impl Handle {
        pub(super) fn new(file: File) -> Self {
            Self(file)
        }

        pub(super) fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
            self.0.read_at(buf, offset)
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::fs::File;
    use std::io;
    use std::os::windows::fs::FileExt;

    #[derive(Debug)]
    pub(super) struct Handle(File);

    impl Handle {
        pub(super) fn new(file: File) -> Self {
            Self(file)
        }

        /// `seek_read` passes the offset in OVERLAPPED, so concurrent calls
        /// on a synchronous handle do not race on a shared cursor.
        pub(super) fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
            self.0.seek_read(buf, offset)
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    use std::fs::File;
    use std::io::{self, Read, Seek, SeekFrom};
    use std::sync::Mutex;

    #[derive(Debug)]
    pub(super) struct Handle(Mutex<File>);

    impl Handle {
        pub(super) fn new(file: File) -> Self {
            Self(Mutex::new(file))
        }

        pub(super) fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
            let mut f = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            f.seek(SeekFrom::Start(offset))?;
            f.read(buf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_ms_conversion() {
        assert_eq!(to_unix_ms(UNIX_EPOCH), 0);
        assert_eq!(
            to_unix_ms(UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000_123)),
            1_700_000_000_123
        );
        assert_eq!(
            to_unix_ms(UNIX_EPOCH - std::time::Duration::from_millis(5)),
            -5
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_names() {
        for bad in ["a:b", "a\\b", "CON", "con.txt", "LPT1", "x?", "nul .mkv"] {
            assert!(check_host_name(bad).is_err(), "{bad:?}");
        }
        for ok in ["CONSOLE", "COM0", "Дюна.mkv", "a b"] {
            assert!(check_host_name(ok).is_ok(), "{ok:?}");
        }
    }
}
