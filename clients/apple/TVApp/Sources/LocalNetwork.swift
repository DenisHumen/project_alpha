// SPDX-License-Identifier: MPL-2.0
// NET-020: LAN discovery on tvOS: NWBrowser reports into the Rust core.
import Foundation
import PACore

/// Local Network access of the app (ADR-0019). Browsing goes through
/// `NWBrowser` (`NWBrowserPlatformBrowser` in PACore), which needs
/// `NSLocalNetworkUsageDescription` and every type of `bonjourServices()`
/// in `NSBonjourServices`; project.yml writes both into Info.plist.
enum LocalNetwork {
    /// Types the core may browse or announce (pa-discovery).
    static var coreBonjourServices: [String] { bonjourServices() }

    /// `NSBonjourServices` of this app's Info.plist.
    static var declaredBonjourServices: [String] {
        Bundle.main.object(forInfoDictionaryKey: "NSBonjourServices") as? [String] ?? []
    }

    /// `NSLocalNetworkUsageDescription` of this app's Info.plist.
    static var usageDescription: String? {
        Bundle.main.object(forInfoDictionaryKey: "NSLocalNetworkUsageDescription") as? String
    }

    /// Discovery of shares and servers for the add-source flow. Start it
    /// with `browse(kinds: discoverySourceKinds())` when that flow opens
    /// (the first browse shows the Local Network prompt), `close()` after.
    static func makeDiscovery() -> LanDiscovery {
        LanDiscovery(browser: NWBrowserPlatformBrowser(), ownIds: [])
    }
}
