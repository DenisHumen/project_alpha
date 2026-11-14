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
            let engine = try MPVEngine(configuration: .headless)
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
            Thread.sleep(forTimeInterval: 1.5)
            engine.settle()
            state = engine.state()
            let expected = min(UInt64(1_000), state.durationMs / 2)
            XCTAssertTrue(state.positionMs >= expected || state.completed, "\(name) plays: \(state.positionMs) ms")
            let stats = try engine.stats()
            print("QA-002 \(name): \(state.durationMs) ms, \(state.videoWidth ?? 0)x\(state.videoHeight ?? 0), "
                + "video \(stats.videoCodec ?? "-"), audio \(stats.audioCodec ?? "-"), "
                + "\(state.audioTracks.count) audio, \(state.subtitleTracks.count) subtitles, at \(state.positionMs) ms")
        }
    }
}
