// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import android.app.Activity
import android.content.Context
import android.view.LayoutInflater
import android.view.SurfaceView
import android.view.View
import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.PlayerView
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel
import io.flutter.plugin.common.StandardMessageCodec
import io.flutter.plugin.platform.PlatformView
import io.flutter.plugin.platform.PlatformViewFactory

const val PLAYER_VIEW_TYPE = "pa_spike/player_view"

/**
 * Variants C (`platform_view_hc`) and D (`platform_view_tlhc`): the player lives inside the FlutterActivity as a
 * PlatformView. Dart decides the composition mode (PlatformViewLink + initExpensiveAndroidView = Hybrid
 * Composition, AndroidView = Texture Layer Hybrid Composition). With TLHC the view is drawn into a GL texture,
 * so the video goes through Flutter's SDR compositor; a TextureView is used there on purpose (SurfaceView is not
 * supported by TLHC and makes Flutter fall back to Virtual Display).
 */
class PlayerViewFactory(private val activity: Activity, private val messenger: BinaryMessenger) :
    PlatformViewFactory(StandardMessageCodec.INSTANCE) {
    override fun create(context: Context, viewId: Int, args: Any?): PlatformView {
        @Suppress("UNCHECKED_CAST")
        val params = args as? Map<String, Any?> ?: emptyMap()
        return PlayerPlatformView(activity, messenger, viewId, PlayerOptions.from { params[it] })
    }
}

@OptIn(UnstableApi::class)
class PlayerPlatformView(activity: Activity, messenger: BinaryMessenger, viewId: Int, options: PlayerOptions) : PlatformView {
    private val player = SpikePlayer(activity, options)
    private val channel = MethodChannel(messenger, "pa_spike/player_view_$viewId")
    private val playerView: PlayerView =
        if (options.variant == "platform_view_tlhc") {
            LayoutInflater.from(activity).inflate(R.layout.player_texture_view, null) as PlayerView
        } else {
            PlayerView(activity)
        }

    init {
        playerView.useController = false
        playerView.player = player.exo
        channel.setMethodCallHandler { call, result -> handleControl(player, call.method, call.arguments, result) { activity.onBackPressed() } }
        player.open { fps -> player.afr.applySurfaceFrameRate((playerView.videoSurfaceView as? SurfaceView)?.holder?.surface, fps) }
    }

    override fun getView(): View = playerView

    override fun dispose() {
        channel.setMethodCallHandler(null)
        player.release()
    }
}
