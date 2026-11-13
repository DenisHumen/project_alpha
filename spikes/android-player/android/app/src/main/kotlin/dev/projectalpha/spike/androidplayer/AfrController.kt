// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import android.app.Activity
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.Display
import android.view.Surface

/**
 * Auto frame rate via `WindowManager.LayoutParams.preferredDisplayModeId` (works on API 23+, the only option on
 * Android TV 9–11 and the one Kodi uses). Applies to the whole window, so it works the same for a separate player
 * Activity and for a PlatformView inside FlutterActivity.
 */
class AfrController(private val activity: Activity) : DisplayManager.DisplayListener {
    private val displayManager = activity.getSystemService(DisplayManager::class.java)!!
    private val handler = Handler(Looper.getMainLooper())
    private val originalModeId = display().mode.modeId
    private var targetModeId = 0
    private var requestedAt = 0L
    private var pending: (() -> Unit)? = null

    @Suppress("DEPRECATION")
    private fun display(): Display =
        if (Build.VERSION.SDK_INT >= 30) activity.display!! else activity.windowManager.defaultDisplay

    fun describeCurrentMode(): String = display().mode.let { "${it.physicalWidth}x${it.physicalHeight}@${"%.3f".format(it.refreshRate)}" }

    /** Switches the display to the best mode for [fps] and calls [onReady] when the switch is done (or skipped). */
    fun switchTo(fps: Float, onReady: () -> Unit) {
        val d = display()
        val current = d.mode.toSpec()
        val target = AfrModeSelector.select(d.supportedModes.map { it.toSpec() }, current, fps)
        if (target == null || target.id == current.id) {
            SpikeLog.metric("afr_skip", mapOf("fps" to fps, "current" to describeCurrentMode(), "reason" to if (target == null) "no_match" else "already_matching"))
            onReady()
            return
        }
        targetModeId = target.id
        requestedAt = SpikeLog.now()
        pending = onReady
        displayManager.registerDisplayListener(this, handler)
        setPreferredMode(target.id)
        SpikeLog.metric("afr_request", mapOf("fps" to fps, "from" to describeCurrentMode(), "to" to "${target.width}x${target.height}@${target.refreshRate}"))
        handler.postDelayed({ finish("timeout") }, SWITCH_TIMEOUT_MS)
    }

    /** API 31+: let the platform switch (honours the user's "Match content frame rate" setting on Android 12+). */
    fun applySurfaceFrameRate(surface: Surface?, fps: Float) {
        if (Build.VERSION.SDK_INT < 31 || surface == null || fps <= 0f) {
            SpikeLog.metric("afr_surface_unsupported", mapOf("sdk" to Build.VERSION.SDK_INT, "fps" to fps))
            return
        }
        surface.setFrameRate(fps, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE, Surface.CHANGE_FRAME_RATE_ALWAYS)
        SpikeLog.metric("afr_surface", mapOf("fps" to fps, "current" to describeCurrentMode()))
    }

    override fun onDisplayChanged(displayId: Int) {
        if (displayId == display().displayId && display().mode.modeId == targetModeId) finish("switched")
    }

    override fun onDisplayAdded(displayId: Int) = Unit
    override fun onDisplayRemoved(displayId: Int) = Unit

    private fun finish(result: String) {
        val callback = pending ?: return
        pending = null
        handler.removeCallbacksAndMessages(null)
        displayManager.unregisterDisplayListener(this)
        SpikeLog.metric("afr_done", mapOf("result" to result, "switch_ms" to SpikeLog.now() - requestedAt, "mode" to describeCurrentMode()))
        callback()
    }

    /** Restores the mode that was active before playback (the system would do it on window exit anyway). */
    fun restore() {
        pending = null
        handler.removeCallbacksAndMessages(null)
        displayManager.unregisterDisplayListener(this)
        if (targetModeId != 0) setPreferredMode(originalModeId)
    }

    private fun setPreferredMode(id: Int) {
        val window = activity.window
        window.attributes = window.attributes.apply { preferredDisplayModeId = id }
    }

    private fun Display.Mode.toSpec() = ModeSpec(modeId, physicalWidth, physicalHeight, refreshRate)

    companion object {
        private const val SWITCH_TIMEOUT_MS = 6000L
    }
}
