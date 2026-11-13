// SPDX-License-Identifier: MPL-2.0
// TV-001: the app launches in the tvOS simulator, shows Home with the core
// version, and the Siri Remote moves through the top tab bar.
import XCTest

final class TVAppUITests: XCTestCase {
    private let tabs = ["Home", "Movies", "TV Shows", "Twitch", "Music", "Search", "Settings"]

    override func setUp() {
        continueAfterFailure = false
    }

    func testHomeShowsCoreVersionAndTabBar() {
        let app = XCUIApplication()
        app.launch()

        XCTAssertTrue(app.staticTexts["screen-home"].waitForExistence(timeout: 20))
        let version = app.staticTexts["core-version"]
        XCTAssertTrue(version.waitForExistence(timeout: 5))
        XCTAssertTrue(version.label.hasPrefix("Core "), version.label)
        XCTAssertTrue(version.label.hasSuffix("· store"), version.label)

        for title in tabs {
            XCTAssertTrue(app.tabBars.buttons[title].exists, "tab \(title) is missing")
        }

        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "home"
        shot.lifetime = .keepAlways
        add(shot)
    }

    func testRemoteMovesFocusToTheNextTab() {
        let app = XCUIApplication()
        app.launch()
        XCTAssertTrue(app.staticTexts["screen-home"].waitForExistence(timeout: 20))

        let remote = XCUIRemote.shared
        // Up from the content reaches the tab bar; right moves to Movies,
        // and a tvOS tab bar switches the section on focus.
        remote.press(.up)
        remote.press(.up)
        remote.press(.right)

        XCTAssertTrue(app.staticTexts["screen-movies"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.tabBars.buttons["Movies"].hasFocus)
    }
}
