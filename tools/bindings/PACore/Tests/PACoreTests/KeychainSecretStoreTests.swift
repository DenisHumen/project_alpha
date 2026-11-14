// SPDX-License-Identifier: MPL-2.0
// CORE-015: KeychainSecretStore on the iOS and tvOS simulators, directly and
// through the Rust core (UniFFI). macOS skips it: there the core uses the
// login keychain itself, and an unsigned test runner may hit a locked keychain.
import Foundation
import PACore
import Security
import XCTest

private let testService = "app.kinolune.secrets.tests"

final class KeychainSecretStoreTests: XCTestCase {
    private var store: KeychainSecretStore!

    override func setUpWithError() throws {
        #if os(macOS)
        throw XCTSkip("KeychainSecretStore is for iOS, iPadOS and tvOS")
        #else
        store = KeychainSecretStore(service: testService)
        try store.removeAll()
        #endif
    }

    override func tearDownWithError() throws {
        secretsClose()
        try store?.removeAll()
    }

    func testWriteReadOverwriteDelete() throws {
        let key = "source.password:1"
        XCTAssertNil(try store.load(key: key))
        XCTAssertFalse(try store.remove(key: key))

        try store.save(key: key, value: "hunter2-кино")
        XCTAssertEqual(try store.load(key: key), "hunter2-кино")
        // Another instance (as after a restart) reads the same item.
        XCTAssertEqual(try KeychainSecretStore(service: testService).load(key: key), "hunter2-кино")

        try store.save(key: key, value: "second")
        XCTAssertEqual(try store.load(key: key), "second")

        XCTAssertTrue(try store.remove(key: key))
        XCTAssertNil(try store.load(key: key))
        XCTAssertFalse(try store.remove(key: key))

        let big = String(repeating: "x", count: 1280) // the core's maximum value size
        try store.save(key: "twitch.tokens:42", value: big)
        XCTAssertEqual(try store.load(key: "twitch.tokens:42"), big)
    }

    func testItemsAreAfterFirstUnlockAndNotSynced() throws {
        try store.save(key: "provider.api_key:tmdb", value: "v")
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
        let other = KeychainSecretStore(service: testService + ".other")
        defer { try? other.removeAll() }
        try store.save(key: "k", value: "mine")
        XCTAssertNil(try other.load(key: "k"))
        try other.save(key: "k", value: "theirs")
        XCTAssertEqual(try store.load(key: "k"), "mine")
    }

    func testThroughTheRustCore() throws {
        let info = try secretsOpen(platform: store)
        XCTAssertEqual(info.backend, .platform)
        XCTAssertTrue(info.secure)

        let key = "source.password:7"
        XCTAssertNil(try secretGet(key: key))
        try secretSet(key: key, value: "через-ядро")
        XCTAssertEqual(try store.load(key: key), "через-ядро")
        XCTAssertEqual(try secretGet(key: key), "через-ядро")

        // Reopen with a new store object: the value lives in the Keychain,
        // not in the process.
        secretsClose()
        _ = try secretsOpen(platform: KeychainSecretStore(service: testService))
        XCTAssertEqual(try secretGet(key: key), "через-ядро")
        try secretSet(key: key, value: "second")
        XCTAssertEqual(try secretGet(key: key), "second")
        XCTAssertTrue(try secretDelete(key: key))
        XCTAssertFalse(try secretDelete(key: key))
        XCTAssertNil(try secretGet(key: key))
    }
}

/// A real process restart: `tools/bindings/test-pacore.sh ios|tvos` runs
/// `testAWriteBeforeRestart` and `testBReadAfterRestart` in two separate
/// `xcodebuild test` runs on the same simulator. In the full suite both run
/// in one process and the second one is skipped.
final class KeychainRestartTests: XCTestCase {
    private let service = testService + ".restart"
    private let pidKey = "test.restart.pid"

    override func setUpWithError() throws {
        #if os(macOS)
        throw XCTSkip("KeychainSecretStore is for iOS, iPadOS and tvOS")
        #endif
    }

    override func tearDown() {
        secretsClose()
    }

    private var pid: String { String(ProcessInfo.processInfo.processIdentifier) }

    func testAWriteBeforeRestart() throws {
        let store = KeychainSecretStore(service: service)
        try store.removeAll()
        try store.save(key: pidKey, value: pid)
        _ = try secretsOpen(platform: store)
        try secretSet(key: "source.password:1", value: "first")
        try secretSet(key: "trakt.tokens:9", value: "to-delete")
    }

    func testBReadAfterRestart() throws {
        let store = KeychainSecretStore(service: service)
        let writer = try XCTUnwrap(try store.load(key: pidKey), "testAWriteBeforeRestart did not run")
        if writer == pid {
            throw XCTSkip("same process as the writer; test-pacore.sh runs this step after a restart")
        }
        _ = try secretsOpen(platform: store)
        XCTAssertEqual(try secretGet(key: "source.password:1"), "first")
        XCTAssertEqual(try secretGet(key: "trakt.tokens:9"), "to-delete")
        try secretSet(key: "source.password:1", value: "overwritten")
        XCTAssertTrue(try secretDelete(key: "trakt.tokens:9"))
        secretsClose()

        // A fresh store object sees the changes.
        let again = KeychainSecretStore(service: service)
        XCTAssertEqual(try again.load(key: "source.password:1"), "overwritten")
        XCTAssertNil(try again.load(key: "trakt.tokens:9"))
        try again.removeAll()
    }
}
