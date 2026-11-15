import Foundation
import ZerusReplica

public struct TaskEditContent: Sendable {
    public let title: String, text: String
    public let kept: [String]
    public let files: [TaskMediaInput]
    public init(title: String, text: String, kept: [String], files: [TaskMediaInput]) { self.title = title; self.text = text; self.kept = kept; self.files = files }
}
public struct TaskEditInput: Sendable {
    public let content: TaskEditContent?
    public let target: TaskTarget?
    public let rank: String?
    public let comment: String
    public let commentFiles: [TaskMediaInput]
    public init(content: TaskEditContent? = nil, target: TaskTarget? = nil, rank: String? = nil, comment: String = "", commentFiles: [TaskMediaInput] = []) {
        self.content = content; self.target = target; self.rank = rank; self.comment = comment; self.commentFiles = commentFiles
    }
    var hasChanges: Bool { content != nil || target != nil || rank != nil || !comment.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !commentFiles.isEmpty }
}
public struct TaskEditDraftHeld: Error, Sendable { public let saved: TaskEditSavedDraft }

/// A live original handle. Saved draft bytes cannot reconstruct this object.
public actor TaskEditSession {
    public let scope: TaskEditDraftScope
    public let original: TaskEditSavedDraft
    public let view: TaskEditView
    public var id: String { original.draftId }
    public var editable: Bool { !closed && !busy }
    private let store: TaskStore
    private let qualification: TaskEditQualification
    private let draft: TaskEditDraft
    private let transport: TaskEditTransport
    private let files: TaskEditDraftFiles
    private var busy = false, closed = false
    private init(store: TaskStore, qualification: TaskEditQualification, draft: TaskEditDraft, transport: TaskEditTransport,
                 files: TaskEditDraftFiles, original: TaskEditSavedDraft, view: TaskEditView) {
        self.store = store; self.qualification = qualification; self.draft = draft; self.transport = transport
        self.files = files; self.original = original; self.view = view; scope = transport.scope
    }
    public static func open(replicas: TaskReplicas, projectName: String, transport: TaskEditTransport, files: TaskEditDraftFiles,
                            originalForm: @escaping @Sendable (TaskEditView) throws -> String) async throws -> TaskEditSession {
        let scope = transport.scope
        if let saved = try await taskEditIO({ try files.read(scope) }) { throw TaskEditDraftHeld(saved: saved) }
        let store = replicas.store(transport.key, name: projectName)
        let qualification = try await taskEditIO { try store.qualifyEdit(reference: scope.task, route: transport.route) }
        let request = qualification.request(), editId = qualification.editId()
        let record = try await taskEditIO {
            try files.create(scope, machineId: transport.route.machineId, hostDevice: transport.route.hostDevice,
                             qualificationId: request.requestId, editId: editId, original: originalForm(qualification.view()))
        }
        do {
            let proof = try await transport.proof(request) { _ = try files.reserve(scope, draftId: record.draftId, requestId: request.requestId, payload: request.payload) }
            let draft = try await taskEditIO { try qualification.accept(proof: proof) }
            _ = try await taskEditIO { try files.settle(scope, draftId: record.draftId, requestId: request.requestId, outcome: .acknowledged) }
            try await transport.verified(request.requestId)
            return try await taskEditIO { TaskEditSession(store: store, qualification: qualification, draft: draft, transport: transport, files: files, original: record, view: draft.view()) }
        } catch {
            try? await taskEditIO {
                if let saved = try files.read(scope), saved.state == .pending, let pending = saved.pendingId {
                    _ = try files.settle(scope, draftId: record.draftId, requestId: pending, outcome: Self.refused(error) ? .refused : .unknown)
                }
            }
            throw error
        }
    }
    public func persist(sequence: Int64, changes: String) async throws {
        guard !closed && !busy else { throw ZerusError.invalid("The original task edit is no longer editable.") }
        let scope = scope, id = original.draftId, files = files
        _ = try await taskEditIO { try files.edit(scope, draftId: id, sequence: sequence, changes: changes) }
    }
    public func save(sequence: Int64, changes: String, input: TaskEditInput) async throws {
        guard !closed && !busy && input.hasChanges else { throw ZerusError.invalid("The original task edit is unavailable or has no changes.") }
        busy = true
        var frozen = false
        defer { busy = false; if frozen { closed = true } }
        let scope = scope, id = original.draftId, files = files, draft = draft
        try await taskEditIO {
            _ = try files.edit(scope, draftId: id, sequence: sequence, changes: changes)
            try draft.resetEdits()
            if let content = input.content { try draft.content(title: content.title, text: content.text, kept: content.kept, files: content.files) }
            if let target = input.target { try draft.target(target: target) }
            if let rank = input.rank { try draft.rank(rank: rank) }
            if !input.comment.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !input.commentFiles.isEmpty { try draft.comment(text: input.comment, files: input.commentFiles) }
        }
        let manifest = try await taskEditIO { try draft.freeze() }; frozen = true
        _ = try await taskEditIO { try files.freeze(scope, draftId: id, manifest: manifest, expectedSequence: sequence) }
        for _ in 0..<256 {
            guard let request = try await taskEditIO({ try draft.nextRequest() }) else { return }
            do {
                let proof = try await transport.proof(request) { _ = try files.reserve(scope, draftId: id, requestId: request.requestId, payload: request.payload) }
                let progress = try await taskEditIO { try draft.accept(proof: proof) }
                _ = try await taskEditIO { try files.settle(scope, draftId: id, requestId: request.requestId, outcome: progress.complete ? .completed : .acknowledged) }
                try await transport.verified(request.requestId)
                if progress.complete { return }
            } catch {
                try? await taskEditIO {
                    if !Self.refused(error) { try? draft.holdUnknown() }
                    if let saved = try files.read(scope), saved.state == .pending, saved.pendingId == request.requestId {
                        _ = try files.settle(scope, draftId: id, requestId: request.requestId, outcome: Self.refused(error) ? .refused : .unknown)
                    }
                }
                throw error
            }
        }
        throw TaskEditError.Unknown(message: "The original task upload exceeded its bounded part budget. Its draft was kept for review.")
    }
    public func close() { closed = true }
    private nonisolated static func refused(_ error: Error) -> Bool { if case TaskEditError.Refused = error { return true }; return false }
}
