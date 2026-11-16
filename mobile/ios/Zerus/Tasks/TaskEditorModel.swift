import Foundation
import Observation
import ZerusKit
import ZerusReplica

func taskEditText(_ key: String) -> String { NSLocalizedString(key, comment: "") }
struct TaskEditContext: Sendable {
    let connection: Connection, key: ProjectKey, projectName: String, computer: Machine
    let features: [MachineKey: Set<String>]
}
struct TaskEditorState: Identifiable {
    let id: UUID, context: TaskEditContext, task: TaskInfo
    var session: TaskEditSession?, form: TaskEditForm?, saved: TaskEditSavedDraft?
    var loading = false, busy = false, error = "", complete = false
}
struct TaskEditorPicker: Equatable {
    let id = UUID()
    let token: UUID
    let draftId: String
    let scope: TaskEditDraftScope
    let comment: Bool
}

@MainActor @Observable final class TaskEditorModel {
    private(set) var state: TaskEditorState?
    @ObservationIgnored private unowned let app: AppModel
    @ObservationIgnored private let replicas: TaskReplicas
    @ObservationIgnored private let files: TaskEditDraftFiles
    @ObservationIgnored private let assets: AttachmentStore
    @ObservationIgnored private let changed: @MainActor (TaskEditContext, String, UUID) -> Void
    @ObservationIgnored private var picker: TaskEditorPicker?
    @ObservationIgnored private var checkpoints: TaskEditCheckpoints?
    private func release(_ writer: TaskEditCheckpoints) { writer.close(); if checkpoints === writer { checkpoints = nil } }
    init(app: AppModel, replicas: TaskReplicas, files: TaskEditDraftFiles, assets: AttachmentStore, changed: @escaping @MainActor (TaskEditContext, String, UUID) -> Void) {
        self.app = app; self.replicas = replicas; self.files = files; self.assets = assets; self.changed = changed
    }
    func open(_ context: TaskEditContext, _ task: TaskInfo) {
        if state?.busy == true || state?.session != nil && state?.complete != true { return }
        checkpoints?.close(); checkpoints = nil
        if let old = state?.session { Task { await old.close() } }
        let token = UUID()
        state = TaskEditorState(id: token, context: context, task: task, loading: true)
        Task {
            do {
                let transport = try await TaskEditTransport.open(client: app.client, connection: context.connection, key: context.key, task: task.uuid, computer: context.computer, features: context.features)
                let scope = transport.scope, files = files
                if let previous = try await replicas.run({ try files.read(scope) }), previous.state == .completed {
                    try await replicas.run { try files.releaseCompleted(scope, draftId: previous.draftId) }
                }
                let session = try await TaskEditSession.open(replicas: replicas, projectName: context.projectName, transport: transport, files: files) { try TaskEditForm($0).json() }
                guard state?.id == token else { await session.close(); return }
                checkpoints = TaskEditCheckpoints(write: { value in
                    try await session.persist(sequence: value.sequence, changes: value.changes)
                }, failed: { [weak self] value, _ in
                    if self?.state?.id == token && self?.state?.form?.sequence == value.sequence { self?.state?.error = taskEditText("task_edit_storage_failed") }
                })
                state = TaskEditorState(id: token, context: context, task: task, session: session, form: TaskEditForm(session.view))
            } catch let held as TaskEditDraftHeld {
                guard state?.id == token else { return }
                state = TaskEditorState(id: token, context: context, task: task,
                    form: try? TaskEditForm.read(held.saved.changes, sequence: held.saved.changeSequence), saved: held.saved)
            } catch {
                guard state?.id == token else { return }
                state = TaskEditorState(id: token, context: context, task: task, error: TaskSyncError.reason(error))
            }
        }
    }
    func reportError(_ token: UUID, _ message: String) {
        if state?.id == token { state?.error = message.isEmpty ? taskEditText("task_edit_unavailable") : message }
    }
    func dismiss() {
        guard let captured = state, !captured.busy else { return }
        guard let session = captured.session, let form = captured.form, !captured.complete else { checkpoints?.close(); checkpoints = nil; state = nil; return }
        guard let writer = checkpoints else { state?.error = taskEditText("task_edit_storage_failed"); return }
        state?.busy = true
        Task {
            do {
                try await writer.flush(TaskEditCheckpoint(sequence: form.sequence, changes: form.json()))
                release(writer); await session.close()
                if state?.id == captured.id { state = nil }
            } catch {
                if state?.id == captured.id { state?.busy = false; state?.error = taskEditText("task_edit_storage_failed") }
            }
        }
    }
    func change(_ token: UUID, _ transform: (inout TaskEditForm) -> Void) {
        guard var captured = state, captured.id == token, !captured.busy, !captured.complete, captured.session != nil, var form = captured.form else { return }
        transform(&form); form.sequence += 1; captured.form = form; captured.error = ""; state = captured
        do {
            if checkpoints?.offer(TaskEditCheckpoint(sequence: form.sequence, changes: try form.json())) != true { state?.error = taskEditText("task_edit_storage_failed") }
        } catch { state?.error = taskEditText("task_edit_storage_failed") }
    }
    func save(_ token: UUID) {
        guard var captured = state, captured.id == token, !captured.busy, !captured.complete, let session = captured.session, let form = captured.form, form.changed(session.view) else { return }
        guard let writer = checkpoints else { state?.error = taskEditText("task_edit_storage_failed"); return }
        captured.busy = true; captured.error = ""; state = captured
        Task {
            do {
                try await writer.flush(TaskEditCheckpoint(sequence: form.sequence, changes: form.json()))
                let input = try await form.input(view: session.view, assets: assets)
                try await session.save(sequence: form.sequence, changes: form.json(), input: input)
                release(writer)
                if state?.id == token { state?.complete = true; state?.busy = false }
                changed(captured.context, captured.task.uuid, captured.id)
            } catch {
                let scope = session.scope, files = files
                let saved = try? await replicas.run { try files.read(scope) }
                let editable = await session.editable
                if state?.id == token {
                    let held = !editable || saved.map({ [.pending, .unknown, .refused, .frozen].contains($0.state) }) == true
                    state?.busy = false; state?.saved = held ? saved : nil; state?.error = TaskSyncError.reason(error)
                    if held { release(writer); state?.session = nil }
                }
            }
        }
    }
    /// Freeze picker identity and save current text before asking a document provider.
    func preparePicker(_ token: UUID, comment: Bool, ready: @escaping (TaskEditorPicker) -> Void) {
        guard var captured = state, captured.id == token, !captured.busy, let session = captured.session, let form = captured.form else { return }
        guard form.files.count + form.commentFiles.count < 8, comment || form.kept.count + form.files.count < 8 else {
            state?.error = taskEditText("task_media_limit"); return
        }
        guard let writer = checkpoints else { state?.error = taskEditText("task_edit_storage_failed"); return }
        captured.busy = true; state = captured
        Task {
            do {
                try await writer.flush(TaskEditCheckpoint(sequence: form.sequence, changes: form.json()))
                guard state?.id == token else { return }
                let ticket = TaskEditorPicker(token: token, draftId: session.original.draftId, scope: session.scope, comment: comment)
                picker = ticket; ready(ticket)
            } catch { if state?.id == token { state?.busy = false; state?.error = taskEditText("task_edit_storage_failed") } }
        }
    }
    func picked(_ ticket: TaskEditorPicker, sources: [PickedSource]) {
        guard picker == ticket else { ConversationModel.discardTemporary(sources); return }
        picker = nil
        guard let captured = state, captured.id == ticket.token, let session = captured.session,
              session.original.draftId == ticket.draftId, session.scope == ticket.scope, let form = captured.form else {
            ConversationModel.discardTemporary(sources); return
        }
        guard !sources.isEmpty else { state?.busy = false; return }
        guard let writer = checkpoints else { ConversationModel.discardTemporary(sources); state?.error = taskEditText("task_edit_storage_failed"); return }
        let assets = assets
        Task {
            var added: [Attachment] = [], persisting = false
            defer { ConversationModel.discardTemporary(sources) }
            do {
                for source in sources {
                    let current = form.files + form.commentFiles + added
                    let file = try await replicas.run { try ConversationModel.importSource(source, store: assets, current: current) }
                    added.append(file)
                    if !ticket.comment && (form.kept.count + form.files.count + added.count > 8 || form.kept.reduce(Int64(0), { $0 + Int64($1.size) }) + form.files.reduce(0, { $0 + $1.bytes }) + added.reduce(0, { $0 + $1.bytes }) > AttachmentPolicy.maxTotalBytes) {
                        throw ZerusError.invalid(taskEditText("task_media_limit"))
                    }
                }
                var next = form
                if ticket.comment { next.commentFiles += added } else { next.files += added }
                next.sequence += 1; persisting = true
                try await writer.flush(TaskEditCheckpoint(sequence: next.sequence, changes: next.json()))
                if state?.id == ticket.token { state?.form = next; state?.busy = false }
            } catch {
                if persisting { release(writer); await session.close() }
                else { for file in added { _ = try? await replicas.run { try assets.remove(file.id, protected: Set((form.files + form.commentFiles).map(\.id))) } } }
                if state?.id == ticket.token { state?.busy = false; state?.error = TaskSyncError.reason(error); if persisting { state?.session = nil } }
            }
        }
    }
    func discard(_ token: UUID) {
        guard let captured = state, captured.id == token, !captured.busy, let saved = captured.saved else { return }
        let files = files
        Task {
            do {
                try await replicas.run { try files.discardUnsubmitted(saved.scope, draftId: saved.draftId) }
                guard state?.id == token else { return }; state = nil; open(captured.context, captured.task)
            } catch { if state?.id == token { state?.error = taskEditText("task_edit_draft_kept") } }
        }
    }
    func preview(_ scope: TaskEditContext, _ task: String, _ file: TaskMediaInfo) {
        let replicas = replicas
        app.viewer.openTask(key: JSON([scope.key.connectionId, scope.key.swarmId, scope.key.id, task, "", file.token]).compact, file: file) {
            try await replicas.run {
                let store = replicas.store(scope.key, name: scope.projectName), metadata = try store.media(reference: task, entry: nil)
                guard let index = metadata.firstIndex(where: { $0.token == file.token }) else { throw ZerusError.invalid("Refresh this task to open its media.") }
                return try store.readMedia(reference: task, entry: nil, index: UInt32(index), expectedToken: file.token)
            }
        }
    }
    func preview(_ file: Attachment) { app.viewer.open(nil, file.displayed()) }
}
