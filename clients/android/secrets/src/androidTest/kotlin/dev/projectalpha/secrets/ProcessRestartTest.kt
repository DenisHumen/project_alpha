// SPDX-License-Identifier: MPL-2.0
// CORE-015: secrets survive a process restart. The Android Test Orchestrator
// (build.gradle.kts) runs every test method in a new process; the methods run
// in name order and each checks it is not in the process of the step before.
package dev.projectalpha.secrets

import android.content.Context
import android.os.Process
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.projectalpha.core.secretDelete
import dev.projectalpha.core.secretGet
import dev.projectalpha.core.secretSet
import dev.projectalpha.core.secretsOpen
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.FixMethodOrder
import org.junit.Test
import org.junit.runner.RunWith
import org.junit.runners.MethodSorters

@RunWith(AndroidJUnit4::class)
@FixMethodOrder(MethodSorters.NAME_ASCENDING)
class ProcessRestartTest {
    private val context: Context = InstrumentationRegistry.getInstrumentation().targetContext
    private val name = "kinolune_secrets_restart"
    private val pidFile = context.filesDir.resolve("secrets-restart-test.pid")

    /** Opens the store through the core, as the app does on start. */
    private fun openStore(): KeystoreSecretStore {
        val store = KeystoreSecretStore(context, name)
        secretsOpen(CorePlatformSecretStore(store))
        return store
    }

    /** Fails unless the previous step ran in another process; records this one. */
    private fun nextProcess(first: Boolean = false) {
        val pid = Process.myPid()
        if (!first) {
            assertTrue("previous step did not run (run the whole class)", pidFile.exists())
            assertNotEquals(
                "same process as the previous step: the orchestrator is not active",
                pidFile.readText().trim().toInt(),
                pid,
            )
        }
        pidFile.writeText(pid.toString())
    }

    @Test
    fun step1_writeBeforeRestart() {
        nextProcess(first = true)
        KeystoreSecretStore(context, name).clear()
        openStore()
        secretSet("source.password:1", "first")
        secretSet("trakt.tokens:9", "to-delete")
    }

    @Test
    fun step2_readOverwriteDeleteAfterRestart() {
        nextProcess()
        openStore()
        assertEquals("first", secretGet("source.password:1"))
        assertEquals("to-delete", secretGet("trakt.tokens:9"))
        secretSet("source.password:1", "overwritten")
        assertTrue(secretDelete("trakt.tokens:9"))
    }

    @Test
    fun step3_changesSurviveAnotherRestart() {
        nextProcess()
        val store = openStore()
        assertEquals("overwritten", secretGet("source.password:1"))
        assertNull(secretGet("trakt.tokens:9"))
        store.clear()
        pidFile.delete()
    }
}
