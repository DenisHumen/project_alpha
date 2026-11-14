// SPDX-License-Identifier: MPL-2.0
// CORE-015: the core keeps passwords and tokens in the Keychain
// (KeychainSecretStore in PACore, kSecAttrAccessibleAfterFirstUnlock).
import Foundation
import PACore

/// The core's secret store on tvOS. ``open(service:)`` runs at app start;
/// the rest is for diagnostics and tests.
enum CoreSecrets {
    static let defaultService = KeychainSecretStore.defaultService

    /// Makes the Keychain the secret store of the core. Returns whether the
    /// core reports the platform backend.
    @discardableResult
    static func open(service: String = defaultService) throws -> Bool {
        let info = try secretsOpen(platform: KeychainSecretStore(service: service))
        return info.backend == .platform && info.secure
    }

    static func close() { secretsClose() }

    static func get(_ key: String) throws -> String? { try secretGet(key: key) }

    static func set(_ key: String, _ value: String) throws { try secretSet(key: key, value: value) }

    @discardableResult
    static func delete(_ key: String) throws -> Bool { try secretDelete(key: key) }

    /// Reads the Keychain directly, bypassing the core.
    static func keychainValue(service: String, key: String) throws -> String? {
        try KeychainSecretStore(service: service).load(key: key)
    }

    /// Deletes every item of `service`.
    static func removeAll(service: String) throws {
        try KeychainSecretStore(service: service).removeAll()
    }
}
