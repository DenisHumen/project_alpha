// SPDX-License-Identifier: MPL-2.0
//! The pa-vfs contract suite against the in-process WebDAV server, in the
//! configurations real servers differ in.

mod mock;

use mock::{Auth, MockServer, Options, PASS, USER};
use pa_vfs::VfsPath;
use pa_vfs::contract::{self, Harness};
use pa_vfs_webdav::{Credentials, WebDavConfig, WebDavFs};

fn harness(opts: Options, config: impl FnOnce(String) -> WebDavConfig) -> Harness {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.path().join("media").join("контракт");
    std::fs::create_dir_all(&base).expect("base");
    std::fs::write(dir.path().join("media").join("other.txt"), b"x").expect("other");
    contract::write_fixture(&base).expect("fixture");
    let server = MockServer::start(dir.path(), opts);
    let fs = WebDavFs::new(config(server.url.clone())).expect("WebDavFs::new");
    Harness::new(fs, VfsPath::parse("/media/контракт").expect("base")).with_guard((server, dir))
}

fn creds() -> Credentials {
    Credentials::new(USER, PASS)
}

/// Anonymous, with read-ahead (the default configuration).
mod plain {
    use super::*;
    pa_vfs::vfs_contract_tests!(|| harness(Options::default(), WebDavConfig::new));
}

/// Every read is its own ranged GET.
mod no_read_ahead {
    use super::*;
    pa_vfs::vfs_contract_tests!(|| harness(Options::default(), |u| {
        WebDavConfig::new(u).with_read_ahead(0)
    }));
}

/// Basic auth, nginx-style slash redirects on collections.
mod basic_redirects {
    use super::*;
    pa_vfs::vfs_contract_tests!(|| harness(
        Options { auth: Auth::Basic, slash_redirect: true, ..Options::default() },
        |u| WebDavConfig::new(u).with_credentials(creds())
    ));
}

/// Digest auth with a nonce that goes stale every 7 requests; small
/// read-ahead blocks so reads cross block borders.
mod digest_stale {
    use super::*;
    pa_vfs::vfs_contract_tests!(|| harness(
        Options { auth: Auth::Digest { rotate: 7 }, ..Options::default() },
        |u| WebDavConfig::new(u).with_credentials(creds()).with_read_ahead(10_000)
    ));
}

/// A server that ignores `Range` and does not report sizes in PROPFIND.
mod no_range_no_length {
    use super::*;
    pa_vfs::vfs_contract_tests!(|| harness(
        Options { ignore_range: true, no_length: true, ..Options::default() },
        |u| WebDavConfig::new(u).with_read_ahead(64 * 1024)
    ));
}
