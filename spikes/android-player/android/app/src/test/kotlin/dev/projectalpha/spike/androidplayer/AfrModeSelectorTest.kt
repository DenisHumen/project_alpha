// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AfrModeSelectorTest {
    // Typical 4K Android TV mode list (refresh rates as reported by real devices).
    private val uhd60 = ModeSpec(1, 3840, 2160, 60f)
    private val uhd5994 = ModeSpec(2, 3840, 2160, 59.94006f)
    private val uhd50 = ModeSpec(3, 3840, 2160, 50f)
    private val uhd24 = ModeSpec(4, 3840, 2160, 24f)
    private val uhd23976 = ModeSpec(5, 3840, 2160, 23.976025f)
    private val fhd23976 = ModeSpec(6, 1920, 1080, 23.976025f)
    private val all = listOf(uhd60, uhd5994, uhd50, uhd24, uhd23976, fhd23976)

    @Test fun `23_976 content picks 23_976 Hz, not 24 Hz`() =
        assertEquals(uhd23976, AfrModeSelector.select(all, uhd60, 23.976f))

    @Test fun `24 content picks 24 Hz`() = assertEquals(uhd24, AfrModeSelector.select(all, uhd60, 24f))

    @Test fun `25 content picks 50 Hz when there is no 25 Hz mode`() =
        assertEquals(uhd50, AfrModeSelector.select(all, uhd60, 25f))

    @Test fun `29_97 content keeps current 59_94 Hz mode`() =
        assertEquals(uhd5994, AfrModeSelector.select(all, uhd5994, 29.97f))

    @Test fun `59_94 content from 60 Hz switches to 59_94 Hz`() =
        assertEquals(uhd5994, AfrModeSelector.select(all, uhd60, 59.94f))

    @Test fun `resolution is kept`() =
        assertEquals(ModeSpec(7, 1920, 1080, 23.976025f), AfrModeSelector.select(listOf(uhd60, ModeSpec(7, 1920, 1080, 23.976025f), ModeSpec(8, 1920, 1080, 60f)), ModeSpec(8, 1920, 1080, 60f), 23.976f))

    @Test fun `23_976 content falls back to 24 Hz when it is the only 24p mode`() =
        assertEquals(uhd24, AfrModeSelector.select(listOf(uhd60, uhd24), uhd60, 23.976f))

    @Test fun `no matching mode keeps current`() = assertNull(AfrModeSelector.select(listOf(uhd60, uhd50), uhd60, 24f))

    @Test fun `unknown frame rate keeps current`() {
        assertNull(AfrModeSelector.select(all, uhd60, 0f))
        assertNull(AfrModeSelector.select(all, uhd60, Float.NaN))
    }

    @Test fun `120 Hz panel keeps current for 24 fps`() {
        val uhd120 = ModeSpec(9, 3840, 2160, 120f)
        assertEquals(uhd120, AfrModeSelector.select(listOf(uhd120, uhd24), uhd120, 24f))
    }

    @Test fun `estimator recovers 23_976 from millisecond-rounded MKV timestamps`() {
        val stamps = (0 until 25).map { i -> (i * 1_001_000L / 24) / 1000 * 1000 }
        assertEquals(23.976f, FrameRateEstimator.fromTimestampsUs(stamps), 0.0001f)
    }

    @Test fun `estimator handles out-of-order input and 25 fps`() =
        assertEquals(25f, FrameRateEstimator.fromTimestampsUs((0 until 25).map { it * 40_000L }.shuffled()), 0.0001f)

    @Test fun `estimator ignores gaps left by dropped frames`() {
        val stamps = (0 until 80).filter { it % 4 != 3 }.map { i -> (i * 1_001_000L / 24) / 1000 * 1000 }
        assertEquals(23.976f, FrameRateEstimator.fromTimestampsUs(stamps), 0.0001f)
    }

    @Test fun `estimator tells 29_97 from 30 with 72 millisecond-rounded frames`() {
        val ntsc = (0 until 72).map { i -> (i * 1_001_000L / 30) / 1000 * 1000 }
        val pal = (0 until 72).map { i -> (i * 1_000_000L / 30) / 1000 * 1000 }
        assertEquals(29.97f, FrameRateEstimator.fromTimestampsUs(ntsc), 0.0001f)
        assertEquals(30f, FrameRateEstimator.fromTimestampsUs(pal), 0.0001f)
    }

    @Test fun `snap fixes the 24_9 fps Media3 reports for a 25 fps MP4`() {
        assertEquals(25f, FrameRateEstimator.snap(24.900398f), 0f)
        assertEquals(23.976f, FrameRateEstimator.snap(23.976023f), 0f)
        assertEquals(-1f, FrameRateEstimator.snap(-1f), 0f)
        assertEquals(12.5f, FrameRateEstimator.snap(12.5f), 0f)
    }

    @Test fun `estimator returns -1 without enough frames`() = assertEquals(-1f, FrameRateEstimator.fromTimestampsUs(listOf(0L)), 0f)

    @Test fun `multipleOf rejects 24 Hz for 23_976 in strict mode`() {
        assertNull(AfrModeSelector.multipleOf(24f, 23.976f, 0.0005f))
        assertEquals(2, AfrModeSelector.multipleOf(47.952f, 23.976f, 0.0005f))
    }
}
