// SPDX-License-Identifier: MPL-2.0
// CORE-015: KeystoreSecretStore on an emulator or device, directly and through the
// Rust core (UniFFI). Run: `just android secrets-test-device`.
package dev.projectalpha.secrets

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.projectalpha.core.SecretBackend
import dev.projectalpha.core.secretDelete
import dev.projectalpha.core.secretGet
import dev.projectalpha.core.secretSet
import dev.projectalpha.core.secretsClose
import dev.projectalpha.core.secretsOpen
import java.security.KeyStore
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class KeystoreSecretStoreTest {
    private val context: Context = InstrumentationRegistry.getInstrumentation().targetContext
    private val name = "kinolune_secrets_test"
    private lateinit var store: KeystoreSecretStore

    @Before
    fun setUp() {
        store = KeystoreSecretStore(context, name).apply { clear() }
    }

    @After
    fun tearDown() {
        secretsClose()
        store.clear()
    }

    private fun prefsFile() = context.filesDir.parentFile!!.resolve("shared_prefs/$name.xml")

    @Test
    fun writeReadOverwriteDelete() {
        val key = "source.password:1"
        assertNull(store.load(key))
        assertFalse(store.remove(key))

        store.save(key, "hunter2-кино")
        assertEquals("hunter2-кино", store.load(key))
        // A new instance (same file, same Keystore key) reads it too.
        assertEquals("hunter2-кино", KeystoreSecretStore(context, name).load(key))

        store.save(key, "second")
        assertEquals("second", store.load(key))

        assertTrue(store.remove(key))
        assertNull(store.load(key))
        assertFalse(store.remove(key))
    }

    @Test
    fun valuesAreEncryptedAtRest() {
        val value = "plain-text-should-not-appear-0123456789"
        store.save("provider.api_key:tmdb", value)
        store.save("twitch.tokens:42", "x".repeat(1280)) // the core's maximum value size
        assertEquals("x".repeat(1280), store.load("twitch.tokens:42"))
        val xml = prefsFile().readText()
        assertTrue("prefs file missing the entry: $xml", xml.contains("provider.api_key:tmdb"))
        assertFalse("value stored in plain text", xml.contains(value))
        assertFalse(xml.contains("xxxxxxxx"))
        // Two encryptions of the same value differ (random IV).
        store.save("a", value)
        store.save("b", value)
        val prefs = context.getSharedPreferences(name, Context.MODE_PRIVATE)
        assertTrue(prefs.getString("a", null) != prefs.getString("b", null))
    }

    @Test
    fun ciphertextIsBoundToItsKey() {
        store.save("source.password:1", "one")
        val prefs = context.getSharedPreferences(name, Context.MODE_PRIVATE)
        val sealed = prefs.getString("source.password:1", null)
        prefs.edit().putString("source.password:2", sealed).commit()
        // Copied ciphertext does not decrypt under another key and is dropped.
        assertNull(store.load("source.password:2"))
        assertFalse(prefs.contains("source.password:2"))
        assertEquals("one", store.load("source.password:1"))
    }

    @Test
    fun lostKeystoreKeyMeansMissingNotBroken() {
        store.save("source.password:1", "one")
        KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry("$name.aes-gcm")
        assertNull(store.load("source.password:1"))
        // A new key is made on the next save.
        store.save("source.password:1", "again")
        assertEquals("again", store.load("source.password:1"))

        context.getSharedPreferences(name, Context.MODE_PRIVATE).edit().putString("bad", "!!not base64!!").commit()
        assertNull(store.load("bad"))
    }

    @Test
    fun throughTheRustCore() {
        val info = secretsOpen(CorePlatformSecretStore(store))
        assertEquals(SecretBackend.PLATFORM, info.backend)
        assertTrue(info.secure)

        val key = "source.password:7"
        assertNull(secretGet(key))
        secretSet(key, "через-ядро")
        assertEquals("через-ядро", store.load(key))
        assertEquals("через-ядро", secretGet(key))
        secretSet(key, "second")
        assertEquals("second", secretGet(key))
        assertTrue(secretDelete(key))
        assertFalse(secretDelete(key))
        assertNull(secretGet(key))
        assertNull(store.load(key))
    }
}
