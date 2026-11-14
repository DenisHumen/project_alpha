// SPDX-License-Identifier: MPL-2.0
// PLAY-APL-001: QA-002 MKV samples play, and the linked libmpv is LGPL.

import AlphaPlayerKit
import Foundation
import PACore
import XCTest

final class MPVMediaTests: XCTestCase {
    func testLinkedMPVIsLGPL() {
        let report = MPVBuildInfo.report()
        print("mpv: \(report.mpvVersion ?? "?"), FFmpeg: \(report.ffmpegVersion ?? "?"), API \(MPVBuildInfo.clientAPIVersion)")
        print("mpv-configuration: \(report.mpvConfiguration ?? "?")")
        XCTAssertNotNil(report.mpvVersion, "mpv starts")
        #if canImport(Darwin)
        // Apple builds link MPVKit: the LGPL product only, no opt-out.
        // (Only the Linux dev run, which uses the distro libmpv, is exempt.)
        XCTAssertTrue(report.isLGPL, "libmpv must be built with -Dgpl=false")
        #endif
    }

    /// Every MKV of QA-002 that was generated opens and plays.
    func testQAMediaMKVsPlay() throws {
        let dir = try TestMedia.directory("PA_QA_MEDIA")
        let files = (FileManager.default.enumerator(at: dir, includingPropertiesForKeys: nil)?.allObjects as? [URL] ?? [])
            .filter { $0.pathExtension == "mkv" }
            .sorted { $0.path < $1.path }
        XCTAssertFalse(files.isEmpty, "no MKV files in \(dir.path)")
        for file in files {
            let name = file.path.replacingOccurrences(of: dir.path + "/", with: "")
            // The app's decoder setting (VideoToolbox, software fallback), so
            // the samples play as they would in the app.
            var configuration = MPVEngineConfiguration.headless
            configuration.hardwareDecoding = true
            let engine = try MPVEngine(configuration: configuration)
            defer { try? engine.close() }
            try engine.openMedia(request: OpenRequest(
                items: [MediaSource(uri: file.absoluteString, title: nil)], index: 0, startPositionMs: 0, play: true
            ))
            engine.settle(timeout: 20)
            var state = engine.state()
            XCTAssertNil(state.error, "\(name): \(String(describing: state.error))")
            XCTAssertTrue(state.loaded, "\(name) loads")
            XCTAssertGreaterThan(state.durationMs, 0, name)
            XCTAssertFalse(state.audioTracks.isEmpty && state.videoWidth == nil, "\(name) has tracks")
            // Same bar as before (1 s of media, or half of a short file), but
            // measured within 10 s of wall time: a software-decoded 1080p HEVC
            // Main10 sample on a CI simulator VM can run below real time.
            let expected = min(UInt64(1_000), state.durationMs / 2)
            let started = Date()
            repeat {
                Thread.sleep(forTimeInterval: 0.5)
                engine.settle()
                state = engine.state()
            } while state.positionMs < expected && !state.completed && state.error == nil
                && Date().timeIntervalSince(started) < 10
            let wall = Date().timeIntervalSince(started)
            XCTAssertNil(state.error, "\(name) plays without error: \(String(describing: state.error))")
            XCTAssertTrue(state.positionMs >= expected || state.completed,
                          "\(name) plays: \(state.positionMs) ms after \(String(format: "%.1f", wall)) s")
            let stats = try engine.stats()
            print("QA-002 \(name): reached \(state.positionMs) ms in \(String(format: "%.1f", wall)) s wall time, "
                + "hwdec \(stats.hwDecoding == true ? "yes" : "no")")
            print("QA-002 \(name): \(state.durationMs) ms, \(state.videoWidth ?? 0)x\(state.videoHeight ?? 0), "
                + "video \(stats.videoCodec ?? "-"), audio \(stats.audioCodec ?? "-"), "
                + "\(state.audioTracks.count) audio, \(state.subtitleTracks.count) subtitles, at \(state.positionMs) ms")
        }
    }
}
