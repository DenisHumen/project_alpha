// SPDX-License-Identifier: MPL-2.0
//! Behaviour beyond the contract: auth errors, redirects, TLS pinning,
//! read-ahead, URL validation.

mod mock;

use std::sync::atomic::Ordering;
use std::time::Duration;

use mock::{Auth, MockServer, Options, PASS, USER};
use pa_vfs::contract::{self, SAMPLE, SAMPLE_SIZE};
use pa_vfs::{ByteSource, ErrorKind, Vfs, VfsPath};
use pa_vfs_webdav::{
    Credentials, Fingerprint, TlsTrust, WebDavConfig, WebDavFs, fetch_fingerprint,
    is_untrusted_certificate,
};

fn seeded() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    contract::write_fixture(dir.path()).expect("fixture");
    dir
}

fn p(s: &str) -> VfsPath {
    VfsPath::parse(s).unwrap()
}

#[test]
fn wrong_or_missing_password_is_authentication_failed() {
    for auth in [Auth::Basic, Auth::Digest { rotate: 0 }] {
        let dir = seeded();
        let server = MockServer::start(
            dir.path(),
            Options {
                auth,
                ..Options::default()
            },
        );

        let anon = WebDavFs::new(WebDavConfig::new(&server.url)).unwrap();
        let e = anon.list(&VfsPath::root()).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::AuthenticationFailed, "{auth:?}: {e}");

        let bad = WebDavFs::new(
            WebDavConfig::new(&server.url).with_credentials(Credentials::new(USER, "wrong")),
        )
        .unwrap();
        let e = bad.stat(&VfsPath::root()).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::AuthenticationFailed, "{auth:?}: {e}");
        assert!(!e.to_string().contains("wrong"), "password leaked: {e}");

        let good = WebDavFs::new(
            WebDavConfig::new(&server.url).with_credentials(Credentials::new(USER, PASS)),
        )
        .unwrap();
        assert!(good.stat(&p("/dir")).unwrap().is_dir());
    }
}

#[test]
fn credentials_are_sent_preemptively_after_the_first_challenge() {
    let dir = seeded();
    let server = MockServer::start(
        dir.path(),
        Options {
            auth: Auth::Digest { rotate: 0 },
            ..Options::default()
        },
    );
    let fs = WebDavFs::new(
        WebDavConfig::new(&server.url).with_credentials(Credentials::new(USER, PASS)),
    )
    .unwrap();
    fs.stat(&VfsPath::root()).unwrap();
    assert_eq!(
        server.requests.load(Ordering::SeqCst),
        2,
        "challenge + retry"
    );
    for _ in 0..5 {
        fs.stat(&p("/dir")).unwrap();
    }
    assert_eq!(
        server.requests.load(Ordering::SeqCst),
        7,
        "one request per call"
    );
}

#[test]
fn moved_source_follows_redirects_keeping_the_method() {
    let dir = seeded();
    let server = MockServer::start(
        dir.path(),
        Options {
            moved: true,
            slash_redirect: true,
            auth: Auth::Basic,
            ..Options::default()
        },
    );
    let old = format!("{}/old/", server.origin);
    let fs = WebDavFs::new(WebDavConfig::new(old).with_credentials(Credentials::new(USER, PASS)))
        .unwrap();
    // Hrefs in the replies point to /dav/, the moved root.
    assert!(fs.stat(&p("/dir")).unwrap().is_dir());
    let mut names: Vec<_> = fs
        .list(&p("/dir"))
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    names.sort();
    assert_eq!(names, ["nested.txt", "sub"]);
    contract::run_all(&fs, &VfsPath::root());
    let src = fs.open(&p(&format!("/{SAMPLE}"))).unwrap();
    let mut buf = [0u8; 16];
    src.read_exact_at(100, &mut buf).unwrap();
    assert_eq!(buf[..], contract::sample_bytes()[100..116]);
}

#[test]
fn read_ahead_batches_small_reads() {
    let dir = seeded();
    let server = MockServer::start(dir.path(), Options::default());
    let fs = WebDavFs::new(WebDavConfig::new(&server.url).with_read_ahead(64 * 1024)).unwrap();
    let src = fs.open(&p(&format!("/{SAMPLE}"))).unwrap();
    let mut buf = [0u8; 4096];
    // 64 KiB sequentially in 4 KiB reads: one GET.
    for i in 0..16u64 {
        src.read_exact_at(i * 4096, &mut buf).unwrap();
    }
    assert_eq!(server.ranges.lock().unwrap().as_slice(), ["bytes=0-65535"]);
    // The tail block is clamped to the file size.
    let tail = SAMPLE_SIZE as u64 - 100;
    src.read_exact_at(tail, &mut buf[..100]).unwrap();
    assert_eq!(
        server.ranges.lock().unwrap()[1],
        format!("bytes={tail}-{}", SAMPLE_SIZE - 1)
    );
    // Back to the start: still cached.
    src.read_exact_at(0, &mut buf).unwrap();
    assert_eq!(server.ranges.lock().unwrap().len(), 2);
}

#[test]
fn missing_server_is_unavailable() {
    let fs = WebDavFs::new(WebDavConfig {
        connect_timeout: Duration::from_secs(2),
        ..WebDavConfig::new("http://127.0.0.1:9/dav/")
    })
    .unwrap();
    let e = fs.list(&VfsPath::root()).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Unavailable, "{e}");
    assert!(e.kind().is_transient());
}

#[test]
fn bad_urls_are_rejected() {
    for url in ["ftp://h/", "http://user:secret@h/", "nonsense"] {
        let e = WebDavFs::new(WebDavConfig::new(url)).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidPath, "{url}");
        assert!(!e.to_string().contains("secret"), "{e}");
    }
}

#[test]
fn self_signed_certificate_tofu() {
    let dir = seeded();
    let (der, cfg) = mock::self_signed();
    let server = MockServer::start(
        dir.path(),
        Options {
            tls: Some(cfg),
            ..Options::default()
        },
    );
    let pin = Fingerprint::of_der(&der);

    // Not trusted by the platform: a distinct, recognizable error.
    let system = WebDavFs::new(WebDavConfig::new(&server.url)).unwrap();
    let e = system.list(&VfsPath::root()).unwrap_err();
    assert!(is_untrusted_certificate(&e), "{e}");

    // The probe shows the certificate without sending a request.
    let before = server.requests.load(Ordering::SeqCst);
    let seen = fetch_fingerprint(&server.url, Duration::from_secs(5)).unwrap();
    assert_eq!(seen, pin);
    assert_eq!(server.requests.load(Ordering::SeqCst), before);

    // Pinned: works.
    let pinned =
        WebDavFs::new(WebDavConfig::new(&server.url).with_tls(TlsTrust::Pinned(pin))).unwrap();
    assert!(pinned.stat(&p("/dir")).unwrap().is_dir());
    contract::run_all(&pinned, &VfsPath::root());

    // Pinned to another certificate: refused.
    let other = WebDavFs::new(
        WebDavConfig::new(&server.url).with_tls(TlsTrust::Pinned(Fingerprint::of_der(b"other"))),
    )
    .unwrap();
    let e = other.stat(&VfsPath::root()).unwrap_err();
    assert!(is_untrusted_certificate(&e), "{e}");
}
