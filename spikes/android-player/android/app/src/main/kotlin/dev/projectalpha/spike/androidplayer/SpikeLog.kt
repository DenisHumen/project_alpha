// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import android.os.SystemClock
import android.util.Log
import org.json.JSONObject

/**
 * Metrics are written to logcat as one JSON object per line so the manual test plan can collect them with
 * `adb logcat -s PaSpike` and paste them into the findings table.
 */
object SpikeLog {
    private const val TAG = "PaSpike"

    fun now(): Long = SystemClock.elapsedRealtime()

    fun metric(event: String, data: Map<String, Any?> = emptyMap()) {
        val json = JSONObject()
        json.put("event", event)
        json.put("t", now())
        data.forEach { (k, v) -> json.put(k, v ?: JSONObject.NULL) }
        Log.i(TAG, "METRIC $json")
    }
}

/** Options shared by every embedding variant. Parsed from Intent extras or PlatformView creation params. */
data class PlayerOptions(
    val url: String,
    /** `activity_native_osd`, `activity_flutter_osd`, `platform_view_hc`, `platform_view_tlhc`. */
    val variant: String,
    /** `mode` (preferredDisplayModeId), `surface` (Surface.setFrameRate ALWAYS, API 31+), `off`. */
    val afr: String,
    val tunneling: Boolean,
    /** Extra pause after the display mode switch so the TV finishes HDMI resync before playback starts. */
    val settleMs: Long,
) {
    companion object {
        fun from(get: (String) -> Any?): PlayerOptions = PlayerOptions(
            url = get("url") as? String ?: "",
            variant = get("variant") as? String ?: "activity_native_osd",
            afr = get("afr") as? String ?: "mode",
            tunneling = get("tunneling") as? Boolean ?: false,
            settleMs = (get("settle_ms") as? Number)?.toLong() ?: 0L,
        )
    }
}
