// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import dev.projectalpha.core.EngineException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class Media3MappingTest {
    private fun format(mime: String, codecs: String? = null) =
        Format.Builder().setSampleMimeType(mime).setCodecs(codecs).build()

    @Test
    fun codecNamesUseFfmpegSpelling() {
        assertEquals("h264", Media3Mapping.codecName(format(MimeTypes.VIDEO_H264)))
        assertEquals("hevc", Media3Mapping.codecName(format(MimeTypes.VIDEO_H265)))
        assertEquals("eac3", Media3Mapping.codecName(format(MimeTypes.AUDIO_E_AC3_JOC)))
        assertEquals("truehd", Media3Mapping.codecName(format(MimeTypes.AUDIO_TRUEHD)))
        // Subtitles parsed during extraction keep the original type in `codecs`.
        assertEquals("subrip", Media3Mapping.codecName(format(MimeTypes.APPLICATION_MEDIA3_CUES, MimeTypes.APPLICATION_SUBRIP)))
        assertEquals("matroska", Media3Mapping.codecName(format("video/x-matroska")))
        assertNull(Media3Mapping.codecName(null))
    }

    @Test
    fun subtitleMimeTypeByExtension() {
        assertEquals(MimeTypes.APPLICATION_SUBRIP, Media3Mapping.subtitleMimeType("file:///a/b.srt"))
        assertEquals(MimeTypes.TEXT_SSA, Media3Mapping.subtitleMimeType("http://h/x.ASS?token=1"))
        assertEquals(MimeTypes.TEXT_VTT, Media3Mapping.subtitleMimeType("/x.vtt"))
    }

    @Test
    fun errorsMapToContractKinds() {
        fun kind(code: Int) = Media3Mapping.error(PlaybackException("x", null, code))
        assertTrue(kind(PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND) is EngineException.Source)
        assertTrue(kind(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED) is EngineException.Source)
        assertTrue(kind(PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED) is EngineException.Decoder)
        assertTrue(kind(PlaybackException.ERROR_CODE_DECODER_INIT_FAILED) is EngineException.Decoder)
        assertTrue(kind(PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED) is EngineException.Unsupported)
        assertTrue(kind(PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED) is EngineException.Unsupported)
        assertTrue(kind(PlaybackException.ERROR_CODE_UNSPECIFIED) is EngineException.Internal)
    }

    @Test
    fun trackModel() {
        val f = Format.Builder().setId("2").setSampleMimeType(MimeTypes.AUDIO_AAC).setLanguage("rus")
            .setLabel("Russian").setChannelCount(6).setSelectionFlags(C.SELECTION_FLAG_DEFAULT).build()
        val t = Media3Mapping.track(f, Media3Mapping.trackId(f, C.TRACK_TYPE_AUDIO, 1, 0))
        assertEquals("2", t.id)
        assertEquals("Russian", t.title)
        assertEquals("ru", t.language) // Media3 normalises to BCP 47
        assertEquals("aac", t.codec)
        assertEquals(6u, t.channels)
        assertTrue(t.default)
        assertFalse(t.forced || t.external)
        val noId = Format.Builder().setSampleMimeType(MimeTypes.APPLICATION_SUBRIP).build()
        assertEquals("3-0-1", Media3Mapping.trackId(noId, C.TRACK_TYPE_TEXT, 0, 1))
        val merged = Format.Builder().setId("0:4").build()
        assertEquals("4", Media3Mapping.trackId(merged, C.TRACK_TYPE_TEXT, 0, 0))
        val ext = Format.Builder().setId("ext-0").setLanguage(C.LANGUAGE_UNDETERMINED).build()
        val e = Media3Mapping.track(ext, "ext-0")
        assertTrue(e.external)
        assertNull(e.language)
    }

    @Test
    fun hardwareDecoderByName() {
        assertTrue(Media3Mapping.isHardwareDecoder("c2.amlogic.hevc.decoder"))
        assertTrue(Media3Mapping.isHardwareDecoder("OMX.MTK.VIDEO.DECODER.AVC"))
        assertFalse(Media3Mapping.isHardwareDecoder("c2.android.hevc.decoder"))
        assertFalse(Media3Mapping.isHardwareDecoder("OMX.google.h264.decoder"))
    }
}
