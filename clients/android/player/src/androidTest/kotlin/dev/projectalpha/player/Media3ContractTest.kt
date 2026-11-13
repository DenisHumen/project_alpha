// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player

import dev.projectalpha.core.playerEngineContractCases
import dev.projectalpha.core.playerEngineContractRun
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.junit.runners.Parameterized

/**
 * The core contract suite (`pa_playback::contract`, ADR-0017) against Media3 on a device: every case the mock
 * passes runs here through `playerEngineContractRun`, on H.264 and HEVC MKV files with two audio tracks and
 * subtitles.
 */
@RunWith(Parameterized::class)
class Media3ContractTest(private val case: String) {
    @Test
    fun contractCase() {
        EngineUnderTest().use { under ->
            val failure = playerEngineContractRun(case, Media3Harness(under))
            assertNull("contract case $case: $failure", failure)
        }
    }

    companion object {
        @JvmStatic
        @Parameterized.Parameters(name = "{0}")
        fun cases(): List<String> = playerEngineContractCases()
    }
}
