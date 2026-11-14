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

/// Build flavor of the core: `"store"` or `"full"` (ADR-0007, CORE-013).
///
/// Clients compare it with their own compile-time flavor (Swift
/// `PA_FLAVOR_*`, Gradle flavor, Dart `KINOLUNE_FLAVOR`) on startup: a
/// mismatch is a packaging error.
#[uniffi::export]
pub fn build_flavor() -> String {
    pa_flavor::Flavor::CURRENT.as_str().to_owned()
}

/// Full-only capabilities compiled into the core, e.g. `twitch-native`.
/// Empty in store builds.
#[uniffi::export]
pub fn build_capabilities() -> Vec<String> {
    // From the markers, which keeps them in the shipped library for the
    // artifact check (tools/flavor/check-artifacts.sh).
    pa_flavor::full_only_markers()
        .iter()
        .filter_map(|m| m.split_once(':').map(|(_, id)| id.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_flavor_matches_capabilities() {
        let flavor = build_flavor();
        let caps = build_capabilities();
        assert_eq!(
            pa_flavor::Flavor::parse(&flavor),
            Some(pa_flavor::Flavor::CURRENT)
        );
        assert_eq!(flavor == "store", caps.is_empty(), "{flavor}: {caps:?}");
        let expected: Vec<String> = pa_flavor::enabled_capabilities()
            .iter()
            .map(|c| c.id().to_owned())
            .collect();
        assert_eq!(caps, expected);
    }

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
