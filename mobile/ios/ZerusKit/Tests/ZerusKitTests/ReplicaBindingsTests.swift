import Foundation
import XCTest
import ZerusReplica

/// The Rust task replica through its Swift bindings: a store files a task and
/// another replica of the project catches up through one exchange.
final class ReplicaBindingsTests: XCTestCase {
    private func folder() -> String { FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).path }

    func testMediaBindingsKeepBytesScopeAndTheOriginalContentRevision() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {
            do { try FileManager.default.removeItem(at: root) }
            catch { XCTFail("Private task fixture cleanup failed: \(error)") }
        }
        let computer = TaskStore(root: root.appendingPathComponent("computer").path, project: "project-1", projectName: "orchard")
        let phone = TaskStore(root: root.appendingPathComponent("phone").path, project: "project-1", projectName: "orchard")
        let bytes = Data([0, 255, 10])
        let picked = TaskMediaInput(name: "picked-note.txt", mime: "text/plain", data: bytes)
        XCTAssertEqual(picked.reference, "")
        let task = try computer.addWithMedia(text: "Inspect the picked file", stage: .todo,
            target: .anySession, position: .end, files: [picked])
        XCTAssertEqual(task.schema, 2)
        let page = try computer.updatesFor(theirs: phone.inventory(), limit: 1_048_576)
        XCTAssertFalse(page.more)
        _ = try phone.importUpdates(updates: page.updates)
        let loaded = try phone.get(reference: task.uuid)
        XCTAssertEqual(loaded.attachments, task.attachments)
        let meta = try XCTUnwrap(loaded.attachments.first)
        XCTAssertEqual(meta.size, 3)
        XCTAssertEqual(try phone.readMedia(reference: task.uuid, entry: nil, index: 0, expectedToken: meta.token).data, bytes)
        let arrived = try phone.comment(reference: task.uuid, text: "Arrived while editing")
        let comment = try XCTUnwrap(arrived.log.first { $0.kind == "comment" })
        XCTAssertFalse(comment.entryId.isEmpty)
        XCTAssertEqual(comment.attachmentCount, 0)
        let edited = try phone.editContent(reference: task.uuid, text: "Reviewed content", kept: [meta.token], files: [],
            expectedRevision: loaded.contentRevision)
        XCTAssertTrue(edited.log.contains { $0.text == "Arrived while editing" })
        XCTAssertThrowsError(try phone.editContent(reference: task.uuid, text: "Stale draft", kept: [meta.token], files: [],
            expectedRevision: loaded.contentRevision))
        XCTAssertEqual(try phone.get(reference: task.uuid).text, "Reviewed content")
        XCTAssertEqual(try phone.readMedia(reference: task.uuid, entry: nil, index: 0, expectedToken: meta.token).data, bytes)
        XCTAssertThrowsError(try phone.readMedia(reference: task.id, entry: nil, index: 0, expectedToken: meta.token))
        let removed = try phone.editContent(reference: task.uuid, text: edited.text, kept: [], files: [],
            expectedRevision: edited.contentRevision)
        XCTAssertTrue(removed.attachments.isEmpty)
        XCTAssertEqual(removed.schema, 2)
    }

    func testImmutableSessionIdentitySurvivesGeneratedBindingsAndExchange() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {
            do { try FileManager.default.removeItem(at: root) }
            catch { XCTFail("Private task fixture cleanup failed: \(error)") }
        }
        let legacy = SessionRef(machine: "fixture-machine", session: "codex/orchard/original-label")
        XCTAssertEqual(legacy.sessionId, "")
        let original = SessionRef(machine: legacy.machine, session: legacy.session,
                                  sessionId: "92ea863f-833d-4f58-a4a6-b30cb780e14a")
        let computer = TaskStore(root: root.appendingPathComponent("computer").path, project: "project-1", projectName: "orchard")
        let phone = TaskStore(root: root.appendingPathComponent("phone").path, project: "project-1", projectName: "orchard")
        let task = try computer.add(text: "Inspect the fixture", stage: .todo, target: .session(session: original), position: .end)
        let page = try computer.updatesFor(theirs: phone.inventory(), limit: 1_048_576)
        XCTAssertFalse(page.more)
        _ = try phone.importUpdates(updates: page.updates)
        guard case .session(let retained) = try phone.get(reference: task.uuid).target else { return XCTFail("Task target changed kind") }
        XCTAssertEqual(retained, original)
        XCTAssertEqual(retained.sessionId, original.sessionId)
        _ = try computer.setTarget(reference: task.uuid, target: .session(session: legacy))
        guard case .session(let nameOnly) = try computer.get(reference: task.uuid).target else { return XCTFail("Task target changed kind") }
        XCTAssertEqual(nameOnly.sessionId, "")
    }

    func testAStoreFilesATaskAndAnswersAnExchange() throws {
        let computer = TaskStore(root: folder(), project: "project-1", projectName: "orchard")
        let phone = TaskStore(root: folder(), project: "project-1", projectName: "orchard")
        let task = try computer.add(text: "Add rate limiting", stage: .todo, target: .anySession, position: .end)
        XCTAssertEqual(try computer.list().tasks.count, 1)
        XCTAssertNotNil(task.id.range(of: "^T-[a-z0-9]{6}$", options: .regularExpression), task.id)
        let digest = try computer.liveDigest()
        XCTAssertNotNil(digest.range(of: "^[0-9a-f]{64}$", options: .regularExpression), digest)
        let exchange = phone.exchange()
        while let request = try exchange.nextRequest() {
            _ = try exchange.accept(answer: try computer.answerExchange(request: request))
        }
        XCTAssertEqual(try phone.liveDigest(), digest)
        XCTAssertEqual(try phone.get(reference: task.uuid).text, "Add rate limiting")
    }
}
