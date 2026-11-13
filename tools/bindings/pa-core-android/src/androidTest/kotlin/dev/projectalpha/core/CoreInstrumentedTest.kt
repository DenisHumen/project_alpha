// SPDX-License-Identifier: MPL-2.0
// CORE-004: the Rust core answers over UniFFI + JNA on an Android device or
// emulator. Run: `just core test-aar` (needs a booted emulator or device).
package dev.projectalpha.core

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.CopyOnWriteArrayList
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class CoreInstrumentedTest {
    @After
    fun tearDown() {
        coreShutdown()
    }

    @Test
    fun coreVersionIsSemver() {
        // The first call loads libpa_ffi.so and runs UniFFI's contract-version
        // and checksum checks.
        val version = coreVersion()
        assertTrue("unexpected core version: $version", Regex("""\d+\.\d+\.\d+""").matches(version))
    }

    @Test
    fun coreStartsInAppDirsAndKeepsSettings() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val root = context.filesDir.resolve("pa-core-test").apply { deleteRecursively() }
        val dirs = CoreDirs(
            data = root.resolve("data").path,
            config = root.resolve("config").path,
            cache = context.cacheDir.resolve("pa-core-test").path,
        )
        val states = CopyOnWriteArrayList<CoreLifecycle>()
        val callbacks = object : CoreCallbacks {
            override fun onLifecycleChanged(state: CoreLifecycle) {
                states += state
            }
        }

        coreInit(CoreConfig(dirs = dirs, workerThreads = 1u, startInBackground = false), callbacks)
        assertEquals(CoreLifecycle.FOREGROUND, coreLifecycle())
        assertEquals(dirs, coreDirs())

        settingsSet("player.volume", "0.5")
        assertEquals("0.5", settingsGet("player.volume"))
        coreEnterBackground()
        assertEquals(CoreLifecycle.BACKGROUND, coreLifecycle())
        coreShutdown()
        assertEquals(CoreLifecycle.NOT_INITIALIZED, coreLifecycle())

        // The settings database lives in the app's files dir and survives a restart.
        coreInit(CoreConfig(dirs = dirs, workerThreads = 1u, startInBackground = true), null)
        assertEquals(CoreLifecycle.BACKGROUND, coreLifecycle())
        assertEquals("0.5", settingsGet("player.volume"))
        assertTrue(settingsRemove("player.volume"))
        assertNull(settingsGet("player.volume"))

        assertTrue("callbacks: $states", states.containsAll(listOf(CoreLifecycle.FOREGROUND, CoreLifecycle.BACKGROUND)))
    }
}
