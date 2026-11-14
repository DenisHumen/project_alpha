// SPDX-License-Identifier: MPL-2.0
// PLAY-APL-001: MPVEngine passes the PlayerEngine contract of the core
// (pa_playback::contract::CASES, run from Rust over UniFFI).

import AlphaPlayerKit
import PACore
import XCTest

final class MPVEngineContractTests: XCTestCase {
    /// Every case below, in the order of the core suite.
    private static let cases = [
        "open_loads_item", "open_at_start_position", "open_paused", "open_broken_source",
        "commands_need_open_item", "play_pause", "seek_clamps", "rate", "volume", "select_tracks",
        "external_subtitle", "end_of_playlist", "playlist_advance_and_jump", "repeat_one", "stats", "close",
    ]

    func testCoversEveryCoreCase() {
        XCTAssertEqual(playerEngineContractCases(), Self.cases, "add new contract cases as test methods")
    }

    private func run(_ name: String) throws {
        let harness = try MPVHarness(fixture: TestMedia.contractFixture())
        defer { try? harness.mpv.closeEngine() }
        if let failure = playerEngineContractRun(name: name, harness: harness) {
            XCTFail("\(name): \(failure)")
        }
    }

    func testOpenLoadsItem() throws { try run("open_loads_item") }
    func testOpenAtStartPosition() throws { try run("open_at_start_position") }
    func testOpenPaused() throws { try run("open_paused") }
    func testOpenBrokenSource() throws { try run("open_broken_source") }
    func testCommandsNeedOpenItem() throws { try run("commands_need_open_item") }
    func testPlayPause() throws { try run("play_pause") }
    func testSeekClamps() throws { try run("seek_clamps") }
    func testRate() throws { try run("rate") }
    func testVolume() throws { try run("volume") }
    func testSelectTracks() throws { try run("select_tracks") }
    func testExternalSubtitle() throws { try run("external_subtitle") }
    func testEndOfPlaylist() throws { try run("end_of_playlist") }
    func testPlaylistAdvanceAndJump() throws { try run("playlist_advance_and_jump") }
    func testRepeatOne() throws { try run("repeat_one") }
    func testStats() throws { try run("stats") }
    func testClose() throws { try run("close") }
}

/// The Rust mock behind the same FFI traits: checks the bindings and the
/// contract runner on the platform, independent of mpv.
final class MockEngineRoundTripTests: XCTestCase {
    func testMockPassesEveryCaseOverFFI() throws {
        for name in playerEngineContractCases() {
            XCTAssertNil(playerEngineContractRun(name: name, harness: mockEngineHarness()), name)
        }
    }

    func testStateFoldIsTheCoreReducer() {
        var state = engineStateApply(state: engineStateDefault(), event: .rateChanged(rate: 1.5))
        state = engineStateApply(state: state, event: .loading(index: 1))
        XCTAssertEqual(state.rate, 1.5)
        XCTAssertEqual(state.playlistIndex, 1)
        XCTAssertTrue(state.buffering)
    }
}
