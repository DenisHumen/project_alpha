// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player

import android.app.Activity
import android.os.Bundle
import android.os.SystemClock
import android.view.SurfaceView
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import dev.projectalpha.core.ContractFixture
import dev.projectalpha.core.MediaSource
import dev.projectalpha.core.PlayerEngine
import dev.projectalpha.core.PlayerEngineHarness
import java.io.File

/** Full-screen SurfaceView: the video output of the tests, as in the player Activity (ADR-0008). */
class SurfaceActivity : Activity() {
    lateinit var surfaceView: SurfaceView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        surfaceView = SurfaceView(this)
        setContentView(surfaceView)
    }
}

/** Runs [block] on the main thread and returns its result. */
fun <T> onMain(block: () -> T): T {
    var result: Result<T>? = null
    InstrumentationRegistry.getInstrumentation().runOnMainSync { result = runCatching(block) }
    return result!!.getOrThrow()
}

/** Test media from tools/make-fixtures.sh, copied from the test APK assets to the cache directory. */
object Fixtures {
    /** Durations as Media3 reports them (12 s of video; AAC adds a few ms). */
    const val DURATION_MS = 12_000uL

    private val dir: File by lazy {
        val context = InstrumentationRegistry.getInstrumentation().context
        val target = InstrumentationRegistry.getInstrumentation().targetContext
        File(target.cacheDir, "fixtures").apply {
            mkdirs()
            for (name in listOf("h264.mkv", "hevc.mkv", "external.srt")) {
                context.assets.open(name).use { input -> File(this, name).outputStream().use { input.copyTo(it) } }
            }
        }
    }

    fun uri(name: String): String = "file://" + File(dir, name).absolutePath

    /** H.264 and HEVC: the contract plays both codecs. */
    fun contract() = ContractFixture(
        items = listOf(MediaSource(uri("h264.mkv"), "H.264"), MediaSource(uri("hevc.mkv"), "HEVC")),
        durationsMs = listOf(DURATION_MS, DURATION_MS),
        broken = MediaSource(uri("missing.mkv"), null),
        externalSubtitle = uri("external.srt"),
    )
}

/** A [Media3Engine] rendering into [SurfaceActivity]. Close it after the test. */
class EngineUnderTest : AutoCloseable {
    private val scenario = ActivityScenario.launch(SurfaceActivity::class.java)
    val engine: Media3Engine = onMain {
        Media3Engine(InstrumentationRegistry.getInstrumentation().targetContext)
    }

    init {
        scenario.onActivity { engine.setVideoSurfaceView(it.surfaceView) }
    }

    /**
     * Waits [ms] of wall time, then until the engine has finished the commands sent so far (loading, seeking,
     * track changes), then publishes the current position: what the contract means by `advance`.
     */
    fun advance(ms: Long) {
        if (ms > 0) Thread.sleep(ms)
        val deadline = SystemClock.elapsedRealtime() + SETTLE_TIMEOUT_MS
        while (!onMain { engine.isSettled() } && SystemClock.elapsedRealtime() < deadline) Thread.sleep(20)
        onMain { engine.publishPosition() }
    }

    override fun close() {
        runCatching { engine.closeEngine() }
        scenario.close()
    }

    companion object {
        const val SETTLE_TIMEOUT_MS = 20_000L
    }
}

/** The contract harness of pa-ffi over a real [Media3Engine]. */
class Media3Harness(private val under: EngineUnderTest) : PlayerEngineHarness {
    override fun engine(): PlayerEngine = under.engine
    override fun fixture(): ContractFixture = Fixtures.contract()
    override fun advance(ms: ULong) = under.advance(ms.toLong())
}
