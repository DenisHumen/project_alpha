// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player.flutter

import android.app.Activity
import android.content.Intent
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.flutter.plugin.common.PluginRegistry

/**
 * `pa_player_android` channel of the app engine: `open` starts [PlayerActivity] and answers with the final engine
 * state (JSON) when it closes; `prewarmOsd` starts the OSD engine in advance.
 */
class PaPlayerAndroidPlugin : FlutterPlugin, ActivityAware, MethodChannel.MethodCallHandler, PluginRegistry.ActivityResultListener {
    private var channel: MethodChannel? = null
    private var binding: ActivityPluginBinding? = null
    private var pending: MethodChannel.Result? = null

    override fun onAttachedToEngine(flutter: FlutterPlugin.FlutterPluginBinding) {
        channel = MethodChannel(flutter.binaryMessenger, "pa_player_android").also { it.setMethodCallHandler(this) }
    }

    override fun onDetachedFromEngine(flutter: FlutterPlugin.FlutterPluginBinding) {
        channel?.setMethodCallHandler(null)
        channel = null
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        val activity = binding?.activity
        when (call.method) {
            "open" -> {
                if (activity == null) return result.error("no_activity", "the player needs an Activity", null)
                if (pending != null) return result.error("busy", "the player is already open", null)
                val request = call.argument<String>("request")
                if (PlayerActivity.parseRequest(request) == null) return result.error("invalid_argument", "bad request", null)
                pending = result
                activity.startActivityForResult(
                    PlayerActivity.intent(
                        activity,
                        request!!,
                        call.argument<Boolean>("native_osd") ?: false,
                        call.argument<String>("osd_entrypoint") ?: PlayerActivity.DEFAULT_ENTRYPOINT,
                    ),
                    REQUEST_CODE,
                )
            }
            "prewarmOsd" -> {
                val context = activity ?: return result.error("no_activity", "needs an Activity", null)
                OsdEngines.prewarm(context, call.argument<String>("osd_entrypoint") ?: PlayerActivity.DEFAULT_ENTRYPOINT)
                result.success(null)
            }
            else -> result.notImplemented()
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_CODE) return false
        pending?.success(data?.getStringExtra(PlayerActivity.EXTRA_STATE).takeIf { resultCode == Activity.RESULT_OK })
        pending = null
        return true
    }

    override fun onAttachedToActivity(binding: ActivityPluginBinding) {
        this.binding = binding
        binding.addActivityResultListener(this)
    }

    override fun onDetachedFromActivityForConfigChanges() = onDetachedFromActivity()

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) = onAttachedToActivity(binding)

    override fun onDetachedFromActivity() {
        binding?.removeActivityResultListener(this)
        binding = null
    }

    private companion object {
        const val REQUEST_CODE = 0x5041 // "PA"
    }
}
