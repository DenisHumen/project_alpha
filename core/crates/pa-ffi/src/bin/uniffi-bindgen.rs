// SPDX-License-Identifier: MPL-2.0
//! `uniffi-bindgen` CLI pinned to the workspace UniFFI version.
//!
//! Used by `just core bindings` in library mode; built only with `--features cli`.

fn main() {
    uniffi::uniffi_bindgen_main()
}
