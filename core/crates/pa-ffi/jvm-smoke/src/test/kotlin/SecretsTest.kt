// SPDX-License-Identifier: MPL-2.0
import dev.projectalpha.core.PlatformSecretStore
import dev.projectalpha.core.SecretBackend
import dev.projectalpha.core.SecretStoreException
import dev.projectalpha.core.redactSecrets
import dev.projectalpha.core.secretDelete
import dev.projectalpha.core.secretGet
import dev.projectalpha.core.secretSet
import dev.projectalpha.core.secretsClose
import dev.projectalpha.core.secretsOpen
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * The secret store callback as Android implements it (there with Keystore + Tink),
 * here backed by a map: checks the UniFFI callback path in both directions.
 */
class SecretsTest {
    private class MemoryKeystore : PlatformSecretStore {
        val items = mutableMapOf<String, String>()

        override fun load(key: String): String? = items[key]

        override fun save(key: String, value: String) {
            items[key] = value
        }

        override fun remove(key: String): Boolean = items.remove(key) != null
    }

    @Test
    fun secretsGoThroughThePlatformCallback() {
        val keystore = MemoryKeystore()
        val info = secretsOpen(keystore)
        assertEquals(SecretBackend.PLATFORM, info.backend)
        assertTrue(info.secure)

        secretSet("twitch.tokens:42", "tok-1234567")
        assertEquals("tok-1234567", keystore.items["twitch.tokens:42"])
        assertEquals("tok-1234567", secretGet("twitch.tokens:42"))
        assertEquals("token ***", redactSecrets("token tok-1234567"))

        assertFailsWith<SecretStoreException.InvalidKey> { secretSet("bad key", "x") }

        assertTrue(secretDelete("twitch.tokens:42"))
        assertNull(secretGet("twitch.tokens:42"))
        secretsClose()
    }
}
