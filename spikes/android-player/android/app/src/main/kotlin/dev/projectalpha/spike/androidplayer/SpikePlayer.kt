// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.spike.androidplayer

import android.app.Activity
import android.media.MediaFormat
import android.os.Handler
import android.os.Looper
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.Tracks
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DecoderReuseEvaluation
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import androidx.media3.exoplayer.trackselection.DefaultTrackSelector
import androidx.media3.exoplayer.video.VideoFrameMetadataListener

/**
 * ExoPlayer wrapper shared by all embedding variants: same renderers, same AFR flow, same metrics,
 * so the variants differ only in how the video surface is attached to the UI.
 *
 * Start-up flow: prepare paused → video track known → AFR switch (and optional settle delay) → play.
 */
@OptIn(UnstableApi::class)
class SpikePlayer(private val activity: Activity, private val options: PlayerOptions) {
    val afr = AfrController(activity)
    val exo: ExoPlayer
    private var openedAt = SpikeLog.now()
    private var afrStarted = false
    private var droppedFrames = 0
    private var decoderName: String? = null
    private var videoFormat: Format? = null
    private var firstFrameMs: Long? = null

    private val metrics = object : AnalyticsListener {
        override fun onRenderedFirstFrame(eventTime: AnalyticsListener.EventTime, output: Any, renderTimeMs: Long) {
            firstFrameMs = SpikeLog.now() - openedAt
            SpikeLog.metric("first_frame", mapOf("since_open_ms" to firstFrameMs, "output" to output.javaClass.simpleName))
        }

        override fun onIsPlayingChanged(eventTime: AnalyticsListener.EventTime, isPlaying: Boolean) {
            if (isPlaying) SpikeLog.metric("playing", mapOf("since_open_ms" to SpikeLog.now() - openedAt))
        }

        override fun onVideoDecoderInitialized(eventTime: AnalyticsListener.EventTime, decoderName: String, initializedTimestampMs: Long, initializationDurationMs: Long) {
            this@SpikePlayer.decoderName = decoderName
            SpikeLog.metric("decoder", mapOf("name" to decoderName, "init_ms" to initializationDurationMs))
        }

        override fun onVideoInputFormatChanged(eventTime: AnalyticsListener.EventTime, format: Format, decoderReuseEvaluation: DecoderReuseEvaluation?) {
            videoFormat = format
        }

        override fun onDroppedVideoFrames(eventTime: AnalyticsListener.EventTime, droppedFrames: Int, elapsedMs: Long) {
            this@SpikePlayer.droppedFrames += droppedFrames
            SpikeLog.metric("dropped_frames", mapOf("count" to droppedFrames, "total" to this@SpikePlayer.droppedFrames, "elapsed_ms" to elapsedMs))
        }

        override fun onPlayerError(eventTime: AnalyticsListener.EventTime, error: PlaybackException) {
            SpikeLog.metric("error", mapOf("code" to error.errorCodeName, "message" to error.message, "cause" to error.cause?.toString()))
        }
    }

    init {
        val renderers = DefaultRenderersFactory(activity)
            .setEnableDecoderFallback(true)
            .setExtensionRendererMode(DefaultRenderersFactory.EXTENSION_RENDERER_MODE_OFF)
        val trackSelector = DefaultTrackSelector(activity).apply {
            setParameters(buildUponParameters().setTunnelingEnabled(options.tunneling))
        }
        exo = ExoPlayer.Builder(activity, renderers).setTrackSelector(trackSelector).build()
        // `surface` AFR calls Surface.setFrameRate itself with CHANGE_FRAME_RATE_ALWAYS; Media3 must not override it.
        exo.videoChangeFrameRateStrategy = if (options.afr == "surface") {
            C.VIDEO_CHANGE_FRAME_RATE_STRATEGY_OFF
        } else {
            C.VIDEO_CHANGE_FRAME_RATE_STRATEGY_ONLY_IF_SEAMLESS
        }
        exo.addAnalyticsListener(metrics)
        exo.addListener(object : Player.Listener {
            override fun onTracksChanged(tracks: Tracks) = maybeStartAfr(tracks)
        })
    }

    /** Called by the variant once the video output (SurfaceView/TextureView) is attached. */
    fun open(onAfrSurface: (Float) -> Unit = {}) {
        this.onAfrSurface = onAfrSurface
        openedAt = SpikeLog.now()
        SpikeLog.metric("open", mapOf("variant" to options.variant, "afr" to options.afr, "tunneling" to options.tunneling, "url" to options.url))
        exo.setMediaItem(MediaItem.fromUri(options.url))
        exo.playWhenReady = false
        exo.prepare()
    }

    private var onAfrSurface: (Float) -> Unit = {}

    private fun maybeStartAfr(tracks: Tracks) {
        if (afrStarted) return
        val group = tracks.groups.firstOrNull { it.type == C.TRACK_TYPE_VIDEO && it.isSelected } ?: return
        val format = (0 until group.length).firstOrNull { group.isTrackSelected(it) }?.let { group.getTrackFormat(it) } ?: return
        afrStarted = true
        SpikeLog.metric("video_track", formatMap(format) + mapOf("since_open_ms" to SpikeLog.now() - openedAt))
        when {
            options.afr == "off" -> play()
            // Container rates are approximate (24.900398 for a 25 fps MP4), so snap them like estimates.
            format.frameRate > 0 -> applyAfr(FrameRateEstimator.snap(format.frameRate))
            else -> estimateFrameRateThenApplyAfr()
        }
    }

    private fun applyAfr(fps: Float) {
        if (options.afr == "mode") afr.switchTo(fps) { play() } else { onAfrSurface(fps); play() }
    }

    /**
     * Container has no frame rate: play ~3 s (needed to tell 23.976 from 24 with 1 ms timestamps), estimate from
     * rendered timestamps, pause, switch, resume. Production should get the rate before playback (pa-probe).
     */
    private fun estimateFrameRateThenApplyAfr() {
        val stamps = ArrayList<Long>()
        val main = Handler(Looper.getMainLooper())
        val listener = object : VideoFrameMetadataListener {
            private var done = false

            override fun onVideoFrameAboutToBeRendered(presentationTimeUs: Long, releaseTimeNs: Long, format: Format, mediaFormat: MediaFormat?) {
                synchronized(stamps) {
                    if (done) return
                    stamps.add(presentationTimeUs)
                    if (presentationTimeUs - stamps.first() < ESTIMATE_SPAN_US) return
                    done = true
                }
                main.post {
                    exo.clearVideoFrameMetadataListener(this)
                    val fps = FrameRateEstimator.fromTimestampsUs(synchronized(stamps) { stamps.toList() })
                    SpikeLog.metric("fps_estimated", mapOf("fps" to fps, "since_open_ms" to SpikeLog.now() - openedAt))
                    if (options.afr == "mode") exo.pause()
                    applyAfr(fps)
                }
            }
        }
        exo.setVideoFrameMetadataListener(listener)
        SpikeLog.metric("fps_unknown", mapOf("action" to "estimate_from_timestamps"))
        exo.play()
    }

    private fun play() {
        if (options.settleMs > 0) {
            activity.window.decorView.postDelayed({ exo.play() }, options.settleMs)
        } else {
            exo.play()
        }
    }

    fun release() {
        SpikeLog.metric("release", stats())
        exo.release()
        afr.restore()
    }

    fun stats(): Map<String, Any?> = buildMap {
        put("variant", options.variant)
        put("position_ms", exo.currentPosition)
        put("duration_ms", exo.duration.takeIf { it != C.TIME_UNSET })
        put("is_playing", exo.isPlaying)
        put("decoder", decoderName)
        put("dropped_frames", droppedFrames)
        put("first_frame_ms", firstFrameMs)
        put("display", afr.describeCurrentMode())
        videoFormat?.let { putAll(formatMap(it)) }
    }

    companion object {
        private const val ESTIMATE_SPAN_US = 3_000_000L

        fun formatMap(f: Format): Map<String, Any?> = mapOf(
            "mime" to f.sampleMimeType,
            "codecs" to f.codecs,
            "width" to f.width,
            "height" to f.height,
            "fps" to f.frameRate,
            // C.COLOR_TRANSFER_ST2084 = 6 (PQ / HDR10 / DV), C.COLOR_TRANSFER_HLG = 7.
            "color_transfer" to f.colorInfo?.colorTransfer,
            "color_space" to f.colorInfo?.colorSpace,
            "hdr_static_info" to (f.colorInfo?.hdrStaticInfo != null),
        )
    }
}
