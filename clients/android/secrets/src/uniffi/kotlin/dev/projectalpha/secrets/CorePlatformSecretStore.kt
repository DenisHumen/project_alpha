// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.secrets

import dev.projectalpha.core.PlatformSecretStore
import dev.projectalpha.core.SecretStoreException

/**
 * [KeystoreSecretStore] as the core's UniFFI `PlatformSecretStore` (pa-ffi): pass it to `secretsOpen` right after
 * `coreInit`. The Flutter app goes through flutter_rust_bridge instead (package `pa_platform_secrets`).
 */
class CorePlatformSecretStore(private val store: KeystoreSecretStore) : PlatformSecretStore {
    override fun load(key: String): String? = mapped { store.load(key) }

    override fun save(key: String, value: String) = mapped { store.save(key, value) }

    override fun remove(key: String): Boolean = mapped { store.remove(key) }

    private inline fun <T> mapped(block: () -> T): T = try {
        block()
    } catch (e: KeystoreSecretException) {
        val details = e.message ?: e.kind.name
        throw when (e.kind) {
            KeystoreSecretException.Kind.UNAVAILABLE -> SecretStoreException.Unavailable(details)
            KeystoreSecretException.Kind.LOCKED -> SecretStoreException.Locked(details)
            KeystoreSecretException.Kind.FAILED -> SecretStoreException.Failed(details)
        }
    }
}
