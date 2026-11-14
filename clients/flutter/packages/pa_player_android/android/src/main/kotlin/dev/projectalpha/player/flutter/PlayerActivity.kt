// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player.flutter

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.view.SurfaceView
import android.view.View
import android.view.WindowManager
import android.widget.FrameLayout
import androidx.annotation.OptIn
import androidx.fragment.app.FragmentActivity
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.PlayerView
import dev.projectalpha.core.MediaSource
import dev.projectalpha.core.OpenRequest
import dev.projectalpha.core.engineStateToJson
import dev.projectalpha.player.Media3Engine
import io.flutter.FlutterInjector
import io.flutter.embedding.android.FlutterFragment
import io.flutter.embedding.android.RenderMode
import io.flutter.embedding.android.TransparencyMode
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.FlutterEngineCache
import io.flutter.embedding.engine.FlutterEngineGroup
import io.flutter.embedding.engine.dart.DartExecutor
import org.json.JSONObject

/**
 * The player screen (ADR-0008, variant B): Media3 renders into a [SurfaceView] — its own SurfaceFlinger layer, so
 * HDR/Dolby Vision and display-mode switching are not touched by Flutter — and the OSD is a transparent
 * [FlutterFragment] above it, run by a separate engine from [OsdEngines]. With `native_osd` the Media3 controller is
 * used instead (variant A, fallback).
 *
 * `prepare()` starts before the OSD attaches, so the OSD engine start-up overlaps loading.
 */
@OptIn(UnstableApi::class)
class PlayerActivity : FragmentActivity() {
    private lateinit var engine: Media3Engine
    private var bridge: OsdBridge? = null
    private var osdEngine: FlutterEngine? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val request = parseRequest(intent.getStringExtra(EXTRA_REQUEST))
        if (request == null) {
            finish()
            return
        }
        val nativeOsd = intent.getBooleanExtra(EXTRA_NATIVE_OSD, false)

        val root = FrameLayout(this)
        setContentView(root)
        engine = Media3Engine(this)
        if (nativeOsd) {
            val view = PlayerView(this).apply { player = engine.exoPlayer }
            root.addView(view, FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT)
            view.requestFocus()
        } else {
            val surface = SurfaceView(this)
            root.addView(surface, FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT)
            engine.setVideoSurfaceView(surface)
        }
        engine.openMedia(request)
        if (!nativeOsd) attachFlutterOsd(root, intent.getStringExtra(EXTRA_OSD_ENTRYPOINT) ?: DEFAULT_ENTRYPOINT)
    }

    private fun attachFlutterOsd(root: FrameLayout, entrypoint: String) {
        val flutterEngine = OsdEngines.take(this, entrypoint)
        osdEngine = flutterEngine
        val bridge = OsdBridge(flutterEngine.dartExecutor.binaryMessenger, engine) { finish() }
        this.bridge = bridge
        engine.setListener(bridge)
        val container = FrameLayout(this).apply { id = View.generateViewId() }
        root.addView(container, FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT)
        // Texture + transparent: the OSD is an ordinary SDR layer of the window above the video layer, exactly like
        // the native Media3 controller.
        val fragment = FlutterFragment.withCachedEngine(OsdEngines.CACHE_ID)
            .renderMode(RenderMode.texture)
            .transparencyMode(TransparencyMode.transparent)
            .shouldAttachEngineToActivity(true)
            .build<FlutterFragment>()
        supportFragmentManager.beginTransaction().add(container.id, fragment).commitNow()
    }

    override fun finish() {
        if (::engine.isInitialized) {
            setResult(Activity.RESULT_OK, Intent().putExtra(EXTRA_STATE, engineStateToJson(engine.state())))
        }
        super.finish()
    }

    override fun onDestroy() {
        if (::engine.isInitialized) {
            engine.setListener(null)
            engine.closeEngine()
        }
        bridge?.dispose()
        // The OSD fragment detaches from its engine in super.onDestroy(); destroy the engine only after that.
        super.onDestroy()
        osdEngine?.let { OsdEngines.release(it) }
    }

    companion object {
        const val EXTRA_REQUEST = "pa.request"
        const val EXTRA_NATIVE_OSD = "pa.native_osd"
        const val EXTRA_OSD_ENTRYPOINT = "pa.osd_entrypoint"
        const val EXTRA_STATE = "pa.state"
        const val DEFAULT_ENTRYPOINT = "osdMain"

        fun intent(context: Context, requestJson: String, nativeOsd: Boolean, osdEntrypoint: String): Intent =
            Intent(context, PlayerActivity::class.java)
                .putExtra(EXTRA_REQUEST, requestJson)
                .putExtra(EXTRA_NATIVE_OSD, nativeOsd)
                .putExtra(EXTRA_OSD_ENTRYPOINT, osdEntrypoint)

        /** `OpenRequest` from the JSON the Dart side sends (field names as in pa-playback). */
        internal fun parseRequest(json: String?): OpenRequest? = runCatching {
            val o = JSONObject(json!!)
            val items = o.getJSONArray("items")
            OpenRequest(
                items = (0 until items.length()).map { i ->
                    val item = items.getJSONObject(i)
                    MediaSource(item.getString("uri"), item.optString("title").takeIf { item.has("title") })
                },
                index = o.optInt("index", 0).toUInt(),
                startPositionMs = o.optLong("start_position_ms", 0).coerceAtLeast(0).toULong(),
                play = o.optBoolean("play", true),
            )
        }.getOrNull()
    }
}

/**
 * OSD engines from one shared [FlutterEngineGroup] (cheap to spawn). One engine is kept warm under [CACHE_ID];
 * the player takes it and a new one is warmed on the next [prewarm].
 */
object OsdEngines {
    const val CACHE_ID = "pa_player_android_osd"
    private var group: FlutterEngineGroup? = null
    private var warm: Pair<String, FlutterEngine>? = null

    /** Starts the OSD engine now; the player then attaches without waiting for it. */
    fun prewarm(context: Context, entrypoint: String) {
        if (warm?.first == entrypoint) return
        warm?.second?.destroy()
        warm = entrypoint to spawn(context, entrypoint)
    }

    internal fun take(context: Context, entrypoint: String): FlutterEngine {
        val engine = warm?.takeIf { it.first == entrypoint }?.second ?: spawn(context, entrypoint)
        if (warm?.second !== engine) warm?.second?.destroy()
        warm = null
        FlutterEngineCache.getInstance().put(CACHE_ID, engine)
        return engine
    }

    internal fun release(engine: FlutterEngine) {
        if (FlutterEngineCache.getInstance().get(CACHE_ID) === engine) FlutterEngineCache.getInstance().remove(CACHE_ID)
        engine.destroy()
    }

    private fun spawn(context: Context, entrypoint: String): FlutterEngine {
        // The group must exist before findAppBundlePath(): its constructor initialises FlutterLoader, which is not
        // done yet when the player starts without the Flutter app (deep link, launcher row; ADR-0008).
        val group = group ?: FlutterEngineGroup(context.applicationContext).also { group = it }
        val bundle = FlutterInjector.instance().flutterLoader().findAppBundlePath()
        return group.createAndRunEngine(context.applicationContext, DartExecutor.DartEntrypoint(bundle, entrypoint))
    }
}
