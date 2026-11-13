// SPDX-License-Identifier: MPL-2.0
//! Calls the exported C ABI exactly like generated Kotlin and Swift bindings do:
//! through the `extern "C"` scaffolding symbols, `RustCallStatus` and `RustBuffer`.

use uniffi::{RustBuffer, RustCallStatus, RustCallStatusCode};

unsafe extern "C" {
    fn ffi_pa_ffi_uniffi_contract_version() -> u32;
    fn uniffi_pa_ffi_checksum_func_core_version() -> u16;
    fn uniffi_pa_ffi_fn_func_core_version(call_status: &mut RustCallStatus) -> RustBuffer;
}

// Keeps the rlib (and its `#[no_mangle]` symbols) linked into this test binary.
#[allow(dead_code)]
static LINK: fn() -> String = pa_ffi::core_version;

#[test]
fn contract_version_is_exported() {
    // Bindings check this on load and refuse a library with a different contract.
    let version = unsafe { ffi_pa_ffi_uniffi_contract_version() };
    assert_ne!(version, 0);
}

#[test]
fn core_version_has_api_checksum() {
    // Bindings compare this checksum with the one baked in at generation time.
    let _ = unsafe { uniffi_pa_ffi_checksum_func_core_version() };
}

#[test]
fn core_version_over_c_abi() {
    let mut status = RustCallStatus::default();
    let buf = unsafe { uniffi_pa_ffi_fn_func_core_version(&mut status) };
    assert!(matches!(status.code, RustCallStatusCode::Success));

    // A returned `String` is lowered as a RustBuffer with raw UTF-8 bytes.
    let version = String::from_utf8(buf.destroy_into_vec()).expect("UTF-8");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert_eq!(version, pa_ffi::core_version());
}
