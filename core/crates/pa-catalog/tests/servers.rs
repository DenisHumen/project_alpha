// SPDX-License-Identifier: MPL-2.0
//! LIB-018 "done when": an SMB share (Samba) and a WebDAV collection
//! (nginx) are scanned and their files streamed through the loopback
//! proxy, with the password only in the secret store and nothing mounted.
//! Run with `tests/servers/run.sh`, which starts the servers, seeds the
//! data and sets the variables read here.
#![cfg(feature = "server-tests")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;

use pa_catalog::{Catalog, SourceOpener, SourcesConnector};
use pa_proxy::{Proxy, ProxyOptions};
use pa_scanner::CancelToken;
use pa_secrets::{MemoryStore, SecretString};
use pa_sources::{NewSource, SourceKind, SourceManager};
use pa_vfs::VfsPath;

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set: use tests/servers/run.sh"))
}

struct Setup {
    _dir: tempfile::TempDir,
    cat: Arc<Catalog>,
    manager: Arc<SourceManager>,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("library.db");
    let cat = Arc::new(Catalog::open(&db).unwrap());
    let manager = Arc::new(SourceManager::open(&db, Arc::new(MemoryStore::new())).unwrap());
    cat.set_connector(Arc::new(SourcesConnector::new(Arc::clone(&manager))));
    Setup {
        _dir: dir,
        cat,
        manager,
    }
}

fn add(s: &Setup, kind: SourceKind, name: &str, address: &str, password: &str) -> i64 {
    s.manager
        .create(
            NewSource::new(kind, name, address)
                .with_login(var("PA_SERVERS_USER"), SecretString::new(password)),
        )
        .unwrap()
        .id
}

fn scan(cat: &Catalog, id: i64) -> Vec<String> {
    let mut errors = Vec::new();
    cat.scan(Some(id), &CancelToken::new(), &mut |u| {
        errors.extend(u.error)
    })
    .unwrap();
    errors
}

fn titles(cat: &Catalog) -> Vec<String> {
    let mut t: Vec<_> = cat.movies().unwrap().into_iter().map(|m| m.title).collect();
    t.sort();
    t
}

fn get(proxy: &Proxy, url: &str, range: &str) -> (u16, Vec<u8>) {
    let rest = url.strip_prefix("http://").unwrap();
    let path = &rest[rest.find('/').unwrap()..];
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
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        reply[split + 4..].to_vec(),
    )
}

/// Scans the source, checks the library and streams two ranges of a file
/// against the seeded data.
fn scan_and_stream(kind: SourceKind, address: &str) {
    let s = setup();
    let id = add(&s, kind, "NAS", address, &var("PA_SERVERS_PASS"));
    assert_eq!(scan(&s.cat, id), Vec::<String>::new());
    assert_eq!(titles(&s.cat), ["Dune", "The Matrix", "Брат"]);
    let shows = s.cat.shows().unwrap();
    assert_eq!(shows.len(), 1);
    assert_eq!(s.cat.episodes(shows[0].id).unwrap().len(), 2);

    let rel = "Фильмы/Брат (1997)/Брат.1997.BDRip.mkv";
    let local = std::fs::read(PathBuf::from(var("PA_SERVERS_DATA")).join(rel)).unwrap();
    let proxy = Proxy::start(
        Arc::new(SourceOpener::new(Arc::clone(&s.cat))),
        ProxyOptions::default(),
    )
    .unwrap();
    let url = proxy.url(&SourceOpener::resource(id, &VfsPath::parse(rel).unwrap()));
    let (status, body) = get(&proxy, &url, "bytes=100000-199999");
    assert_eq!(status, 206);
    assert_eq!(body, local[100_000..200_000]);
    let (_, tail) = get(&proxy, &url, "bytes=-4096");
    assert_eq!(tail, local[local.len() - 4096..]);
    let (status, all) = get(&proxy, &url, "bytes=0-");
    assert_eq!(status, 206);
    assert_eq!(all, local);

    // An incremental rescan goes through the same connection path.
    let r = s
        .cat
        .rescan_folders(
            id,
            &[VfsPath::parse("/Movies").unwrap()],
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!((r.files_seen, r.added), (2, 0));
}

#[test]
fn smb_share_is_scanned_and_streamed() {
    scan_and_stream(SourceKind::Smb, &var("PA_SERVERS_SMB"));
}

#[test]
fn webdav_collection_is_scanned_and_streamed() {
    scan_and_stream(SourceKind::WebDav, &var("PA_SERVERS_WEBDAV"));
}

#[test]
fn a_wrong_password_is_reported_without_leaking_it() {
    let s = setup();
    let wrong = "wrong-pass-0451";
    let smb = add(&s, SourceKind::Smb, "NAS", &var("PA_SERVERS_SMB"), wrong);
    let dav = add(
        &s,
        SourceKind::WebDav,
        "DAV",
        &var("PA_SERVERS_WEBDAV"),
        wrong,
    );
    for id in [smb, dav] {
        let errors = scan(&s.cat, id);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("authentication failed"), "{errors:?}");
        assert!(!errors[0].contains(wrong));
    }
    assert!(titles(&s.cat).is_empty());
}
