// SPDX-License-Identifier: MPL-2.0
// TV-001: what the app reports about itself: Rust core version and build flavour.
import PACore

/// Build flavour (ADR-0007). Chosen at compile time by the Xcode build settings
/// (`SWIFT_ACTIVE_COMPILATION_CONDITIONS` in project.yml).
enum BuildFlavor: String {
    case store
    case full

    static var current: BuildFlavor {
        #if PA_FLAVOR_FULL
            return .full
        #else
            return .store
        #endif
    }
}

enum BuildInfo {
    /// `core_version()` of the Rust core over UniFFI (PACore, CORE-003).
    static var core: String { coreVersion() }

    static var flavor: BuildFlavor { .current }

    /// One-line status shown on Home and in Settings, e.g. "Core 0.1.0 · store".
    static var summary: String { "Core \(core) · \(flavor.rawValue)" }
}
