// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player

import dev.projectalpha.core.MediaSource
import dev.projectalpha.core.OpenRequest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.junit.runners.Parameterized
import kotlin.math.abs

/**
 * PLAY-AND-002 "done when": an H.264 and an HEVC MKV play (frames are decoded and rendered to a SurfaceView),
 * seek works, audio and subtitle tracks switch.
 */
@RunWith(Parameterized::class)
class Media3PlaybackTest(private val file: String, private val codec: String) {
    @Test
    fun playsSeeksAndSwitchesTracks() {
        EngineUnderTest().use { under ->
            val engine = under.engine
            engine.open(OpenRequest(listOf(MediaSource(Fixtures.uri(file), null)), 0u, 0uL, true))
            under.advance(0)
            var s = engine.state()
            assertTrue("loaded: $s", s.loaded && s.playing && s.error == null)
            assertEquals(320u, s.videoWidth)
            assertEquals(240u, s.videoHeight)
            assertNear(Fixtures.DURATION_MS, s.durationMs, "duration")
            assertEquals(codec, engine.stats().videoCodec)
            assertEquals("aac", engine.stats().audioCodec)

            // Plays: the position moves and frames reach the surface.
            under.advance(2_000)
            s = engine.state()
            assertTrue("position after 2 s: ${s.positionMs}", s.positionMs >= 1_200uL)
            val (rendered, decoder) = onMain { engine.videoOutput() }
            assertTrue("rendered frames: $rendered", rendered >= 20)
            assertNotNull("video decoder", decoder)

            // Seek forward and keep playing.
            engine.seek(7_000uL)
            under.advance(0)
            assertNear(7_000uL, engine.state().positionMs, "after seek")
            under.advance(1_000)
            assertNear(8_000uL, engine.state().positionMs, "playing after seek")

            // Tracks as in the file: two audio (eng default, rus), one subtitle (eng, off).
            s = engine.state()
            assertEquals(listOf("eng", "rus"), s.audioTracks.map { it.language })
            assertEquals(listOf("eng"), s.subtitleTracks.map { it.language })
            assertEquals(s.audioTracks.first { it.default }.id, s.audioTrack)
            assertNull(s.subtitleTrack)

            // Switch audio to Russian and turn subtitles on, then off.
            val rus = s.audioTracks.first { it.language == "rus" }.id
            val sub = s.subtitleTracks.first().id
            engine.setAudioTrack(rus)
            engine.setSubtitleTrack(sub)
            under.advance(0)
            s = engine.state()
            assertEquals(rus, s.audioTrack)
            assertEquals(sub, s.subtitleTrack)
            assertTrue("still playing: $s", s.playing && s.error == null)
            engine.setSubtitleTrack(null)
            under.advance(0)
            assertNull(engine.state().subtitleTrack)

            // Seek back to the start while paused.
            engine.pause()
            engine.seek(500uL)
            under.advance(500)
            s = engine.state()
            assertTrue(!s.playing)
            assertNear(500uL, s.positionMs, "paused after seek back")
        }
    }

    private fun assertNear(expected: ULong, actual: ULong, what: String) =
        assertTrue("$what: expected ~$expected ms, got $actual ms", abs(expected.toLong() - actual.toLong()) <= 500)

    companion object {
        @JvmStatic
        @Parameterized.Parameters(name = "{1}")
        fun media(): List<Array<String>> = listOf(arrayOf("h264.mkv", "h264"), arrayOf("hevc.mkv", "hevc"))
    }
}
