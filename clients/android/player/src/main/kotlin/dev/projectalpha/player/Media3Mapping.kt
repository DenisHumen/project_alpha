// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player

import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Tracks
import androidx.media3.common.util.UnstableApi
import dev.projectalpha.core.EngineException
import dev.projectalpha.core.Track

/** Media3 → contract mapping (pure functions, covered by JUnit). */
@OptIn(UnstableApi::class)
internal object Media3Mapping {
    /** Ids of external subtitles start with this; Media3 keeps it as `Format.id`. */
    const val EXTERNAL_PREFIX = "ext-"

    private val codecNames = mapOf(
        MimeTypes.VIDEO_H264 to "h264",
        MimeTypes.VIDEO_H265 to "hevc",
        MimeTypes.VIDEO_DOLBY_VISION to "hevc",
        MimeTypes.VIDEO_AV1 to "av1",
        MimeTypes.VIDEO_VP9 to "vp9",
        MimeTypes.VIDEO_VP8 to "vp8",
        MimeTypes.VIDEO_MP4V to "mpeg4",
        MimeTypes.VIDEO_MPEG2 to "mpeg2video",
        MimeTypes.VIDEO_VC1 to "vc1",
        MimeTypes.AUDIO_AAC to "aac",
        MimeTypes.AUDIO_AC3 to "ac3",
        MimeTypes.AUDIO_E_AC3 to "eac3",
        MimeTypes.AUDIO_E_AC3_JOC to "eac3",
        MimeTypes.AUDIO_AC4 to "ac4",
        MimeTypes.AUDIO_TRUEHD to "truehd",
        MimeTypes.AUDIO_DTS to "dts",
        MimeTypes.AUDIO_DTS_HD to "dts",
        MimeTypes.AUDIO_DTS_EXPRESS to "dts",
        MimeTypes.AUDIO_OPUS to "opus",
        MimeTypes.AUDIO_VORBIS to "vorbis",
        MimeTypes.AUDIO_FLAC to "flac",
        MimeTypes.AUDIO_ALAC to "alac",
        MimeTypes.AUDIO_MPEG to "mp3",
        MimeTypes.AUDIO_MPEG_L2 to "mp2",
        MimeTypes.AUDIO_RAW to "pcm",
        MimeTypes.APPLICATION_SUBRIP to "subrip",
        MimeTypes.TEXT_SSA to "ass",
        MimeTypes.TEXT_VTT to "webvtt",
        MimeTypes.APPLICATION_TTML to "ttml",
        MimeTypes.APPLICATION_PGS to "hdmv_pgs_subtitle",
        MimeTypes.APPLICATION_VOBSUB to "dvd_subtitle",
        MimeTypes.APPLICATION_DVBSUBS to "dvb_subtitle",
        MimeTypes.APPLICATION_TX3G to "mov_text",
    )

    /** Codec name in FFmpeg spelling, as the contract wants (`h264`, `eac3`, `subrip`). */
    fun codecName(format: Format?): String? {
        format ?: return null
        // Subtitles parsed during extraction become Media3 cues; the original type is in `codecs`.
        val mime = if (format.sampleMimeType == MimeTypes.APPLICATION_MEDIA3_CUES) format.codecs else format.sampleMimeType
        mime ?: return null
        return codecNames[mime] ?: mime.substringAfter('/').removePrefix("x-")
    }

    /** MIME type of a subtitle file by its extension (Media3 needs it for side-loaded files). */
    fun subtitleMimeType(uri: String): String = when (uri.substringBefore('?').substringAfterLast('.').lowercase()) {
        "ass", "ssa" -> MimeTypes.TEXT_SSA
        "vtt" -> MimeTypes.TEXT_VTT
        "ttml", "dfxp" -> MimeTypes.APPLICATION_TTML
        else -> MimeTypes.APPLICATION_SUBRIP
    }

    /** Engine error for a Media3 playback error (`EngineError` in pa-playback). */
    fun error(e: PlaybackException): EngineException {
        val details = "${e.errorCodeName}: ${e.message ?: e.cause?.toString() ?: "no details"}"
        val code = e.errorCode
        return when {
            code == PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED ||
                code == PlaybackException.ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED ||
                code == PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED ||
                code == PlaybackException.ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES ||
                code in 6000..6999 -> EngineException.Unsupported(details) // DRM
            code in 2000..2999 -> EngineException.Source(details) // IO
            code in 3000..5999 -> EngineException.Decoder(details) // parsing, decoding, audio sink
            else -> EngineException.Internal(details)
        }
    }

    /**
     * Contract id of a track: the container id (MKV track number), else its position. Media3 prefixes ids with
     * the source index (`0:4`, `1:ext-0`) once an item has side-loaded subtitles; the prefix is dropped so ids
     * stay the same when a subtitle file is added.
     */
    fun trackId(format: Format, type: Int, group: Int, track: Int): String =
        format.id?.replace(MERGED_PREFIX, "") ?: "$type-$group-$track"

    private val MERGED_PREFIX = Regex("^\\d+:")

    fun track(format: Format, id: String): Track = Track(
        id = id,
        title = format.label,
        language = format.language?.takeUnless { it == C.LANGUAGE_UNDETERMINED },
        codec = codecName(format),
        channels = format.channelCount.takeIf { it > 0 }?.toUInt(),
        external = id.startsWith(EXTERNAL_PREFIX),
        default = format.selectionFlags and C.SELECTION_FLAG_DEFAULT != 0,
        forced = format.selectionFlags and C.SELECTION_FLAG_FORCED != 0,
    )

    /** Playable tracks of [type] (`C.TRACK_TYPE_AUDIO` or `C.TRACK_TYPE_TEXT`). */
    fun tracks(tracks: Tracks, type: Int): List<Track> = buildList {
        tracks.groups.forEachIndexed { g, group ->
            if (group.type != type) return@forEachIndexed
            for (i in 0 until group.length) {
                if (!group.isTrackSupported(i)) continue
                val format = group.getTrackFormat(i)
                add(track(format, trackId(format, type, g, i)))
            }
        }
    }

    /** Id of the selected track of [type], `null` if none (or the type is disabled). */
    fun selectedId(tracks: Tracks, type: Int): String? {
        tracks.groups.forEachIndexed { g, group ->
            if (group.type != type || !group.isSelected) return@forEachIndexed
            for (i in 0 until group.length) {
                if (group.isTrackSelected(i)) return trackId(group.getTrackFormat(i), type, g, i)
            }
        }
        return null
    }

    /** Group and index of the track with contract [id]. */
    fun find(tracks: Tracks, type: Int, id: String): Pair<Tracks.Group, Int>? {
        tracks.groups.forEachIndexed { g, group ->
            if (group.type != type) return@forEachIndexed
            for (i in 0 until group.length) {
                if (trackId(group.getTrackFormat(i), type, g, i) == id) return group to i
            }
        }
        return null
    }

    fun selectedFormat(tracks: Tracks, type: Int): Format? {
        for (group in tracks.groups) {
            if (group.type != type || !group.isSelected) continue
            for (i in 0 until group.length) if (group.isTrackSelected(i)) return group.getTrackFormat(i)
        }
        return null
    }

    /** Software decoders by name; everything else is treated as hardware. */
    fun isHardwareDecoder(name: String): Boolean =
        !(name.startsWith("OMX.google.") || name.startsWith("c2.android.") || name.contains(".sw.") ||
            name.startsWith("Ffmpeg") || name.startsWith("lib"))
}
