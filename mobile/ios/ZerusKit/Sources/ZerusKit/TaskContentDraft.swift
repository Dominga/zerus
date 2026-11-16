import Foundation
import ZerusReplica

/// Local legacy composition. It never reconstructs a qualified native handle or a network request.
public struct TaskContentDraft: Equatable, Sendable {
    public let scope: TaskEditDraftScope
    public let id: String
    public let computer: String
    public let revision: String?
    public let stage: TaskStage
    public var form: TaskEditForm
    public var state: State = .editing
    public enum State: String, Sendable { case editing, applying, completed }
    public var isNew: Bool { revision == nil }
    public static let newSlot = "00000000-0000-0000-0000-000000000000"
    public static func existing(_ key: ProjectKey, computer: String, task: TaskInfo) -> Self {
        Self(scope: TaskEditDraftScope(connectionId: key.connectionId, swarmId: key.swarmId, project: key.id, task: task.uuid),
             id: UUID().uuidString.lowercased(), computer: computer, revision: task.contentRevision, stage: task.stage,
             form: TaskEditForm(TaskEditView(project: key.id, task: task.uuid, title: "", text: task.text, target: task.target, rank: task.rank, attachments: task.attachments)))
    }
    public static func new(_ key: ProjectKey, computer: String, stage: TaskStage, target: TaskTarget, text: String = "") -> Self {
        Self(scope: TaskEditDraftScope(connectionId: key.connectionId, swarmId: key.swarmId, project: key.id, task: newSlot),
             id: UUID().uuidString.lowercased(), computer: computer, revision: nil, stage: stage,
             form: TaskEditForm(TaskEditView(project: key.id, task: newSlot, title: "", text: text, target: target, rank: "", attachments: [])))
    }
    public func apply(_ store: TaskStore, files: [TaskMediaInput]) throws -> TaskInfo {
        if isNew { return try store.addWithMedia(text: form.text, stage: stage, target: form.target ?? form.originalTarget, position: .end, files: files) }
        _ = try store.editContent(reference: scope.task, text: form.text, kept: form.kept.map(\.token), files: files, expectedRevision: revision!)
        if let target = form.target { _ = try store.setTarget(reference: scope.task, target: target) }
        return try store.get(reference: scope.task)
    }
    public func json() throws -> JSONObject {
        ["version": 1, "scope": .object(["connection": .string(scope.connectionId), "swarm": .string(scope.swarmId), "project": .string(scope.project), "task": .string(scope.task)]),
         "id": .string(id), "computer": .string(computer), "revision": revision.map(JSON.string) ?? .null,
         "stage": .string(stage == .backlog ? "backlog" : "todo"), "form": .string(try form.json()), "sequence": .int(form.sequence), "state": .string(state.rawValue)]
    }
    public func validate() throws {
        guard UUID(uuidString: id)?.uuidString.lowercased() == id, UUID(uuidString: scope.task)?.uuidString.lowercased() == scope.task,
              [.todo, .backlog].contains(stage), form.sequence >= 0, form.title.isEmpty, form.rank == nil, form.comment.isEmpty, form.commentFiles.isEmpty,
              form.kept.count + form.files.count <= 8, form.kept.allSatisfy({ $0.size <= UInt64(AttachmentPolicy.maxFileBytes) }), Set(form.kept.map(\.token)).count == form.kept.count,
              form.kept.reduce(UInt64(0), { $0 + $1.size }) <= UInt64(AttachmentPolicy.maxTotalBytes),
              form.kept.reduce(Int64(0), { $0 + Int64($1.size) }) + (try AttachmentPolicy.selection(form.files)) <= AttachmentPolicy.maxTotalBytes,
              revision != "", !isNew || scope.task == Self.newSlot && form.kept.isEmpty else { throw ZerusError.invalid("The original task draft is unavailable. Its bytes were kept.") }
        for value in [scope.connectionId, scope.swarmId, scope.project, computer] {
            guard !value.isEmpty, value.utf8.count <= 512, !value.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains) else { throw ZerusError.invalid("The original task draft scope is unavailable.") }
        }
    }
    public static func read(_ value: JSONObject) throws -> Self {
        try WorkspaceProtocol.fields(value, ["version", "scope", "id", "computer", "revision", "stage", "form", "sequence", "state"])
        guard try WorkspaceProtocol.number(value, "version") == 1, let scope = value["scope"]?.object,
              let state = State(rawValue: try WorkspaceProtocol.string(value, "state")) else { throw ZerusError.invalid("The saved task draft is unavailable.") }
        try WorkspaceProtocol.fields(scope, ["connection", "swarm", "project", "task"])
        let stage = try WorkspaceProtocol.string(value, "stage")
        guard ["todo", "backlog"].contains(stage) else { throw ZerusError.invalid("The saved task draft stage is unavailable.") }
        var result = try Self(scope: TaskEditDraftScope(connectionId: WorkspaceProtocol.string(scope, "connection"), swarmId: WorkspaceProtocol.string(scope, "swarm"), project: WorkspaceProtocol.string(scope, "project"), task: WorkspaceProtocol.string(scope, "task")),
                              id: WorkspaceProtocol.string(value, "id"), computer: WorkspaceProtocol.string(value, "computer"),
                              revision: value["revision"]?.isNull == true ? nil : WorkspaceProtocol.string(value, "revision"), stage: stage == "todo" ? .todo : .backlog,
                              form: TaskEditForm.read(WorkspaceProtocol.string(value, "form"), sequence: WorkspaceProtocol.number(value, "sequence")))
        result.state = state; try result.validate(); return result
    }
}

/// Exact picker identity, including the captured target and sequence; a changed form cannot consume it.
public struct TaskContentPicker: Equatable, Sendable {
    public let token: String
    public let draft: TaskContentDraft
    public init(_ draft: TaskContentDraft) { token = UUID().uuidString.lowercased(); self.draft = draft }
    public func matches(_ value: TaskContentDraft) -> Bool { value == draft && value.state == .editing }
}
