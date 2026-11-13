// SPDX-License-Identifier: MPL-2.0
// PLAY-APL-001: the core `PlayerEngine` contract (ADR-0017) on libmpv from
// MPVKit (LGPL build only).

import Dispatch
import Foundation
import Libmpv
import PACore

/// The MPVKit engine (`EngineKind.mpv`): the Apple fallback of the Auto chain
/// AVFoundation → own engine → MPVKit (ADR-0003, ADR-0017).
///
/// How it maps the contract onto mpv:
///
/// - All state lives on one serial queue. Commands run on it synchronously
///   and only start work in mpv; results arrive as `EngineEvent`s.
/// - Every event goes through `EngineStateFold` (the reducer of the core)
///   and then to the listener, both on the engine queue, so `state()` is
///   always the fold of the events. The listener may call back into the
///   engine: calls from the engine queue run inline.
/// - mpv holds one file at a time with `keep-open=yes`; the playlist,
///   repeat and the end of the playlist are handled here, so the rules of
///   the contract (no `Ended` mid-playlist, `play()` after the end starts
///   over) do not depend on mpv playlist behaviour.
/// - `playing` follows the commands, not mpv's `pause` property.
public final class MPVEngine: PlayerEngine, @unchecked Sendable {
    /// One `open`/`jump`/auto-advance in progress.
    private struct Load {
        var generation: Int
        var index: UInt32
        var play: Bool
        /// mpv playlist entry of this load, from `START_FILE`.
        var entryID: Int64?
        var fileLoaded = false
        /// Seek requested while loading.
        var pendingSeekMs: UInt64?
    }

    private static let maxVolume = 300.0
    private static let maxRate = 100.0
    /// A loaded file normally starts within a frame; this only guards
    /// against a missing `PLAYBACK_RESTART`.
    private static let finishLoadAfter: TimeInterval = 3

    private let configuration: MPVEngineConfiguration
    private let queue = DispatchQueue(label: "dev.projectalpha.player.mpv", qos: .userInitiated)
    private let onQueue = DispatchSpecificKey<Bool>()
    private let fold = EngineStateFold()
    private var mpv: MPVHandle?
    private var wakeup: WakeupTarget?
    private var listener: EngineListener?
    private var items: [MediaSource] = []
    private var load: Load?
    private var loadGeneration = 0
    private var currentEntryID: Int64?
    private var seeking = false
    private var closed = false
    private var timer: DispatchSourceTimer?

    /// Creates and starts an mpv core. Throws `EngineError.Internal` if mpv
    /// cannot start with the configuration.
    public init(configuration: MPVEngineConfiguration = MPVEngineConfiguration()) throws {
        self.configuration = configuration
        queue.setSpecific(key: onQueue, value: true)
        guard let mpv = MPVHandle() else {
            throw EngineError.Internal(details: "mpv_create failed")
        }
        do {
            try Self.configure(mpv, configuration)
        } catch {
            mpv.destroy()
            throw error
        }
        mpv.observe("eof-reached", MPV_FORMAT_FLAG)
        mpv.observe("paused-for-cache", MPV_FORMAT_FLAG)
        self.mpv = mpv
        let wakeup = WakeupTarget { [weak self] in
            self?.queue.async { self?.drainEvents() }
        }
        self.wakeup = wakeup
        mpv.setWakeup(wakeup)
    }

    deinit {
        timer?.cancel()
        mpv?.destroy()
    }

    private static func configure(_ mpv: MPVHandle, _ config: MPVEngineConfiguration) throws {
        func required(_ name: String, _ value: String) throws {
            let code = mpv.setOption(name, value)
            if code < 0 {
                throw EngineError.Internal(details: "mpv option \(name)=\(value): \(MPVHandle.describe(code))")
            }
        }
        // Optional options may be missing from some builds.
        func optional(_ name: String, _ value: String) {
            mpv.setOption(name, value)
        }
        try required("idle", "yes")
        try required("keep-open", "yes")
        try required("pause", "yes")
        try required("volume-max", String(Int(maxVolume)))
        optional("config", "no")
        optional("terminal", "no")
        optional("input-default-bindings", "no")
        optional("input-vo-keyboard", "no")
        optional("load-scripts", "no")
        optional("ytdl", "no")
        // External subtitles and audio are chosen by the core (LIB-007), not
        // by mpv looking next to the file.
        optional("sub-auto", "no")
        optional("audio-file-auto", "no")
        try required("hwdec", config.hardwareDecoding ? "auto-safe" : "no")
        switch config.videoOutput {
        case .none:
            try required("vo", "null")
        #if canImport(QuartzCore)
        case let .metal(layer):
            let pointer = Int64(Int(bitPattern: Unmanaged.passUnretained(layer).toOpaque()))
            let code = mpv.setOption("wid", int64: pointer)
            if code < 0 {
                throw EngineError.Internal(details: "mpv option wid: \(MPVHandle.describe(code))")
            }
            try required("vo", "gpu-next")
            try required("gpu-api", "vulkan")
            try required("gpu-context", "moltenvk")
        #endif
        }
        if let audioOutput = config.audioOutput {
            try required("ao", audioOutput)
        }
        for option in config.extraOptions {
            try required(option.name, option.value)
        }
        if config.log != nil {
            mpv.requestLogMessages(config.logLevel)
        }
        let code = mpv.initialize()
        if code < 0 {
            throw EngineError.Internal(details: "mpv_initialize: \(MPVHandle.describe(code))")
        }
    }

    // MARK: - Queue and events

    /// Runs `body` on the engine queue; inline if already there (listener
    /// callbacks may call back into the engine).
    private func sync<T>(_ body: () throws -> T) rethrows -> T {
        if DispatchQueue.getSpecific(key: onQueue) == true {
            return try body()
        }
        return try queue.sync(execute: body)
    }

    /// Applies `event` to the state and sends it. Engine queue only.
    private func emit(_ event: EngineEvent) {
        guard !closed else { return }
        _ = fold.apply(event: event)
        listener?.onEvent(event: event)
    }

    private var current: EngineState { fold.state() }

    private func checkAlive() throws {
        if closed { throw EngineError.Closed }
    }

    /// The current item is loaded (not loading, not failed).
    private func checkLoaded() throws {
        try checkAlive()
        guard load == nil, current.loaded else {
            throw EngineError.InvalidState(details: load == nil ? "no item is open" : "the item is still loading")
        }
    }

    private func drainEvents() {
        while !closed, let mpv, let event = mpv.nextEvent() {
            handle(event)
        }
    }

    private func handle(_ event: mpv_event) {
        switch event.event_id {
        case MPV_EVENT_START_FILE:
            currentEntryID = event.startFileEntryID
            if load != nil, load?.entryID == nil {
                load?.entryID = currentEntryID
            }
        case MPV_EVENT_FILE_LOADED:
            guard let pending = load, pending.entryID != nil else { return }
            load?.fileLoaded = true
            let generation = pending.generation
            queue.asyncAfter(deadline: .now() + Self.finishLoadAfter) { [weak self] in
                guard let self, let load = self.load, load.generation == generation, load.fileLoaded else { return }
                self.finishLoad()
            }
        case MPV_EVENT_PLAYBACK_RESTART:
            if load?.fileLoaded == true {
                finishLoad()
            } else if seeking {
                seeking = false
                sendPosition()
            }
        case MPV_EVENT_END_FILE:
            guard let endFile = event.endFile, endFile.reason == MPV_END_FILE_REASON_ERROR else { return }
            failed(entryID: endFile.playlist_entry_id, code: endFile.error)
        case MPV_EVENT_PROPERTY_CHANGE:
            guard let property = event.propertyFlag, let value = property.value else { return }
            switch property.name {
            case "eof-reached" where value:
                reachedEnd()
            case "paused-for-cache":
                let state = current
                if load == nil, state.loaded, state.buffering != value {
                    emit(.bufferingChanged(buffering: value))
                }
            default:
                break
            }
        case MPV_EVENT_LOG_MESSAGE:
            if let line = event.logLine {
                configuration.log?(line.level, line.prefix, line.text)
            }
        default:
            break
        }
    }

    // MARK: - Loading

    private func startLoad(index: UInt32, startMs: UInt64, play: Bool) {
        guard let mpv else { return }
        stopTimer()
        seeking = false
        loadGeneration += 1
        load = Load(generation: loadGeneration, index: index, play: play)
        emit(.loading(index: index))
        mpv.set("pause", "yes")
        // Positions beyond the end are clamped by mpv: it stops at the end
        // (keep-open) and `Position` is clamped to the duration.
        mpv.set("start", String(format: "%.3f", Double(startMs) / 1000))
        let uri = items[Int(index)].uri
        let code = mpv.command(["loadfile", Self.mpvPath(uri), "replace"])
        if code < 0 {
            load = nil
            emit(.error(error: .Source(details: "cannot open \(uri): \(MPVHandle.describe(code))")))
        }
    }

    /// The file of the current load is open and its first frame is ready.
    private func finishLoad() {
        guard let pending = load, let mpv else { return }
        load = nil
        let durationMs = Self.milliseconds(mpv.double("duration") ?? 0)
        let tracks = readTracks()
        emit(.loaded(durationMs: durationMs, videoWidth: tracks.videoWidth, videoHeight: tracks.videoHeight))
        emit(.tracksChanged(audio: tracks.audio.map(\.track), subtitles: tracks.subtitles.map(\.track)))
        emit(.trackSelected(kind: .audio, id: tracks.audio.first(where: \.selected)?.track.id))
        emit(.trackSelected(kind: .subtitle, id: tracks.subtitles.first(where: \.selected)?.track.id))
        sendPosition()
        if let seekMs = pending.pendingSeekMs {
            startSeek(seekMs)
        }
        if pending.play {
            mpv.set("pause", "no")
            emit(.playingChanged(playing: true))
            startTimer()
        }
    }

    private func failed(entryID: Int64, code: Int32) {
        let reason = MPVHandle.describe(code)
        if let pending = load {
            guard pending.entryID == entryID else { return }
            load = nil
            let uri = items[Int(pending.index)].uri
            emit(.error(error: .Source(details: "cannot open \(uri): \(reason)")))
        } else if entryID == currentEntryID, current.loaded {
            stopTimer()
            emit(.error(error: .Decoder(details: reason)))
        }
    }

    // MARK: - Playback

    /// mpv reached the end of the file (keep-open: it paused on the last frame).
    private func reachedEnd() {
        seeking = false
        let state = current
        guard load == nil, state.loaded, state.playing else { return }
        itemEnded()
    }

    private func itemEnded() {
        guard let mpv else { return }
        let state = current
        let isLast = Int(state.playlistIndex) + 1 >= items.count
        switch (state.repeat, isLast) {
        case (.one, _):
            startSeek(0)
            mpv.set("pause", "no")
        case (.off, true):
            stopTimer()
            mpv.set("pause", "yes")
            emit(.position(positionMs: state.durationMs, bufferedMs: state.durationMs))
            emit(.ended)
        case (.all, true):
            startLoad(index: 0, startMs: 0, play: true)
        case (_, false):
            startLoad(index: state.playlistIndex + 1, startMs: 0, play: true)
        }
    }

    private func startSeek(_ positionMs: UInt64) {
        guard let mpv else { return }
        let state = current
        let target = min(positionMs, state.durationMs)
        let code = mpv.command(["seek", String(format: "%.3f", Double(target) / 1000), "absolute+exact"])
        guard code >= 0 else { return }
        seeking = true
        emit(.position(positionMs: target, bufferedMs: min(max(target, state.bufferedMs), state.durationMs)))
    }

    /// Sends the position mpv reports now. Only for a loaded item.
    private func sendPosition() {
        guard let mpv else { return }
        let state = current
        guard state.loaded else { return }
        let position = min(Self.milliseconds(mpv.double("time-pos") ?? 0), state.durationMs)
        let cached = mpv.double("demuxer-cache-time").map(Self.milliseconds) ?? position
        emit(.position(positionMs: position, bufferedMs: min(max(cached, position), state.durationMs)))
    }

    private func startTimer() {
        guard timer == nil else { return }
        let timer = DispatchSource.makeTimerSource(queue: queue)
        let interval = configuration.positionInterval
        timer.schedule(deadline: .now() + interval, repeating: interval, leeway: .milliseconds(20))
        timer.setEventHandler { [weak self] in
            guard let self, self.load == nil, !self.seeking, self.current.playing else { return }
            self.sendPosition()
        }
        timer.resume()
        self.timer = timer
    }

    private func stopTimer() {
        timer?.cancel()
        timer = nil
    }

    // MARK: - Tracks

    private struct Listed {
        var track: Track
        var selected: Bool
    }

    private func readTracks() -> (audio: [Listed], subtitles: [Listed], videoWidth: UInt32?, videoHeight: UInt32?) {
        let snapshot = trackSnapshot()
        return (snapshot.audio, snapshot.subtitles, snapshot.videoWidth, snapshot.videoHeight)
    }

    private func trackSnapshot() -> (audio: [Listed], subtitles: [Listed], videoWidth: UInt32?, videoHeight: UInt32?,
                                     videoCodec: String?, audioCodec: String?) {
        var audio: [Listed] = []
        var subtitles: [Listed] = []
        var width: UInt32?
        var height: UInt32?
        var videoCodec: String?
        var audioCodec: String?
        guard let mpv else { return ([], [], nil, nil, nil, nil) }
        let count = mpv.int("track-list/count") ?? 0
        for i in 0 ..< max(count, 0) {
            let p = "track-list/\(i)/"
            let type = mpv.string(p + "type")
            let selected = mpv.flag(p + "selected") ?? false
            let codec = mpv.string(p + "codec")
            switch type {
            case "video":
                if mpv.flag(p + "image") == true { continue }
                if width == nil || selected {
                    width = mpv.int(p + "demux-w").flatMap { UInt32(exactly: $0) }
                    height = mpv.int(p + "demux-h").flatMap { UInt32(exactly: $0) }
                }
                if selected { videoCodec = codec }
            case "audio", "sub":
                let track = Track(
                    id: mpv.string(p + "id") ?? String(i),
                    title: mpv.string(p + "title"),
                    language: mpv.string(p + "lang"),
                    codec: codec,
                    channels: type == "audio" ? mpv.int(p + "demux-channel-count").flatMap { UInt32(exactly: $0) } : nil,
                    external: mpv.flag(p + "external") ?? false,
                    default: mpv.flag(p + "default") ?? false,
                    forced: mpv.flag(p + "forced") ?? false
                )
                if type == "audio" {
                    audio.append(Listed(track: track, selected: selected))
                    if selected { audioCodec = codec }
                } else {
                    subtitles.append(Listed(track: track, selected: selected))
                }
            default:
                break
            }
        }
        return (audio, subtitles, width, height, videoCodec, audioCodec)
    }

    private func select(_ kind: TrackKind, _ id: String?) throws {
        try checkLoaded()
        guard let mpv else { throw EngineError.Closed }
        let state = current
        let tracks = kind == .audio ? state.audioTracks : state.subtitleTracks
        if let id, !tracks.contains(where: { $0.id == id }) {
            throw EngineError.InvalidArgument(details: "unknown \(kind) track \(id)")
        }
        let property = kind == .audio ? "aid" : "sid"
        let code = mpv.set(property, id ?? "no")
        if code < 0 {
            throw EngineError.Internal(details: "\(property)=\(id ?? "no"): \(MPVHandle.describe(code))")
        }
        emit(.trackSelected(kind: kind, id: id))
    }

    // MARK: - Helpers

    private static func milliseconds(_ seconds: Double) -> UInt64 {
        guard seconds.isFinite, seconds > 0 else { return 0 }
        return UInt64((seconds * 1000).rounded())
    }

    /// mpv takes plain paths and URLs; `file://` URLs become paths.
    static func mpvPath(_ uri: String) -> String {
        if uri.hasPrefix("file://"), let url = URL(string: uri), url.isFileURL {
            return url.path
        }
        return uri
    }

    // MARK: - PlayerEngine

    public func kind() -> EngineKind {
        .mpv
    }

    public func setListener(listener: EngineListener?) {
        sync { self.listener = listener }
    }

    public func state() -> EngineState {
        sync { fold.state() }
    }

    public func open(request: OpenRequest) throws {
        try sync {
            try checkAlive()
            guard Int(request.index) < request.items.count else {
                throw EngineError.InvalidArgument(details: "index outside the playlist")
            }
            items = request.items
            startLoad(index: request.index, startMs: request.startPositionMs, play: request.play)
        }
    }

    public func play() throws {
        try sync {
            try checkAlive()
            if load != nil {
                load?.play = true
                return
            }
            try checkLoaded()
            guard let mpv else { return }
            let state = current
            let atEnd = mpv.flag("eof-reached") == true
            if state.completed {
                startSeek(0)
            }
            mpv.set("pause", "no")
            if !state.playing {
                emit(.playingChanged(playing: true))
            }
            startTimer()
            if atEnd, !state.completed {
                itemEnded()
            }
        }
    }

    public func pause() throws {
        try sync {
            try checkAlive()
            if load != nil {
                load?.play = false
                return
            }
            try checkLoaded()
            guard let mpv else { return }
            mpv.set("pause", "yes")
            stopTimer()
            if !seeking {
                sendPosition()
            }
            if current.playing {
                emit(.playingChanged(playing: false))
            }
        }
    }

    public func seek(positionMs: UInt64) throws {
        try sync {
            try checkAlive()
            if load != nil {
                load?.pendingSeekMs = positionMs
                return
            }
            try checkLoaded()
            startSeek(positionMs)
        }
    }

    public func setRate(rate: Double) throws {
        try sync {
            try checkAlive()
            guard rate.isFinite, rate > 0 else {
                throw EngineError.InvalidArgument(details: "rate \(rate)")
            }
            guard rate <= Self.maxRate else {
                throw EngineError.Unsupported(details: "rate \(rate) above \(Self.maxRate)")
            }
            guard let mpv else { return }
            let code = mpv.set("speed", double: rate)
            if code < 0 {
                throw EngineError.Unsupported(details: "rate \(rate): \(MPVHandle.describe(code))")
            }
            // Position at the old rate, so time measured from here is exact.
            if load == nil, current.loaded, !seeking {
                sendPosition()
            }
            emit(.rateChanged(rate: rate))
        }
    }

    public func setVolume(volume: Double) throws {
        try sync {
            try checkAlive()
            guard volume.isFinite, volume >= 0 else {
                throw EngineError.InvalidArgument(details: "volume \(volume)")
            }
            let applied = min(volume, Self.maxVolume)
            guard let mpv else { return }
            let code = mpv.set("volume", double: applied)
            if code < 0 {
                throw EngineError.Internal(details: "volume \(applied): \(MPVHandle.describe(code))")
            }
            emit(.volumeChanged(volume: applied))
        }
    }

    public func setRepeat(repeat: RepeatMode) throws {
        try sync {
            try checkAlive()
            emit(.repeatChanged(repeat: `repeat`))
        }
    }

    public func jump(index: UInt32) throws {
        try sync {
            try checkAlive()
            guard !items.isEmpty else {
                throw EngineError.InvalidState(details: "no playlist")
            }
            guard Int(index) < items.count else {
                throw EngineError.InvalidArgument(details: "index outside the playlist")
            }
            let play = load?.play ?? current.playing
            startLoad(index: index, startMs: 0, play: play)
        }
    }

    public func setAudioTrack(id: String?) throws {
        try sync { try select(.audio, id) }
    }

    public func setSubtitleTrack(id: String?) throws {
        try sync { try select(.subtitle, id) }
    }

    public func addExternalSubtitle(uri: String, title: String?) throws {
        try sync {
            try checkLoaded()
            guard !uri.isEmpty else {
                throw EngineError.InvalidArgument(details: "empty subtitle uri")
            }
            guard let mpv else { throw EngineError.Closed }
            var args = ["sub-add", Self.mpvPath(uri), "select"]
            if let title { args.append(title) }
            let code = mpv.command(args)
            if code < 0 {
                throw EngineError.Source(details: "cannot load \(uri): \(MPVHandle.describe(code))")
            }
            let tracks = readTracks()
            emit(.tracksChanged(audio: tracks.audio.map(\.track), subtitles: tracks.subtitles.map(\.track)))
            emit(.trackSelected(kind: .subtitle, id: tracks.subtitles.first(where: \.selected)?.track.id))
        }
    }

    public func stats() throws -> PlaybackStats {
        try sync {
            try checkLoaded()
            guard let mpv else { throw EngineError.Closed }
            let tracks = trackSnapshot()
            let hwdec = mpv.string("hwdec-current")
            let dropped = (mpv.int("frame-drop-count") ?? 0) + (mpv.int("decoder-frame-drop-count") ?? 0)
            let bitrates = [mpv.double("video-bitrate"), mpv.double("audio-bitrate")].compactMap { $0 }
            return PlaybackStats(
                videoCodec: tracks.videoCodec,
                audioCodec: tracks.audioCodec,
                hwDecoding: hwdec.map { !$0.isEmpty && $0 != "no" } ?? false,
                audioPassthrough: false,
                droppedFrames: UInt64(max(dropped, 0)),
                bitrateBps: bitrates.isEmpty ? nil : UInt64(max(bitrates.reduce(0, +), 0)),
                bufferedMs: current.bufferedMs
            )
        }
    }

    public func close() throws {
        sync {
            guard !closed else { return }
            stopTimer()
            load = nil
            emit(.closed)
            closed = true
            listener = nil
            mpv?.destroy()
            mpv = nil
        }
    }

    // MARK: - Test and integration hooks

    /// Waits until pending opens and seeks have finished (or `timeout`
    /// passed), then sends a fresh `Position` if playing. For contract
    /// harnesses and UI tests; never call it from a listener.
    @discardableResult
    public func settle(timeout: TimeInterval = 10) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        var settled = false
        while true {
            settled = queue.sync { () -> Bool in
                drainEvents()
                return closed || (load == nil && !seeking)
            }
            if settled || Date() >= deadline { break }
            Thread.sleep(forTimeInterval: 0.01)
        }
        queue.sync {
            drainEvents()
            let state = current
            if !closed, load == nil, !seeking, state.loaded, state.playing {
                sendPosition()
            }
        }
        return settled
    }

    /// A string property of the mpv core (`mpv-version`, `ffmpeg-version`,
    /// `mpv-configuration`, …); `nil` after `close`.
    public func mpvProperty(_ name: String) -> String? {
        sync { mpv?.string(name) }
    }
}
