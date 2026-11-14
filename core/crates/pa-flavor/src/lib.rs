// SPDX-License-Identifier: MPL-2.0
//! Build flavors `store` and `full` (ADR-0007, ADR-0011 §5, CORE-013).
//!
//! One mechanism for the whole workspace:
//!
//! - **Cargo features** of this crate: `store`, `full`, and the full-only
//!   capabilities `twitch-native`, `twitch-archive`, `donation-links`
//!   (`full` enables all three). Artifact crates (`pa-frb`, `pa-ffi`) have
//!   `store`/`full` features that forward here; tests and tools use
//!   `--features pa-flavor/full`. A build is `store` exactly when no
//!   full-only capability is enabled, so the default is `store`.
//! - **Rust cfg**: crates with full-only code depend on `pa-flavor` and call
//!   [`build::emit_cfgs`] from their build script. Cargo unifies the
//!   features of this crate, so every crate of one build sees the same
//!   `cfg(pa_flavor = "store" | "full")`, `cfg(pa_twitch_native)`,
//!   `cfg(pa_twitch_archive)` and `cfg(pa_donation_links)`. Full-only code
//!   goes under those cfgs and is not compiled into store builds.
//! - **Runtime**: [`Flavor::CURRENT`] and [`enabled_capabilities`]; the
//!   clients read them through `pa-frb` (`buildFlavor()` in Dart) and
//!   `pa-ffi` (`buildFlavor()` in Swift and Kotlin).
//!
//! CI scans store artifacts for the [`full_only_markers`] of the full build
//! and the strings and symbols of full-only code
//! (`tools/flavor/check-artifacts.sh`, `tools/flavor/full-only-patterns.txt`).

use std::fmt;

/// Build flavor of this binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flavor {
    /// App Store, Google Play, Microsoft Store: no full-only capability.
    Store,
    /// GitHub Releases, F-Droid, Flathub, AUR, winget, ISO, Docker:
    /// every full-only capability.
    Full,
}

impl Flavor {
    /// The flavor this binary was compiled as.
    pub const CURRENT: Flavor = if cfg!(pa_flavor = "full") {
        Flavor::Full
    } else {
        Flavor::Store
    };

    /// `"store"` or `"full"`: the name used by every client, the release
    /// workflow and artifact labels.
    pub const fn as_str(self) -> &'static str {
        match self {
            Flavor::Store => "store",
            Flavor::Full => "full",
        }
    }

    /// Parses [`Self::as_str`] back.
    pub fn parse(name: &str) -> Option<Flavor> {
        match name {
            "store" => Some(Flavor::Store),
            "full" => Some(Flavor::Full),
            _ => None,
        }
    }
}

impl fmt::Display for Flavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A full-only capability: compiled only into full builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Native Twitch playback through GQL + usher (TWI-004).
    TwitchNative,
    /// Twitch VOD downloads and live recording (TWI-017, TWI-018).
    TwitchArchive,
    /// External donation links (DEC-003).
    DonationLinks,
}

impl Capability {
    /// Every capability, enabled or not.
    pub const ALL: [Capability; 3] = [
        Capability::TwitchNative,
        Capability::TwitchArchive,
        Capability::DonationLinks,
    ];

    /// Cargo feature name of the capability, e.g. `twitch-archive`.
    pub const fn id(self) -> &'static str {
        match self {
            Capability::TwitchNative => "twitch-native",
            Capability::TwitchArchive => "twitch-archive",
            Capability::DonationLinks => "donation-links",
        }
    }

    /// Whether the capability is compiled into this binary.
    pub const fn is_enabled(self) -> bool {
        match self {
            Capability::TwitchNative => cfg!(pa_twitch_native),
            Capability::TwitchArchive => cfg!(pa_twitch_archive),
            Capability::DonationLinks => cfg!(pa_donation_links),
        }
    }
}

/// Marker strings of the enabled capabilities, one per capability:
/// `kinolune-full-only:<id>`. Only full builds contain them, which gives the
/// artifact check (`tools/flavor/check-artifacts.sh`) a positive control:
/// a full artifact must contain them, a store artifact must not. The client
/// facades return them from `build_capabilities()`, which keeps them in the
/// shipped libraries.
const FULL_ONLY_MARKERS: &[&str] = &[
    #[cfg(pa_twitch_native)]
    "kinolune-full-only:twitch-native",
    #[cfg(pa_twitch_archive)]
    "kinolune-full-only:twitch-archive",
    #[cfg(pa_donation_links)]
    "kinolune-full-only:donation-links",
];

/// See [`FULL_ONLY_MARKERS`]. Empty in store builds.
pub fn full_only_markers() -> &'static [&'static str] {
    FULL_ONLY_MARKERS
}

/// The full-only capabilities compiled into this binary. Empty in store
/// builds. Derived from [`full_only_markers`] (not from a prefix literal,
/// which would end up in store binaries too).
pub fn enabled_capabilities() -> Vec<Capability> {
    full_only_markers()
        .iter()
        .filter_map(|marker| {
            let (_, id) = marker.split_once(':')?;
            Capability::ALL.into_iter().find(|c| c.id() == id)
        })
        .collect()
}

/// Helpers for the build scripts of crates with full-only code.
pub mod build {
    use std::env;

    /// cfgs set from the `links` metadata of `pa-flavor`:
    /// (metadata key in `DEP_PA_FLAVOR_*`, cfg name).
    const CAPABILITY_CFGS: [(&str, &str); 3] = [
        ("TWITCH_NATIVE", "pa_twitch_native"),
        ("TWITCH_ARCHIVE", "pa_twitch_archive"),
        ("DONATION_LINKS", "pa_donation_links"),
    ];

    /// Sets `cfg(pa_flavor = "store" | "full")` and the capability cfgs on
    /// the calling crate, from the features `pa-flavor` was built with.
    ///
    /// The calling crate needs `pa-flavor` both in `[dependencies]` (Cargo
    /// passes `DEP_PA_FLAVOR_*` only to direct dependents) and in
    /// `[build-dependencies]`, and a `build.rs` whose `main` calls:
    ///
    /// ```no_run
    /// pa_flavor::build::emit_cfgs();
    /// ```
    ///
    /// # Panics
    ///
    /// If `pa-flavor` is not a normal dependency of the calling crate.
    pub fn emit_cfgs() {
        println!("cargo::rustc-check-cfg=cfg(pa_flavor, values(\"store\", \"full\"))");
        for (_, cfg) in CAPABILITY_CFGS {
            println!("cargo::rustc-check-cfg=cfg({cfg})");
        }
        let flavor = env::var("DEP_PA_FLAVOR_FLAVOR").expect(
            "DEP_PA_FLAVOR_FLAVOR is unset: add pa-flavor to [dependencies] \
             as well as [build-dependencies]",
        );
        assert!(
            flavor == "store" || flavor == "full",
            "unexpected pa-flavor flavor {flavor:?}"
        );
        println!("cargo::rustc-cfg=pa_flavor=\"{flavor}\"");
        for (key, cfg) in CAPABILITY_CFGS {
            if env::var(format!("DEP_PA_FLAVOR_{key}")).as_deref() == Ok("1") {
                println!("cargo::rustc-cfg={cfg}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for flavor in [Flavor::Store, Flavor::Full] {
            assert_eq!(Flavor::parse(flavor.as_str()), Some(flavor));
            assert_eq!(flavor.to_string(), flavor.as_str());
        }
        assert_eq!(Flavor::parse("Full"), None);
        assert_eq!(Flavor::parse(""), None);
    }

    #[test]
    fn store_means_no_full_only_capability() {
        let enabled = enabled_capabilities();
        assert_eq!(Flavor::CURRENT == Flavor::Store, enabled.is_empty());
        assert_eq!(full_only_markers().len(), enabled.len());
        for c in Capability::ALL {
            assert_eq!(c.is_enabled(), enabled.contains(&c), "{}", c.id());
        }
    }

    #[cfg(pa_flavor = "store")]
    #[test]
    fn store_build_has_no_markers() {
        assert_eq!(Flavor::CURRENT, Flavor::Store);
        assert!(full_only_markers().is_empty());
    }

    // `full` enables every capability.
    #[cfg(all(feature = "full", pa_flavor = "full"))]
    #[test]
    fn full_build_has_every_capability() {
        assert_eq!(Flavor::CURRENT, Flavor::Full);
        assert_eq!(enabled_capabilities(), Capability::ALL.to_vec());
        for (marker, c) in full_only_markers().iter().zip(Capability::ALL) {
            assert_eq!(*marker, format!("kinolune-full-only:{}", c.id()));
        }
    }
}
