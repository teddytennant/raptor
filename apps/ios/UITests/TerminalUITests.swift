import XCTest

/// Drives the real app against a live Rex server. Needs a server saved in the app
/// and a session named by SHREW_TEST_SESSION (default "shrew-demo").
final class TerminalUITests: XCTestCase {
    var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchEnvironment = [
            "SHREW_NO_PROMPTS": "1",
            "SHREW_OPEN": ProcessInfo.processInfo.environment["SHREW_TEST_SESSION"] ?? "shrew-demo",
        ]
        app.launch()
    }

    private func shot(_ name: String) {
        let a = XCTAttachment(screenshot: app.screenshot())
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    func testTypingKeysAndGestures() throws {
        let terminal = app.textViews["Terminal"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 15), "terminal never appeared")
        sleep(3)
        shot("opened")

        // Soft keyboard typing goes through UIKeyInput.insertText.
        app.typeText("printf 'ui-%s\\n' 42\n")
        sleep(2)
        shot("typed")

        // Key bar: ctrl (sticky) then c interrupts a running command.
        app.typeText("sleep 30\n")
        sleep(1)
        app.buttons["ctrl"].tap()
        app.typeText("c")
        sleep(1)
        app.typeText("echo after-ctrl-c\n")
        sleep(2)

        // Arrow up recalls history; enter runs it again.
        app.buttons["↑"].tap()
        app.typeText("\n")
        sleep(2)
        shot("keys")

        // Second page of the key bar.
        app.buttons["•••"].tap()
        XCTAssertTrue(app.buttons["home"].exists)
        app.buttons["abc"].tap()

        // Scroll back through history, then pinch to resize text.
        app.typeText("seq 1 120\n")
        sleep(2)
        terminal.swipeDown(velocity: .fast)
        sleep(1)
        shot("scrolled")
        terminal.pinch(withScale: 1.4, velocity: 1)
        sleep(1)
        shot("pinched")

        // Long-press selects a word and shows the edit menu.
        terminal.press(forDuration: 0.8)
        sleep(1)
        shot("selection")
        let copy = app.descendants(matching: .any).matching(identifier: "Copy").firstMatch
        XCTAssertTrue(copy.waitForExistence(timeout: 3), "edit menu has no Copy")
        copy.tap()
        sleep(1)
        // Paste it back through the app's own menu (the runner can't read the pasteboard).
        app.typeText("echo pasted:")
        app.buttons["More"].tap()
        let paste = app.descendants(matching: .any).matching(identifier: "Paste").firstMatch
        XCTAssertTrue(paste.waitForExistence(timeout: 3), "no Paste in the menu")
        paste.tap()
        app.typeText("\n")
        sleep(2)
        shot("pasted")
    }

    func testSwipeBetweenPanesAndCommandPalette() throws {
        let terminal = app.textViews["Terminal"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        sleep(3)
        app.typeText("echo pane-one\n")
        sleep(1)

        // Swipe left: the next pane (the split's second block) takes the keyboard.
        terminal.swipeLeft()
        sleep(2)
        shot("swiped")
        app.typeText("echo pane-two\n")
        sleep(2)

        // Command palette from the ••• menu: search, then run a server action.
        app.buttons["More"].tap()
        let open = app.descendants(matching: .any).matching(identifier: "Command Palette").firstMatch
        XCTAssertTrue(open.waitForExistence(timeout: 3))
        open.tap()
        let search = app.searchFields.firstMatch
        XCTAssertTrue(search.waitForExistence(timeout: 3))
        search.tap()
        search.typeText("clear")
        let clear = app.buttons.matching(NSPredicate(format: "label CONTAINS 'Clear Terminal'")).firstMatch
        XCTAssertTrue(clear.waitForExistence(timeout: 6), "server action missing from palette")
        shot("palette")
        clear.tap()
        sleep(2)
        shot("cleared")
    }

    func testSessionSwitcherAndSettings() throws {
        XCTAssertTrue(app.textViews["Terminal"].waitForExistence(timeout: 15))
        app.buttons.matching(identifier: "Sessions").firstMatch.tap()
        XCTAssertTrue(app.navigationBars["Sessions"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons.matching(NSPredicate(format: "label CONTAINS 'shrew-demo'")).firstMatch.exists)
        shot("switcher")
        app.buttons["Settings"].tap()
        XCTAssertTrue(app.navigationBars["Settings"].waitForExistence(timeout: 5))
        shot("settings")
        app.swipeUp()
        shot("settings-2")
        app.buttons["Done"].tap()
        XCTAssertTrue(app.textViews["Terminal"].waitForExistence(timeout: 5))
    }
}
