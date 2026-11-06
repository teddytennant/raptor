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
        let png = app.screenshot().pngRepresentation
        if let dir = env["RAPTOR_SHOTS"] {
            try? png.write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(name).png"))
        }
        let a = XCTAttachment(screenshot: app.screenshot())
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
