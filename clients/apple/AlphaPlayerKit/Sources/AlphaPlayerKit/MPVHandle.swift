// SPDX-License-Identifier: MPL-2.0
// Thin wrapper over the libmpv client API (MPVKit, LGPL build).

import Foundation
import Libmpv

/// One mpv core. The libmpv client API is thread-safe; this wrapper only
/// converts types. Calls after `destroy()` are not allowed.
final class MPVHandle {
    let raw: OpaquePointer

    init?() {
        guard let raw = mpv_create() else { return nil }
        self.raw = raw
    }

    static func describe(_ code: Int32) -> String {
        String(cString: mpv_error_string(code))
    }

    // MARK: Options (before `initialize`)

    @discardableResult
    func setOption(_ name: String, _ value: String) -> Int32 {
        mpv_set_option_string(raw, name, value)
    }

    @discardableResult
    func setOption(_ name: String, int64 value: Int64) -> Int32 {
        var value = value
        return mpv_set_option(raw, name, MPV_FORMAT_INT64, &value)
    }

    func initialize() -> Int32 {
        mpv_initialize(raw)
    }

    // MARK: Properties

    @discardableResult
    func set(_ name: String, _ value: String) -> Int32 {
        mpv_set_property_string(raw, name, value)
    }

    @discardableResult
    func set(_ name: String, double value: Double) -> Int32 {
        var value = value
        return mpv_set_property(raw, name, MPV_FORMAT_DOUBLE, &value)
    }

    func string(_ name: String) -> String? {
        guard let cString = mpv_get_property_string(raw, name) else { return nil }
        defer { mpv_free(cString) }
        return String(cString: cString)
    }

    func double(_ name: String) -> Double? {
        var value = 0.0
        return mpv_get_property(raw, name, MPV_FORMAT_DOUBLE, &value) >= 0 ? value : nil
    }

    func int(_ name: String) -> Int64? {
        var value: Int64 = 0
        return mpv_get_property(raw, name, MPV_FORMAT_INT64, &value) >= 0 ? value : nil
    }

    func flag(_ name: String) -> Bool? {
        var value: Int32 = 0
        return mpv_get_property(raw, name, MPV_FORMAT_FLAG, &value) >= 0 ? value != 0 : nil
    }

    func observe(_ name: String, _ format: mpv_format) {
        mpv_observe_property(raw, 0, name, format)
    }

    // MARK: Commands and events

    /// Runs a command synchronously; returns the mpv error code (`< 0` on failure).
    @discardableResult
    func command(_ args: [String]) -> Int32 {
        var cArgs: [UnsafePointer<CChar>?] = args.map { UnsafePointer(strdup($0)) }
        cArgs.append(nil)
        defer {
            for pointer in cArgs {
                free(UnsafeMutablePointer(mutating: pointer))
            }
        }
        return cArgs.withUnsafeMutableBufferPointer { mpv_command(raw, $0.baseAddress) }
    }

    /// Next queued event without waiting; `nil` when the queue is empty.
    func nextEvent() -> mpv_event? {
        guard let event = mpv_wait_event(raw, 0), event.pointee.event_id != MPV_EVENT_NONE else {
            return nil
        }
        return event.pointee
    }

    func requestLogMessages(_ level: String) {
        mpv_request_log_messages(raw, level)
    }

    /// `callback` runs on an mpv thread whenever new events are queued.
    func setWakeup(_ target: WakeupTarget?) {
        if let target {
            mpv_set_wakeup_callback(raw, { context in
                guard let context else { return }
                Unmanaged<WakeupTarget>.fromOpaque(context).takeUnretainedValue().fire()
            }, Unmanaged.passUnretained(target).toOpaque())
        } else {
            mpv_set_wakeup_callback(raw, nil, nil)
        }
    }

    /// Stops playback and frees the core. Blocks until mpv has shut down.
    func destroy() {
        setWakeup(nil)
        mpv_terminate_destroy(raw)
    }
}

/// Receiver of the mpv wakeup callback. Outlives the core it is attached to.
final class WakeupTarget: @unchecked Sendable {
    private let action: () -> Void

    init(_ action: @escaping () -> Void) {
        self.action = action
    }

    func fire() {
        action()
    }
}

extension mpv_event {
    /// Payload of `MPV_EVENT_PROPERTY_CHANGE`: name and, for flags, the value.
    var propertyFlag: (name: String, value: Bool?)? {
        guard event_id == MPV_EVENT_PROPERTY_CHANGE, let data else { return nil }
        let property = data.assumingMemoryBound(to: mpv_event_property.self).pointee
        let name = String(cString: property.name)
        guard property.format == MPV_FORMAT_FLAG, let value = property.data else { return (name, nil) }
        return (name, value.assumingMemoryBound(to: Int32.self).pointee != 0)
    }

    var startFileEntryID: Int64? {
        guard event_id == MPV_EVENT_START_FILE, let data else { return nil }
        return data.assumingMemoryBound(to: mpv_event_start_file.self).pointee.playlist_entry_id
    }

    var endFile: mpv_event_end_file? {
        guard event_id == MPV_EVENT_END_FILE, let data else { return nil }
        return data.assumingMemoryBound(to: mpv_event_end_file.self).pointee
    }

    var logLine: (level: String, prefix: String, text: String)? {
        guard event_id == MPV_EVENT_LOG_MESSAGE, let data else { return nil }
        let message = data.assumingMemoryBound(to: mpv_event_log_message.self).pointee
        return (String(cString: message.level), String(cString: message.prefix), String(cString: message.text))
    }
}
