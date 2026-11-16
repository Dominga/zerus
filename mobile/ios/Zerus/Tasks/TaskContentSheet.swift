import SwiftUI
import UniformTypeIdentifiers
import ZerusKit
import ZerusReplica

struct TaskContentSheet: View {
    let model: TaskContentController
    let tasks: TasksModel
    @State private var ticket: TaskContentPicker?
    @State private var picking = false
    @State private var discard = false
    var body: some View {
        NavigationStack {
            if let state = model.state {
                Form {
                    Section {
                        Text(taskEditText(state.draft?.isNew == true ? "task_content_new" : "task_content_open")).font(.headline)
                        Text(state.context.projectName + " / " + state.context.computer.name).font(.caption)
                        if state.loading { ProgressView(taskEditText("task_edit_prepare")) }
                    }
                    if let draft = state.draft {
                        let live = !state.busy && !state.loading && !state.complete && !state.held
                        Section {
                            TextField(taskEditText("task_edit_requirement"), text: Binding(get: { model.state?.draft?.form.text ?? "" }, set: { value in model.change(state.id) { $0.text = value } }), axis: .vertical).lineLimit(3...12)
                                .disabled(!live)
                            ForEach(draft.form.kept, id: \.token) { file in
                                HStack {
                                    TaskMediaButton(key: state.context.key, task: draft.scope.task, file: file) {
                                        tasks.openTaskMedia(state.context.key, task: draft.scope.task, file: file)
                                    }
                                    Spacer()
                                    Button(role: .destructive) { model.change(state.id) { $0.kept.removeAll { $0.token == file.token } } } label: { Image(systemName: "minus.circle") }.disabled(!live)
                                }
                            }
                            ForEach(draft.form.files, id: \.id) { file in
                                HStack {
                                    Button(file.name) { tasks.previewContentFile(file) }; Spacer()
                                    Button(role: .destructive) { model.removeFile(state.id, file: file.id) } label: { Image(systemName: "minus.circle") }.disabled(!live)
                                }
                            }
                            Button(taskEditText("task_media_attach")) { model.preparePicker(state.id) { original in ticket = original; picking = true } }.disabled(!live)
                            Menu(taskEditText("task_edit_target") + ": " + tasks.contentTargetLabel(state)) {
                                Button(taskEditText("task_edit_keep_target")) { model.change(state.id) { $0.target = nil } }
                                Button(taskEditText("task_edit_any")) { model.change(state.id) { $0.target = .anySession } }
                                ForEach(tasks.editorSessions(state.context.key)) { session in
                                    Button(session.title.isEmpty ? session.target.session : session.title) { tasks.chooseContentSession(state.id, session) }
                                }
                            }.disabled(!live)
                            if state.complete { Text(taskEditText("task_edit_saved")) }
                            else {
                                Button(taskEditText("task_edit_save")) { model.save(state.id) }.disabled(!live || draft.form.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                                if live { Button(taskEditText("task_edit_discard"), role: .destructive) { discard = true } }
                            }
                        }
                    }
                    if !state.error.isEmpty { Section { Text(state.error).foregroundStyle(.red) } }
                }
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button(taskEditText("task_edit_close")) { model.dismiss() }.disabled(state.busy) } }
                .interactiveDismissDisabled()
                .confirmationDialog(taskEditText("task_edit_discard_question"), isPresented: $discard, titleVisibility: .visible) {
                    Button(taskEditText("task_edit_discard"), role: .destructive) { model.discard(state.id) }
                    Button(taskEditText("task_edit_cancel"), role: .cancel) {}
                }
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            guard let original = ticket else { return }; ticket = nil
            let sources: [PickedSource]
            if case .success(let urls) = result {
                sources = urls.map { .file($0, name: $0.lastPathComponent, mime: PickedTypes.mime($0), scoped: true, temporary: false) }
            } else { sources = [] }
            tasks.contentPicked(original, sources: sources)
        } onCancellation: {
            if let original = ticket { ticket = nil; tasks.contentPicked(original, sources: []) }
        }
    }
}

/// One immutable entry's bounded metadata, dropped on selection change. Bodies are read only on a tap.
struct TaskCommentFiles: View {
    let tasks: TasksModel, key: ProjectKey, task: String, entry: String
    @State private var files: [TaskMediaInfo]?
    @State private var error = ""
    var body: some View {
        VStack(alignment: .leading) {
            if let files {
                ForEach(files, id: \.token) { file in TaskMediaButton(key: key, task: task, file: file, entry: entry) { tasks.openTaskMedia(key, task: task, file: file, entry: entry) } }
            } else if error.isEmpty { ProgressView() }
            if !error.isEmpty { Text(error).font(.caption).foregroundStyle(.red) }
        }
        .task(id: entry) {
            files = nil; error = ""
            do {
                let selected = try await tasks.commentFiles(key, task: task, entry: entry)
                if !Task.isCancelled { files = selected }
            } catch { if !Task.isCancelled { self.error = TaskSyncError.reason(error) } }
        }
    }
}
