// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player.flutter

import android.os.Handler
import android.os.Looper
import dev.projectalpha.core.EngineEvent
import dev.projectalpha.core.EngineException
import dev.projectalpha.core.EngineListener
import dev.projectalpha.core.RepeatMode
import dev.projectalpha.core.engineEventToJson
import dev.projectalpha.core.engineStateToJson
import dev.projectalpha.player.Media3Engine
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import org.json.JSONObject

/**
 * Connects the Flutter OSD (`PlayerOsdBridge` in Dart) with the [Media3Engine] of the player Activity:
 * `pa_player_android/osd` carries the contract commands, `pa_player_android/events` every event together with
 * the state after it (JSON of the core types, `engineEventToJson` / `engineStateToJson`).
 */
internal class OsdBridge(
    messenger: BinaryMessenger,
    private val engine: Media3Engine,
    private val onClose: () -> Unit,
) : EngineListener {
    private val main = Handler(Looper.getMainLooper())
    private val commands = MethodChannel(messenger, "pa_player_android/osd")
    private val events = EventChannel(messenger, "pa_player_android/events")
    private var sink: EventChannel.EventSink? = null

    init {
        commands.setMethodCallHandler(::handle)
        events.setStreamHandler(object : EventChannel.StreamHandler {
            override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
                sink = events
                // A late OSD starts from the current state.
                send("""{"event":"snapshot"}""", engineStateToJson(engine.state()))
            }

            override fun onCancel(arguments: Any?) {
                sink = null
            }
        })
    }

    override fun onEvent(event: EngineEvent) {
        val json = engineEventToJson(event)
        val state = engineStateToJson(engine.state())
        // `Closed` may come from the thread that closed the engine; channels need the main thread.
        if (Looper.myLooper() == Looper.getMainLooper()) send(json, state) else main.post { send(json, state) }
    }

    private fun send(event: String, state: String) {
        sink?.success("""{"event":$event,"state":$state}""")
    }

    fun dispose() {
        commands.setMethodCallHandler(null)
        events.setStreamHandler(null)
        sink = null
    }

    private fun handle(call: MethodCall, result: MethodChannel.Result) {
        try {
            when (call.method) {
                "play" -> engine.play()
                "pause" -> engine.pause()
                "seek" -> engine.seek(call.long().toULong())
                "setRate" -> engine.setRate(call.arguments<Number>()!!.toDouble())
                "setVolume" -> engine.setVolume(call.arguments<Number>()!!.toDouble())
                "setRepeat" -> engine.setRepeat(RepeatMode.valueOf(call.arguments<String>()!!.uppercase()))
                "jump" -> engine.jump(call.long().toUInt())
                "setAudioTrack" -> engine.setAudioTrack(call.arguments<String>())
                "setSubtitleTrack" -> engine.setSubtitleTrack(call.arguments<String>())
                "addExternalSubtitle" -> engine.addExternalSubtitle(call.argument<String>("uri")!!, call.argument<String>("title"))
                "stats" -> return result.success(statsJson())
                "close" -> onClose()
                else -> return result.notImplemented()
            }
            result.success(null)
        } catch (e: EngineException) {
            result.error(errorCode(e), e.message, null)
        } catch (e: RuntimeException) {
            result.error("invalid_argument", e.toString(), null)
        }
    }

    private fun MethodCall.long(): Long = arguments<Number>()!!.toLong().also {
        if (it < 0) throw EngineException.InvalidArgument("negative value $it")
    }

    private fun statsJson(): String = engine.stats().let { s ->
        JSONObject()
            .put("video_codec", s.videoCodec)
            .put("audio_codec", s.audioCodec)
            .put("hw_decoding", s.hwDecoding)
            .put("audio_passthrough", s.audioPassthrough)
            .put("dropped_frames", s.droppedFrames.toLong())
            .put("bitrate_bps", s.bitrateBps?.toLong())
            .put("buffered_ms", s.bufferedMs.toLong())
            .toString()
    }

    private fun errorCode(e: EngineException): String = when (e) {
        is EngineException.InvalidState -> "invalid_state"
        is EngineException.InvalidArgument -> "invalid_argument"
        is EngineException.Unsupported -> "unsupported"
        is EngineException.Source -> "source"
        is EngineException.Decoder -> "decoder"
        is EngineException.Closed -> "closed"
        is EngineException.Internal -> "internal"
    }
}
