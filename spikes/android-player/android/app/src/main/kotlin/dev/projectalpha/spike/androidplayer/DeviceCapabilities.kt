// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import android.app.Activity
import android.app.UiModeManager
import android.content.res.Configuration
import android.media.MediaCodecInfo.CodecProfileLevel
import android.media.MediaCodecList
import android.os.Build
import android.view.Display
import androidx.annotation.OptIn
import androidx.media3.common.AudioAttributes
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.audio.AudioCapabilities

/** Snapshot of what the device/display/TV chain claims to support. Logged and shown in the app. */
object DeviceCapabilities {
    @OptIn(UnstableApi::class)
    @Suppress("DEPRECATION")
    fun collect(activity: Activity): Map<String, Any?> {
        val display: Display = if (Build.VERSION.SDK_INT >= 30) activity.display!! else activity.windowManager.defaultDisplay
        val uiMode = activity.getSystemService(UiModeManager::class.java).currentModeType
        val audio = AudioCapabilities.getCapabilities(activity, AudioAttributes.DEFAULT, null)
        val info = mapOf(
            "device" to "${Build.MANUFACTURER} ${Build.MODEL} (${Build.DEVICE})",
            "sdk" to Build.VERSION.SDK_INT,
            "is_tv" to (uiMode == Configuration.UI_MODE_TYPE_TELEVISION),
            "display_mode" to display.mode.let { "${it.physicalWidth}x${it.physicalHeight}@${it.refreshRate}" },
            "display_modes" to display.supportedModes.map { "${it.modeId}:${it.physicalWidth}x${it.physicalHeight}@${it.refreshRate}" },
            "display_is_hdr" to (Build.VERSION.SDK_INT >= 26 && display.isHdr),
            "display_hdr_types" to (display.hdrCapabilities?.supportedHdrTypes?.map(::hdrName) ?: emptyList<String>()),
            "decoders_dolby_vision" to decoders("video/dolby-vision"),
            "decoders_hevc" to decoders("video/hevc"),
            "decoders_av1" to decoders("video/av01"),
            "audio_passthrough" to audio.toString(),
        )
        SpikeLog.metric("device", info)
        return info
    }

    private fun hdrName(type: Int): String = when (type) {
        Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION -> "DolbyVision"
        Display.HdrCapabilities.HDR_TYPE_HDR10 -> "HDR10"
        Display.HdrCapabilities.HDR_TYPE_HLG -> "HLG"
        Display.HdrCapabilities.HDR_TYPE_HDR10_PLUS -> "HDR10+"
        else -> "type$type"
    }

    /** Decoder name, hardware flag and the HDR-relevant profiles it advertises. */
    private fun decoders(mime: String): List<String> =
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
            .filter { !it.isEncoder && it.supportedTypes.any { t -> t.equals(mime, ignoreCase = true) } }
            .map { codec ->
                val caps = codec.getCapabilitiesForType(mime)
                val hw = if (Build.VERSION.SDK_INT >= 29) codec.isHardwareAccelerated.toString() else "?"
                val profiles = caps.profileLevels.map { profileName(mime, it.profile) }.distinct()
                "${codec.name} hw=$hw profiles=$profiles"
            }

    private fun profileName(mime: String, profile: Int): String = when {
        mime == "video/hevc" && profile == CodecProfileLevel.HEVCProfileMain -> "Main"
        mime == "video/hevc" && profile == CodecProfileLevel.HEVCProfileMain10 -> "Main10"
        mime == "video/hevc" && profile == CodecProfileLevel.HEVCProfileMain10HDR10 -> "Main10HDR10"
        mime == "video/hevc" && profile == CodecProfileLevel.HEVCProfileMain10HDR10Plus -> "Main10HDR10+"
        mime == "video/dolby-vision" && profile == CodecProfileLevel.DolbyVisionProfileDvheStn -> "P5(dvhe.05)"
        mime == "video/dolby-vision" && profile == CodecProfileLevel.DolbyVisionProfileDvheDtb -> "P7(dvhe.07)"
        mime == "video/dolby-vision" && profile == CodecProfileLevel.DolbyVisionProfileDvheSt -> "P8(dvhe.08)"
        mime == "video/dolby-vision" && profile == CodecProfileLevel.DolbyVisionProfileDvavSe -> "P9(dvav.09)"
        mime == "video/dolby-vision" && profile == CodecProfileLevel.DolbyVisionProfileDvav110 -> "P10(dav1.10)"
        else -> "0x${profile.toString(16)}"
    }
}
