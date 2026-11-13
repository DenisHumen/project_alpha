// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import android.content.Intent
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

/** Flutter home screen. Hosts the PlatformView variants and launches the Activity variants. */
class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val messenger = flutterEngine.dartExecutor.binaryMessenger
        flutterEngine.platformViewsController.registry.registerViewFactory(PLAYER_VIEW_TYPE, PlayerViewFactory(this, messenger))
        MethodChannel(messenger, "pa_spike/main").setMethodCallHandler { call, result ->
            when (call.method) {
                "openActivity" -> {
                    val args = call.arguments as Map<*, *>
                    val intent = Intent(this, PlayerActivity::class.java)
                    args.forEach { (k, v) ->
                        when (v) {
                            is String -> intent.putExtra(k as String, v)
                            is Boolean -> intent.putExtra(k as String, v)
                            is Number -> intent.putExtra(k as String, v.toLong())
                        }
                    }
                    SpikeLog.metric("launch_activity", mapOf("variant" to args["variant"]))
                    startActivity(intent)
                    result.success(null)
                }
                "deviceInfo" -> result.success(DeviceCapabilities.collect(this))
                else -> result.notImplemented()
            }
        }
    }
}
