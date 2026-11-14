// SPDX-License-Identifier: MPL-2.0
// TV-001: unit tests of the tvOS app skeleton.
@testable import TVApp
import XCTest

final class TVAppTests: XCTestCase {
    func testTabOrderFollowsTheTVInformationArchitecture() {
        // docs/research/01 §2.3: Home · Movies · TV Shows · Twitch · Music · Search · Settings.
        XCTAssertEqual(
            AppTab.allCases.map(\.title),
            ["Home", "Movies", "TV Shows", "Twitch", "Music", "Search", "Settings"]
        )
    }

    func testEveryTabHasADesignSystemIcon() {
        for tab in AppTab.allCases {
            XCTAssertTrue(tab.icon.assetName.hasPrefix("pa."), "\(tab) icon \(tab.icon.assetName)")
        }
        XCTAssertEqual(Set(AppTab.allCases.map(\.screenID)).count, AppTab.allCases.count)
    }

    func testCoreVersionComesFromTheRustCore() {
        let parts = BuildInfo.core.split(separator: ".")
        XCTAssertEqual(parts.count, 3, "expected MAJOR.MINOR.PATCH, got \(BuildInfo.core)")
        XCTAssertTrue(parts.allSatisfy { UInt($0) != nil }, "non-numeric version: \(BuildInfo.core)")
    }

    func testTVOSBuildIsTheStoreFlavor() {
        // tvOS is distributed only through the App Store (ADR-0007).
        XCTAssertEqual(BuildInfo.flavor, .store)
        XCTAssertTrue(BuildInfo.summary.hasSuffix("· store"), BuildInfo.summary)
    }

    func testRustCoreIsTheSameFlavor() {
        // CORE-013: PA_FLAVOR of the app and of PACoreFFI.xcframework agree.
        XCTAssertEqual(BuildInfo.coreFlavor, BuildInfo.flavor.rawValue)
        XCTAssertEqual(BuildInfo.coreCapabilities.isEmpty, BuildInfo.flavor == .store)
    }
}
