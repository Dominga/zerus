import SwiftUI
import UniformTypeIdentifiers
import ZerusKit
import ZerusReplica

struct TaskEditorSheet: View {
    let model: TaskEditorModel
    let tasks: TasksModel
    @State private var ticket: TaskEditorPicker?
    @State private var picking = false
    @State private var discard = false
    var body: some View {
        NavigationStack {
            if let state = model.state {
                Form {
                    Section {
                        Text(state.task.id).font(.headline)
                        Text(state.context.computer.name).font(.caption)
                        if state.loading { ProgressView(taskEditText("task_edit_prepare")) }
                        if state.complete || state.saved?.state == .completed { Text(taskEditText("task_edit_saved")) }
                        if let saved = state.saved {
                            Text(taskEditText("task_edit_saved_draft")).font(.headline)
                            Text(taskEditText([.pending, .unknown].contains(saved.state) ? "task_edit_unknown" : "task_edit_draft_kept"))
                        }
                    }
                    if let form = state.form {
                        if let session = state.session, state.saved == nil, !state.complete {
                            if [.todo, .backlog].contains(state.task.stage) {
                                Section {
                                    TextField(taskEditText("task_edit_title"), text: field(state, \.title))
                                    TextField(taskEditText("task_edit_requirement"), text: field(state, \.text), axis: .vertical).lineLimit(3...12)
                                    ForEach(form.kept, id: \.token) { file in
                                        HStack {
                                            TaskMediaButton(key: state.context.key, task: state.task.uuid, file: file) { model.preview(state.context, state.task.uuid, file) }
                                            Spacer()
                                            Button(role: .destructive) { model.change(state.id) { $0.kept.removeAll { $0.token == file.token } } } label: { Image(systemName: "minus.circle") }
                                        }
                                    }
                                    localFiles(state, form.files, comment: false)
                                    Button(taskEditText("task_media_attach")) { attach(state, comment: false) }
                                    Menu(taskEditText("task_edit_target")) {
                                        Button(taskEditText("task_edit_keep_target")) { model.change(state.id) { $0.target = nil } }
                                        Button(taskEditText("task_edit_any")) { model.change(state.id) { $0.target = .anySession } }
                                        ForEach(tasks.editorSessions(state.context.key)) { session in
                                            Button(session.title.isEmpty ? session.target.session : session.title) { tasks.chooseEditorSession(state.id, session) }
                                        }
                                    }
                                    if state.task.stage == .todo { Button(taskEditText("task_edit_top")) { tasks.moveEditorTop(state.id) } }
                                }
                            } else { Section { Text(form.title); Text(form.text) } }
                            Section {
                                TextField(taskEditText("task_edit_comment"), text: field(state, \.comment), axis: .vertical).lineLimit(2...8)
                                localFiles(state, form.commentFiles, comment: true)
                                Button(taskEditText("task_media_comment_attach")) { attach(state, comment: true) }
                                Button(taskEditText(state.busy ? "task_edit_saving" : "task_edit_save")) { model.save(state.id) }
                                    .disabled(state.busy || !form.changed(session.view))
                            }
                            .disabled(state.busy)
                        } else {
                            Section {
                                Text(form.title).font(.headline); Text(form.text)
                                if !form.comment.isEmpty { Text(form.comment) }
                                ForEach(form.kept, id: \.token) { file in TaskMediaButton(key: state.context.key, task: state.task.uuid, file: file) { model.preview(state.context, state.task.uuid, file) } }
                                ForEach(form.files + form.commentFiles, id: \.id) { file in Button(file.name) { model.preview(file) } }
                            }
                        }
                    }
                    if !state.error.isEmpty { Section { Text(state.error).foregroundStyle(.red) } }
                    if let saved = state.saved, [.editing, .refused, .frozen].contains(saved.state) {
                        Section { Button(taskEditText("task_edit_discard"), role: .destructive) { discard = true } }
                    }
                }
                .disabled(state.busy)
                .navigationTitle(taskEditText("task_edit_open"))
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button(taskEditText("task_edit_close")) { model.dismiss() }.disabled(state.busy) } }
                .interactiveDismissDisabled(state.busy)
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
            model.picked(original, sources: sources)
        } onCancellation: {
            if let original = ticket { ticket = nil; model.picked(original, sources: []) }
        }
    }
    private func field(_ state: TaskEditorState, _ path: WritableKeyPath<TaskEditForm, String>) -> Binding<String> {
        Binding(get: { model.state?.id == state.id ? model.state?.form?[keyPath: path] ?? "" : "" },
                set: { text in model.change(state.id) { $0[keyPath: path] = text } })
    }
    private func attach(_ state: TaskEditorState, comment: Bool) {
        model.preparePicker(state.id, comment: comment) { original in ticket = original; picking = true }
    }
    private func localFiles(_ state: TaskEditorState, _ files: [Attachment], comment: Bool) -> some View {
        ForEach(files, id: \.id) { file in
            HStack {
                Button(file.name) { model.preview(file) }; Spacer()
                Button(role: .destructive) { model.change(state.id) { form in
                    if comment { form.commentFiles.removeAll { $0.id == file.id } } else { form.files.removeAll { $0.id == file.id } }
                } } label: { Image(systemName: "minus.circle") }
            }
        }
    }
}

struct TaskMediaButton: View {
    @Environment(AppModel.self) private var app
    let key: ProjectKey, task: String, file: TaskMediaInfo
    let action: () -> Void
    var body: some View {
        let state = app.viewer.rows[JSON([key.connectionId, key.swarmId, key.id, task, file.token]).compact]
        VStack(alignment: .leading) {
            Button(action: action) { HStack { Text(file.name); if state?.loading == true { ProgressView() } } }.disabled(state?.loading == true)
            if let error = state?.error, !error.isEmpty { Text(error).font(.caption).foregroundStyle(.red) }
        }
    }
}
