import XCTest

/// Walks the app against a seeded test server and saves screenshots for review.
/// RAPTOR_TEST_ADDRESS names the server (default 127.0.0.1:7993) and
/// RAPTOR_SHOTS the directory the PNGs go to.
final class TourUITests: XCTestCase {
    var app: XCUIApplication!
    let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = true
        app = XCUIApplication()
        app.launchEnvironment = ["RAPTOR_NO_PROMPTS": "1"]
    }

    private func shot(_ name: String) {
        let screen = XCUIScreen.main.screenshot()
        let png = screen.pngRepresentation
        if let dir = env["RAPTOR_SHOTS"] {
            try? png.write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(name).png"))
        }
        let a = XCTAttachment(screenshot: screen)
        a.name = name
        a.lifetime = .keepAlways
        add(a)
    }

    private func open(_ session: String) {
        app.launchEnvironment["RAPTOR_OPEN"] = session
        app.launch()
    }

    func testAddServer() throws {
        app.launch()
        let add = app.buttons["Add a server"]
        XCTAssertTrue(add.waitForExistence(timeout: 10))
        shot("welcome")
        add.tap()
        let field = app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.typeText("127.0.0.1:1")
        sleep(3)
        shot("add-bad")
        field.clearAndType(env["RAPTOR_TEST_ADDRESS"] ?? "127.0.0.1:7993")
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label BEGINSWITH 'Connected'")).firstMatch.waitForExistence(timeout: 10))
        shot("add-ok")
        app.buttons["Save"].tap()
        sleep(3)
        shot("after-add")
    }

    func testHome() throws {
        app.launch()
        sleep(3)
        shot("home-start")
        let sessions = app.buttons.matching(identifier: "Sessions").firstMatch
        if sessions.exists { sessions.tap(); sleep(2); shot("switcher") }
        if app.buttons["Show Sessions"].exists { app.buttons["Show Sessions"].tap(); sleep(2); shot("switcher") }
        app.swipeUp()
        sleep(1)
        shot("switcher-large")
    }

    func testTerminalShowcase() throws {
        open(env["RAPTOR_TEST_SESSION"] ?? "raptor-demo")
        let terminal = app.textViews["Terminal"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        sleep(3)
        shot("term-open")
        terminal.tap()
        sleep(2)
        shot("term-keyboard")
        app.typeText("echo héllo — “quotes” 日本\n")
        sleep(2)
        shot("term-typed")
        terminal.swipeDown(velocity: .fast)
        sleep(2)
        shot("term-scrolled")
    }

    /// Run with a Japanese Romaji keyboard as the simulator's only keyboard.
    func testJapaneseInput() throws {
        open(env["RAPTOR_TEST_SESSION"] ?? "raptor-demo")
        let terminal = app.textViews["Terminal"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        terminal.tap()
        sleep(2)
        // typeText skips the input method, so tap the keys.
        for ch in "nihongo" { app.keys[String(ch)].tap() }
        sleep(2)
        shot("ime-composing")
        app.keyboards.buttons.allElementsBoundByIndex.last?.tap()
        sleep(2)
        shot("ime-committed")
        app.typeText("\n")
        sleep(2)
        shot("ime-ran")
    }

    /// Quotes and dashes typed on the keyboard must reach the shell as ASCII.
    func testNoSmartPunctuation() throws {
        open(env["RAPTOR_TEST_SESSION"] ?? "raptor-demo")
        let terminal = app.textViews["Terminal"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        terminal.tap()
        sleep(2)
        app.keys["more"].tap()
        for k in ["\"", "a", "\"", "-", "-", "'", "b", "'"] {
            let key = app.keys[k]
            if key.exists { key.tap() } else { app.keys["more"].tap(); app.keys[k].tap(); app.keys["more"].tap() }
        }
        sleep(1)
        shot("punctuation")
        app.typeText("\n")
        sleep(1)
    }

    /// Needs an agent in session "api" that goes blocked during the test (the
    /// driver answers it from outside). Allows notifications on first run.
    func testNotificationDeepLink() throws {
        app.launchEnvironment = ["RAPTOR_OPEN": "raptor-demo"]
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        app.launch()
        let allow = springboard.buttons["Allow"]
        if allow.waitForExistence(timeout: 8) { allow.tap() }
        XCTAssertTrue(app.textViews["Terminal"].waitForExistence(timeout: 15))
        let banner = springboard.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS 'needs you'")).firstMatch
        XCTAssertTrue(banner.waitForExistence(timeout: 90), "no notification")
        shot("notify-banner")
        banner.tap()
        sleep(3)
        shot("notify-opened")
        XCTAssertTrue(app.staticTexts["api"].exists, "didn't open the agent's session")
    }

    /// Same setup as testNotificationDeepLink: answer from the notification itself.
    func testNotificationReply() throws {
        app.launchEnvironment = ["RAPTOR_OPEN": "raptor-demo"]
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        app.launch()
        XCTAssertTrue(app.textViews["Terminal"].waitForExistence(timeout: 15))
        let banner = springboard.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS 'needs you'")).firstMatch
        XCTAssertTrue(banner.waitForExistence(timeout: 90), "no notification")
        banner.press(forDuration: 2)
        let reply = springboard.buttons["Reply"]
        shot("notify-expanded")
        XCTAssertTrue(reply.waitForExistence(timeout: 5), "no Reply action")
        shot("notify-actions")
        reply.tap()
        sleep(1)
        springboard.typeText("from-notification")
        springboard.buttons["Send"].tap()
        sleep(3)
        shot("notify-replied")
    }

    func testRotation() throws {
        open(env["RAPTOR_TEST_SESSION"] ?? "editor")
        XCTAssertTrue(app.textViews["Terminal"].waitForExistence(timeout: 15))
        sleep(3)
        shot("rotate-portrait")
        XCUIDevice.shared.orientation = .landscapeLeft
        sleep(3)
        shot("rotate-landscape")
        app.textViews["Terminal"].tap()
        sleep(2)
        shot("rotate-landscape-keyboard")
        XCUIDevice.shared.orientation = .portrait
        sleep(3)
        shot("rotate-back")
    }

    /// The driver prints "resumed" into the session while the app is away.
    func testBackgroundResume() throws {
        open(env["RAPTOR_TEST_SESSION"] ?? "raptor-demo")
        XCTAssertTrue(app.textViews["Terminal"].waitForExistence(timeout: 15))
        sleep(2)
        XCUIDevice.shared.press(.home)
        sleep(UInt32(env["RAPTOR_AWAY"] ?? "50") ?? 50)
        app.activate()
        let start = Date()
        shot("resume-0")
        sleep(1)
        shot("resume-1")
        sleep(3)
        shot("resume-4")
        print("RESUME took-at-least", Date().timeIntervalSince(start))
    }

    func testPanes() throws {
        open("editor")
        let terminals = app.textViews.matching(identifier: "Terminal")
        XCTAssertTrue(terminals.firstMatch.waitForExistence(timeout: 15))
        sleep(4)
        shot("panes")
        terminals.element(boundBy: 0).tap()
        sleep(2)
        shot("panes-focus-left")
        XCUIDevice.shared.orientation = .portrait
        sleep(3)
        shot("panes-portrait")
        XCUIDevice.shared.orientation = .landscapeLeft
        sleep(2)
    }

    func testSelectionZoomAndSizing() throws {
        open("raptor-demo")
        let terminal = app.textViews["Terminal"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        sleep(3)
        let from = terminal.coordinate(withNormalizedOffset: CGVector(dx: 0.15, dy: 0.3))
        let to = terminal.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.36))
        from.press(forDuration: 0.8, thenDragTo: to)
        sleep(1)
        shot("select-drag")
        terminal.tap()
        terminal.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.3)).doubleTap()
        sleep(1)
        shot("select-word")
        terminal.tap()
        terminal.pinch(withScale: 1.6, velocity: 1)
        sleep(2)
        shot("pinch-bigger")
        terminal.pinch(withScale: 0.6, velocity: -1)
        sleep(2)
        app.buttons["More"].tap()
        sleep(1)
        shot("more-menu")
        let desk = app.buttons["Use Desktop Size"]
        if desk.waitForExistence(timeout: 3) {
            desk.tap()
            sleep(3)
            shot("mirror")
            app.buttons["More"].tap()
            app.buttons["Fit to This Screen"].tap()
            sleep(3)
            shot("fit-again")
        }
    }

    func testHideKeyboard() throws {
        open("raptor-demo")
        let terminal = app.textViews["Terminal"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        terminal.tap()
        sleep(2)
        shot("hide-before")
        let w = app.windows.firstMatch.frame
        let kbTop = app.keyboards.firstMatch.exists ? app.keyboards.firstMatch.frame.minY : w.height * 0.62
        app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: w.width - 24, dy: kbTop - 30)).tap()
        sleep(2)
        shot("hide-after")
    }

    /// iPad with a hardware keyboard: shortcuts from the Terminal menu.
    func testHardwareKeyboard() throws {
        open("raptor-demo")
        let terminal = app.textViews["Terminal"].firstMatch
        XCTAssertTrue(terminal.waitForExistence(timeout: 15))
        terminal.tap()
        sleep(1)
        app.typeKey("t", modifierFlags: .command)
        sleep(3)
        shot("hw-new-window")
        app.typeText("clear; echo typed-on-hardware")
        app.typeKey(.enter, modifierFlags: [])
        app.typeKey("c", modifierFlags: .control)
        sleep(1)
        app.typeKey("k", modifierFlags: .command)
        sleep(2)
        shot("hw-palette")
        app.typeKey(.escape, modifierFlags: [])
        sleep(1)
        app.typeKey("w", modifierFlags: .command)
        sleep(3)
        shot("hw-closed")
    }

    /// Answer an agent from the Needs-you inbox without opening its terminal.
    func testInboxReply() throws {
        app.launchEnvironment["RAPTOR_OPEN"] = nil
        app.launch()
        let sessions = app.buttons.matching(identifier: "Sessions").firstMatch
        if sessions.waitForExistence(timeout: 10) { sessions.tap() }
        let reply = app.textFields["Reply"].firstMatch
        XCTAssertTrue(reply.waitForExistence(timeout: 10))
        reply.tap()
        reply.typeText("from-inbox")
        shot("inbox-typing")
        app.buttons["Send reply"].firstMatch.tap()
        sleep(2)
        shot("inbox-sent")
    }

    func testSidebarAndLightTheme() throws {
        open("build")
        XCTAssertTrue(app.textViews["Terminal"].firstMatch.waitForExistence(timeout: 15))
        sleep(3)
        shot("sidebar-shown")
        app.buttons.matching(identifier: "Sessions").firstMatch.tap()
        sleep(2)
        shot("sidebar-hidden")
        if !app.buttons["Settings"].firstMatch.exists { app.buttons.matching(identifier: "Sessions").firstMatch.tap(); sleep(1) }
        app.buttons["Settings"].firstMatch.tap()
        app.buttons["Solarized"].firstMatch.tap()
        sleep(1)
        shot("light-settings")
        app.buttons["Done"].tap()
        sleep(2)
        shot("light-theme")
        if !app.buttons["Settings"].firstMatch.exists { app.buttons.matching(identifier: "Sessions").firstMatch.tap(); sleep(1) }
        app.buttons["Settings"].firstMatch.tap()
        app.buttons["Tokyo"].firstMatch.tap()
        app.buttons["Done"].tap()
    }

    func testSettings() throws {
        app.launch()
        sleep(2)
        let gear = app.buttons["Settings"]
        if !gear.exists { app.buttons.matching(identifier: "Sessions").firstMatch.tap(); sleep(1) }
        app.buttons["Settings"].firstMatch.tap()
        sleep(2)
        shot("settings-1")
        app.swipeUp()
        sleep(1)
        shot("settings-2")
        app.swipeUp()
        sleep(1)
        shot("settings-3")
    }

    func testOtherSessions() throws {
        for s in ["api", "monitor", "editor", "build"] {
            open(s)
            XCTAssertTrue(app.textViews["Terminal"].waitForExistence(timeout: 15))
            sleep(4)
            shot("session-\(s)")
            app.terminate()
        }
    }
}

extension XCUIElement {
    func clearAndType(_ text: String) {
        tap()
        if let v = value as? String, !v.isEmpty {
            typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: v.count))
        }
        typeText(text)
    }
}
