// SPDX-License-Identifier: MPL-2.0
// CORE-003: the Rust core answers over UniFFI on macOS and the iOS/tvOS simulators.
import PACore
import XCTest

final class CoreVersionTests: XCTestCase {
    func testCoreVersionIsSemver() {
        // The first call also runs UniFFI's contract-version and checksum checks.
        let version = coreVersion()
        let parts = version.split(separator: ".")
        XCTAssertEqual(parts.count, 3, "expected MAJOR.MINOR.PATCH, got \(version)")
        XCTAssertTrue(parts.allSatisfy { UInt($0) != nil }, "non-numeric version: \(version)")
    }
}
