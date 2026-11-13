// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import android.os.Bundle
import android.view.SurfaceView
import android.view.View
import android.view.WindowManager
import android.widget.FrameLayout
import androidx.annotation.OptIn
import androidx.fragment.app.FragmentActivity
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.PlayerView
import io.flutter.FlutterInjector
import io.flutter.embedding.android.FlutterFragment
import io.flutter.embedding.android.RenderMode
import io.flutter.embedding.android.TransparencyMode
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.FlutterEngineCache
import io.flutter.embedding.engine.FlutterEngineGroup
import io.flutter.embedding.engine.dart.DartExecutor
import io.flutter.embedding.engine.renderer.FlutterUiDisplayListener
import io.flutter.plugin.common.MethodChannel

/**
 * Variants A (`activity_native_osd`) and B (`activity_flutter_osd`): a dedicated Activity whose video output is a
 * real SurfaceView in its own window. The OSD is either the Media3 PlayerView controller or a transparent
 * FlutterFragment (second engine spawned from a FlutterEngineGroup, Dart entrypoint `osdMain`) drawn above it.
 *
 * Can be launched without the Flutter UI for scripted tests:
 * `adb shell am start -n dev.projectalpha.spike.androidplayer/.PlayerActivity --es url <URL> --es afr mode`
 */
@OptIn(UnstableApi::class)
class PlayerActivity : FragmentActivity() {
    private lateinit var options: PlayerOptions
    private lateinit var player: SpikePlayer
    private var osdEngine: FlutterEngine? = null

    @Suppress("DEPRECATION")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val createdAt = SpikeLog.now()
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        options = PlayerOptions.from { intent.extras?.get(it) }.let {
            if (it.variant.startsWith("activity_")) it else it.copy(variant = "activity_native_osd")
        }
        val flutterOsd = options.variant == "activity_flutter_osd"

        val root = FrameLayout(this).apply { id = View.generateViewId() }
        // PlayerView defaults to surface_type=surface_view, i.e. a SurfaceView with its own SurfaceFlinger layer.
        val playerView = PlayerView(this).apply { useController = !flutterOsd }
        root.addView(playerView, FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT)
        setContentView(root)

        player = SpikePlayer(this, options)
        playerView.player = player.exo
        // Start loading media first so prepare() overlaps the OSD engine start-up (it blocks the main thread).
        player.open { fps -> player.afr.applySurfaceFrameRate((playerView.videoSurfaceView as? SurfaceView)?.holder?.surface, fps) }
        if (flutterOsd) attachFlutterOsd(root, createdAt) else playerView.requestFocus()
    }

    private fun attachFlutterOsd(root: FrameLayout, createdAt: Long) {
        // The group must exist first: its constructor initialises FlutterLoader (needed when launched via adb).
        val group = engineGroup(this)
        val entrypoint = DartExecutor.DartEntrypoint(FlutterInjector.instance().flutterLoader().findAppBundlePath(), "osdMain")
        val engine = group.createAndRunEngine(this, entrypoint)
        osdEngine = engine
        FlutterEngineCache.getInstance().put(OSD_ENGINE_ID, engine)
        MethodChannel(engine.dartExecutor.binaryMessenger, "pa_spike/osd").setMethodCallHandler { call, result ->
            handleControl(player, call.method, call.arguments, result) { finish() }
        }
        engine.renderer.addIsDisplayingFlutterUiListener(object : FlutterUiDisplayListener {
            override fun onFlutterUiDisplayed() = SpikeLog.metric("osd_first_frame", mapOf("since_create_ms" to SpikeLog.now() - createdAt))
            override fun onFlutterUiNoLongerDisplayed() = Unit
        })
        val container = FrameLayout(this).apply { id = View.generateViewId() }
        root.addView(container, FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT)
        // Texture + transparent: the OSD is an ordinary SDR view layer above the video SurfaceView,
        // exactly like the native Media3 controller, so it must not affect the HDR video layer.
        val fragment = FlutterFragment.withCachedEngine(OSD_ENGINE_ID)
            .renderMode(RenderMode.texture)
            .transparencyMode(TransparencyMode.transparent)
            .shouldAttachEngineToActivity(true)
            .build<FlutterFragment>()
        supportFragmentManager.beginTransaction().add(container.id, fragment).commitNow()
    }

    override fun onDestroy() {
        player.release()
        FlutterEngineCache.getInstance().remove(OSD_ENGINE_ID)
        osdEngine?.destroy()
        super.onDestroy()
    }

    companion object {
        private const val OSD_ENGINE_ID = "pa_spike_osd"
        private var group: FlutterEngineGroup? = null

        fun engineGroup(activity: FragmentActivity): FlutterEngineGroup =
            group ?: FlutterEngineGroup(activity.applicationContext).also { group = it }
    }
}

/** Shared control protocol for the Flutter OSD (Activity variant) and the PlatformView variants. */
fun handleControl(player: SpikePlayer, method: String, args: Any?, result: MethodChannel.Result, close: () -> Unit) {
    when (method) {
        "toggle" -> { if (player.exo.isPlaying) player.exo.pause() else player.exo.play(); result.success(null) }
        "seekBy" -> { player.exo.seekTo(player.exo.currentPosition + ((args as? Number)?.toLong() ?: 0L)); result.success(null) }
        "stats" -> result.success(player.stats())
        "close" -> { close(); result.success(null) }
        else -> result.notImplemented()
    }
}
