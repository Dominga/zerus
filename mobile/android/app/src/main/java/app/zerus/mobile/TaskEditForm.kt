package app.zerus.mobile

import app.zerus.mobile.replica.SessionRef
import app.zerus.mobile.replica.TaskEditView
import app.zerus.mobile.replica.TaskMediaInfo
import app.zerus.mobile.replica.TaskMediaInput
import app.zerus.mobile.replica.TaskTarget
import org.json.JSONArray
import org.json.JSONObject
import java.util.Base64

/** Mutable form values are copied into one immutable save snapshot. No route is stored here. */
internal data class TaskEditForm(
    val title: String, val text: String, val kept: List<TaskMediaInfo>, val files: List<Attachment> = emptyList(),
    val target: TaskTarget? = null, val rank: String? = null, val comment: String = "", val commentFiles: List<Attachment> = emptyList(),
    val sequence: Long = 0, val originalTarget: TaskTarget = TaskTarget.AnySession, val originalRank: String = ""
) {
    fun changed(view: TaskEditView) = title != view.title || text != view.text || kept.map { it.token } != view.attachments.map { it.token } ||
        files.isNotEmpty() || target != null || rank != null || comment.isNotBlank() || commentFiles.isNotEmpty()
    fun json(): String = JSONObject().put("version", 1).put("title", title).put("text", text)
        .put("kept", JSONArray(kept.map { value -> JSONObject().put("name", value.name).put("mime", value.mime).put("reference", value.reference)
            .put("size", value.size.toLong()).put("sha256", value.sha256).put("token", value.token) }))
        .put("files", JSONArray(files.map(Attachment::toJson))).put("comment", comment).put("comment_files", JSONArray(commentFiles.map(Attachment::toJson)))
        .put("target", targetJson(target)).put("original_target", targetJson(originalTarget)).put("original_rank", originalRank)
        .put("rank", rank ?: JSONObject.NULL).toString()
    private fun targetJson(target: TaskTarget?): Any = when (val picked = target) {
            null -> JSONObject.NULL
            TaskTarget.AnySession -> JSONObject().put("kind", "any")
            is TaskTarget.Session -> JSONObject().put("kind", "session").put("machine", picked.session.machine)
                .put("session", picked.session.session).put("session_id", picked.session.sessionId)
        }
    suspend fun input(view: TaskEditView, store: AttachmentStore): TaskEditInput {
        suspend fun media(files: List<Attachment>) = files.map { file ->
            val payload = store.readForWire(file)
            TaskMediaInput(file.name, file.mime, Base64.getDecoder().decode(payload.getString("data_base64")), "")
        }
        val content = if (title != view.title || text != view.text || kept.map { it.token } != view.attachments.map { it.token } || files.isNotEmpty())
            TaskEditContent(title, text, kept.map { it.token }, media(files)) else null
        return TaskEditInput(content, target, rank, comment, media(commentFiles))
    }
    companion object {
        fun initial(view: TaskEditView) = TaskEditForm(view.title, view.text, view.attachments.toList(), originalTarget = view.target, originalRank = view.rank)
        /** Restore presentation only. A saved form never creates a live FFI handle. */
        fun read(text: String, sequence: Long): TaskEditForm {
            val value = JSONObject(text)
            WorkspaceProtocol.exactFields(value, setOf("version", "title", "text", "kept", "files", "target", "rank", "comment", "comment_files", "original_target", "original_rank"))
            check(WorkspaceProtocol.number(value, "version") == 1L)
            val kept = value.getJSONArray("kept"); val files = value.getJSONArray("files"); val comments = value.getJSONArray("comment_files")
            check(kept.length() <= 8 && files.length() + comments.length() <= 8)
            fun target(name: String): TaskTarget? = if (value.isNull(name)) null else value.getJSONObject(name).let { raw ->
                when (WorkspaceProtocol.string(raw, "kind")) {
                    "any" -> { WorkspaceProtocol.exactFields(raw, setOf("kind")); TaskTarget.AnySession }
                    "session" -> {
                        WorkspaceProtocol.exactFields(raw, setOf("kind", "machine", "session", "session_id"))
                        TaskTarget.Session(SessionRef(WorkspaceProtocol.string(raw, "machine"), WorkspaceProtocol.string(raw, "session"), WorkspaceProtocol.string(raw, "session_id")))
                    }
                    else -> error("This saved task target is unavailable. Its draft was kept.")
                }
            }
            return TaskEditForm(WorkspaceProtocol.string(value, "title"), WorkspaceProtocol.string(value, "text"),
                (0 until kept.length()).map { index -> kept.getJSONObject(index).let { raw ->
                    WorkspaceProtocol.exactFields(raw, setOf("name", "mime", "reference", "size", "sha256", "token"))
                    TaskMediaInfo(WorkspaceProtocol.string(raw, "name"), WorkspaceProtocol.string(raw, "mime"), WorkspaceProtocol.string(raw, "reference"),
                        WorkspaceProtocol.number(raw, "size").toULong(), WorkspaceProtocol.string(raw, "sha256"), WorkspaceProtocol.string(raw, "token"))
                } }, (0 until files.length()).map { Attachment.fromJson(files.getJSONObject(it)) }, target("target"), if (value.isNull("rank")) null else WorkspaceProtocol.string(value, "rank"),
                WorkspaceProtocol.string(value, "comment"), (0 until comments.length()).map { Attachment.fromJson(comments.getJSONObject(it)) }, sequence,
                checkNotNull(target("original_target")), WorkspaceProtocol.string(value, "original_rank"))
        }
    }
}
