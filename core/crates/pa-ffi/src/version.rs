// SPDX-License-Identifier: MPL-2.0
//! Version of the core library, the first function exported over FFI.

/// Returns the semantic version of the core library (`pa-ffi` crate version).
///
/// Clients call it on startup to check that the native library is loaded and
/// answers over FFI.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_version_is_crate_version() {
        assert_eq!(core_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn core_version_is_semver() {
        let version = core_version();
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 3, "expected MAJOR.MINOR.PATCH, got {version}");
        assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()));
    }
}
