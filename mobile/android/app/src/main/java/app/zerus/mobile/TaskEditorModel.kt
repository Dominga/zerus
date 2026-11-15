package app.zerus.mobile

import android.app.Application
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.zerus.mobile.replica.TaskInfo
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.UUID

internal data class TaskEditContext(val connection: Connection, val key: ProjectKey, val projectName: String,
                                    val computer: Machine, val features: Map<MachineKey, Set<String>>)
internal data class TaskEditorState(val token: String, val context: TaskEditContext, val task: TaskInfo,
                                    val session: TaskEditSession? = null, val form: TaskEditForm? = null,
                                    val saved: TaskEditSavedDraft? = null, val loading: Boolean = false,
                                    val busy: Boolean = false, val error: String = "", val complete: Boolean = false)
internal data class TaskEditorPicker(val token: String, val draftId: String, val scope: TaskEditDraftScope, val comment: Boolean)

/** At most one visible live editor; every asynchronous operation carries its original context. */
internal class TaskEditorModel(private val app: Application, private val scope: CoroutineScope, private val api: () -> RelayApi,
    private val replicas: TaskReplicas, private val files: TaskEditDraftFiles, private val assets: AttachmentStore,
    private val changed: (TaskEditContext, String, String) -> Unit) {
    var state by mutableStateOf<TaskEditorState?>(null); private set
    private var autosave: Job? = null
    private fun text(id: Int) = app.getString(id)
    fun open(context: TaskEditContext, task: TaskInfo) {
        val old = state
        if (old?.busy == true) return
        if (old?.session != null && !old.complete) return
        old?.session?.close()
        val token = UUID.randomUUID().toString()
        state = TaskEditorState(token, context, task, loading = true)
        scope.launch {
            try {
                val transport = TaskEditTransport.open(api(), context.connection, context.key, task.uuid, context.computer, context.features)
                val original = transport.scope
                val previous = withContext(Dispatchers.IO) { files.read(original) }
                if (previous?.state == TaskEditDraftState.Completed) withContext(Dispatchers.IO) { files.releaseCompleted(original, previous.draftId) }
                val session = TaskEditSession.open(replicas, context.projectName, transport, files)
                if (state?.token != token) { session.close(); return@launch }
                state = TaskEditorState(token, context, task, session, TaskEditForm.initial(session.view))
            } catch (held: TaskEditDraftHeld) {
                if (state?.token == token) state = TaskEditorState(token, context, task, saved = held.saved,
                    form = runCatching { TaskEditForm.read(held.saved.changes, held.saved.changeSequence) }.getOrNull())
            } catch (error: CancellationException) { throw error }
            catch (error: Exception) {
                if (state?.token == token) state = TaskEditorState(token, context, task,
                    error = error.message.orEmpty().ifBlank { text(R.string.task_edit_unavailable) })
            }
        }
    }
    fun reportError(token: String, message: String) {
        if (state?.token == token) state = state?.copy(error = message.ifBlank { text(R.string.task_edit_unavailable) })
    }
    fun dismiss() {
        val captured = state?.takeIf { !it.busy } ?: return
        val session = captured.session; val form = captured.form
        autosave?.cancel(); autosave = null
        if (session == null || form == null || captured.complete) { session?.close(); state = null; return }
        state = captured.copy(busy = true)
        scope.launch {
            try {
                session.persist(form.sequence, form.json())
                session.close()
                if (state?.token == captured.token) state = null
            } catch (error: Exception) {
                if (state?.token == captured.token) state = captured.copy(busy = false, error = text(R.string.task_edit_storage_failed))
            }
        }
    }
    fun change(token: String, transform: (TaskEditForm) -> TaskEditForm) {
        val old = state?.takeIf { it.token == token && !it.busy && it.session != null && it.form != null && !it.complete } ?: return
        val form = transform(old.form!!).copy(sequence = old.form.sequence + 1)
        state = old.copy(form = form, error = "")
        autosave?.cancel()
        autosave = scope.launch {
            try { delay(150); old.session!!.persist(form.sequence, form.json()) }
            catch (error: CancellationException) { throw error }
            catch (error: Exception) { if (state?.token == token && state?.form?.sequence == form.sequence) state = state?.copy(error = text(R.string.task_edit_storage_failed)) }
        }
    }
    fun save(token: String) {
        val captured = state?.takeIf { it.token == token && !it.busy && !it.complete && it.session != null && it.form != null } ?: return
        val session = captured.session!!; val form = captured.form!!
        if (!form.changed(session.view)) return
        autosave?.cancel(); autosave = null
        state = captured.copy(busy = true, error = "")
        scope.launch {
            try {
                val input = form.input(session.view, assets)
                session.save(form.sequence, form.json(), input)
                if (state?.token == token) state = captured.copy(complete = true, busy = false)
                changed(captured.context, captured.task.uuid, captured.token)
            } catch (error: CancellationException) { throw error }
            catch (error: Exception) {
                val saved = withContext(Dispatchers.IO) { runCatching { files.read(session.scope) }.getOrNull() }
                val held = !session.editable || saved?.state in setOf(TaskEditDraftState.Pending, TaskEditDraftState.Unknown, TaskEditDraftState.Refused, TaskEditDraftState.Frozen)
                if (state?.token == token) state = captured.copy(busy = false, saved = if (held) saved else null,
                    session = if (held) null else session,
                    error = error.message.orEmpty().ifBlank { text(R.string.task_edit_draft_kept) })
            }
        }
    }
    private var pendingPicker: TaskEditorPicker? = null
    fun picker(token: String, comment: Boolean, ready: (TaskEditorPicker) -> Unit) {
        val captured = state?.takeIf { it.token == token && !it.busy && it.session != null && it.form != null && !it.complete } ?: return
        val form = captured.form!!
        if (form.files.size + form.commentFiles.size >= AttachmentPolicy.MAX_FILES || !comment && form.kept.size + form.files.size >= AttachmentPolicy.MAX_FILES) {
            state = captured.copy(error = text(R.string.task_media_limit)); return
        }
        autosave?.cancel(); autosave = null
        val ticket = TaskEditorPicker(token, captured.session!!.id, captured.session.scope, comment)
        state = captured.copy(busy = true)
        scope.launch {
            try {
                captured.session!!.persist(form.sequence, form.json())
                if (state?.token == token) { pendingPicker = ticket; ready(ticket) }
            } catch (error: Exception) {
                if (state?.token == token) state = captured.copy(busy = false, error = text(R.string.task_edit_storage_failed))
            }
        }
    }
    fun picked(ticket: TaskEditorPicker, uris: List<Uri>) {
        if (pendingPicker != ticket) return
        pendingPicker = null
        val captured = state?.takeIf { it.token == ticket.token && it.session?.id == ticket.draftId && it.session.scope == ticket.scope && it.form != null }
            ?: run { state = state?.copy(error = text(R.string.task_media_picker_changed)); return }
        if (uris.isEmpty()) { state = captured.copy(busy = false); return }
        state = captured.copy(busy = true)
        scope.launch {
            val added = mutableListOf<Attachment>()
            var persisting = false
            try {
                val form = captured.form!!
                for (uri in uris) {
                    val file = assets.importDocument(uri, form.files + form.commentFiles + added)
                    added += file
                    if (!ticket.comment) check(form.kept.size + form.files.size + added.size <= 8 &&
                        form.kept.sumOf { it.size.toLong() } + form.files.sumOf { it.bytes } + added.sumOf { it.bytes } <= AttachmentPolicy.MAX_TOTAL_BYTES)
                }
                val next = (if (ticket.comment) form.copy(commentFiles = form.commentFiles + added) else form.copy(files = form.files + added)).copy(sequence = form.sequence + 1)
                persisting = true
                captured.session!!.persist(next.sequence, next.json())
                if (state?.token == ticket.token && state?.session?.id == ticket.draftId) state = captured.copy(form = next, busy = false)
            } catch (error: CancellationException) { throw error }
            catch (error: Exception) {
                if (persisting) captured.session?.close()
                if (state?.token == ticket.token) state = captured.copy(busy = false, session = if (persisting) null else captured.session,
                    error = error.message.orEmpty().ifBlank { text(R.string.task_media_import_failed) })
            } finally {
                if (!persisting) withContext(NonCancellable) {
                    val protected = (captured.form!!.files + captured.form.commentFiles).map { it.id }.toSet()
                    added.forEach { runCatching { assets.remove(it.id, protected) } }
                }
            }
        }
    }
    fun discard(token: String) {
        val captured = state?.takeIf { it.token == token && !it.busy && it.saved != null } ?: return
        val saved = captured.saved!!
        scope.launch {
            try {
                withContext(Dispatchers.IO) { files.discardUnsubmitted(saved.scope, saved.draftId) }
                if (state?.token == token) { state = null; open(captured.context, captured.task) }
            } catch (error: Exception) { if (state?.token == token) state = captured.copy(error = text(R.string.task_edit_draft_kept)) }
        }
    }
}
