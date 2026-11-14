// SPDX-License-Identifier: MPL-2.0
//! Behaviour that needs no SMB server. Tests that need libsmb2 itself read
//! `PA_SMB2_LIB` and are skipped when it is unset.

use pa_smb2::{LIB_ENV, Smb2Lib};
use pa_vfs::{ErrorKind, Vfs, VfsPath};
use pa_vfs_smb::{SmbConfig, SmbFs};

fn lib() -> Option<&'static Smb2Lib> {
    if std::env::var_os(LIB_ENV).is_none_or(|v| v.is_empty()) {
        eprintln!("{LIB_ENV} is not set: skipping");
        return None;
    }
    Some(Smb2Lib::shared().expect("load libsmb2"))
}

#[test]
fn smb1_flag_is_refused_with_advice() {
    let e = SmbFs::new(SmbConfig::new("nas", "media").with_smb1(true)).expect_err("SMB1");
    assert_eq!(e.kind(), ErrorKind::Unsupported);
    assert!(e.to_string().contains("enable SMB2"), "{e}");
}

#[test]
fn bad_share_names_are_refused() {
    for (server, share) in [("", "media"), ("nas", ""), ("nas", "a/b"), ("nas", "a\\b")] {
        let e = SmbFs::new(SmbConfig::new(server, share)).expect_err("invalid");
        assert_eq!(e.kind(), ErrorKind::InvalidPath, "{server:?} {share:?}");
    }
}

#[test]
fn unreachable_server_is_unavailable() {
    let Some(lib) = lib() else { return };
    let fs = SmbFs::new(
        SmbConfig::new("127.0.0.1:1", "media")
            .with_library(lib)
            .with_timeout(std::time::Duration::from_secs(5)),
    )
    .expect("SmbFs::new");
    let e = fs.stat(&VfsPath::root()).expect_err("closed port");
    assert_eq!(e.kind(), ErrorKind::Unavailable, "{e}");
    assert!(e.kind().is_transient());
    assert!(e.to_string().contains("smb://127.0.0.1:1/media"), "{e}");
}
