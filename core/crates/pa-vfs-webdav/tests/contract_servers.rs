// SPDX-License-Identifier: MPL-2.0
//! The pa-vfs contract suite against real WebDAV servers in Docker
//! (NET-004 "done when"): nginx + dav-ext, Apache mod_dav, rclone serve
//! webdav, Nextcloud. Run with `tests/servers/run.sh`, which starts the
//! servers, seeds the fixture and sets the variables read here.
#![cfg(feature = "server-tests")]

use pa_vfs::contract::Harness;
use pa_vfs::{ErrorKind, Vfs, VfsPath};
use pa_vfs_webdav::{Credentials, WebDavConfig, WebDavFs};

/// Fixture location on every server (next to unrelated data).
const BASE: &str = "/media/контракт";

fn server(var: &str) -> Harness {
    let url =
        std::env::var(var).unwrap_or_else(|_| panic!("{var} is not set: use tests/servers/run.sh"));
    let user = std::env::var("PA_WEBDAV_USER").unwrap_or_default();
    let pass = std::env::var("PA_WEBDAV_PASS").unwrap_or_default();
    // A 64 KiB read-ahead exercises both cached and ranged reads on the
    // 300 KB sample.
    let config = WebDavConfig::new(url)
        .with_credentials(Credentials::new(user, pass))
        .with_read_ahead(64 * 1024);
    let fs = WebDavFs::new(config).expect("WebDavFs::new");
    Harness::new(fs, VfsPath::parse(BASE).expect("base"))
}

/// The server really requires the credentials (Basic on nginx, rclone and
/// Nextcloud, Digest on Apache), so the suite above went through auth.
fn anonymous_is_refused(var: &str) {
    let url = std::env::var(var).unwrap_or_else(|_| panic!("{var} is not set"));
    let fs = WebDavFs::new(WebDavConfig::new(url)).expect("WebDavFs::new");
    let e = fs
        .stat(&VfsPath::root())
        .expect_err("anonymous access must fail");
    assert_eq!(e.kind(), ErrorKind::AuthenticationFailed, "{e}");
}

macro_rules! server_tests {
    ($($name:ident => $var:literal),+ $(,)?) => {$(
        mod $name {
            use super::*;
            pa_vfs::vfs_contract_tests!(|| server($var));

            #[test]
            fn anonymous_access_is_refused() {
                anonymous_is_refused($var);
            }
        }
    )+};
}

server_tests! {
    nginx => "PA_WEBDAV_NGINX",
    apache => "PA_WEBDAV_APACHE",
    rclone => "PA_WEBDAV_RCLONE",
    nextcloud => "PA_WEBDAV_NEXTCLOUD",
}
