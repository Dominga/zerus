package app.zerus.mobile

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.zerus.mobile.replica.TaskStage
import app.zerus.mobile.replica.TaskTarget
import app.zerus.mobile.replica.TaskMediaInput
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.UUID

internal data class TaskContentState(val token: String, val context: TaskEditContext, val draft: TaskContentDraft?,
    val busy: Boolean = false, val loading: Boolean = false, val complete: Boolean = false, val held: Boolean = false, val error: String = "")

/** A local content editor. Selection is never read by an in-flight save, picker or completion. */
internal class TaskContentModel(private val text: (Int) -> String, private val scope: CoroutineScope,
    private val replicas: TaskReplicas, private val files: TaskContentDraftFiles, private val read: suspend (Attachment) -> ByteArray,
    private val remove: suspend (String, Set<String>) -> Unit,
    private val upload: (TaskEditContext, TaskTarget) -> Boolean,
    private val changed: (TaskEditContext, String) -> Unit) {
    var state by mutableStateOf<TaskContentState?>(null); private set
    private var checkpoints: TaskEditCheckpoints? = null
    private var pendingPicker: TaskContentPicker? = null
    fun open(context: TaskEditContext, proposed: TaskContentDraft) {
        if (state != null) return
        val token = UUID.randomUUID().toString()
        state = TaskContentState(token, context, null, loading = true)
        scope.launch {
            try {
                proposed.validate()
                check(proposed.scope.connectionId == context.key.connectionId && proposed.scope.swarmId == context.key.swarmId &&
                    proposed.scope.project == context.key.id && context.connection.id == context.key.connectionId &&
                    context.computer.connectionId == context.key.connectionId && proposed.computer == context.computer.id) { text(R.string.task_content_stale) }
                val draft = withContext(Dispatchers.IO) {
                    val saved = files.read(proposed.scope, proposed.isNew)
                    if (saved?.state == TaskContentDraft.State.Completed) files.release(saved)
                    saved?.takeUnless { it.state == TaskContentDraft.State.Completed } ?: files.create(proposed)
                }
                if (state?.token != token) return@launch
                val held = draft.state != TaskContentDraft.State.Editing || draft.computer != context.computer.id
                if (!held) checkpoints = TaskEditCheckpoints(scope, write = { value ->
                    withContext(Dispatchers.IO) { files.checkpoint(draft, value.sequence, value.changes) }
                }, failed = { _, _ -> if (state?.token == token) report(token, text(R.string.task_edit_storage_failed)) })
                state = TaskContentState(token, context, draft, held = held,
                    error = if (held) text(R.string.task_content_held) else "")
            } catch (error: CancellationException) { throw error }
            catch (error: Exception) { if (state?.token == token) state = TaskContentState(token, context, null, error = error.message.orEmpty()) }
        }
    }
    fun report(token: String, message: String) { if (state?.token == token) state = state?.copy(error = message) }
    fun change(token: String, transform: (TaskEditForm) -> TaskEditForm) {
        val captured = editable(token) ?: return
        val draft = captured.draft!!
        val form = transform(draft.form).copy(sequence = draft.form.sequence + 1)
        state = captured.copy(draft = draft.copy(form = form), error = "")
        if (checkpoints?.offer(TaskEditCheckpoint(form.sequence, form.json())) != true) report(token, text(R.string.task_edit_storage_failed))
    }
    private fun editable(token: String) = state?.takeIf { it.token == token && !it.busy && !it.complete && !it.held && it.draft?.state == TaskContentDraft.State.Editing }
    fun dismiss() {
        val captured = state?.takeUnless { it.busy } ?: return
        val writer = checkpoints; val draft = captured.draft
        if (writer == null || draft == null || captured.complete || captured.held) { writer?.close(); checkpoints = null; pendingPicker = null; state = null; return }
        state = captured.copy(busy = true)
        scope.launch {
            try {
                writer.flush(TaskEditCheckpoint(draft.form.sequence, draft.form.json()))
                writer.close(); checkpoints = null; pendingPicker = null
                if (state?.token == captured.token) state = null
            } catch (error: Exception) { if (state?.token == captured.token) state = captured.copy(error = text(R.string.task_edit_storage_failed)) }
        }
    }
    fun picker(token: String, ready: (TaskContentPicker) -> Unit) {
        val captured = editable(token) ?: return
        val draft = captured.draft!!
        if (!upload(captured.context, draft.form.target ?: draft.form.originalTarget)) { report(token, text(R.string.task_content_upload)); return }
        if (draft.form.kept.size + draft.form.files.size >= 8) { report(token, text(R.string.task_media_limit)); return }
        val writer = checkpoints ?: return
        state = captured.copy(busy = true)
        scope.launch {
            try {
                writer.flush(TaskEditCheckpoint(draft.form.sequence, draft.form.json()))
                if (state?.token != token) return@launch
                check(upload(captured.context, draft.form.target ?: draft.form.originalTarget)) { text(R.string.task_content_upload) }
                val ticket = TaskContentPicker(draft = draft); pendingPicker = ticket; ready(ticket)
            } catch (error: Exception) { if (state?.token == token) state = captured.copy(error = error.message.orEmpty()) }
        }
    }
    fun picked(ticket: TaskContentPicker, imports: List<suspend (List<Attachment>) -> Attachment>) {
        if (pendingPicker != ticket) return
        pendingPicker = null
        val captured = state?.takeIf { it.draft?.let(ticket::matches) == true } ?: return
        if (imports.isEmpty()) { state = captured.copy(busy = false); return }
        val writer = checkpoints ?: return
        scope.launch {
            val added = mutableListOf<Attachment>(); var persisting = false
            try {
                val draft = captured.draft!!; val form = draft.form
                check(upload(captured.context, form.target ?: form.originalTarget)) { text(R.string.task_content_upload) }
                for (import in imports) {
                    added += import(form.files + added)
                    check(form.kept.size + form.files.size + added.size <= 8 &&
                        form.kept.sumOf { it.size.toLong() } + (form.files + added).sumOf { it.bytes } <= AttachmentPolicy.MAX_TOTAL_BYTES) { text(R.string.task_media_limit) }
                }
                check(state?.draft?.let(ticket::matches) == true) { text(R.string.task_media_picker_changed) }
                val next = draft.copy(form = form.copy(files = form.files + added, sequence = form.sequence + 1))
                persisting = true
                writer.flush(TaskEditCheckpoint(next.form.sequence, next.form.json()))
                if (state?.token == captured.token) state = captured.copy(draft = next, busy = false)
            } catch (error: CancellationException) { throw error }
            catch (error: Exception) {
                if (persisting) { writer.close(); checkpoints = null }
                if (state?.token == captured.token) state = captured.copy(busy = false, held = persisting, error = error.message.orEmpty())
            } finally {
                if (!persisting) withContext(NonCancellable) { added.forEach { runCatching { remove(it.id, captured.draft!!.form.files.map { file -> file.id }.toSet()) } } }
            }
        }
    }
    fun removeFile(token: String, id: String) {
        val captured = editable(token) ?: return
        val draft = captured.draft!!
        if (draft.form.files.none { it.id == id }) return
        val writer = checkpoints ?: return
        val next = draft.copy(form = draft.form.copy(files = draft.form.files.filterNot { it.id == id }, sequence = draft.form.sequence + 1))
        state = captured.copy(busy = true)
        scope.launch {
            try {
                writer.flush(TaskEditCheckpoint(next.form.sequence, next.form.json()))
                if (state?.token == token) state = captured.copy(draft = next, busy = false)
                remove(id, next.form.files.map { it.id }.toSet())
            } catch (error: Exception) { if (state?.token == token) state = state?.copy(busy = false, error = text(R.string.task_edit_storage_failed)) }
        }
    }
    fun save(token: String) {
        val captured = editable(token) ?: return
        val draft = captured.draft!!; val form = draft.form
        if (form.text.isBlank()) return
        if (form.files.isNotEmpty() && !upload(captured.context, form.target ?: form.originalTarget)) { report(token, text(R.string.task_content_upload)); return }
        val writer = checkpoints ?: return
        state = captured.copy(busy = true)
        scope.launch {
            var applying = false
            try {
                writer.flush(TaskEditCheckpoint(form.sequence, form.json()))
                val inputs = form.files.map { file -> TaskMediaInput(file.name, file.mime, read(file), "") }
                check(form.files.isEmpty() || upload(captured.context, form.target ?: form.originalTarget)) { text(R.string.task_content_upload) }
                val task = withContext(Dispatchers.IO) {
                    taskReplicasSave(captured, draft, inputs) { applying = true }
                }
                writer.close(); checkpoints = null
                if (state?.token == token) state = captured.copy(busy = false, complete = true)
                changed(captured.context, task.uuid)
                withContext(Dispatchers.IO) { files.release(draft) }
                form.files.forEach { runCatching { remove(it.id, emptySet()) } }
            } catch (error: CancellationException) { throw error }
            catch (error: Exception) {
                if (applying) { writer.close(); checkpoints = null }
                if (state?.token == token && state?.complete != true) state = captured.copy(busy = false, held = applying,
                    error = if (applying) text(R.string.task_content_held) else error.message.orEmpty())
            }
        }
    }
    private fun taskReplicasSave(captured: TaskContentState, draft: TaskContentDraft, inputs: List<TaskMediaInput>, entered: () -> Unit) =
        replicas.store(captured.context.key, captured.context.projectName).use { store ->
            if (!draft.isNew) {
                val current = store.get(draft.scope.task)
                check(current.contentRevision == draft.revision && current.writable && current.stage in setOf(TaskStage.TODO, TaskStage.BACKLOG)) { text(R.string.task_content_stale) }
            }
            draft.validate()
            // A failed fsync can have published Applying; keep the original hold even then.
            entered(); files.begin(draft)
            draft.apply(store, inputs).also { files.completed(draft) }
        }
    fun discard(token: String) {
        val captured = editable(token) ?: return
        val draft = captured.draft!!; val writer = checkpoints ?: return
        state = captured.copy(busy = true)
        scope.launch {
            try {
                writer.flush(TaskEditCheckpoint(draft.form.sequence, draft.form.json())); writer.close(); checkpoints = null
                withContext(Dispatchers.IO) { files.release(draft) }
                draft.form.files.forEach { runCatching { remove(it.id, emptySet()) } }
                if (state?.token == token) state = null
            } catch (error: Exception) { if (state?.token == token) state = captured.copy(held = true, error = text(R.string.task_edit_storage_failed)) }
        }
    }
}
