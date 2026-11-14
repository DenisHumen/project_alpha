// SPDX-License-Identifier: MPL-2.0
//! How the catalog reaches the files of a source (LIB-018).
//!
//! The scan, the change watcher and playback read every source through a
//! [`Vfs`] that a [`Connector`] builds from the `sources` row:
//!
//! - [`LocalConnector`] (the default) opens `file://` folders only;
//! - [`SourcesConnector`] opens every kind the source manager knows
//!   (`pa-sources`, NET-011): local folders, SMB shares through libsmb2
//!   and WebDAV collections, with the password from the secret store
//!   (`pa-secrets`). Nothing is mounted; the app never sees the password.
//!
//! [`SourceOpener`] serves files of any source to the loopback HTTP
//! proxy (`pa-proxy`, NET-007), so players that only take a URL can play
//! network files.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

use pa_vfs::{ByteSource, ErrorKind, LocalFs, Vfs, VfsError, VfsPath};

use crate::{Catalog, Error, Source};

/// Builds the [`Vfs`] of a source.
///
/// Errors are shown to the user (scan status, Sources screen), so their
/// text names the problem and never a secret. A local folder that is not
/// there must fail with a message starting with `folder not found:` (the
/// app classifies by it).
pub trait Connector: Send + Sync + fmt::Debug {
    /// The file system rooted at `source`'s root. May connect lazily: an
    /// unreachable server can also show up on the first call of the `Vfs`.
    fn connect(&self, source: &Source) -> Result<Arc<dyn Vfs>, Error>;
}

/// A local folder source, or the error the scan reports for it.
fn local(source: &Source) -> Result<Arc<dyn Vfs>, Error> {
    let Some(root) = &source.local_root else {
        return Err(Error::Io(format!("not a local folder: {}", source.uri)));
    };
    if !root.is_dir() {
        // Offline drive, unmounted disk: the scan keeps the items.
        return Err(Error::Io(format!("folder not found: {}", root.display())));
    }
    Ok(Arc::new(LocalFs::new(root)?))
}

/// Local folders only: network sources fail with an `unsupported` error.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalConnector;

impl Connector for LocalConnector {
    fn connect(&self, source: &Source) -> Result<Arc<dyn Vfs>, Error> {
        if source.local_root.is_some() {
            return local(source);
        }
        Err(Error::Io(format!(
            "{}: unsupported: no connection for {} sources here",
            source.name, source.kind
        )))
    }
}

/// Every source kind through [`pa_sources::SourceManager::connect`].
#[derive(Debug, Clone)]
pub struct SourcesConnector {
    manager: Arc<pa_sources::SourceManager>,
}

impl SourcesConnector {
    /// Connects through `manager`, which must work on the same library
    /// database as the catalog (the source ids are shared).
    #[must_use]
    pub fn new(manager: Arc<pa_sources::SourceManager>) -> Self {
        Self { manager }
    }
}

impl Connector for SourcesConnector {
    fn connect(&self, source: &Source) -> Result<Arc<dyn Vfs>, Error> {
        if source.local_root.is_some() {
            return local(source);
        }
        self.manager
            .connect(source.id)
            .map(Arc::from)
            .map_err(|e| Error::Io(e.to_string()))
    }
}

impl Catalog {
    /// Replaces how sources are reached (default: [`LocalConnector`]).
    pub fn set_connector(&self, connector: Arc<dyn Connector>) {
        *self
            .connector
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = connector;
    }

    /// The current connector.
    #[must_use]
    pub fn connector(&self) -> Arc<dyn Connector> {
        Arc::clone(
            &self
                .connector
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// The file system of source `source_id`, through the connector.
    pub fn connect(&self, source_id: i64) -> Result<Arc<dyn Vfs>, Error> {
        let source = crate::sources::get(&self.lock(), source_id)?
            .ok_or_else(|| Error::NotFound(format!("source {source_id}")))?;
        self.connector().connect(&source)
    }
}

/// Files recently opened by [`SourceOpener`], kept for the next ranges.
const OPEN_FILES: usize = 4;

/// Serves `<source id>/<path inside the source>` to the loopback proxy
/// (`pa-proxy`): a player seeking in a network file sends a new range
/// request every time, so connections and open files are reused.
///
/// A source that fails is connected again on the next request (the share
/// came back, the password changed).
pub struct SourceOpener {
    catalog: Arc<Catalog>,
    state: Mutex<OpenerState>,
}

#[derive(Default)]
struct OpenerState {
    vfs: HashMap<i64, Arc<dyn Vfs>>,
    files: VecDeque<(i64, VfsPath, Arc<dyn ByteSource>)>,
}

impl fmt::Debug for SourceOpener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceOpener").finish_non_exhaustive()
    }
}

impl SourceOpener {
    /// An opener over the sources of `catalog`.
    #[must_use]
    pub fn new(catalog: Arc<Catalog>) -> Self {
        Self {
            catalog,
            state: Mutex::new(OpenerState::default()),
        }
    }

    /// The proxy resource of a file: `<source id>/<path>` (not encoded).
    #[must_use]
    pub fn resource(source_id: i64, path: &VfsPath) -> String {
        format!("{source_id}{}", path.as_str())
    }

    /// Opens `path` of source `id`, reusing a recent open of the same file.
    pub fn open_file(&self, id: i64, path: &VfsPath) -> pa_vfs::Result<Arc<dyn ByteSource>> {
        let cached_vfs = {
            let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, _, f)) = st.files.iter().find(|(s, p, _)| *s == id && p == path) {
                return Ok(Arc::clone(f));
            }
            st.vfs.get(&id).cloned()
        };
        let opened = match cached_vfs {
            Some(vfs) => match vfs.open(path) {
                Ok(f) => Ok(f),
                // A stale connection: connect once more below.
                Err(e) if e.kind() == ErrorKind::NotFound => Err(e),
                Err(_) => self.fresh(id)?.open(path),
            },
            None => self.fresh(id)?.open(path),
        };
        let file: Arc<dyn ByteSource> = Arc::from(opened?);
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.files.push_front((id, path.clone(), Arc::clone(&file)));
        st.files.truncate(OPEN_FILES);
        Ok(file)
    }

    fn fresh(&self, id: i64) -> pa_vfs::Result<Arc<dyn Vfs>> {
        let vfs = self.catalog.connect(id).map_err(|e| match e {
            Error::NotFound(m) => VfsError::new(ErrorKind::NotFound, m),
            e => VfsError::new(ErrorKind::Unavailable, e.to_string()),
        })?;
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.vfs.insert(id, Arc::clone(&vfs));
        st.files.retain(|(s, _, _)| *s != id);
        Ok(vfs)
    }

    /// Forgets cached connections and open files (sources changed).
    pub fn reset(&self) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.vfs.clear();
        st.files.clear();
    }
}

impl pa_proxy::Opener for SourceOpener {
    fn open(&self, resource: &str) -> pa_vfs::Result<Arc<dyn ByteSource>> {
        let (id, path) = resource
            .split_once('/')
            .ok_or_else(|| VfsError::new(ErrorKind::InvalidPath, "no source id"))?;
        let id: i64 = id
            .parse()
            .map_err(|_| VfsError::new(ErrorKind::InvalidPath, "bad source id"))?;
        self.open_file(id, &VfsPath::parse(path)?)
    }
}
