import CryptoKit
import Foundation
import XCTest
@testable import ZerusKit
import ZerusReplica

final class TaskContentDraftTests: XCTestCase {
    private let key = ProjectKey(connectionId: "connection", swarmId: "swarm", id: "project")
    private func fixture(_ body: (TaskStore, TaskContentDraftFiles, URL) throws -> Void) throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let store = TaskStore(root: root.appendingPathComponent("replica").path, project: key.id, projectName: "Fixture")
        let files = TaskContentDraftFiles(directory: root.appendingPathComponent("drafts"), keys: StaticKeyProvider(SymmetricKey(size: .bits256)), syncDirectory: {})
        try body(store, files, root)
    }
    func testStaleContentKeepsDraftAndConcurrentBody() throws {
        try fixture { store, _, _ in
            let task = try store.add(text: "Original", stage: .todo, target: .anySession, position: .end)
            var draft = TaskContentDraft.existing(key, computer: "computer", task: task); draft.form.text = "Draft"
            _ = try store.editText(reference: task.uuid, text: "Concurrent")
            XCTAssertThrowsError(try draft.apply(store, files: []))
            XCTAssertEqual(try store.get(reference: task.uuid).text, "Concurrent")
            XCTAssertEqual(draft.form.text, "Draft")
        }
    }
    func testRetainedFilesNewBytesAndUnrelatedHistory() throws {
        try fixture { store, _, _ in
            let task = try store.addWithMedia(text: "Original", stage: .todo, target: .anySession, position: .end, files: [TaskMediaInput(name: "first.txt", mime: "text/plain", data: Data([1]), reference: "")])
            var draft = TaskContentDraft.existing(key, computer: "computer", task: task); draft.form.kept = []; draft.form.text = "Reviewed"
            _ = try store.comment(reference: task.uuid, text: "Unrelated arrival")
            let bytes = Data([0, 255, 13, 10])
            let saved = try draft.apply(store, files: [TaskMediaInput(name: "new.txt", mime: "text/plain", data: bytes, reference: "")])
            XCTAssertEqual(saved.attachments.map(\.name), ["new.txt"])
            XCTAssertTrue(saved.log.contains { $0.text == "Unrelated arrival" })
            XCTAssertEqual(try store.readMedia(reference: task.uuid, entry: nil, index: 0, expectedToken: saved.attachments[0].token).data, bytes)
        }
    }
    func testPickerRequiresWholeOriginalDraftAndFreshToken() throws {
        let draft = TaskContentDraft.new(key, computer: "computer", stage: .todo, target: .anySession)
        let ticket = TaskContentPicker(draft)
        XCTAssertTrue(ticket.matches(draft)); XCTAssertNotEqual(ticket.token, TaskContentPicker(draft).token)
        var changed = draft; changed.form.sequence += 1
        XCTAssertFalse(ticket.matches(changed))
        changed = draft; changed.form.target = .session(session: SessionRef(machine: "other", session: "label", sessionId: "launch_id:original"))
        XCTAssertFalse(ticket.matches(changed))
        changed = draft; changed.state = .applying; XCTAssertFalse(ticket.matches(changed))
        let foreign = TaskContentDraft.new(ProjectKey(connectionId: "other", swarmId: key.swarmId, id: key.id), computer: "computer", stage: .todo, target: .anySession)
        XCTAssertFalse(ticket.matches(foreign))
    }
    func testApplyingSurvivesRestartAndCannotBeRecreatedReleasedOrEdited() throws {
        try fixture { _, files, _ in
            let draft = try files.create(TaskContentDraft.new(key, computer: "computer", stage: .todo, target: .anySession, text: "Kept"))
            let held = try files.begin(draft)
            XCTAssertEqual(try files.read(draft.scope, isNew: true), held)
            XCTAssertThrowsError(try files.create(draft))
            XCTAssertThrowsError(try files.begin(draft))
            XCTAssertThrowsError(try files.checkpoint(draft, sequence: 1, changes: draft.form.json()))
            XCTAssertThrowsError(try files.release(draft))
            _ = try files.completed(draft)
            try files.release(draft)
            XCTAssertNil(try files.read(draft.scope, isNew: true))
        }
    }
    func testMonotonicCheckpointAndQuotaKeepPreviousBytes() throws {
        try fixture { _, files, root in
            let draft = try files.create(TaskContentDraft.new(key, computer: "computer", stage: .todo, target: .anySession))
            var form = draft.form; form.text = "Latest"; form.sequence = 4
            let next = try files.checkpoint(draft, sequence: 4, changes: form.json())
            XCTAssertThrowsError(try files.checkpoint(draft, sequence: 3, changes: draft.form.json()))
            XCTAssertThrowsError(try files.checkpoint(draft, sequence: 4, changes: draft.form.json()))
            let bounded = TaskContentDraftFiles(directory: root.appendingPathComponent("drafts"), keys: StaticKeyProvider(SymmetricKey(size: .bits256)), maxEntries: 1, syncDirectory: {})
            let foreign = TaskContentDraft.new(ProjectKey(connectionId: key.connectionId, swarmId: key.swarmId, id: "other"), computer: "computer", stage: .todo, target: .anySession)
            XCTAssertThrowsError(try bounded.create(foreign))
            XCTAssertEqual(try files.read(draft.scope, isNew: true), next)
        }
    }

    func testFailedPrivateSyncIsVisibleAndKeepsApplyingHold() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let fault = ContentSyncFault(), keys = StaticKeyProvider(SymmetricKey(size: .bits256))
        let files = TaskContentDraftFiles(directory: root, keys: keys, syncDirectory: { try fault.sync() })
        let draft = try files.create(TaskContentDraft.new(key, computer: "computer", stage: .todo, target: .anySession))
        fault.enable()
        XCTAssertThrowsError(try files.begin(draft))
        XCTAssertEqual(try files.read(draft.scope, isNew: true)?.state, .applying)
        XCTAssertThrowsError(try files.begin(draft))
    }
}
private final class ContentSyncFault: @unchecked Sendable {
    private let lock = NSLock()
    private var failed = false
    func enable() { lock.lock(); failed = true; lock.unlock() }
    func sync() throws {
        lock.lock(); let fail = failed; lock.unlock()
        if fail { throw ZerusError.invalid("forced private sync refusal") }
    }
}
