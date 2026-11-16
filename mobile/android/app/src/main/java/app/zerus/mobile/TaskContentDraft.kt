package app.zerus.mobile

import app.zerus.mobile.replica.TaskEditView
import app.zerus.mobile.replica.TaskInfo
import app.zerus.mobile.replica.TaskMediaInput
import app.zerus.mobile.replica.TaskPosition
import app.zerus.mobile.replica.TaskStage
import app.zerus.mobile.replica.TaskStore
import app.zerus.mobile.replica.TaskTarget
import java.util.UUID
import org.json.JSONObject

/** Local legacy composition, separate from qualified native edit handles and request journals. */
internal data class TaskContentDraft(
    val scope: TaskEditDraftScope, val id: String, val computer: String, val revision: String?,
    val stage: TaskStage, val form: TaskEditForm, val state: State = State.Editing,
) {
    enum class State { Editing, Applying, Completed }
    val isNew get() = revision == null
    fun apply(store: TaskStore, files: List<TaskMediaInput>): TaskInfo {
        if (isNew) return store.addWithMedia(form.text, stage, form.target ?: form.originalTarget, TaskPosition.End, files)
        store.editContent(scope.task, form.text, form.kept.map { it.token }, files, checkNotNull(revision))
        form.target?.let { store.setTarget(scope.task, it) }
        return store.get(scope.task)
    }
    fun validate() {
        scope.validate(); check(UUID.fromString(id).toString() == id)
        check(computer.isNotEmpty() && computer.toByteArray().size <= 512 && computer.none(Char::isISOControl))
        check(stage in setOf(TaskStage.TODO, TaskStage.BACKLOG) && form.sequence >= 0 && revision != "")
        check(form.title.isEmpty() && form.rank == null && form.comment.isEmpty() && form.commentFiles.isEmpty())
        check(form.kept.size + form.files.size <= 8 && form.kept.map { it.token }.distinct().size == form.kept.size)
        check(form.kept.all { it.size <= AttachmentPolicy.MAX_FILE_BYTES.toULong() })
        check(form.kept.sumOf { it.size.toLong() } + AttachmentPolicy.selection(form.files) <= AttachmentPolicy.MAX_TOTAL_BYTES)
        check(!isNew || scope.task == NEW_SLOT && form.kept.isEmpty())
    }
    fun json() = JSONObject().put("version", 1).put("scope", scope.json()).put("id", id).put("computer", computer)
        .put("revision", revision ?: JSONObject.NULL).put("stage", stage.name).put("form", form.json())
        .put("sequence", form.sequence).put("state", state.name)
    companion object {
        const val NEW_SLOT = "00000000-0000-0000-0000-000000000000"
        fun parse(value: JSONObject): TaskContentDraft {
            WorkspaceProtocol.exactFields(value, setOf("version", "scope", "id", "computer", "revision", "stage", "form", "sequence", "state"))
            check(WorkspaceProtocol.number(value, "version") == 1L)
            return TaskContentDraft(TaskEditDraftScope.parse(value.getJSONObject("scope")), WorkspaceProtocol.string(value, "id"),
                WorkspaceProtocol.string(value, "computer"), if (value.isNull("revision")) null else WorkspaceProtocol.string(value, "revision"),
                TaskStage.valueOf(WorkspaceProtocol.string(value, "stage")), TaskEditForm.read(WorkspaceProtocol.string(value, "form"),
                    WorkspaceProtocol.number(value, "sequence")), State.valueOf(WorkspaceProtocol.string(value, "state"))).also { it.validate() }
        }

        fun existing(key: ProjectKey, computer: String, task: TaskInfo) = TaskContentDraft(
            TaskEditDraftScope(key.connectionId, key.swarmId, key.id, task.uuid), UUID.randomUUID().toString(), computer,
            task.contentRevision, task.stage,
            TaskEditForm.initial(TaskEditView(key.id, task.uuid, "", task.text, task.target, task.rank, task.attachments)))
        fun new(key: ProjectKey, computer: String, stage: TaskStage, target: TaskTarget, text: String = ""): TaskContentDraft {
            val id = UUID.randomUUID().toString()
            return TaskContentDraft(TaskEditDraftScope(key.connectionId, key.swarmId, key.id, NEW_SLOT), id, computer, null, stage,
                TaskEditForm.initial(TaskEditView(key.id, NEW_SLOT, "", text, target, "", emptyList())))
        }
    }
}

/** Consumed once by the controller; all captured fields must still match after asynchronous import. */
internal data class TaskContentPicker(val token: String = UUID.randomUUID().toString(), val draft: TaskContentDraft) {
    fun matches(value: TaskContentDraft) = value == draft && value.state == TaskContentDraft.State.Editing
}
