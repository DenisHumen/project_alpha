// SPDX-License-Identifier: MPL-2.0
//! Sources reached through a connector (LIB-018): a network source is
//! scanned, rescanned, followed by the watcher and played through the
//! loopback proxy without any local path; the source manager's connector
//! reaches local folders and reports unreachable shares without leaking
//! the password. Real Samba and WebDAV: `tests/servers.rs`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use pa_catalog::watch::{LibraryWatcher, WatchMode, WatchOptions};
use pa_catalog::{Catalog, Connector, Error, Source, SourceOpener, SourcesConnector};
use pa_proxy::{Proxy, ProxyOptions};
use pa_scanner::CancelToken;
use pa_secrets::{MemoryStore, SecretString};
use pa_sources::{NewSource, SourceKind, SourceManager};
use pa_vfs::{ByteSource, DirEntry, EntryKind, ErrorKind, Metadata, Vfs, VfsError, VfsPath};

/// A network share in memory: files by path, folders implied.
#[derive(Debug, Default)]
struct MemFs {
    files: Mutex<BTreeMap<String, Arc<Vec<u8>>>>,
}

#[derive(Debug)]
struct MemFile(Arc<Vec<u8>>);

impl ByteSource for MemFile {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> pa_vfs::Result<usize> {
        let start = usize::try_from(offset).unwrap_or(usize::MAX).min(self.0.len());
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(n)
    }
}

fn content(seed: u8) -> Vec<u8> {
    (0..120_000u32)
        .map(|i| (i as u8).wrapping_mul(29).wrapping_add(seed))
        .collect()
}

impl MemFs {
    fn put(&self, path: &str, seed: u8) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), Arc::new(content(seed)));
    }

    fn file(size: usize) -> Metadata {
        Metadata {
            kind: EntryKind::File,
            size: size as u64,
            modified_ms: Some(1_700_000_000_000),
        }
    }

    fn dir() -> Metadata {
        Metadata {
            kind: EntryKind::Dir,
            size: 0,
            modified_ms: None,
        }
    }
}

impl Vfs for MemFs {
    fn list(&self, dir: &VfsPath) -> pa_vfs::Result<Vec<DirEntry>> {
        let prefix = if dir.is_root() {
            "/".to_owned()
        } else {
            format!("{}/", dir.as_str())
        };
        let files = self.files.lock().unwrap();
        let mut out: BTreeMap<String, Metadata> = BTreeMap::new();
        for (path, data) in files.iter() {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((d, _)) => {
                    out.entry(d.to_owned()).or_insert_with(MemFs::dir);
                }
                None => {
                    out.insert(rest.to_owned(), MemFs::file(data.len()));
                }
            }
        }
        if out.is_empty() && !dir.is_root() {
            return Err(VfsError::new(ErrorKind::NotFound, dir.as_str()));
        }
        out.into_iter()
            .map(|(name, metadata)| {
                Ok(DirEntry {
                    path: dir.join(&name)?,
                    name,
                    metadata,
                })
            })
            .collect()
    }

    fn stat(&self, path: &VfsPath) -> pa_vfs::Result<Metadata> {
        if path.is_root() {
            return Ok(MemFs::dir());
        }
        let files = self.files.lock().unwrap();
        if let Some(d) = files.get(path.as_str()) {
            return Ok(MemFs::file(d.len()));
        }
        let prefix = format!("{}/", path.as_str());
        if files.keys().any(|k| k.starts_with(&prefix)) {
            return Ok(MemFs::dir());
        }
        Err(VfsError::new(ErrorKind::NotFound, path.as_str()))
    }

    fn open(&self, path: &VfsPath) -> pa_vfs::Result<Box<dyn ByteSource>> {
        match self.files.lock().unwrap().get(path.as_str()) {
            Some(d) => Ok(Box::new(MemFile(Arc::clone(d)))),
            None => Err(VfsError::new(ErrorKind::NotFound, path.as_str())),
        }
    }
}

/// Connects `smb` sources to one shared [`MemFs`] and counts connections.
#[derive(Debug)]
struct MemConnector {
    fs: Arc<MemFs>,
    online: Mutex<bool>,
    connects: AtomicUsize,
}

impl Connector for MemConnector {
    fn connect(&self, source: &Source) -> Result<Arc<dyn Vfs>, Error> {
        self.connects.fetch_add(1, Ordering::Relaxed);
        if source.kind != "smb" {
            return Err(Error::Invalid(source.kind.clone()));
        }
        if !*self.online.lock().unwrap() {
            return Err(Error::Io(format!(
                "source {}: source unavailable: connection refused",
                source.id
            )));
        }
        Ok(Arc::clone(&self.fs) as Arc<dyn Vfs>)
    }
}

fn share() -> Arc<MemFs> {
    let fs = Arc::new(MemFs::default());
    fs.put("/Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv", 1);
    fs.put("/Movies/The Matrix 1999.mkv", 2);
    fs.put("/Shows/Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv", 3);
    fs
}

fn setup() -> (tempfile::TempDir, Arc<Catalog>, Arc<MemConnector>, i64) {
    let dir = tempfile::tempdir().unwrap();
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    let id = cat
        .add_source(None, "smb", "NAS", "smb://nas.local/media", Some("anna"))
        .unwrap()
        .id;
    let connector = Arc::new(MemConnector {
        fs: share(),
        online: Mutex::new(true),
        connects: AtomicUsize::new(0),
    });
    cat.set_connector(Arc::clone(&connector) as Arc<dyn Connector>);
    (dir, Arc::new(cat), connector, id)
}

fn titles(cat: &Catalog) -> Vec<String> {
    let mut t: Vec<_> = cat.movies().unwrap().into_iter().map(|m| m.title).collect();
    t.sort();
    t
}

fn scan_errors(cat: &Catalog) -> Vec<String> {
    let mut errors = Vec::new();
    cat.scan(None, &CancelToken::new(), &mut |u| errors.extend(u.error))
        .unwrap();
    errors
}

#[test]
fn a_network_source_is_scanned_through_the_connector() {
    let (_dir, cat, _c, _id) = setup();
    assert_eq!(scan_errors(&cat), Vec::<String>::new());
    assert_eq!(titles(&cat), ["Dune", "The Matrix"]);
    assert_eq!(cat.shows().unwrap().len(), 1);
    // Fingerprints were read through the connector too.
    let conn = pa_library::open(cat.path()).unwrap();
    let hashed: i64 = conn
        .query_row("SELECT count(*) FROM files WHERE oshash IS NOT NULL", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(hashed, 3);
}

#[test]
fn without_a_network_connector_the_scan_reports_and_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir_all(media.join("Heat (1995)")).unwrap();
    std::fs::write(media.join("Heat (1995)/Heat.1995.mkv"), content(5)).unwrap();
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "smb", "NAS", "smb://nas.local/media", None)
        .unwrap();
    cat.add_source(None, "local", "Disk", media.to_str().unwrap(), None)
        .unwrap();
    let errors = scan_errors(&cat);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("unsupported"), "{errors:?}");
    assert_eq!(titles(&cat), ["Heat"]);
}

#[test]
fn an_offline_share_keeps_its_items_and_records_the_error() {
    let (_dir, cat, connector, id) = setup();
    scan_errors(&cat);
    *connector.online.lock().unwrap() = false;
    let errors = scan_errors(&cat);
    assert!(errors[0].contains("source unavailable"), "{errors:?}");
    assert_eq!(titles(&cat), ["Dune", "The Matrix"]);
    let conn = pa_library::open(cat.path()).unwrap();
    let last: Option<String> = conn
        .query_row("SELECT last_error FROM sources WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(last.unwrap().contains("source unavailable"));
}

#[test]
fn folders_of_a_network_source_are_rescanned() {
    let (_dir, cat, connector, id) = setup();
    scan_errors(&cat);
    connector.fs.put("/Movies/Heat (1995)/Heat.1995.mkv", 9);
    let r = cat
        .rescan_folders(id, &[VfsPath::parse("/Movies").unwrap()], &CancelToken::new())
        .unwrap();
    assert_eq!((r.files_seen, r.added), (3, 1));
    assert_eq!(titles(&cat), ["Dune", "Heat", "The Matrix"]);
}

#[test]
fn the_watcher_follows_network_sources_on_the_timer() {
    let (_dir, cat, connector, id) = setup();
    let (tx, rx) = mpsc::channel();
    let watcher = LibraryWatcher::start(
        Arc::clone(&cat),
        WatchOptions {
            network_interval: Some(Duration::from_millis(300)),
            ..WatchOptions::default()
        },
        Box::new(move |e| {
            let _ = tx.send(e);
        }),
    )
    .unwrap();
    assert_eq!(watcher.sources().len(), 1);
    assert_eq!(watcher.sources()[0].mode, WatchMode::Timer);
    assert_eq!(watcher.sources()[0].root, None);

    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(first.full && first.error.is_none(), "{first:?}");
    assert_eq!(titles(&cat), ["Dune", "The Matrix"]);

    connector.fs.put("/Movies/Heat (1995)/Heat.1995.mkv", 9);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !titles(&cat).contains(&"Heat".to_owned()) {
        assert!(std::time::Instant::now() < deadline, "not rescanned");
        let e = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(e.source_id, id);
    }
    drop(watcher);
}

fn get(proxy: &Proxy, url: &str, range: &str) -> (u16, Vec<u8>) {
    let path = &url[url.find("//").unwrap() + 2..];
    let path = &path[path.find('/').unwrap()..];
    let mut s = TcpStream::connect(proxy.local_addr()).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nRange: {range}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut reply = Vec::new();
    s.read_to_end(&mut reply).unwrap();
    let split = reply.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&reply[..split]).into_owned();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, reply[split + 4..].to_vec())
}

#[test]
fn network_files_play_through_the_proxy() {
    let (_dir, cat, connector, id) = setup();
    let opener = Arc::new(SourceOpener::new(Arc::clone(&cat)));
    let proxy = Proxy::start(opener, ProxyOptions::default()).unwrap();
    let path = VfsPath::parse("/Movies/Dune (2021)/Dune.2021.2160p.WEB-DL.mkv").unwrap();
    let url = proxy.url(&SourceOpener::resource(id, &path));
    let all = content(1);

    let (status, body) = get(&proxy, &url, "bytes=1000-1999");
    assert_eq!(status, 206);
    assert_eq!(body, all[1000..2000]);
    // Seeking again reuses the connection and the open file.
    let before = connector.connects.load(Ordering::Relaxed);
    let (_, body) = get(&proxy, &url, "bytes=-100");
    assert_eq!(body, all[all.len() - 100..]);
    assert_eq!(connector.connects.load(Ordering::Relaxed), before);

    let missing = proxy.url(&SourceOpener::resource(id, &VfsPath::parse("/nope.mkv").unwrap()));
    assert_eq!(get(&proxy, &missing, "bytes=0-").0, 404);
    let unknown = proxy.url(&SourceOpener::resource(999, &path));
    assert_eq!(get(&proxy, &unknown, "bytes=0-").0, 404);
}

#[test]
fn the_source_manager_connector_reaches_local_folders_and_masks_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media");
    std::fs::create_dir_all(media.join("Heat (1995)")).unwrap();
    std::fs::write(media.join("Heat (1995)/Heat.1995.mkv"), content(5)).unwrap();
    let db = dir.path().join("library.db");
    let cat = Catalog::open(&db).unwrap();
    let manager = Arc::new(SourceManager::open(&db, Arc::new(MemoryStore::new())).unwrap());
    manager
        .create(NewSource::new(
            SourceKind::Local,
            "Disk",
            pa_sources::path_to_file_uri(&media),
        ))
        .unwrap();
    // Nothing listens on port 9: the connection is refused at once.
    let secret = "pa55-very-secret";
    manager
        .create(
            NewSource::new(SourceKind::WebDav, "DAV", "http://127.0.0.1:9/dav/")
                .with_login("anna", SecretString::new(secret)),
        )
        .unwrap();
    cat.set_connector(Arc::new(SourcesConnector::new(manager)));
    let errors = scan_errors(&cat);
    assert_eq!(titles(&cat), ["Heat"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("DAV: "), "{errors:?}");
    assert!(!errors[0].contains(secret));
    let conn = pa_library::open(&db).unwrap();
    let stored: Vec<String> = conn
        .prepare("SELECT ifnull(last_error, '') FROM sources")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(stored.iter().all(|e| !e.contains(secret)));
}
