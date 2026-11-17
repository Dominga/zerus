import SwiftUI
import ZerusKit
import ZerusReplica

/// One history line, as the desktop writes it: who, when and what happened.
private func historyLine(_ entry: TaskLogEntry) -> (who: String, what: String) {
    let who: String = switch entry.actor {
    case .agent(let session): session.session
    case .person: "You"
    }
    let stage: String = switch entry.text {
    case "backlog": "Backlog"
    case "todo": "To do"
    case "active": "In progress"
    case "review": "In review"
    case "done": "Done"
    default: entry.text
    }
    let what: String = switch entry.kind {
    case "created": "created the task"
    case "edited": "edited the prompt"
    case "stage": "moved it to \(stage)"
    case "target": "changed the session: \(entry.text)"
    case "sent": "sent it to \(entry.session?.session ?? "")"
    case "report": "reported: \(entry.text)"
    case "cancelled": "cancelled it"
    case "comment": "commented: \(entry.text)"
    default: entry.text.isEmpty ? entry.kind : entry.text
    }
    let when = Date(timeIntervalSince1970: TimeInterval(entry.at) / 1000).formatted(.dateTime.day().month(.abbreviated).hour().minute())
    return ("\(who)  \(when)", what)
}

/// The task settings: the prompt, the session, the stage's actions, the history and a comment.
struct TaskSettingsSheet: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    let key: ProjectKey
    let uuid: String
    let onCancel: (TaskInfo) -> Void
    @State private var original: TaskInfo?
    @State private var comment = ""
    @State private var commentEntry: String?

    var body: some View {
        let tasks = app.tasks!
        NavigationStack {
            if let task = (tasks.tasksKey == key ? tasks.taskListing.first(where: { $0.uuid == uuid }) : original) {
                content(tasks, task).onAppear { if original == nil { original = task } }
            } else {
                Text("This task is no longer here.").foregroundStyle(Theme.muted)
            }
        }
        .sheet(isPresented: Binding(get: { app.tasks?.taskEditor?.state != nil }, set: { if !$0 { app.tasks?.taskEditor?.dismiss() } })) {
            if let tasks = app.tasks, let editor = tasks.taskEditor { TaskEditorSheet(model: editor, tasks: tasks) }
        }
        .sheet(isPresented: Binding(get: { app.tasks?.taskContent?.state != nil }, set: { if !$0 { app.tasks?.taskContent?.dismiss() } })) {
            if let tasks = app.tasks, let editor = tasks.taskContent { TaskContentSheet(model: editor, tasks: tasks) }
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
        .presentationBackground(TasksColors.sheet)
    }

    private func content(_ tasks: TasksModel, _ task: TaskInfo) -> some View {
        let tab = TasksPresentation.tab(task.stage)
        let editable = task.writable && [.todo, .backlog].contains(task.stage)
        let draft = task
        let actions = (TasksPresentation.cards([draft], tab: tab, chosen: tasks.displayedTaskChosen, onlyThisSession: false,
                                               send: tasks.sendRoute, cancel: tasks.cancelRoute, current: tasks.taskCurrentSession).first?.actions ?? [])
            .filter { $0.action != .edit }
        return ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 8) {
                    Image(systemName: tab.symbol).foregroundStyle(Theme.muted)
                    Text(task.id).font(.headline)
                    Spacer()
                    Text(tab.label).font(.caption).foregroundStyle(Theme.muted)
                }
                Text(task.text).textSelection(.enabled)
                Text("Session: \(tasks.taskSessionLabel(task))").font(.subheadline).foregroundStyle(Theme.muted)
                if editable {
                    Button(taskEditText("task_content_open")) { tasks.openTaskContent(key, task) }.buttonStyle(.bordered)
                }
                let note = TasksPresentation.note(task)
                if !note.isEmpty { Text(note).font(.footnote).foregroundStyle(Theme.muted) }
                SavedExecutionSection(state: SavedExecutionPresentation.section(task, projectId: tasks.tasksKey?.id, stale: tasks.taskExecutionStale) {
                    Date(timeIntervalSince1970: TimeInterval($0) / 1000).formatted(.dateTime.day().month(.abbreviated).hour().minute())
                })
                if task.schema == 8 { Button(taskEditText("task_edit_open")) { tasks.openTaskEditor(task) }.buttonStyle(.bordered) }
                ForEach(task.attachments, id: \.token) { file in
                    TaskMediaButton(key: key, task: task.uuid, file: file) { tasks.openTaskMedia(key, task: task.uuid, file: file) }
                }
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(actions, id: \.action) { state in
                        Button {
                            guard tasks.tasksKey == key else { return }
                            if state.action == .cancel {
                                onCancel(task)
                                dismiss()
                            } else {
                                if tasks.tasksKey == key { tasks.taskAction(task, state.action) }
                                dismiss()
                            }
                        } label: { Label(state.action.label, systemImage: state.action.symbol) }
                            .buttonStyle(.bordered)
                            .disabled(!state.enabled)
                        if !state.enabled && !state.reason.isEmpty {
                            Text(state.reason).font(.caption).foregroundStyle(Theme.muted)
                        }
                    }
                }
                Divider().overlay(Theme.outline)
                Text("History").font(.subheadline.weight(.semibold))
                ForEach(Array(task.log.enumerated()), id: \.offset) { _, entry in
                    let line = historyLine(entry)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(line.who).font(.caption).foregroundStyle(Theme.muted)
                        Text(line.what).font(.footnote).textSelection(.enabled)
                        if entry.attachmentCount > 0 && !entry.entryId.isEmpty {
                            Button(taskEditText(commentEntry == entry.entryId ? "task_content_comment_close" : "task_content_comment_files")) {
                                commentEntry = commentEntry == entry.entryId ? nil : entry.entryId
                            }
                            if commentEntry == entry.entryId { TaskCommentFiles(tasks: tasks, key: key, task: task.uuid, entry: entry.entryId).id(entry.entryId) }
                        }
                    }
                }
                if task.writable {
                    HStack(alignment: .bottom, spacing: 8) {
                        TextField("Comment", text: $comment, axis: .vertical)
                            .lineLimit(1...4)
                            .padding(10)
                            .background(Theme.background, in: RoundedRectangle(cornerRadius: 10))
                            .overlay { RoundedRectangle(cornerRadius: 10).stroke(Theme.outline) }
                        Button("Add") {
                            guard tasks.tasksKey == key else { return }
                            tasks.commentTask(task.uuid, comment.trimmingCharacters(in: .whitespacesAndNewlines))
                            comment = ""
                        }
                        .disabled(comment.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                }
            }
            .padding(20)
        }
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }

        }
    }
}

/// Any session or one of the project's sessions, for a task in To do or Backlog.
struct TaskSessionSheet: View {
    @Environment(AppModel.self) private var app
    let key: ProjectKey
    let onPick: (TaskTarget) -> Void

    var body: some View {
        NavigationStack {
            List {
                Button("Any session") { onPick(.anySession) }
                ForEach(app.tasks.taskChips(key).filter { !$0.session.machineId.isEmpty }, id: \.session) { chip in
                    Button {
                        onPick(.session(session: SessionRef(machine: chip.session.machineId, session: chip.session.name, sessionId: chip.session.sessionId)))
                    } label: {
                        HStack(spacing: 8) {
                            StatusDot(dot: chip.dot)
                            Text(TasksPresentation.shortName(chip.session.name))
                        }
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .navigationTitle("Session")
            .navigationBarTitleDisplayMode(.inline)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .presentationBackground(TasksColors.sheet)
    }
}

/// The ⋯ chip: the sessions of every project; one of another project shows that project's tasks.
struct AllSessionsSheet: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let tasks = app.tasks!
        NavigationStack {
            List {
                ForEach(app.projects.filter(\.catalogued), id: \.key) { project in
                    let chips = tasks.taskChips(project.key)
                    if !chips.isEmpty {
                        Section(project.name) {
                            ForEach(chips, id: \.session) { chip in
                                Button {
                                    tasks.chooseTaskSession(project.key, chip.session)
                                    dismiss()
                                } label: {
                                    HStack(spacing: 8) {
                                        StatusDot(dot: chip.dot)
                                        Text(TasksPresentation.shortName(chip.session.name))
                                    }
                                }
                            }
                        }
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .navigationTitle("Sessions")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .presentationBackground(TasksColors.sheet)
    }
}
