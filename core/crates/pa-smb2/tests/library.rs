// SPDX-License-Identifier: MPL-2.0
//! Tests against a real libsmb2 build: `PA_SMB2_LIB` points at the shared
//! library from `tools/libsmb2/out/<target>/`. Skipped (they pass without
//! checking anything) when the variable is unset, like the player I/O
//! tests (ADR-0027).

use pa_smb2::{Context, Encryption, ErrorKind, LIB_ENV, Signing, Smb2Lib, Version};

fn lib() -> Option<&'static Smb2Lib> {
    if std::env::var_os(LIB_ENV).is_none_or(|v| v.is_empty()) {
        eprintln!("{LIB_ENV} is not set: skipping libsmb2 tests");
        return None;
    }
    Some(Smb2Lib::shared().expect("load libsmb2 from PA_SMB2_LIB"))
}

#[test]
fn missing_library_is_reported() {
    let err = Smb2Lib::load("/nonexistent/libsmb2.so").expect_err("must fail");
    assert_eq!(err.kind(), ErrorKind::LibraryUnavailable);
    assert!(err.message().contains("cannot load"), "{err}");
}

#[test]
fn not_libsmb2_is_rejected() {
    // Any shared library without the smb2_* functions: the C runtime.
    let libc = if cfg!(target_os = "macos") {
        "/usr/lib/libSystem.B.dylib"
    } else if cfg!(target_os = "linux") {
        "libc.so.6"
    } else {
        return;
    };
    let err = Smb2Lib::load(libc).expect_err("libc is not libsmb2");
    assert_eq!(err.kind(), ErrorKind::LibraryUnavailable);
    assert!(err.message().contains("smb2_init_context"), "{err}");
}

#[test]
fn loads_and_configures_a_context() {
    let Some(lib) = lib() else { return };
    let mut ctx = Context::new(lib).expect("context");
    ctx.set_timeout(5);
    ctx.set_version(Version::Any3);
    ctx.set_signing(Signing::Required);
    ctx.set_encryption(Encryption::Required);
    ctx.set_credentials("guest", "pass\u{e9}word", Some("WORKGROUP"))
        .expect("credentials");
    ctx.set_workstation("kinolune-test").expect("workstation");
    assert!(!ctx.is_connected());
    assert_eq!(ctx.open_files(), 0);
    assert!(ctx.set_credentials("a", "nul\0inside", None).is_err());
}

#[test]
fn closed_port_is_unavailable() {
    let Some(lib) = lib() else { return };
    let mut ctx = Context::new(lib).expect("context");
    ctx.set_timeout(5);
    // Port 1 on loopback is closed on test machines.
    let err = ctx
        .connect("127.0.0.1:1", "share", Some("guest"))
        .expect_err("closed port");
    assert_eq!(err.kind(), ErrorKind::Unavailable, "{err}");
    assert!(!ctx.is_connected());
}

#[test]
fn many_contexts_on_threads() {
    let Some(lib) = lib() else { return };
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(move || {
                let mut ctx = Context::new(lib).expect("context");
                ctx.set_timeout(5);
                ctx.connect("127.0.0.1:1", "share", None).is_err()
            })
        })
        .collect();
    for t in threads {
        assert!(t.join().expect("thread"));
    }
}
