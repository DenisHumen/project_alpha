// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import kotlin.math.abs
import kotlin.math.roundToInt

/** Display mode description decoupled from `android.view.Display.Mode` so the logic is JVM-testable. */
data class ModeSpec(val id: Int, val width: Int, val height: Int, val refreshRate: Float)

/**
 * Picks the display mode for auto frame rate (AFR, "match content frame rate").
 *
 * Policy (mirrors what Kodi does by default):
 * - keep the current resolution (the TV usually upscales better than the SoC);
 * - prefer a refresh rate equal to the content rate, then the smallest integer multiple;
 * - if the current mode is already an exact multiple, do not switch (avoids an HDMI resync);
 * - 23.976 must not be "matched" by 24.000 unless nothing better exists (loose pass).
 */
object AfrModeSelector {
    /** Relative error accepted as an exact match (23.976 vs 24.000 is 0.1 %, so it is rejected). */
    private const val STRICT_TOLERANCE = 0.0005f

    /** Relative error accepted as a fallback match (24.000 Hz for 23.976 fps content). */
    private const val LOOSE_TOLERANCE = 0.002f

    fun select(modes: List<ModeSpec>, current: ModeSpec, videoFps: Float): ModeSpec? {
        if (videoFps <= 1f || videoFps.isNaN()) return null
        val sameResolution = modes.filter { it.width == current.width && it.height == current.height }
        if (multipleOf(current.refreshRate, videoFps, STRICT_TOLERANCE) != null) return current
        return best(sameResolution, videoFps, STRICT_TOLERANCE)
            ?: best(sameResolution, videoFps, LOOSE_TOLERANCE)
    }

    private fun best(modes: List<ModeSpec>, fps: Float, tolerance: Float): ModeSpec? =
        modes.mapNotNull { mode -> multipleOf(mode.refreshRate, fps, tolerance)?.let { k -> mode to k } }
            .minWithOrNull(compareBy<Pair<ModeSpec, Int>> { it.second }.thenBy { relativeError(it.first.refreshRate, fps, it.second) })
            ?.first

    /** Returns k if `refresh ≈ k * fps` within [tolerance], otherwise null. */
    internal fun multipleOf(refresh: Float, fps: Float, tolerance: Float): Int? {
        val k = (refresh / fps).roundToInt()
        if (k < 1) return null
        return if (relativeError(refresh, fps, k) <= tolerance) k else null
    }

    private fun relativeError(refresh: Float, fps: Float, k: Int): Float = abs(refresh - k * fps) / refresh
}

/**
 * Frame rate from rendered presentation timestamps, for containers that do not declare it (MKV without
 * DefaultDuration, many TS files): Media3 then reports `Format.frameRate == -1` and AFR has nothing to match.
 */
object FrameRateEstimator {
    private val standardRates = floatArrayOf(23.976f, 24f, 25f, 29.97f, 30f, 47.952f, 48f, 50f, 59.94f, 60f, 100f, 119.88f, 120f)

    /**
     * The rough frame interval comes from the shortest cluster of deltas (gaps left by dropped frames are ignored);
     * the exact rate is then "frames in span / span", so the 1 ms rounding of MKV timestamps only costs
     * 1 ms per span — ~2 s of frames are needed to tell 29.97 from 30.
     */
    fun fromTimestampsUs(timestampsUs: List<Long>): Float {
        val sorted = timestampsUs.distinct().sorted()
        if (sorted.size < 2) return -1f
        val deltas = sorted.zipWithNext { a, b -> b - a }
        val shortest = deltas.min()
        val roughInterval = deltas.filter { it <= shortest * 3 / 2 }.average()
        val span = (sorted.last() - sorted.first()).toDouble()
        val frames = (span / roughInterval).roundToInt()
        return snap((frames * 1_000_000.0 / span).toFloat())
    }

    /**
     * Snaps to a standard rate when within 0.5 %. Also needed for container values: Media3 reports
     * 24.900398 fps for a 25 fps MP4 from ffmpeg (derived from sample count and duration), which would
     * otherwise match neither 25 nor 50 Hz.
     */
    fun snap(fps: Float): Float {
        if (fps <= 0f) return fps
        return standardRates.minByOrNull { abs(it - fps) }?.takeIf { abs(it - fps) / it < 0.005f } ?: fps
    }
}
