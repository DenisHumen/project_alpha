// SPDX-License-Identifier: MPL-2.0
import dev.projectalpha.core.coreVersion
import kotlin.test.Test
import kotlin.test.assertTrue

class CoreVersionTest {
    @Test
    fun coreVersionIsSemver() {
        val version = coreVersion()
        assertTrue(Regex("""\d+\.\d+\.\d+""").matches(version), "unexpected core version: $version")
    }
}
