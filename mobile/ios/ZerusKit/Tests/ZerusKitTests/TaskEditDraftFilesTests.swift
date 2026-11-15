import CryptoKit
import Foundation
import XCTest
@testable import ZerusKit

final class TaskEditDraftFilesTests: XCTestCase {
    private var directory: URL!
    private let keys = StaticKeyProvider(SymmetricKey(size: .bits256))
    private let scope = TaskEditDraftScope(connectionId: "connection", swarmId: "swarm", project: "project", task: UUID().uuidString.lowercased())
    private let machine = UUID().uuidString.lowercased()
    private let qualification = UUID().uuidString.lowercased()
    private let edit = UUID().uuidString.lowercased()
    private let original = "Private original requirement; keep this draft"

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }
    override func tearDownWithError() throws { try FileManager.default.removeItem(at: directory) }
    private func files(entries: Int = 32, bytes: Int64 = 256 * 1024 * 1024,
                       sync: @escaping @Sendable () throws -> Void = {}) -> TaskEditDraftFiles {
        TaskEditDraftFiles(directory: directory, keys: keys, maxEntries: entries, maxBytes: bytes, syncDirectory: sync)
    }
    private func create(_ files: TaskEditDraftFiles, scope: TaskEditDraftScope? = nil) throws -> TaskEditSavedDraft {
        try files.create(scope ?? self.scope, machineId: machine, hostDevice: "host-device", qualificationId: qualification, editId: edit, original: original)
    }
    private func frozen(_ files: TaskEditDraftFiles) throws -> TaskEditSavedDraft {
        let draft = try create(files)
        _ = try files.reserve(scope, draftId: draft.draftId, requestId: qualification, payload: "qualification bytes")
        _ = try files.settle(scope, draftId: draft.draftId, requestId: qualification, outcome: .acknowledged)
        return try files.freeze(scope, draftId: draft.draftId, manifest: "exact frozen manifest")
    }
    private func blobs() throws -> [URL] { try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) }

    func testOriginalSourceAndFrozenBytesSurviveEncryptedRestart() throws {
        let store = files(), draft = try frozen(store)
        XCTAssertEqual(try files().read(scope), draft)
        XCTAssertEqual(draft.original, original)
        XCTAssertEqual(draft.machineId, machine)
        XCTAssertEqual(draft.hostDevice, "host-device")
        XCTAssertEqual(draft.qualificationId, qualification)
        XCTAssertEqual(draft.editId, edit)
        let bytes = try Data(contentsOf: XCTUnwrap(blobs().first))
        XCTAssertNil(bytes.range(of: Data(original.utf8)))
        XCTAssertNil(bytes.range(of: Data("exact frozen manifest".utf8)))
    }
    func testPendingRequestCannotBeRepostedOrReplacedAfterRestart() throws {
        let store = files(), draft = try frozen(store)
        _ = try store.reserve(scope, draftId: draft.draftId, requestId: edit, payload: "exact payload")
        let reopened = files(), retained = try XCTUnwrap(reopened.read(scope))
        XCTAssertEqual(retained.state, .pending)
        XCTAssertEqual(retained.pendingId, edit)
        XCTAssertEqual(retained.pendingPayload, "exact payload")
        XCTAssertThrowsError(try reopened.reserve(scope, draftId: draft.draftId, requestId: edit, payload: "exact payload"))
        XCTAssertThrowsError(try reopened.reserve(scope, draftId: draft.draftId, requestId: UUID().uuidString.lowercased(), payload: "changed"))
        XCTAssertThrowsError(try create(reopened))
        XCTAssertEqual(try reopened.read(scope), retained)
    }
    func testUnknownKeepsTheDraftAndBlocksEveryNewIdentity() throws {
        let store = files(), draft = try frozen(store)
        _ = try store.reserve(scope, draftId: draft.draftId, requestId: edit, payload: "payload")
        _ = try store.settle(scope, draftId: draft.draftId, requestId: edit, outcome: .unknown)
        let reopened = files()
        XCTAssertEqual(try reopened.read(scope)?.state, .unknown)
        XCTAssertThrowsError(try reopened.reserve(scope, draftId: draft.draftId, requestId: UUID().uuidString.lowercased(), payload: "new input"))
        XCTAssertThrowsError(try reopened.freeze(scope, draftId: draft.draftId, manifest: "replacement"))
        XCTAssertEqual(try reopened.read(scope)?.original, original)
        XCTAssertEqual(try reopened.read(scope)?.manifest, "exact frozen manifest")
    }
    func testOnlyExactPendingAndFinalIdentitiesCanAdvance() throws {
        let store = files(), draft = try frozen(store), part = UUID().uuidString.lowercased()
        _ = try store.reserve(scope, draftId: draft.draftId, requestId: part, payload: "first part")
        XCTAssertThrowsError(try store.settle(scope, draftId: draft.draftId, requestId: edit, outcome: .acknowledged))
        XCTAssertThrowsError(try store.settle(scope, draftId: draft.draftId, requestId: UUID().uuidString.lowercased(), outcome: .acknowledged))
        XCTAssertThrowsError(try store.settle(scope, draftId: draft.draftId, requestId: part, outcome: .completed))
        _ = try store.settle(scope, draftId: draft.draftId, requestId: part, outcome: .acknowledged)
        XCTAssertThrowsError(try store.reserve(scope, draftId: draft.draftId, requestId: part, payload: "first part"))
        _ = try store.reserve(scope, draftId: draft.draftId, requestId: edit, payload: "last part")
        _ = try store.settle(scope, draftId: draft.draftId, requestId: edit, outcome: .completed)
        XCTAssertEqual(try files().read(scope)?.state, .completed)
        XCTAssertThrowsError(try store.reserve(scope, draftId: draft.draftId, requestId: UUID().uuidString.lowercased(), payload: "another input"))
    }
    func testQualificationAndFrozenBytesAreNotReplaceable() throws {
        let store = files(), draft = try create(store)
        XCTAssertThrowsError(try store.reserve(scope, draftId: draft.draftId, requestId: edit, payload: "unqualified"))
        XCTAssertThrowsError(try store.freeze(scope, draftId: draft.draftId, manifest: "unqualified"))
        _ = try store.reserve(scope, draftId: draft.draftId, requestId: qualification, payload: "qualification")
        _ = try store.settle(scope, draftId: draft.draftId, requestId: qualification, outcome: .acknowledged)
        _ = try store.freeze(scope, draftId: draft.draftId, manifest: "first manifest")
        XCTAssertThrowsError(try store.freeze(scope, draftId: draft.draftId, manifest: "second manifest"))
        XCTAssertEqual(try store.read(scope)?.manifest, "first manifest")
    }
    func testScopeAndDraftIdentityDoNotAlias() throws {
        let store = files(), draft = try frozen(store)
        let others = [TaskEditDraftScope(connectionId: "other", swarmId: scope.swarmId, project: scope.project, task: scope.task),
                      TaskEditDraftScope(connectionId: scope.connectionId, swarmId: "other", project: scope.project, task: scope.task),
                      TaskEditDraftScope(connectionId: scope.connectionId, swarmId: scope.swarmId, project: "other", task: scope.task),
                      TaskEditDraftScope(connectionId: scope.connectionId, swarmId: scope.swarmId, project: scope.project, task: UUID().uuidString.lowercased())]
        for other in others {
            XCTAssertNil(try store.read(other))
            XCTAssertThrowsError(try store.reserve(other, draftId: draft.draftId, requestId: edit, payload: "payload"))
        }
        XCTAssertThrowsError(try store.reserve(scope, draftId: UUID().uuidString.lowercased(), requestId: edit, payload: "payload"))
        XCTAssertEqual(try store.read(scope), draft)
    }
    func testCorruptFilesAreReportedAndPreserved() throws {
        let store = files()
        _ = try frozen(store)
        let file = try XCTUnwrap(blobs().first), good = try Data(contentsOf: file)
        try good.prefix(20).write(to: file)
        XCTAssertThrowsError(try files().read(scope))
        XCTAssertThrowsError(try create(files()))
        XCTAssertEqual(try Data(contentsOf: file), good.prefix(20))
    }
    func testStorageLimitRefusesWithoutEvictingTheOriginal() throws {
        let store = files(entries: 1), draft = try frozen(store)
        let other = TaskEditDraftScope(connectionId: scope.connectionId, swarmId: scope.swarmId, project: scope.project, task: UUID().uuidString.lowercased())
        XCTAssertThrowsError(try create(store, scope: other))
        XCTAssertEqual(try store.read(scope), draft)
        XCTAssertEqual(try blobs().count, 1)
        XCTAssertThrowsError(try files(bytes: 30).reserve(scope, draftId: draft.draftId, requestId: edit, payload: "payload"))
        XCTAssertEqual(try store.read(scope), draft)
    }
    func testDurabilityFailureStaysVisibleBeforeTheCallerCanSend() throws {
        let store = files(), draft = try frozen(store)
        var sent = false
        XCTAssertThrowsError(try {
            _ = try files(sync: { throw ZerusError.invalid("Forced directory sync failure") })
                .reserve(scope, draftId: draft.draftId, requestId: edit, payload: "payload")
            sent = true
        }())
        XCTAssertFalse(sent)
        XCTAssertEqual(try store.read(scope)?.state, .pending)
        XCTAssertThrowsError(try store.reserve(scope, draftId: draft.draftId, requestId: edit, payload: "payload"))
    }
    func testTypingSurvivesRestartAndOldCallbacksCannotReplaceNewerChanges() throws {
        let store = files(), draft = try create(store)
        _ = try store.reserve(scope, draftId: draft.draftId, requestId: qualification, payload: "qualification")
        _ = try store.settle(scope, draftId: draft.draftId, requestId: qualification, outcome: .acknowledged)
        _ = try store.edit(scope, draftId: draft.draftId, sequence: 2, changes: "The person's latest edits")
        XCTAssertThrowsError(try store.edit(scope, draftId: draft.draftId, sequence: 1, changes: "Stale callback"))
        _ = try store.edit(scope, draftId: draft.draftId, sequence: 2, changes: "The person's latest edits")
        XCTAssertThrowsError(try store.edit(scope, draftId: draft.draftId, sequence: 2, changes: "Changed bytes under the same sequence"))
        XCTAssertEqual(try files().read(scope)?.changes, "The person's latest edits")
        XCTAssertEqual(try files().read(scope)?.original, original)
        XCTAssertThrowsError(try store.freeze(scope, draftId: draft.draftId, manifest: "old fields", expectedSequence: 1))
        _ = try store.freeze(scope, draftId: draft.draftId, manifest: "current fields", expectedSequence: 2)
        XCTAssertThrowsError(try store.edit(scope, draftId: draft.draftId, sequence: 3, changes: "Late typing"))
        XCTAssertEqual(try files().read(scope)?.manifest, "current fields")
    }
    func testOnlyAnExactKnownCompletionReleasesItsDraft() throws {
        let store = files(), draft = try frozen(store)
        XCTAssertThrowsError(try store.releaseCompleted(scope, draftId: draft.draftId))
        _ = try store.reserve(scope, draftId: draft.draftId, requestId: edit, payload: "payload")
        XCTAssertThrowsError(try store.releaseCompleted(scope, draftId: draft.draftId))
        _ = try store.settle(scope, draftId: draft.draftId, requestId: edit, outcome: .completed)
        XCTAssertThrowsError(try store.releaseCompleted(scope, draftId: UUID().uuidString.lowercased()))
        try store.releaseCompleted(scope, draftId: draft.draftId)
        XCTAssertNil(try store.read(scope))
        XCTAssertNotEqual(try create(store).draftId, draft.draftId)
    }
    func testPersonDiscardOfAnUnsubmittedDraftCannotClearUnknownDelivery() throws {
        let store = files(), first = try create(store)
        XCTAssertThrowsError(try store.discardUnsubmitted(scope, draftId: UUID().uuidString.lowercased()))
        try store.discardUnsubmitted(scope, draftId: first.draftId)
        let next = try frozen(store)
        _ = try store.reserve(scope, draftId: next.draftId, requestId: edit, payload: "payload")
        XCTAssertThrowsError(try store.discardUnsubmitted(scope, draftId: next.draftId))
        _ = try store.settle(scope, draftId: next.draftId, requestId: edit, outcome: .unknown)
        XCTAssertThrowsError(try store.discardUnsubmitted(scope, draftId: next.draftId))
        XCTAssertEqual(try store.read(scope)?.state, .unknown)
    }
    func testReplacementCountsOnlyTheNewRecordWithinTheByteBudget() throws {
        let store = files(), draft = try frozen(store)
        _ = try store.reserve(scope, draftId: draft.draftId, requestId: edit, payload: "exact retained payload")
        let size = try Int64(Data(contentsOf: XCTUnwrap(blobs().first)).count)
        // Replacing this same record with an equally-sized Unknown record must
        // fit without counting the old encrypted file a second time.
        let replacement = try files(bytes: size + 32).settle(scope, draftId: draft.draftId, requestId: edit, outcome: .unknown)
        XCTAssertEqual(replacement.state, .unknown)
        XCTAssertLessThanOrEqual(try Int64(Data(contentsOf: XCTUnwrap(blobs().first)).count), size + 32)
    }

}
