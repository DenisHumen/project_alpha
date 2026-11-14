// SPDX-License-Identifier: MPL-2.0
// CORE-015: the core's Keychain secret store in the tvOS app (hosted tests:
// the Keychain needs an app with entitlements, unlike PACore's package tests).
import Security
@testable import TVApp
import XCTest

private let testService = "app.kinolune.secrets.tests"

final class CoreSecretsTests: XCTestCase {
    override func setUpWithError() throws {
        try CoreSecrets.removeAll(service: testService)
        XCTAssertTrue(try CoreSecrets.open(service: testService))
    }

    override func tearDownWithError() throws {
        CoreSecrets.close()
        try CoreSecrets.removeAll(service: testService)
    }

    func testWriteReadOverwriteDeleteThroughTheCore() throws {
        let key = "source.password:1"
        XCTAssertNil(try CoreSecrets.get(key))
        XCTAssertFalse(try CoreSecrets.delete(key))

        try CoreSecrets.set(key, "hunter2-кино")
        XCTAssertEqual(try CoreSecrets.get(key), "hunter2-кино")
        XCTAssertEqual(try CoreSecrets.keychainValue(service: testService, key: key), "hunter2-кино")

        try CoreSecrets.set(key, "second")
        XCTAssertEqual(try CoreSecrets.get(key), "second")

        // Reopen with a new store object: the value lives in the Keychain.
        CoreSecrets.close()
        try CoreSecrets.open(service: testService)
        XCTAssertEqual(try CoreSecrets.get(key), "second")

        XCTAssertTrue(try CoreSecrets.delete(key))
        XCTAssertFalse(try CoreSecrets.delete(key))
        XCTAssertNil(try CoreSecrets.get(key))
        XCTAssertNil(try CoreSecrets.keychainValue(service: testService, key: key))

        let big = String(repeating: "x", count: 1280) // the core's maximum value size
        try CoreSecrets.set("twitch.tokens:42", big)
        XCTAssertEqual(try CoreSecrets.get("twitch.tokens:42"), big)
    }

    func testItemsAreAfterFirstUnlockAndNotSynced() throws {
        try CoreSecrets.set("provider.api_key:tmdb", "v")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: testService,
            kSecAttrAccount as String: "provider.api_key:tmdb",
            kSecReturnAttributes as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var out: CFTypeRef?
        XCTAssertEqual(SecItemCopyMatching(query as CFDictionary, &out), errSecSuccess)
        let attributes = try XCTUnwrap(out as? [String: Any])
        XCTAssertEqual(
            attributes[kSecAttrAccessible as String] as? String,
            kSecAttrAccessibleAfterFirstUnlock as String
        )
        XCTAssertNotEqual(attributes[kSecAttrSynchronizable as String] as? Bool, true)
    }

    func testServicesAreSeparate() throws {
        let other = testService + ".other"
        defer { try? CoreSecrets.removeAll(service: other) }
        try CoreSecrets.set("k", "mine")
        XCTAssertNil(try CoreSecrets.keychainValue(service: other, key: "k"))
    }
}

/// A real process restart: the `tvapp` CI job runs `testAWriteBeforeRestart`
/// and `testBReadAfterRestart` in two separate `xcodebuild test` runs (the
/// host app is relaunched). In the full suite both run in one process and
/// the second one is skipped.
final class CoreSecretsRestartTests: XCTestCase {
    private let service = testService + ".restart"
    private let pidKey = "test.restart.pid"
    private var pid: String { String(ProcessInfo.processInfo.processIdentifier) }

    override func tearDown() {
        CoreSecrets.close()
    }

    func testAWriteBeforeRestart() throws {
        try CoreSecrets.removeAll(service: service)
        try CoreSecrets.open(service: service)
        try CoreSecrets.set(pidKey, pid)
        try CoreSecrets.set("source.password:1", "first")
        try CoreSecrets.set("trakt.tokens:9", "to-delete")
    }

    func testBReadAfterRestart() throws {
        try CoreSecrets.open(service: service)
        let writer = try XCTUnwrap(try CoreSecrets.get(pidKey), "testAWriteBeforeRestart did not run")
        if writer == pid {
            throw XCTSkip("same process as the writer; CI runs this step after a restart")
        }
        XCTAssertEqual(try CoreSecrets.get("source.password:1"), "first")
        XCTAssertEqual(try CoreSecrets.get("trakt.tokens:9"), "to-delete")
        try CoreSecrets.set("source.password:1", "overwritten")
        XCTAssertTrue(try CoreSecrets.delete("trakt.tokens:9"))

        CoreSecrets.close()
        try CoreSecrets.open(service: service)
        XCTAssertEqual(try CoreSecrets.get("source.password:1"), "overwritten")
        XCTAssertNil(try CoreSecrets.get("trakt.tokens:9"))
        try CoreSecrets.removeAll(service: service)
    }
}
