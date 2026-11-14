// SPDX-License-Identifier: MPL-2.0
//! [`SmbFs`]: the `Vfs` implementation.

use std::fmt;
use std::sync::Arc;

use pa_smb2::{FileType, Smb2Lib, Stat};
use pa_vfs::{ByteSource, DirEntry, EntryKind, ErrorKind, Metadata, Result, Vfs, VfsError, VfsPath};

use crate::config::SmbConfig;
use crate::pool::{Pool, map_error};
use crate::prefetch::Prefetcher;
use crate::source::SmbSource;

/// An SMB2/3 share (or a folder in it) as a [`Vfs`].
///
/// Cheap to clone and share between threads (`Arc` inside). Connections
/// are opened lazily, pooled and reused; credentials are checked on the
/// first request.
#[derive(Clone)]
pub struct SmbFs {
    pub(crate) inner: Arc<FsInner>,
}

pub(crate) struct FsInner {
    pub(crate) pool: Pool,
    pub(crate) prefetch: Prefetcher,
}

impl Drop for FsInner {
    fn drop(&mut self) {
        self.prefetch.close();
    }
}

impl fmt::Debug for SmbFs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.inner.pool.config();
        write!(f, "SmbFs(smb://{}/{}/{})", c.server, c.share, c.root)
    }
}

impl SmbFs {
    /// Creates the backend. Loads libsmb2 ([`Smb2Lib::shared`] unless the
    /// config names one) but does not connect yet.
    ///
    /// Errors: [`ErrorKind::Unsupported`] if libsmb2 is not available or
    /// SMB1 was requested; [`ErrorKind::InvalidPath`] for an empty server or
    /// a share name with slashes.
    pub fn new(config: SmbConfig) -> Result<Self> {
        if config.allow_smb1 {
            return Err(VfsError::new(
                ErrorKind::Unsupported,
                format!(
                    "smb://{}/{}: SMB1 is not supported (insecure and not implemented by libsmb2); \
                     enable SMB2 or SMB3 on the server",
                    config.server, config.share
                ),
            ));
        }
        if config.server.is_empty()
            || config.share.is_empty()
            || config.share.contains(['/', '\\'])
        {
            return Err(VfsError::new(
                ErrorKind::InvalidPath,
                format!("invalid SMB address: server {:?}, share {:?}", config.server, config.share),
            ));
        }
        let lib = match config.lib {
            Some(lib) => lib,
            None => Smb2Lib::shared().map_err(|e| map_error("libsmb2", &e))?,
        };
        let prefetch = Prefetcher::new(config.prefetch_parallel);
        Ok(Self {
            inner: Arc::new(FsInner {
                pool: Pool::new(lib, config),
                prefetch,
            }),
        })
    }

    /// Opens one connection now and reports the negotiated dialect
    /// (`0x0202` … `0x0311`): a "test connection" for the source manager.
    pub fn check(&self) -> Result<u16> {
        self.inner
            .pool
            .with_conn(&self.url(""), |c| Ok(c.ctx.dialect()))
    }

    /// Path inside the share for a `VfsPath` of this source.
    pub(crate) fn share_path(&self, path: &VfsPath) -> String {
        share_path(&self.inner.pool.config().root, path)
    }

    pub(crate) fn url(&self, share_path: &str) -> String {
        let c = self.inner.pool.config();
        format!("smb://{}/{}/{}", c.server, c.share, share_path)
    }
}

pub(crate) fn share_path(root: &str, path: &VfsPath) -> String {
    let mut out = root.to_owned();
    for c in path.components() {
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(c);
    }
    out
}

pub(crate) fn metadata(st: &Stat) -> Metadata {
    let kind = match st.kind {
        FileType::File => EntryKind::File,
        FileType::Dir => EntryKind::Dir,
        _ => EntryKind::Other,
    };
    let modified_ms = (st.mtime_sec > 0).then(|| {
        let ms = st
            .mtime_sec
            .saturating_mul(1000)
            .saturating_add(st.mtime_nsec / 1_000_000);
        i64::try_from(ms).unwrap_or(i64::MAX)
    });
    Metadata {
        kind,
        size: if kind == EntryKind::File { st.size } else { 0 },
        modified_ms,
    }
}

impl Vfs for SmbFs {
    fn list(&self, dir: &VfsPath) -> Result<Vec<DirEntry>> {
        let p = self.share_path(dir);
        let entries = self
            .inner
            .pool
            .with_conn(&self.url(&p), |c| c.ctx.read_dir(&p))?;
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            // Names VfsPath rejects (control characters) cannot be addressed.
            let Ok(path) = dir.join(&e.name) else { continue };
            out.push(DirEntry {
                metadata: metadata(&e.stat),
                name: e.name,
                path,
            });
        }
        Ok(out)
    }

    fn stat(&self, path: &VfsPath) -> Result<Metadata> {
        let p = self.share_path(path);
        let st = self
            .inner
            .pool
            .with_conn(&self.url(&p), |c| c.ctx.stat(&p))?;
        Ok(metadata(&st))
    }

    fn open(&self, path: &VfsPath) -> Result<Box<dyn ByteSource>> {
        let p = self.share_path(path);
        let url = self.url(&p);
        let st = self.inner.pool.with_conn(&url, |c| {
            let st = c.ctx.stat(&p)?;
            if st.kind == FileType::File {
                // Open on this connection now, so the first read reuses it
                // and errors (sharing violations, access) surface here.
                c.file(&p)?;
            }
            Ok(st)
        })?;
        match st.kind {
            FileType::File => {}
            FileType::Dir => {
                return Err(VfsError::new(ErrorKind::IsADirectory, url));
            }
            _ => {
                return Err(VfsError::new(
                    ErrorKind::Unsupported,
                    format!("{url}: not a regular file"),
                ));
            }
        }
        Ok(Box::new(SmbSource::new(Arc::clone(&self.inner), p, url, st.size)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_join_root_and_components() {
        let p = VfsPath::parse("/Фильмы/a b.mkv").unwrap();
        assert_eq!(share_path("", &p), "Фильмы/a b.mkv");
        assert_eq!(share_path("media/4K", &p), "media/4K/Фильмы/a b.mkv");
        assert_eq!(share_path("", &VfsPath::root()), "");
        assert_eq!(share_path("media", &VfsPath::root()), "media");
    }

    #[test]
    fn stat_maps_to_metadata() {
        let st = Stat {
            kind: FileType::File,
            size: 42,
            mtime_sec: 1_700_000_000,
            mtime_nsec: 123_456_789,
        };
        let m = metadata(&st);
        assert_eq!(m.kind, EntryKind::File);
        assert_eq!(m.size, 42);
        assert_eq!(m.modified_ms, Some(1_700_000_000_123));
        let d = metadata(&Stat {
            kind: FileType::Dir,
            size: 4096,
            mtime_sec: 0,
            mtime_nsec: 0,
        });
        assert_eq!((d.kind, d.size, d.modified_ms), (EntryKind::Dir, 0, None));
        let l = metadata(&Stat {
            kind: FileType::Link,
            ..st
        });
        assert_eq!((l.kind, l.size), (EntryKind::Other, 0));
    }
}
