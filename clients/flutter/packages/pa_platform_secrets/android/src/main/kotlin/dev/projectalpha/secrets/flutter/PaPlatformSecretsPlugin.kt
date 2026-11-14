// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.secrets.flutter

import android.os.Handler
import android.os.Looper
import dev.projectalpha.secrets.KeystoreSecretException
import dev.projectalpha.secrets.KeystoreSecretStore
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * `pa_platform_secrets` channel (CORE-015): `load`, `save` and `remove` of [KeystoreSecretStore] for the Rust core,
 * which calls them through a Dart callback (`secretsOpenPlatform`). Keystore work runs on a background thread; the
 * answers go back on the main thread, as the channel requires.
 */
class PaPlatformSecretsPlugin : FlutterPlugin, MethodChannel.MethodCallHandler {
    private var channel: MethodChannel? = null
    private var store: KeystoreSecretStore? = null
    private var executor: ExecutorService? = null
    private val main = Handler(Looper.getMainLooper())

    override fun onAttachedToEngine(flutter: FlutterPlugin.FlutterPluginBinding) {
        store = KeystoreSecretStore(flutter.applicationContext)
        executor = Executors.newSingleThreadExecutor { Thread(it, "pa-platform-secrets") }
        channel = MethodChannel(flutter.binaryMessenger, "pa_platform_secrets").also { it.setMethodCallHandler(this) }
    }

    override fun onDetachedFromEngine(flutter: FlutterPlugin.FlutterPluginBinding) {
        channel?.setMethodCallHandler(null)
        channel = null
        executor?.shutdown()
        executor = null
        store = null
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        val store = store
        val executor = executor
        if (store == null || executor == null) return result.error("unavailable", "plugin detached", null)
        val key = call.argument<String>("key") ?: return result.error("failed", "missing key", null)
        val work: () -> Any? = when (call.method) {
            "load" -> { -> store.load(key) }
            "save" -> {
                val value = call.argument<String>("value") ?: return result.error("failed", "missing value", null)
                ({ store.save(key, value); null })
            }
            "remove" -> { -> store.remove(key) }
            else -> return result.notImplemented()
        }
        executor.execute {
            try {
                val answer = work()
                main.post { result.success(answer) }
            } catch (e: KeystoreSecretException) {
                val code = when (e.kind) {
                    KeystoreSecretException.Kind.UNAVAILABLE -> "unavailable"
                    KeystoreSecretException.Kind.LOCKED -> "locked"
                    KeystoreSecretException.Kind.FAILED -> "failed"
                }
                main.post { result.error(code, e.message, null) }
            } catch (e: RuntimeException) {
                main.post { result.error("failed", "${e.javaClass.simpleName}: ${e.message}", null) }
            }
        }
    }
}
