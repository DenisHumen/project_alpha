// SPDX-License-Identifier: MPL-2.0
// TV-001: what the app reports about itself: Rust core version and build flavour.
import PACore

/// Build flavour (ADR-0007, CORE-013). Chosen at compile time by the Xcode
/// build setting `PA_FLAVOR` (project.yml), which sets the Swift condition
/// `PA_FLAVOR_STORE` or `PA_FLAVOR_FULL`.
enum BuildFlavor: String {
    case store
    case full

    static var current: BuildFlavor {
        #if PA_FLAVOR_FULL
            return .full
        #elseif PA_FLAVOR_STORE
            return .store
        #else
            #error("Set the build setting PA_FLAVOR to store or full (project.yml)")
        #endif
    }
}

enum BuildInfo {
    /// `core_version()` of the Rust core over UniFFI (PACore, CORE-003).
    static var core: String { coreVersion() }

    static var flavor: BuildFlavor { .current }

    /// `build_flavor()` of the Rust core: must equal `flavor`.
    static var coreFlavor: String { buildFlavor() }

    /// Full-only capabilities of the Rust core; empty in store builds.
    static var coreCapabilities: [String] { buildCapabilities() }

    /// One-line status shown on Home and in Settings, e.g. "Core 0.1.0 · store".
    static var summary: String { "Core \(core) · \(flavor.rawValue)" }
}
