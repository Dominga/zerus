import Foundation
import XCTest
import ZerusReplica
@testable import ZerusKit

final class TaskEditFormTests: XCTestCase {
    private func view() -> TaskEditView {
        TaskEditView(project: "project", task: "task", title: "Original title", text: "Original body",
                     target: .session(session: SessionRef(machine: "machine", session: "old label", sessionId: "launch_id:opaque")), rank: "V", attachments: [])
    }
    func testSavedFormKeepsOriginalIdentityAndOnlyExplicitTargetChanges() throws {
        let original = view()
        var form = TaskEditForm(original); form.text = "Draft body"; form.sequence = 7
        let restored = try TaskEditForm.read(form.json(), sequence: form.sequence)
        XCTAssertEqual(form, restored)
        XCTAssertNil(restored.target)
        XCTAssertEqual(restored.originalTarget, original.target)
        XCTAssertTrue(restored.changed(original))
        XCTAssertEqual(original.text, "Original body")
    }
    func testRefreshCannotReplaceTheCapturedBaseline() {
        let original = view()
        var form = TaskEditForm(original); form.comment = "Person's comment"
        var refreshed = original; refreshed.title = "Remote replacement"; refreshed.target = .anySession
        XCTAssertEqual(form.title, "Original title"); XCTAssertNil(form.target)
        XCTAssertEqual(form.originalTarget, original.target); XCTAssertEqual(refreshed.title, "Remote replacement")
    }
    func testMalformedSavedFilesAndMediaSizesFailClosed() throws {
        var value = try XCTUnwrap(JSON.parse(TaskEditForm(view()).json()).object)
        value["files"] = ["not metadata"]
        XCTAssertThrowsError(try TaskEditForm.read(JSON.object(value).compact, sequence: 0))
        value["files"] = []
        value["kept"] = [.object(["name": "file", "mime": "text/plain", "reference": "", "size": -1, "sha256": "hash", "token": "token"])]
        XCTAssertThrowsError(try TaskEditForm.read(JSON.object(value).compact, sequence: 0))
    }
}
