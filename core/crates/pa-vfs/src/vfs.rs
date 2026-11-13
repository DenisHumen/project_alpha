// SPDX-License-Identifier: MPL-2.0
//! The `Vfs` trait and directory listing types.

use std::fmt;

use crate::error::Result;
use crate::path::VfsPath;
use crate::source::ByteSource;

/// Kind of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum EntryKind {
    /// A regular file; can be opened.
    File,
    /// A directory; can be listed.
    Dir,
    /// Anything else: devices, sockets, broken links, entries the backend
    /// cannot classify. The scanner skips them.
    Other,
}

/// What `stat` and `list` report about an entry.
///
/// Symbolic links are followed: a link to a file is a [`EntryKind::File`]
/// with the target's size. A link whose target is missing is
/// [`EntryKind::Other`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Metadata {
    /// File, directory or other.
    pub kind: EntryKind,
    /// Size in bytes for files; `0` for directories and other entries.
    pub size: u64,
    /// Last modification time, Unix milliseconds (as in `pa-model`);
    /// `None` if the backend does not report it.
    pub modified_ms: Option<i64>,
}

impl Metadata {
    /// `true` for [`EntryKind::File`].
    #[must_use]
    pub fn is_file(&self) -> bool {
        self.kind == EntryKind::File
    }

    /// `true` for [`EntryKind::Dir`].
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }
}

/// One entry returned by [`Vfs::list`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirEntry {
    /// The entry name (last path component).
    pub name: String,
    /// Full path: the listed directory joined with `name`.
    pub path: VfsPath,
    /// Same data as [`Vfs::stat`] on `path`.
    pub metadata: Metadata,
}

/// A browsable media source: a local folder, an SMB share, an NFS export,
/// a WebDAV collection, an SFTP directory.
///
/// One `Vfs` value is bound to one source root (and one set of credentials);
/// every path is relative to that root. The trait is object-safe and the
/// methods are blocking (ADR-0013): the scanner and the source manager hold
/// `Arc<dyn Vfs>` and call it from worker threads.
///
/// Every backend must pass the shared [`contract`](crate::contract) suite.
pub trait Vfs: Send + Sync + fmt::Debug {
    /// Lists the direct children of `dir`, without `.` and `..`.
    ///
    /// The order is unspecified. Hidden and system files are returned as
    /// is: filtering is the scanner's job. Entries whose names the backend
    /// cannot represent as UTF-8 are skipped.
    ///
    /// Errors: [`NotFound`](crate::ErrorKind::NotFound),
    /// [`NotADirectory`](crate::ErrorKind::NotADirectory) when `dir` is a file.
    fn list(&self, dir: &VfsPath) -> Result<Vec<DirEntry>>;

    /// Metadata of one entry. The root `/` is always a directory.
    ///
    /// Errors: [`NotFound`](crate::ErrorKind::NotFound).
    fn stat(&self, path: &VfsPath) -> Result<Metadata>;

    /// Opens a file for positional reads.
    ///
    /// Errors: [`NotFound`](crate::ErrorKind::NotFound),
    /// [`IsADirectory`](crate::ErrorKind::IsADirectory) when `path` is a directory.
    fn open(&self, path: &VfsPath) -> Result<Box<dyn ByteSource>>;
}

impl<T: Vfs + ?Sized> Vfs for Box<T> {
    fn list(&self, dir: &VfsPath) -> Result<Vec<DirEntry>> {
        (**self).list(dir)
    }
    fn stat(&self, path: &VfsPath) -> Result<Metadata> {
        (**self).stat(path)
    }
    fn open(&self, path: &VfsPath) -> Result<Box<dyn ByteSource>> {
        (**self).open(path)
    }
}

impl<T: Vfs + ?Sized> Vfs for std::sync::Arc<T> {
    fn list(&self, dir: &VfsPath) -> Result<Vec<DirEntry>> {
        (**self).list(dir)
    }
    fn stat(&self, path: &VfsPath) -> Result<Metadata> {
        (**self).stat(path)
    }
    fn open(&self, path: &VfsPath) -> Result<Box<dyn ByteSource>> {
        (**self).open(path)
    }
}
