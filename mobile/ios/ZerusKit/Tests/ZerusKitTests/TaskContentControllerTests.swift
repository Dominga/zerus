import CryptoKit
import Foundation
import XCTest
@testable import ZerusKit
import ZerusReplica

@MainActor final class TaskContentControllerTests: XCTestCase {
    private let key = ProjectKey(connectionId: "connection", swarmId: "swarm", id: "project")
    private func wait(_ model: TaskContentController, _ description: String, _ condition: (TaskContentState) -> Bool) async throws {
        let limit = ContinuousClock.now + .seconds(10)
        while model.state.map(condition) != true && ContinuousClock.now < limit { try await Task.sleep(for: .milliseconds(5)) }
        XCTAssertTrue(model.state.map(condition) == true, description + ": " + (model.state?.error ?? "no state"))
    }
    private func fixture(_ body: (TaskReplicas, TaskContentDraftFiles, TaskContentContext) async throws -> Void) async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let replicas = try TaskReplicas(root: root.appendingPathComponent("replicas"))
        let files = TaskContentDraftFiles(directory: root.appendingPathComponent("drafts"), keys: StaticKeyProvider(SymmetricKey(size: .bits256)), syncDirectory: {})
        let context = TaskContentContext(connection: Connection(id: key.connectionId, label: "Fixture", url: "https://relay.example.test", token: "private"), key: key, projectName: "Fixture", computer: Machine(connectionId: key.connectionId, id: "computer", name: "Fixture", online: true))
        try await body(replicas, files, context)
    }
    func testProjectSwitchLatePickerAndExactOriginalCompletion() async throws {
        try await fixture { replicas, files, context in
            var completions: [(ProjectKey, String)] = []
            let model = TaskContentController(replicas: replicas, files: files, read: { _ in Data([0, 255]) }, remove: { _, _ in },
                                              upload: { _, _ in true }, changed: { completions.append(($0.key, $1)) }, text: { $0 })
            model.open(context, proposed: TaskContentDraft.new(key, computer: "computer", stage: .todo, target: .anySession, text: "Original project"))
            try await wait(model, "draft open") { !$0.loading && $0.draft != nil }
            var ticket: TaskContentPicker?
            model.preparePicker(model.state!.id) { ticket = $0 }
            try await wait(model, "original picker") { _ in ticket != nil }
            let file = Attachment(id: UUID().uuidString.lowercased(), name: "fixture.txt", mime: "text/plain", bytes: 2, sha256: String(repeating: "ab", count: 32))
            model.picked(ticket!, imports: [{ _ in file }])
            try await wait(model, "private import") { !$0.busy && $0.draft?.form.files.count == 1 }
            var repeated = 0
            model.picked(ticket!, imports: [{ _ in repeated += 1; return file }]); XCTAssertEqual(repeated, 0)
            let foreign = ProjectKey(connectionId: "foreign", swarmId: "other", id: "other")
            model.open(context, proposed: TaskContentDraft.new(foreign, computer: "computer", stage: .todo, target: .anySession))
            XCTAssertEqual(model.state?.draft?.scope.project, key.id)
            model.save(model.state!.id)
            try await wait(model, "original save") { $0.complete }
            XCTAssertEqual(completions.count, 1); XCTAssertEqual(completions[0].0, key)
            let store = replicas.store(key, name: "Fixture"), task = try store.get(reference: completions[0].1)
            XCTAssertEqual(try store.readMedia(reference: task.uuid, entry: nil, index: 0, expectedToken: task.attachments[0].token).data, Data([0, 255]))
            XCTAssertTrue(try replicas.store(foreign, name: "Foreign").list().tasks.isEmpty)
            model.save(model.state!.id); XCTAssertEqual(completions.count, 1)
        }
    }
    func testUnsupportedPickerAndCapabilityLossKeepPrivateBytes() async throws {
        try await fixture { replicas, files, context in
            var upload = false, opened = 0, completed = 0
            let model = TaskContentController(replicas: replicas, files: files, read: { _ in Data([1]) }, remove: { _, _ in },
                                              upload: { _, _ in upload }, changed: { _, _ in completed += 1 }, text: { $0 })
            model.open(context, proposed: TaskContentDraft.new(key, computer: "computer", stage: .todo, target: .anySession, text: "Kept"))
            try await wait(model, "draft open") { !$0.loading && $0.draft != nil }
            let id = model.state!.id
            model.preparePicker(id) { _ in opened += 1 }; XCTAssertEqual(opened, 0); XCTAssertEqual(model.state?.busy, false)
            upload = true; var ticket: TaskContentPicker?
            model.preparePicker(id) { opened += 1; ticket = $0 }
            try await wait(model, "picker open") { _ in ticket != nil }
            let file = Attachment(id: UUID().uuidString.lowercased(), name: "a.txt", mime: "text/plain", bytes: 1, sha256: String(repeating: "ab", count: 32))
            model.picked(ticket!, imports: [{ _ in file }]); try await wait(model, "import saved") { !$0.busy && $0.draft?.form.files.count == 1 }
            upload = false; model.save(id)
            try await wait(model, "save refusal settled") { !$0.busy }
            XCTAssertEqual(completed, 0); XCTAssertEqual(model.state?.draft?.form.files.count, 1)
            XCTAssertTrue(try replicas.store(key, name: "Fixture").list().tasks.isEmpty)
        }
    }
    func testForeignContextCannotCreateDraft() async throws {
        try await fixture { replicas, files, context in
            let model = TaskContentController(replicas: replicas, files: files, read: { _ in Data() }, remove: { _, _ in },
                                              upload: { _, _ in true }, changed: { _, _ in XCTFail("foreign completion") }, text: { $0 })
            let proposed = TaskContentDraft.new(ProjectKey(connectionId: "foreign", swarmId: key.swarmId, id: key.id), computer: "computer", stage: .todo, target: .anySession)
            model.open(context, proposed: proposed)
            try await wait(model, "foreign draft refused") { !$0.loading && !$0.error.isEmpty }
            XCTAssertNil(try files.read(proposed.scope, isNew: true))
        }
    }

    func testCancelledPickerAndPartialImportFailureKeepOriginalComposition() async throws {
        try await fixture { replicas, files, context in
            let removed = ContentRemovedFiles()
            let model = TaskContentController(replicas: replicas, files: files, read: { _ in Data([1]) }, remove: { id, _ in await removed.append(id) },
                                              upload: { _, _ in true }, changed: { _, _ in XCTFail("no save requested") }, text: { $0 })
            model.open(context, proposed: TaskContentDraft.new(key, computer: "computer", stage: .todo, target: .anySession, text: "Kept"))
            try await wait(model, "draft open") { !$0.loading && $0.draft != nil }
            let id = model.state!.id; var first: TaskContentPicker?
            model.preparePicker(id) { first = $0 }; try await wait(model, "first picker") { _ in first != nil }
            model.picked(first!, imports: []); XCTAssertEqual(model.state?.busy, false)
            var imports = 0
            model.picked(first!, imports: [{ _ in imports += 1; throw ZerusError.invalid("cancelled callback") }])
            try await Task.sleep(for: .milliseconds(20)); XCTAssertEqual(imports, 0)
            var second: TaskContentPicker?
            model.preparePicker(id) { second = $0 }; try await wait(model, "second picker") { _ in second != nil }
            let file = Attachment(id: UUID().uuidString.lowercased(), name: "first.txt", mime: "text/plain", bytes: 1, sha256: String(repeating: "ab", count: 32))
            model.picked(second!, imports: [{ _ in file }, { _ in throw ZerusError.invalid("forced provider refusal") }])
            try await wait(model, "partial failure settled") { !$0.busy && $0.error.contains("forced provider refusal") }
            XCTAssertEqual(model.state?.draft?.form.files.count, 0); XCTAssertEqual(model.state?.draft?.form.text, "Kept")
            let ids = await removed.ids; XCTAssertEqual(ids, [file.id])
        }
    }
}
private actor ContentRemovedFiles {
    var ids: [String] = []
    func append(_ id: String) { ids.append(id) }
}
