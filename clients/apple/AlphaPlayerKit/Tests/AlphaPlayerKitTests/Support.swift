// SPDX-License-Identifier: MPL-2.0

import AlphaPlayerKit
import Foundation
import PACore
import XCTest

/// Test media come from the environment, so nothing binary is committed:
/// - `PA_PLAYER_FIXTURES`: directory from `scripts/make-fixtures.sh`
///   (`contract-1.mkv`, `contract-2.mkv`, `external.srt`).
/// - `PA_QA_MEDIA`: directory with MKV files from QA-002
///   (`scripts/make-qa-media.py`).
/// xcodebuild passes `TEST_RUNNER_<NAME>` to simulator tests as `<NAME>`.
enum TestMedia {
    static func directory(_ variable: String) throws -> URL {
        guard let path = ProcessInfo.processInfo.environment[variable], !path.isEmpty else {
            throw XCTSkip("\(variable) is not set; see scripts/test.sh")
        }
        return URL(fileURLWithPath: path, isDirectory: true)
    }

    /// The contract fixture: two items of 12 s and 10 s with two audio
    /// tracks (eng default, rus) and one subtitle track that is off.
    static func contractFixture() throws -> EngineFixture {
        let dir = try directory("PA_PLAYER_FIXTURES")
        let item = { (name: String) in MediaSource(uri: dir.appendingPathComponent(name).absoluteString, title: name) }
        return EngineFixture(
            items: [item("contract-1.mkv"), item("contract-2.mkv")],
            durationsMs: [12_000, 10_000],
            broken: MediaSource(uri: dir.appendingPathComponent("missing.mkv").absoluteString, title: nil),
            externalSubtitle: dir.appendingPathComponent("external.srt").absoluteString
        )
    }
}

extension MPVEngineConfiguration {
    /// No sound device and no picture: decoding and timing as in real
    /// playback, deterministic on CI simulators.
    static var headless: MPVEngineConfiguration {
        MPVEngineConfiguration(
            videoOutput: .none,
            audioOutput: "null",
            hardwareDecoding: false,
            logLevel: "warn",
            log: { level, prefix, text in
                print("[mpv \(level)] \(prefix): \(text)", terminator: text.hasSuffix("\n") ? "" : "\n")
            }
        )
    }
}

/// Contract harness over a real `MPVEngine`: time passes in real time and
/// `advance` waits for pending loads and seeks.
final class MPVHarness: PlayerEngineHarness, @unchecked Sendable {
    let mpv: MPVEngine
    private let fx: EngineFixture

    init(fixture: EngineFixture) throws {
        mpv = try MPVEngine(configuration: .headless)
        fx = fixture
    }

    func engine() -> PlayerEngine {
        mpv
    }

    func fixture() -> EngineFixture {
        fx
    }

    func advance(ms: UInt64) {
        if ms > 0 {
            Thread.sleep(forTimeInterval: Double(ms) / 1000)
        }
        if !mpv.settle(timeout: 15) {
            print("advance(\(ms)): engine did not settle in 15 s")
        }
    }
}
