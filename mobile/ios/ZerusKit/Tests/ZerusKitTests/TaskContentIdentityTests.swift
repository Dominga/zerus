import CryptoKit
import Foundation
import XCTest
@testable import ZerusKit
import ZerusReplica

@MainActor final class TaskContentIdentityTests: XCTestCase {
    func testRestartKeepsAnySessionDraftHeldForReusedRelayComputerId() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let key = ProjectKey(connectionId: "connection", swarmId: "swarm", id: "project")
        let connection = Connection(id: key.connectionId, label: "Fixture", url: "https://relay.example.test", token: "private")
        let old = Machine(connectionId: key.connectionId, id: "same-relay-computer", name: "Original", online: true, machineId: "original-native-machine")
        var replacement = old; replacement.name = "Replacement"; replacement.machineId = "replacement-native-machine"
        let files = TaskContentDraftFiles(directory: root.appendingPathComponent("drafts"), keys: StaticKeyProvider(SymmetricKey(size: .bits256)), syncDirectory: {})
        let original = try files.create(TaskContentDraft.new(key, computer: old.id, machine: old.machineId, stage: .todo, target: .anySession, text: "Original private draft"))
        let model = TaskContentController(replicas: try TaskReplicas(root: root.appendingPathComponent("replicas")), files: files,
                                          read: { _ in Data() }, remove: { _, _ in XCTFail("original files must stay") }, upload: { _, _ in true }, changed: { _, _ in XCTFail("replacement completion") }, text: { $0 })
        model.open(TaskContentContext(connection: connection, key: key, projectName: "Fixture", computer: replacement), proposed: TaskContentDraft.new(key, computer: replacement.id, machine: replacement.machineId, stage: .todo, target: .anySession))
        let limit = ContinuousClock.now + .seconds(10)
        while model.state?.draft == nil && ContinuousClock.now < limit { try await Task.sleep(for: .milliseconds(5)) }
        XCTAssertEqual(model.state?.draft?.id, original.id)
        XCTAssertEqual(model.state?.held, true, "A reused relay computer ID must not rehome an Any-session draft")
        let id = try XCTUnwrap(model.state?.id)
        model.change(id) { $0.text = "Must not replace original" }; model.save(id)
        XCTAssertEqual(try files.read(original.scope, isNew: true)?.form.text, "Original private draft")
    }
}
