// SPDX-License-Identifier: MPL-2.0
// CORE-015: the core's PlatformSecretStore on iOS, iPadOS and tvOS.
import Foundation
import Security

/// Secrets of the core in the Keychain (CORE-015, docs/dev/secrets.md).
///
/// Generic-password items, one per secret: service ``service``, account = the
/// secret key (`source.password:<id>`, `twitch.tokens:<user id>`…; only
/// internal ids, the account name is not encrypted). Items are
/// `kSecAttrAccessibleAfterFirstUnlock`, so background downloads and sync can
/// read them while the device is locked, and are not synchronized to iCloud.
///
/// Pass it to the core right after `coreInit`:
///
/// ```swift
/// _ = try secretsOpen(platform: KeychainSecretStore())
/// ```
///
/// macOS does not need it: the core uses the login keychain itself.
public final class KeychainSecretStore: PlatformSecretStore, @unchecked Sendable {
    /// Keychain service of the app's secrets.
    public static let defaultService = "app.kinolune.secrets"

    /// Keychain service of the items.
    public let service: String
    /// Keychain access group (shared with an extension), `nil` = the app's own.
    public let accessGroup: String?

    public init(service: String = KeychainSecretStore.defaultService, accessGroup: String? = nil) {
        self.service = service
        self.accessGroup = accessGroup
    }

    private func query(_ key: String) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
        ]
        if let accessGroup { q[kSecAttrAccessGroup as String] = accessGroup }
        return q
    }

    public func load(key: String) throws -> String? {
        var q = query(key)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        switch status {
        case errSecSuccess:
            guard let data = out as? Data, let value = String(data: data, encoding: .utf8) else {
                throw SecretStoreError.Failed(details: "Keychain item \(key) is not UTF-8")
            }
            return value
        case errSecItemNotFound:
            return nil
        default:
            throw Self.error(status, "read \(key)")
        }
    }

    public func save(key: String, value: String) throws {
        let data = Data(value.utf8)
        let update: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlock,
        ]
        var status = SecItemUpdate(query(key) as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            var add = query(key)
            add.merge(update) { _, new in new }
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw Self.error(status, "write \(key)") }
    }

    public func remove(key: String) throws -> Bool {
        let status = SecItemDelete(query(key) as CFDictionary)
        switch status {
        case errSecSuccess: return true
        case errSecItemNotFound: return false
        default: throw Self.error(status, "delete \(key)")
        }
    }

    /// Deletes every item of ``service`` (tests, "forget all passwords").
    public func removeAll() throws {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
        ]
        if let accessGroup { q[kSecAttrAccessGroup as String] = accessGroup }
        let status = SecItemDelete(q as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw Self.error(status, "delete all")
        }
    }

    /// Maps a Keychain status to the core's error. Never includes a value.
    static func error(_ status: OSStatus, _ what: String) -> SecretStoreError {
        let message = (SecCopyErrorMessageString(status, nil) as String?) ?? "OSStatus \(status)"
        let details = "Keychain \(what): \(message) (\(status))"
        switch status {
        case errSecInteractionNotAllowed, errSecAuthFailed, errSecUserCanceled:
            // Before the first unlock after a reboot, or access denied.
            return .Locked(details: details)
        case errSecNotAvailable, errSecMissingEntitlement:
            return .Unavailable(details: details)
        default:
            return .Failed(details: details)
        }
    }
}
