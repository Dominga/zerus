import Foundation
import XCTest
@testable import ZerusKit
import ZerusReplica

final class TaskMediaSelectionTests: XCTestCase {
    func testSelectedCommentRemainsImmutableAfterRowsArriveOnAllReadableSchemas() throws {
        let fixture = try XCTUnwrap(SharedFixture.json("task-comment-media.json").object?["cases"]?.array)
        for row in fixture {
            let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: root) }
            let value = try XCTUnwrap(row.object), task = try WorkspaceProtocol.string(value, "task")
            let store = TaskStore(root: root.path, project: "project", projectName: "Fixture")
            func importSnapshot(_ name: String) throws {
                let bytes = try XCTUnwrap(Data(base64Encoded: WorkspaceProtocol.string(value, name)))
                XCTAssertTrue(try store.importUpdates(updates: [DocumentUpdate(task: task, bytes: bytes)]).refused.isEmpty)
            }
            try importSnapshot("before")
            let original = try store.get(reference: task)
            XCTAssertEqual(original.schema, value.bool("sealed") ? 8 : 3); XCTAssertFalse(original.writable)
            let entry = try XCTUnwrap(original.log.first { $0.text == "Original comment" })
            XCTAssertEqual(entry.attachmentCount, 1)
            let metadata = try XCTUnwrap(store.media(reference: task, entry: entry.entryId).first)
            let selected = TaskMediaSelection(task: task, entry: entry.entryId, token: metadata.token)
            XCTAssertEqual(try selected.read(store).data, Data("original-comment".utf8))
            try importSnapshot("after")
            let later = try XCTUnwrap(store.get(reference: task).log.first { $0.text == "Arrived while viewing" })
            XCTAssertEqual(try selected.read(store).data, Data("original-comment".utf8))
            XCTAssertThrowsError(try TaskMediaSelection(task: task, entry: later.entryId, token: metadata.token).read(store))
            XCTAssertThrowsError(try TaskMediaSelection(task: task, entry: nil, token: metadata.token).read(store))
            XCTAssertThrowsError(try TaskMediaSelection(task: UUID().uuidString.lowercased(), entry: entry.entryId, token: metadata.token).read(store))
            XCTAssertThrowsError(try store.comment(reference: task, text: "No ordinary writer widening"))
        }
    }
}
