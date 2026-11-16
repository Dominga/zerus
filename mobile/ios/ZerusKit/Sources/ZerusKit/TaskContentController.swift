import Foundation
import Observation
import ZerusReplica

public struct TaskContentContext: Sendable {
    public let connection: Connection, key: ProjectKey, projectName: String, computer: Machine
    public init(connection: Connection, key: ProjectKey, projectName: String, computer: Machine) {
        self.connection = connection; self.key = key; self.projectName = projectName; self.computer = computer
    }
}
public struct TaskContentState: Identifiable {
    public let id: UUID, context: TaskContentContext
    public var draft: TaskContentDraft?
    public var busy = false, loading = false, complete = false, held = false, error = ""
}
private struct TaskContentHeld: Error {}

/// Local content only. Every asynchronous continuation uses its captured scope and one checkpoint pump.
@MainActor @Observable public final class TaskContentController {
    public private(set) var state: TaskContentState?
    @ObservationIgnored private let replicas: TaskReplicas
    @ObservationIgnored private let files: TaskContentDraftFiles
    @ObservationIgnored private let read: @Sendable (Attachment) async throws -> Data
    @ObservationIgnored private let remove: @Sendable (String, Set<String>) async throws -> Void
    @ObservationIgnored private let upload: (TaskContentContext, TaskTarget) -> Bool
    @ObservationIgnored private let changed: (TaskContentContext, String) -> Void
    @ObservationIgnored private let text: (String) -> String
    @ObservationIgnored private var checkpoints: TaskEditCheckpoints?
    @ObservationIgnored private var picker: TaskContentPicker?
    public init(replicas: TaskReplicas, files: TaskContentDraftFiles,
                read: @escaping @Sendable (Attachment) async throws -> Data, remove: @escaping @Sendable (String, Set<String>) async throws -> Void,
                upload: @escaping (TaskContentContext, TaskTarget) -> Bool, changed: @escaping (TaskContentContext, String) -> Void,
                text: @escaping (String) -> String) {
        self.replicas = replicas; self.files = files; self.read = read; self.remove = remove
        self.upload = upload; self.changed = changed; self.text = text
    }
    public func open(_ context: TaskContentContext, proposed: TaskContentDraft) {
        guard state == nil else { return }
        let id = UUID(); state = TaskContentState(id: id, context: context, loading: true)
        Task {
            do {
                try proposed.validate()
                guard proposed.scope.connectionId == context.key.connectionId, proposed.scope.swarmId == context.key.swarmId,
                      proposed.scope.project == context.key.id, context.connection.id == context.key.connectionId,
                      context.computer.connectionId == context.key.connectionId, proposed.computer == context.computer.id, proposed.machine == context.computer.machineId else { throw ZerusError.invalid(text("task_content_stale")) }
                let files = files
                let draft = try await replicas.run {
                    let previous = try files.read(proposed.scope, isNew: proposed.isNew)
                    if let previous, previous.state == .completed { try files.release(previous) }
                    return try previous.flatMap { $0.state == .completed ? nil : $0 } ?? files.create(proposed)
                }
                guard state?.id == id else { return }
                let held = draft.state != .editing || draft.computer != context.computer.id || draft.machine != context.computer.machineId
                if !held {
                    checkpoints = TaskEditCheckpoints(write: { [replicas] value in
                        _ = try await replicas.run { try files.checkpoint(draft, sequence: value.sequence, changes: value.changes) }
                    }, failed: { [weak self] _, _ in if self?.state?.id == id { self?.report(id, self?.text("task_edit_storage_failed") ?? "") } })
                }
                state = TaskContentState(id: id, context: context, draft: draft, held: held, error: held ? text("task_content_held") : "")
            } catch { if state?.id == id { state = TaskContentState(id: id, context: context, error: TaskSyncError.reason(error)) } }
        }
    }
    public func report(_ id: UUID, _ message: String) { if state?.id == id { state?.error = message } }
    private func editable(_ id: UUID) -> TaskContentState? {
        guard let state, state.id == id, !state.busy, !state.complete, !state.held, state.draft?.state == .editing else { return nil }
        return state
    }
    public func change(_ id: UUID, _ transform: (inout TaskEditForm) -> Void) {
        guard var captured = editable(id), var draft = captured.draft else { return }
        transform(&draft.form); draft.form.sequence += 1; captured.draft = draft; captured.error = ""; state = captured
        do {
            if checkpoints?.offer(TaskEditCheckpoint(sequence: draft.form.sequence, changes: try draft.form.json())) != true { report(id, text("task_edit_storage_failed")) }
        } catch { report(id, text("task_edit_storage_failed")) }
    }
    public func dismiss() {
        guard var captured = state, !captured.busy else { return }
        guard let writer = checkpoints, let draft = captured.draft, !captured.complete, !captured.held else {
            checkpoints?.close(); checkpoints = nil; picker = nil; state = nil; return
        }
        captured.busy = true; state = captured
        Task {
            do {
                try await writer.flush(TaskEditCheckpoint(sequence: draft.form.sequence, changes: draft.form.json()))
                writer.close(); checkpoints = nil; picker = nil
                if state?.id == captured.id { state = nil }
            } catch { if state?.id == captured.id { state?.busy = false; state?.error = text("task_edit_storage_failed") } }
        }
    }
    public func preparePicker(_ id: UUID, ready: @escaping (TaskContentPicker) -> Void) {
        guard let captured = editable(id), let draft = captured.draft, let writer = checkpoints else { return }
        guard upload(captured.context, draft.form.target ?? draft.form.originalTarget) else { report(id, text("task_content_upload")); return }
        guard draft.form.kept.count + draft.form.files.count < 8 else { report(id, text("task_media_limit")); return }
        state?.busy = true
        Task {
            do {
                try await writer.flush(TaskEditCheckpoint(sequence: draft.form.sequence, changes: draft.form.json()))
                guard state?.id == id else { return }
                guard upload(captured.context, draft.form.target ?? draft.form.originalTarget) else { throw ZerusError.invalid(text("task_content_upload")) }
                let ticket = TaskContentPicker(draft); picker = ticket; ready(ticket)
            } catch { if state?.id == id { state?.busy = false; state?.error = TaskSyncError.reason(error) } }
        }
    }
    public func picked(_ ticket: TaskContentPicker, imports: [([Attachment]) async throws -> Attachment], finished: @escaping () -> Void = {}) {
        guard picker == ticket else { finished(); return }
        picker = nil
        guard let captured = state, let draft = captured.draft, ticket.matches(draft), let writer = checkpoints else { finished(); return }
        guard !imports.isEmpty else { state?.busy = false; finished(); return }
        Task {
            var added: [Attachment] = [], persisting = false
            defer { finished() }
            do {
                let form = draft.form
                guard upload(captured.context, form.target ?? form.originalTarget) else { throw ZerusError.invalid(text("task_content_upload")) }
                for importFile in imports {
                    added.append(try await importFile(form.files + added))
                    guard form.kept.count + form.files.count + added.count <= 8,
                          form.kept.reduce(Int64(0), { $0 + Int64($1.size) }) + (form.files + added).reduce(0, { $0 + $1.bytes }) <= AttachmentPolicy.maxTotalBytes else { throw ZerusError.invalid(text("task_media_limit")) }
                }
                guard let current = state?.draft, ticket.matches(current) else { throw ZerusError.invalid(text("task_media_picker_changed")) }
                var next = draft; next.form.files += added; next.form.sequence += 1
                persisting = true
                try await writer.flush(TaskEditCheckpoint(sequence: next.form.sequence, changes: next.form.json()))
                if state?.id == captured.id { state?.draft = next; state?.busy = false }
            } catch {
                if persisting { writer.close(); checkpoints = nil }
                if state?.id == captured.id { state?.busy = false; state?.held = persisting; state?.error = TaskSyncError.reason(error) }
            }
            if !persisting { for file in added { try? await remove(file.id, Set(draft.form.files.map(\.id))) } }
        }
    }
    public func removeFile(_ id: UUID, file: String) {
        guard let captured = editable(id), var draft = captured.draft, draft.form.files.contains(where: { $0.id == file }), let writer = checkpoints else { return }
        draft.form.files.removeAll { $0.id == file }; draft.form.sequence += 1
        state?.busy = true
        Task {
            do {
                try await writer.flush(TaskEditCheckpoint(sequence: draft.form.sequence, changes: draft.form.json()))
                if state?.id == id { state?.draft = draft; state?.busy = false }
                try await remove(file, Set(draft.form.files.map(\.id)))
            } catch { if state?.id == id { state?.busy = false; state?.error = text("task_edit_storage_failed") } }
        }
    }
    public func save(_ id: UUID) {
        guard let captured = editable(id), let draft = captured.draft, let writer = checkpoints,
              !draft.form.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        guard draft.form.files.isEmpty || upload(captured.context, draft.form.target ?? draft.form.originalTarget) else { report(id, text("task_content_upload")); return }
        state?.busy = true
        Task {
            do {
                try await writer.flush(TaskEditCheckpoint(sequence: draft.form.sequence, changes: draft.form.json()))
                var inputs: [TaskMediaInput] = []
                for file in draft.form.files { inputs.append(TaskMediaInput(name: file.name, mime: file.mime, data: try await read(file), reference: "")) }
                guard draft.form.files.isEmpty || upload(captured.context, draft.form.target ?? draft.form.originalTarget) else { throw ZerusError.invalid(text("task_content_upload")) }
                let replicas = replicas, files = files, context = captured.context, frozen = inputs, stale = text("task_content_stale")
                let task = try await replicas.run {
                    let store = replicas.store(context.key, name: context.projectName)
                    if !draft.isNew {
                        let current = try store.get(reference: draft.scope.task)
                        guard current.contentRevision == draft.revision, current.writable, [.todo, .backlog].contains(current.stage) else { throw ZerusError.invalid(stale) }
                    }
                    try draft.validate()
                    do {
                        _ = try files.begin(draft)
                        let task = try draft.apply(store, files: frozen)
                        _ = try files.completed(draft); return task
                    } catch { throw TaskContentHeld() }
                }
                writer.close(); checkpoints = nil
                if state?.id == id { state?.complete = true; state?.busy = false }
                changed(context, task.uuid)
                try await replicas.run { try files.release(draft) }
                for file in draft.form.files { try? await remove(file.id, []) }
            } catch {
                let held = error is TaskContentHeld
                if held { writer.close(); checkpoints = nil }
                if state?.id == id, state?.complete != true { state?.busy = false; state?.held = held; state?.error = held ? text("task_content_held") : TaskSyncError.reason(error) }
            }
        }
    }
    public func discard(_ id: UUID) {
        guard let captured = editable(id), let draft = captured.draft, let writer = checkpoints else { return }
        state?.busy = true
        Task {
            do {
                try await writer.flush(TaskEditCheckpoint(sequence: draft.form.sequence, changes: draft.form.json()))
                writer.close(); checkpoints = nil
                let files = files; try await replicas.run { try files.release(draft) }
                for file in draft.form.files { try? await remove(file.id, []) }
                if state?.id == id { state = nil }
            } catch { if state?.id == id { state?.busy = false; state?.held = true; state?.error = text("task_edit_storage_failed") } }
        }
    }
}
