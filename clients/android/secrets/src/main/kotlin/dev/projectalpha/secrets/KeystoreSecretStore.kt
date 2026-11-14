// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.secrets

import android.content.Context
import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.ProviderException
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Error of [KeystoreSecretStore]; [kind] maps to the core's `SecretStoreError`. Never contains a secret value. */
class KeystoreSecretException(val kind: Kind, message: String, cause: Throwable? = null) : Exception(message, cause) {
    enum class Kind {
        /** The Keystore is missing or broken on this device. */
        UNAVAILABLE,

        /** The Keystore refused to use the key (device locked, user not authenticated). */
        LOCKED,

        /** Anything else: storage write failed, unexpected crypto error. */
        FAILED,
    }
}

/**
 * Secrets of the core on Android (CORE-015): the `PlatformSecretStore` of `pa-secrets`.
 *
 * An AES-256 key that never leaves the Android Keystore (TEE- or StrongBox-backed where the device has one)
 * encrypts each value with AES-GCM; the ciphertexts live in the app-private SharedPreferences file [name]. The
 * secret key is the associated data, so a ciphertext copied to another entry does not decrypt. The key needs no
 * user authentication: background work (downloads, sync) can read secrets, like `AfterFirstUnlock` on Apple.
 *
 * Why plain `javax.crypto` and not Tink: Tink's Android keyset manager does the same thing (a Keystore AES-GCM key
 * wrapping the data key) and adds Tink plus protobuf-lite (about 1.5 MB before shrinking) for one AEAD call that
 * the platform provides; `androidx.security:security-crypto`, the usual Tink wrapper, is deprecated.
 *
 * If a value cannot be decrypted (the Keystore key was lost: app data restored to another device, the user cleared
 * credentials) it is deleted and reported as missing, so the user enters that password again instead of the app
 * failing forever.
 *
 * Calls are blocking and thread-safe; keep them off the main thread.
 */
class KeystoreSecretStore(context: Context, val name: String = DEFAULT_NAME) {
    private val prefs: SharedPreferences = context.applicationContext.getSharedPreferences(name, Context.MODE_PRIVATE)
    private val alias = "$name.aes-gcm"
    private val lock = Any()

    /** The value of [key], `null` if missing. */
    fun load(key: String): String? = synchronized(lock) {
        val stored = prefs.getString(key, null) ?: return null
        val bytes = try {
            Base64.decode(stored, Base64.NO_WRAP)
        } catch (e: IllegalArgumentException) {
            return undecryptable(key, "bad Base64")
        }
        if (bytes.size < 1 + IV_LEN + TAG_BITS / 8 || bytes[0] != FORMAT_V1) return undecryptable(key, "unknown format")
        val secretKey = keystoreCall { existingKey() } ?: return undecryptable(key, "the Keystore key is missing")
        keystoreCall {
            try {
                decrypt(key, secretKey, bytes)
            } catch (e: AEADBadTagException) {
                undecryptable(key, "authentication failed")
            } catch (e: KeyPermanentlyInvalidatedException) {
                undecryptable(key, "the Keystore key was invalidated")
            }
        }
    }

    /** Stores or replaces the value of [key]. */
    fun save(key: String, value: String): Unit = synchronized(lock) {
        val sealed = keystoreCall { encrypt(key, value) }
        if (!prefs.edit().putString(key, sealed).commit()) {
            throw KeystoreSecretException(KeystoreSecretException.Kind.FAILED, "could not write $name")
        }
    }

    /** Deletes the value of [key]; returns whether it existed. */
    fun remove(key: String): Boolean = synchronized(lock) {
        if (!prefs.contains(key)) return false
        if (!prefs.edit().remove(key).commit()) {
            throw KeystoreSecretException(KeystoreSecretException.Kind.FAILED, "could not write $name")
        }
        true
    }

    /** Deletes every value and the Keystore key (tests, "forget all passwords"). */
    fun clear(): Unit = synchronized(lock) {
        prefs.edit().clear().commit()
        keystoreCall { keyStore().deleteEntry(alias) }
    }

    private fun undecryptable(key: String, why: String): String? {
        Log.w(TAG, "secret $key cannot be decrypted ($why) and is deleted")
        prefs.edit().remove(key).commit()
        return null
    }

    private fun encrypt(key: String, value: String): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, secretKey())
        cipher.updateAAD(aad(key))
        val iv = cipher.iv
        if (iv.size != IV_LEN) throw GeneralSecurityException("unexpected GCM IV length ${iv.size}")
        val sealed = cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        val out = ByteArray(1 + IV_LEN + sealed.size)
        out[0] = FORMAT_V1
        iv.copyInto(out, 1)
        sealed.copyInto(out, 1 + IV_LEN)
        return Base64.encodeToString(out, Base64.NO_WRAP)
    }

    private fun decrypt(key: String, secretKey: SecretKey, bytes: ByteArray): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, secretKey, GCMParameterSpec(TAG_BITS, bytes, 1, IV_LEN))
        cipher.updateAAD(aad(key))
        val plain = cipher.doFinal(bytes, 1 + IV_LEN, bytes.size - 1 - IV_LEN)
        return String(plain, Charsets.UTF_8)
    }

    private fun aad(key: String) = "kinolune-secret:v1:$key".toByteArray(Charsets.UTF_8)

    private fun keyStore(): KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    private fun existingKey(): SecretKey? = (keyStore().getEntry(alias, null) as? KeyStore.SecretKeyEntry)?.secretKey

    private fun secretKey(): SecretKey = existingKey() ?: KeyGenerator.getInstance(
        KeyProperties.KEY_ALGORITHM_AES,
        ANDROID_KEYSTORE,
    ).run {
        init(
            KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        generateKey()
    }

    /** Runs a Keystore operation, mapping its failures to [KeystoreSecretException]. */
    private inline fun <T> keystoreCall(block: () -> T): T = try {
        block()
    } catch (e: android.security.keystore.UserNotAuthenticatedException) {
        throw KeystoreSecretException(KeystoreSecretException.Kind.LOCKED, "Keystore: ${e.message}", e)
    } catch (e: java.security.KeyStoreException) {
        throw KeystoreSecretException(KeystoreSecretException.Kind.UNAVAILABLE, "Keystore: ${e.message}", e)
    } catch (e: GeneralSecurityException) {
        throw KeystoreSecretException(KeystoreSecretException.Kind.FAILED, "Keystore: ${e.message}", e)
    } catch (e: ProviderException) {
        throw KeystoreSecretException(KeystoreSecretException.Kind.FAILED, "Keystore: ${e.message}", e)
    } catch (e: java.io.IOException) {
        throw KeystoreSecretException(KeystoreSecretException.Kind.UNAVAILABLE, "Keystore: ${e.message}", e)
    }

    companion object {
        /** SharedPreferences file of the app's secrets (`shared_prefs/kinolune_secrets.xml`). */
        const val DEFAULT_NAME = "kinolune_secrets"
        private const val TAG = "KeystoreSecretStore"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val IV_LEN = 12
        private const val TAG_BITS = 128
        private const val FORMAT_V1: Byte = 1
    }
}
