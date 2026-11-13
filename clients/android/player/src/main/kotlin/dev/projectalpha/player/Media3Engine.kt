// SPDX-License-Identifier: MPL-2.0
package dev.projectalpha.player

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.view.Surface
import android.view.SurfaceView
import androidx.annotation.MainThread
import androidx.annotation.OptIn
import androidx.annotation.VisibleForTesting
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.analytics.AnalyticsListener
import dev.projectalpha.core.EngineEvent
import dev.projectalpha.core.EngineException
import dev.projectalpha.core.EngineKind
import dev.projectalpha.core.EngineListener
import dev.projectalpha.core.EngineState
import dev.projectalpha.core.MediaSource
import dev.projectalpha.core.OpenRequest
import dev.projectalpha.core.PlaybackStats
import dev.projectalpha.core.PlayerEngine
import dev.projectalpha.core.RepeatMode
import dev.projectalpha.core.TrackKind
import dev.projectalpha.core.engineStateApply
import dev.projectalpha.core.engineStateDefault

/**
 * The Media3/ExoPlayer engine behind the core `PlayerEngine` contract (PLAY-AND-002, ADR-0017).
 *
 * - Commands may come from any thread. They are validated at once against [state] (a rejected command throws
 *   [EngineException] and changes nothing) and then run on the main thread, which owns the [ExoPlayer].
 * - Every change is an [EngineEvent], folded into [state] by the core reducer (`engineStateApply`), so the UI on
 *   events and the core on `state()` never disagree.
 * - Events are sent on the main thread, except `Closed`, which [closeEngine] sends before it returns.
 * - The video surface is platform UI (ADR-0008): attach a [SurfaceView] with [setVideoSurfaceView], or give
 *   [exoPlayer] to a Media3 `PlayerView` (native OSD, variant A).
 */
@OptIn(UnstableApi::class)
class Media3Engine(
    context: Context,
    private val playerFactory: (Context) -> ExoPlayer = ::defaultPlayer,
) : PlayerEngine {
    private val appContext = context.applicationContext
    private val main = Handler(Looper.getMainLooper())

    private val stateLock = Any()
    private var state: EngineState = engineStateDefault() // guarded by stateLock

    /** Serialises event delivery, so listeners see events in order and nothing after `Closed`. */
    private val emitLock = Any()
    private var closedSent = false // guarded by emitLock

    @Volatile private var listener: EngineListener? = null
    @Volatile private var closed = false

    /** An item was accepted by `open` and has not failed: commands that need an item are allowed. */
    @Volatile private var hasItem = false
    @Volatile private var itemCount = 0
    @Volatile private var statsSnapshot: PlaybackStats? = null

    // Main thread only.
    private var exo: ExoPlayer? = null
    private var items: List<MediaSource> = emptyList()
    private val externalSubtitles = mutableMapOf<Int, MutableList<MediaItem.SubtitleConfiguration>>()
    private var nextExternal = 0
    private var awaitLoaded = false
    private var pendingExternal: String? = null
    private val pendingSelection = mutableMapOf<TrackKind, String?>()
    private var ticking = false
    private var droppedFrames = 0L
    private var videoDecoder: String? = null

    // ---- PlayerEngine -------------------------------------------------------------------------

    override fun kind(): EngineKind = EngineKind.MEDIA3

    override fun setListener(listener: EngineListener?) {
        this.listener = listener
    }

    override fun state(): EngineState = synchronized(stateLock) { state }

    override fun open(request: OpenRequest) {
        checkAlive()
        if (request.items.isEmpty()) throw EngineException.InvalidArgument("the playlist is empty")
        if (request.index.toInt() !in request.items.indices) {
            throw EngineException.InvalidArgument("index ${request.index} is outside the playlist")
        }
        hasItem = true
        itemCount = request.items.size
        onMain { p -> doOpen(p, request) }
    }

    override fun play() {
        checkItem()
        onMain { p ->
            if (current().completed || p.playbackState == Player.STATE_ENDED) {
                p.seekTo(p.currentMediaItemIndex, 0)
                emitPosition(p, force = true)
            }
            p.play()
            reconcile(p)
        }
    }

    override fun pause() {
        checkItem()
        onMain { p ->
            p.pause()
            reconcile(p)
            emitPosition(p)
        }
    }

    override fun seek(positionMs: ULong) {
        checkItem()
        onMain { p ->
            val duration = p.duration.takeIf { it != C.TIME_UNSET } ?: Long.MAX_VALUE
            val target = positionMs.coerceAtMost(duration.toULong()).toLong()
            p.seekTo(target)
            emitPosition(p, force = true)
            reconcile(p)
        }
    }

    override fun setRate(rate: Double) {
        checkAlive()
        if (!rate.isFinite() || rate <= 0.0) throw EngineException.InvalidArgument("rate $rate")
        if (rate < MIN_RATE || rate > MAX_RATE) throw EngineException.Unsupported("rate $rate (Media3: $MIN_RATE..$MAX_RATE)")
        onMain { p ->
            p.setPlaybackSpeed(rate.toFloat())
            emit(EngineEvent.RateChanged(rate))
        }
    }

    override fun setVolume(volume: Double) {
        checkAlive()
        if (!volume.isFinite() || volume < 0.0 || volume > MAX_VOLUME) {
            throw EngineException.InvalidArgument("volume $volume")
        }
        // No volume boost in Media3 without an audio processor (PLAY-004): clamp to unity and report it.
        val applied = volume.coerceAtMost(UNITY_VOLUME)
        onMain { p ->
            p.volume = (applied / UNITY_VOLUME).toFloat()
            emit(EngineEvent.VolumeChanged(applied))
        }
    }

    override fun setRepeat(repeat: RepeatMode) {
        checkAlive()
        onMain { p ->
            p.repeatMode = when (repeat) {
                RepeatMode.OFF -> Player.REPEAT_MODE_OFF
                RepeatMode.ALL -> Player.REPEAT_MODE_ALL
                RepeatMode.ONE -> Player.REPEAT_MODE_ONE
            }
            emit(EngineEvent.RepeatChanged(repeat))
        }
    }

    override fun jump(index: UInt) {
        checkItem()
        if (index.toInt() !in 0 until itemCount) throw EngineException.InvalidArgument("index $index is outside the playlist")
        onMain { p ->
            startLoading(p, index.toInt())
            p.seekTo(index.toInt(), 0)
            reconcile(p)
        }
    }

    override fun setAudioTrack(id: String?) = select(TrackKind.AUDIO, id)

    override fun setSubtitleTrack(id: String?) = select(TrackKind.SUBTITLE, id)

    override fun addExternalSubtitle(uri: String, title: String?) {
        checkItem()
        onMain { p ->
            val index = current().playlistIndex.toInt()
            val id = Media3Mapping.EXTERNAL_PREFIX + nextExternal++
            val config = MediaItem.SubtitleConfiguration.Builder(Uri.parse(uri))
                .setId(id)
                .setLabel(title)
                .setMimeType(Media3Mapping.subtitleMimeType(uri))
                .build()
            externalSubtitles.getOrPut(index) { mutableListOf() }.add(config)
            // Media3 cannot add a track to a prepared item: reload it at the same position. Not a new item, so
            // no `Loading`; the track list grows when the reload is done (syncTracks), then it is selected.
            val position = p.currentPosition
            p.setMediaItems(mediaItems(), index, position)
            if (p.playbackState == Player.STATE_IDLE) p.prepare()
            pendingExternal = id
        }
    }

    override fun stats(): PlaybackStats {
        checkItem()
        return statsSnapshot ?: PlaybackStats(null, null, null, null, 0uL, null, 0uL)
    }

    override fun closeEngine() {
        synchronized(emitLock) {
            if (closed) return
            closed = true
            hasItem = false
            emit(EngineEvent.Closed)
        }
        main.post {
            main.removeCallbacks(tick)
            exo?.release()
            exo = null
        }
    }

    // ---- Video output ---------------------------------------------------------------------------

    /** The player for a Media3 `PlayerView` (native OSD) or a custom surface. Main thread only. */
    @get:MainThread
    val exoPlayer: ExoPlayer
        get() = player()

    /** Renders video into [view] (its own SurfaceFlinger layer: HDR and AFR keep working, ADR-0008). */
    @MainThread
    fun setVideoSurfaceView(view: SurfaceView?) {
        if (!closed) player().setVideoSurfaceView(view)
    }

    @MainThread
    fun setVideoSurface(surface: Surface?) {
        if (!closed) player().setVideoSurface(surface)
    }

    // ---- For tests ----------------------------------------------------------------------------

    /** All commands sent so far are done and their events sent. Main thread. */
    @VisibleForTesting
    @MainThread
    internal fun isSettled(): Boolean {
        val p = exo ?: return true
        return closed || !hasItem || current().error != null || (
            !awaitLoaded && pendingExternal == null && pendingSelection.isEmpty() &&
                p.playbackState != Player.STATE_BUFFERING
            )
    }

    /** Sends the current position now (the contract harness reads `state()` right after waiting). */
    @VisibleForTesting
    @MainThread
    internal fun publishPosition() {
        val p = exo ?: return
        if (!closed && hasItem && !current().completed) emitPosition(p)
    }

    /** Video frames rendered so far in the current item, and the video decoder. Main thread. */
    @VisibleForTesting
    @MainThread
    internal fun videoOutput(): Pair<Int, String?> =
        (exo?.videoDecoderCounters?.renderedOutputBufferCount ?: 0) to videoDecoder

    // ---- Internals (main thread) -----------------------------------------------------------------

    private fun current(): EngineState = state()

    private fun checkAlive() {
        if (closed) throw EngineException.Closed()
    }

    private fun checkItem() {
        checkAlive()
        if (!hasItem) throw EngineException.InvalidState("no item is open")
    }

    private fun select(kind: TrackKind, id: String?) {
        checkItem()
        val known = current().let { if (kind == TrackKind.AUDIO) it.audioTracks else it.subtitleTracks }
        if (id != null && known.none { it.id == id }) throw EngineException.InvalidArgument("unknown $kind track $id")
        onMain { p -> applySelection(p, kind, id) }
    }

    private fun onMain(block: (ExoPlayer) -> Unit) {
        main.post { if (!closed) block(player()) }
    }

    private fun player(): ExoPlayer = exo ?: playerFactory(appContext).also {
        it.addListener(playerListener)
        it.addAnalyticsListener(analytics)
        exo = it
    }

    private fun emit(event: EngineEvent) {
        synchronized(emitLock) {
            if (closedSent || (closed && event !is EngineEvent.Closed)) return
            synchronized(stateLock) { state = engineStateApply(state, event) }
            if (event is EngineEvent.Closed) closedSent = true
            listener?.onEvent(event)
        }
    }

    private fun mediaItems(): List<MediaItem> = items.mapIndexed { i, source ->
        MediaItem.Builder()
            .setUri(Uri.parse(source.uri))
            .setMediaId(i.toString())
            .setSubtitleConfigurations(externalSubtitles[i].orEmpty())
            .build()
    }

    private fun doOpen(p: ExoPlayer, request: OpenRequest) {
        items = request.items
        externalSubtitles.clear()
        startLoading(p, request.index.toInt())
        p.setMediaItems(mediaItems(), request.index.toInt(), request.startPositionMs.coerceAtMost(Long.MAX_VALUE.toULong()).toLong())
        p.playWhenReady = request.play
        p.prepare()
    }

    /** A new item starts opening: `Loading`, default track selection, wait for `Loaded`. */
    private fun startLoading(p: ExoPlayer, index: Int) {
        pendingExternal = null
        pendingSelection.clear()
        droppedFrames = 0
        p.trackSelectionParameters = p.trackSelectionParameters.buildUpon()
            .clearOverrides()
            .setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, false)
            .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
            .build()
        awaitLoaded = true
        emit(EngineEvent.Loading(index.toUInt()))
    }

    /** Sends `Loaded` and the first track and position events once the item is ready. */
    private fun maybeLoaded(p: ExoPlayer): Boolean {
        val ready = p.playbackState == Player.STATE_READY || p.playbackState == Player.STATE_ENDED
        val duration = p.duration
        if (!ready || duration == C.TIME_UNSET || p.currentMediaItemIndex != current().playlistIndex.toInt() || p.currentTracks.isEmpty) {
            return false
        }
        awaitLoaded = false
        val video = Media3Mapping.selectedFormat(p.currentTracks, C.TRACK_TYPE_VIDEO)
        emit(
            EngineEvent.Loaded(
                durationMs = duration.toULong(),
                videoWidth = video?.width?.takeIf { it > 0 }?.toUInt(),
                videoHeight = video?.height?.takeIf { it > 0 }?.toUInt(),
            ),
        )
        if (p.currentPosition > duration) p.seekTo(duration)
        syncTracks(p, force = true)
        emitPosition(p, force = true)
        return true
    }

    /** Brings the state in line with the player: tracks, buffering, playing. Idempotent. */
    private fun reconcile(p: ExoPlayer) {
        if (closed || !hasItem) return
        if (awaitLoaded && !maybeLoaded(p)) return
        syncTracks(p, force = false)
        val buffering = p.playbackState == Player.STATE_BUFFERING
        if (buffering != current().buffering) emit(EngineEvent.BufferingChanged(buffering))
        val playing = p.playWhenReady && p.playbackState != Player.STATE_ENDED && p.playbackState != Player.STATE_IDLE
        if (playing != current().playing) emit(EngineEvent.PlayingChanged(playing))
        updateStats(p)
        if (playing && !ticking) {
            ticking = true
            main.postDelayed(tick, TICK_MS)
        }
    }

    private fun syncTracks(p: ExoPlayer, force: Boolean) {
        val tracks = p.currentTracks
        // An item being reloaded (external subtitle) has no tracks for a moment: not a change.
        if (tracks.isEmpty && !force) return
        val audio = Media3Mapping.tracks(tracks, C.TRACK_TYPE_AUDIO)
        val subtitles = Media3Mapping.tracks(tracks, C.TRACK_TYPE_TEXT)
        val s = current()
        if (force || audio != s.audioTracks || subtitles != s.subtitleTracks) emit(EngineEvent.TracksChanged(audio, subtitles))
        pendingExternal?.let { id ->
            if (subtitles.any { it.id == id }) {
                pendingExternal = null
                applySelection(p, TrackKind.SUBTITLE, id)
            }
        }
        for (kind in TrackKind.entries) {
            val selected = Media3Mapping.selectedId(tracks, kind.type)
            val known = current().let { if (kind == TrackKind.AUDIO) it.audioTrack else it.subtitleTrack }
            if (force || selected != known) emit(EngineEvent.TrackSelected(kind, selected))
            if (pendingSelection.containsKey(kind) && pendingSelection[kind] == selected) pendingSelection.remove(kind)
        }
    }

    private fun applySelection(p: ExoPlayer, kind: TrackKind, id: String?) {
        val builder = p.trackSelectionParameters.buildUpon()
        if (id == null) {
            builder.setTrackTypeDisabled(kind.type, true)
        } else {
            val (group, index) = Media3Mapping.find(p.currentTracks, kind.type, id) ?: return
            builder.setTrackTypeDisabled(kind.type, false)
                .setOverrideForType(TrackSelectionOverride(group.mediaTrackGroup, index))
        }
        p.trackSelectionParameters = builder.build()
        if (Media3Mapping.selectedId(p.currentTracks, kind.type) != id) pendingSelection[kind] = id
    }

    private fun emitPosition(p: ExoPlayer, force: Boolean = false) {
        val s = current()
        if (!s.loaded) return
        val duration = s.durationMs.toLong()
        val position = p.currentPosition.coerceIn(0, duration).toULong()
        val buffered = p.bufferedPosition.coerceIn(0, duration).toULong()
        if (force || position != s.positionMs || buffered != s.bufferedMs) emit(EngineEvent.Position(position, buffered))
    }

    private fun updateStats(p: ExoPlayer) {
        val video = Media3Mapping.selectedFormat(p.currentTracks, C.TRACK_TYPE_VIDEO)
        val audio = Media3Mapping.selectedFormat(p.currentTracks, C.TRACK_TYPE_AUDIO)
        statsSnapshot = PlaybackStats(
            videoCodec = Media3Mapping.codecName(video),
            audioCodec = Media3Mapping.codecName(audio),
            hwDecoding = videoDecoder?.let(Media3Mapping::isHardwareDecoder),
            // Passthrough arrives with PLAY-AND-004; until then audio is always decoded.
            audioPassthrough = audio?.let { false },
            droppedFrames = droppedFrames.toULong(),
            bitrateBps = (video?.bitrate ?: audio?.bitrate)?.takeIf { it > 0 }?.toULong(),
            bufferedMs = (p.bufferedPosition - p.currentPosition).coerceAtLeast(0).toULong(),
        )
    }

    private val tick: Runnable = object : Runnable {
        override fun run() {
            val p = exo
            if (closed || p == null || !current().playing) {
                ticking = false
                return
            }
            emitPosition(p)
            updateStats(p)
            main.postDelayed(this, TICK_MS)
        }
    }

    private val playerListener = object : Player.Listener {
        override fun onEvents(player: Player, events: Player.Events) {
            val p = exo ?: return
            if (closed || !hasItem) return
            if (events.contains(Player.EVENT_MEDIA_ITEM_TRANSITION)) {
                val index = p.currentMediaItemIndex
                // Auto-advance to the next item (or back to the first with repeat all).
                if (index != current().playlistIndex.toInt()) startLoading(p, index)
            }
            if (events.contains(Player.EVENT_POSITION_DISCONTINUITY)) emitPosition(p)
            val ended = events.contains(Player.EVENT_PLAYBACK_STATE_CHANGED) && p.playbackState == Player.STATE_ENDED
            if (ended && !awaitLoaded && !current().completed) {
                emitPosition(p, force = true)
                reconcile(p)
                emit(EngineEvent.Ended)
                // The contract pauses at the end; `play` starts the item over.
                p.playWhenReady = false
                return
            }
            reconcile(p)
        }

        override fun onPlayerError(error: PlaybackException) {
            if (closed || !hasItem) return
            hasItem = false
            awaitLoaded = false
            pendingExternal = null
            pendingSelection.clear()
            emit(EngineEvent.Error(Media3Mapping.error(error)))
        }
    }

    private val analytics = object : AnalyticsListener {
        override fun onDroppedVideoFrames(eventTime: AnalyticsListener.EventTime, droppedFrames: Int, elapsedMs: Long) {
            this@Media3Engine.droppedFrames += droppedFrames
        }

        override fun onVideoDecoderInitialized(
            eventTime: AnalyticsListener.EventTime,
            decoderName: String,
            initializedTimestampMs: Long,
            initializationDurationMs: Long,
        ) {
            videoDecoder = decoderName
        }
    }

    companion object {
        /** Position ticks while playing; the contract wants at least every 500 ms. */
        const val TICK_MS = 200L
        const val UNITY_VOLUME = 100.0
        const val MAX_VOLUME = 300.0
        const val MIN_RATE = 0.1
        const val MAX_RATE = 4.0

        /** ExoPlayer with decoder fallback (a second decoder is tried when the first fails to start). */
        fun defaultPlayer(context: Context): ExoPlayer {
            val renderers = DefaultRenderersFactory(context).setEnableDecoderFallback(true)
            return ExoPlayer.Builder(context, renderers).setLooper(Looper.getMainLooper()).build()
        }

        private val TrackKind.type: Int
            get() = if (this == TrackKind.AUDIO) C.TRACK_TYPE_AUDIO else C.TRACK_TYPE_TEXT
    }
}
