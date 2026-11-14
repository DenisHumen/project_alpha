// SPDX-License-Identifier: MPL-2.0

import Foundation
#if canImport(QuartzCore)
import QuartzCore
#endif

/// How an `MPVEngine` is set up. Fixed for the life of the engine: mpv
/// reads these options once, before the core starts.
public struct MPVEngineConfiguration {
    /// Where video goes.
    public enum VideoOutput {
        /// Decode, but show nothing (tests, audio-only use).
        case none
        #if canImport(QuartzCore)
        /// Render with `vo=gpu-next` over Vulkan (MoltenVK) into this Metal
        /// layer, usually `MPVVideoView.metalLayer`. The engine keeps a
        /// strong reference to the layer.
        case metal(CAMetalLayer)
        #endif
    }

    public var videoOutput: VideoOutput
    /// mpv `ao` value; `nil` keeps the platform default (AudioUnit on iOS
    /// and tvOS, CoreAudio on macOS). `"null"` plays without a sound
    /// device, in real time.
    public var audioOutput: String?
    /// VideoToolbox hardware decoding (`hwdec=auto-safe`).
    public var hardwareDecoding: Bool
    /// How often `Position` is sent while playing; the contract asks for at
    /// least every 500 ms.
    public var positionInterval: TimeInterval
    /// Extra mpv options set before start, after the engine's own.
    public var extraOptions: [(name: String, value: String)]
    /// mpv log level forwarded to `log` (`"no"`, `"error"`, `"warn"`,
    /// `"info"`, `"v"`, `"debug"`).
    public var logLevel: String
    /// Receives mpv log lines on the engine queue.
    public var log: ((_ level: String, _ prefix: String, _ text: String) -> Void)?

    public init(
        videoOutput: VideoOutput = .none,
        audioOutput: String? = nil,
        hardwareDecoding: Bool = true,
        positionInterval: TimeInterval = 0.25,
        extraOptions: [(name: String, value: String)] = [],
        logLevel: String = "warn",
        log: ((_ level: String, _ prefix: String, _ text: String) -> Void)? = nil
    ) {
        self.videoOutput = videoOutput
        self.audioOutput = audioOutput
        self.hardwareDecoding = hardwareDecoding
        self.positionInterval = positionInterval
        self.extraOptions = extraOptions
        self.logLevel = logLevel
        self.log = log
    }
}
