// SPDX-License-Identifier: MPL-2.0
//! Version of the core library.

use flutter_rust_bridge::frb;

/// Returns the semantic version of the core library.
///
/// Same value as `pa_ffi::core_version()` (UniFFI), so every client reports
/// one version. Synchronous on the Dart side: it is cheap and the app calls
/// it on startup to check that the native library is loaded.
#[frb(sync)]
pub fn core_version() -> String {
    pa_ffi::core_version()
}

/// Build flavor of the core: `"store"` or `"full"` (ADR-0007, CORE-013).
///
/// Same value as `pa_ffi::build_flavor()`. The app compares it with its
/// Dart flavor (`--dart-define=KINOLUNE_FLAVOR`) on startup.
#[frb(sync)]
pub fn build_flavor() -> String {
    pa_ffi::build_flavor()
}

/// Full-only capabilities compiled into the core (`twitch-native`,
/// `twitch-archive`, `donation-links`). Empty in store builds.
#[frb(sync)]
pub fn build_capabilities() -> Vec<String> {
    pa_ffi::build_capabilities()
}

/// Initialises the Rust side when Dart calls `RustLib.init()`.
///
/// Sets up flutter_rust_bridge utilities (panic backtraces in debug builds).
#[frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_uniffi_facade() {
        assert_eq!(core_version(), pa_ffi::core_version());
        assert_eq!(build_flavor(), pa_ffi::build_flavor());
        assert_eq!(build_capabilities(), pa_ffi::build_capabilities());
    }
}
