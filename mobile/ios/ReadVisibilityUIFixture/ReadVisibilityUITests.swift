import XCTest

/// zerus-a4mub.31 merge condition: on screen, a message scrolled fully into view is read after the settled dwell, a
/// sliver is not, and a questions pane taller than twice the viewport counts once its readable part is visible.
final class ReadVisibilityUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launch()
    }

    private func label(_ id: String) -> String { app.staticTexts[id].label }

    /// Waits for at least one more completed dwell, then for the layout to stay settled past another dwell.
    private func settle(after previous: Int, _ step: String) {
        let deadline = Date().addingTimeInterval(10)
        while dwells() <= previous && Date() < deadline { RunLoop.current.run(until: Date().addingTimeInterval(0.1)) }
        XCTAssertGreaterThan(dwells(), previous, "no dwell completed: \(step)")
        RunLoop.current.run(until: Date().addingTimeInterval(1.0))
    }

    private func dwells() -> Int {
        let text = label("dwell-count")
        guard let range = text.range(of: "Dwells: ") else { return -1 }
        return Int(text[range.upperBound...].prefix { $0.isNumber }) ?? -1
    }

    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
        let record = XCTAttachment(string: [label("unread-count"), label("questions-seen"), label("visible-keys"), label("dwell-count")]
            .joined(separator: "\n"))
        record.name = name + "-labels"
        record.lifetime = .keepAlways
        add(record)
    }

    private func tap(_ button: String) -> Int {
        let before = dwells()
        app.buttons[button].tap()
        return before
    }

    func testReadableVisibilityDrivesTheReadDwellOnScreen() {
        XCTAssertTrue(app.staticTexts["unread-count"].waitForExistence(timeout: 20))
        settle(after: 0, "launch")
        // The first reply is on screen; the second is at most a sliver below the fold.
        XCTAssertEqual(label("unread-count"), "Unread: 1")
        XCTAssertFalse(label("visible-keys").contains("event:e2"))
        shot("01-top-first-read")

        settle(after: tap("Sliver e2"), "sliver e2")
        XCTAssertEqual(label("unread-count"), "Unread: 1", "a 20 pt sliver of the second reply must not read it")
        XCTAssertFalse(label("visible-keys").contains("event:e2"))
        shot("02-sliver-not-read")

        settle(after: tap("Full e2"), "full e2")
        XCTAssertTrue(label("visible-keys").contains("event:e2"))
        XCTAssertEqual(label("unread-count"), "Unread: 0", "the second reply fully in view is read after the dwell")
        shot("03-full-read")

        settle(after: tap("Sliver questions"), "sliver questions")
        XCTAssertEqual(label("questions-seen"), "Questions seen: no", "a 20 pt sliver of the questions pane is not seen")
        XCTAssertFalse(label("visible-keys").contains("history:questions"))
        shot("04-questions-sliver-not-seen")

        settle(after: tap("Readable questions"), "readable questions")
        XCTAssertTrue(label("visible-keys").contains("history:questions"))
        XCTAssertEqual(label("questions-seen"), "Questions seen: yes", "the pane counts once its readable part is visible")
        shot("05-questions-readable-seen")
    }
}
