// SPDX-License-Identifier: MPL-2.0
// PLAY-007: subtitle style → mpv options, the system caption values →
// style, and the engine applying style, delay and sidecars.

import AlphaPlayerKit
import Foundation
import PACore
import XCTest

final class SubtitleStyleTests: XCTestCase {
    private func options(_ style: SubtitleStyle) -> [String: String] {
        Dictionary(style.mpvOptions().map { ($0.name, $0.value) }, uniquingKeysWith: { _, last in last })
    }

    func testStandardStyleIsWhiteOutlinedText() {
        let o = options(.standard)
        XCTAssertEqual(o["sub-color"], "#FFFFFFFF")
        XCTAssertEqual(o["sub-border-style"], "outline-and-shadow")
        XCTAssertEqual(o["sub-border-color"], "#FF000000")
        XCTAssertEqual(o["sub-font-size"], "38.4") // 0.0533 × 720
        XCTAssertEqual(o["sub-margin-y"], "58") // 8 % of 720
        XCTAssertEqual(o["sub-shadow-offset"], "0")
    }

    func testSizePositionAndBoxes() {
        var style = SubtitleStyle(fontScale: 1.5, font: .monospace, textColor: 0xFFFF_FF00, bottomMargin: 0.2)
        var o = options(style)
        XCTAssertEqual(o["sub-font-size"], "57.6")
        XCTAssertEqual(o["sub-font"], "Menlo")
        XCTAssertEqual(o["sub-color"], "#FFFFFF00")
        XCTAssertEqual(o["sub-margin-y"], "144")

        style.backgroundColor = 0x9900_0000
        o = options(style)
        XCTAssertEqual(o["sub-border-style"], "background-box")
        XCTAssertEqual(o["sub-back-color"], "#99000000")

        // mpv has one kind of box: the window color is used without a line box.
        style.backgroundColor = 0
        style.windowColor = 0xFF00_00FF
        XCTAssertEqual(options(style)["sub-back-color"], "#FF0000FF")

        style.windowColor = 0
        style.edge = .dropShadow
        o = options(style)
        XCTAssertEqual(o["sub-border-size"], "0")
        XCTAssertNotEqual(o["sub-shadow-offset"], "0")
        style.edge = .none
        XCTAssertEqual(options(style)["sub-shadow-offset"], "0")
    }

    func testSystemCaptionValuesGiveTheStyle() {
        let values = SystemCaptionValues(
            enabled: true,
            relativeSize: 2,
            foreground: 0xFFFF_FF00,
            background: 0xFF00_0000,
            edge: .raised,
            fontName: "Avenir Next",
            languages: ["ru-RU"]
        )
        let style = values.style(bottomMargin: 0.12)
        XCTAssertEqual(style.fontScale, 2)
        XCTAssertEqual(style.textColor, 0xFFFF_FF00)
        XCTAssertEqual(style.backgroundColor, 0xFF00_0000)
        XCTAssertEqual(style.edge, .raised)
        XCTAssertEqual(style.bottomMargin, 0.12)
        XCTAssertEqual(options(style)["sub-font"], "Avenir Next")

        // Unset values fall back to the standard look.
        let plain = SystemCaptionValues(enabled: false).style()
        XCTAssertEqual(plain, .standard)
        XCTAssertEqual(SystemCaptionValues.argb(red: 1, green: 0.5, blue: 0, alpha: 0.6), 0x99FF_8000)
    }

    #if canImport(MediaAccessibility)
    func testSystemCaptionsAreReadable() {
        // Simulator defaults: a style is always there.
        let values = SystemCaptions.current()
        XCTAssertGreaterThan(values.relativeSize, 0)
        XCTAssertNotNil(values.foreground)
    }
    #endif

    func testEngineAppliesStyleAndDelay() throws {
        let engine = try MPVEngine(configuration: .headless)
        defer { try? engine.closeEngine() }
        try engine.setSubtitleStyle(SubtitleStyle(fontScale: 1.5, bottomMargin: 0.2))
        XCTAssertEqual(Double(engine.mpvProperty("sub-font-size") ?? "") ?? 0, 57.6, accuracy: 0.01)
        XCTAssertEqual(engine.mpvProperty("sub-margin-y"), "144")
        try engine.setSubtitleDelay(milliseconds: -1_250)
        XCTAssertEqual(Double(engine.mpvProperty("sub-delay") ?? "") ?? 0, -1.25, accuracy: 0.001)
    }

    func testSidecarIsAddedWithoutSelecting() throws {
        let fixture = try TestMedia.contractFixture()
        let engine = try MPVEngine(configuration: .headless)
        defer { try? engine.closeEngine() }
        try engine.openMedia(request: OpenRequest(items: [fixture.items[0]], index: 0, startPositionMs: 0, play: false))
        engine.settle()
        let before = engine.state().subtitleTracks.count
        try engine.addSidecarSubtitle(uri: fixture.externalSubtitle, title: "Movie.ru.srt", language: "rus")
        engine.settle()
        let state = engine.state()
        XCTAssertEqual(state.subtitleTracks.count, before + 1)
        let added = state.subtitleTracks.first { $0.title == "Movie.ru.srt" }
        XCTAssertEqual(added?.language, "rus")
        XCTAssertEqual(added?.external, true)
        XCTAssertNotEqual(state.subtitleTrack, added?.id, "a sidecar is not selected by itself")
    }
}
