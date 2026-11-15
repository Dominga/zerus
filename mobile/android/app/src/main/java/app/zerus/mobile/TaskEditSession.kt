package app.zerus.mobile

import app.zerus.mobile.replica.TaskEditDraft
import app.zerus.mobile.replica.TaskEditException
import app.zerus.mobile.replica.TaskEditQualification
import app.zerus.mobile.replica.TaskEditView
import app.zerus.mobile.replica.TaskMediaInput
import app.zerus.mobile.replica.TaskStore
import app.zerus.mobile.replica.TaskTarget
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

internal data class TaskEditContent(val title: String, val text: String, val kept: List<String>, val files: List<TaskMediaInput>)
internal data class TaskEditInput(val content: TaskEditContent? = null, val target: TaskTarget? = null, val rank: String? = null,
                                 val comment: String = "", val commentFiles: List<TaskMediaInput> = emptyList()) {
    fun snapshot() = copy(content = content?.copy(kept = content.kept.toList(), files = content.files.map { it.copy(data = it.data.copyOf()) }),
        commentFiles = commentFiles.map { it.copy(data = it.data.copyOf()) })
    fun hasChanges() = content != null || target != null || rank != null || comment.isNotBlank() || commentFiles.isNotEmpty()
}
internal class TaskEditDraftHeld(val saved: TaskEditSavedDraft) : Exception("This task has a saved draft for review.")

/** One live original handle. Saved bytes alone can never reopen this object. */
internal class TaskEditSession private constructor(
    private val store: TaskStore, private val qualification: TaskEditQualification, private val draft: TaskEditDraft,
    private val transport: TaskEditTransport, private val files: TaskEditDraftFiles, val original: TaskEditSavedDraft
) : AutoCloseable {
    val scope = transport.scope
    val view: TaskEditView = draft.view()
    val id = original.draftId
    private val gate = Mutex()
    private val lifecycle = Any()
    private var busy = false
    private var closed = false
    private var dispose = false
    val submitting get() = synchronized(lifecycle) { busy }
    val editable get() = synchronized(lifecycle) { !closed && !busy }

    suspend fun persist(sequence: Long, changes: String): TaskEditSavedDraft = withContext(Dispatchers.IO) {
        synchronized(lifecycle) { check(!closed && !busy) { "The original task edit is no longer editable." } }
        files.edit(scope, id, sequence, changes)
    }
    suspend fun save(sequence: Long, changes: String, input: TaskEditInput) {
        // Capture mutable attachment buffers before any suspension or selection change.
        val snapshot = input.snapshot()
        check(snapshot.hasChanges()) { "There are no task changes to save." }
        gate.withLock {
            synchronized(lifecycle) { check(!closed && !busy); busy = true }
            var frozen = false
            try {
                withContext(Dispatchers.IO) {
                    files.edit(scope, id, sequence, changes)
                    draft.resetEdits()
                    snapshot.content?.let { draft.content(it.title, it.text, it.kept, it.files) }
                    snapshot.target?.let(draft::target)
                    snapshot.rank?.let(draft::rank)
                    if (snapshot.comment.isNotBlank() || snapshot.commentFiles.isNotEmpty()) draft.comment(snapshot.comment, snapshot.commentFiles)
                    val manifest = draft.freeze()
                    frozen = true
                    files.freeze(scope, id, manifest, expectedSequence = sequence)
                }
                var parts = 0
                while (true) {
                    check(++parts <= 256) { "The original task upload exceeded its part budget." }
                    val request = withContext(Dispatchers.IO) { draft.nextRequest() } ?: break
                    try {
                        val proof = transport.proof(request) { files.reserve(scope, id, request.requestId, request.payload) }
                        val progress = withContext(Dispatchers.IO) { draft.accept(proof) }
                        withContext(NonCancellable + Dispatchers.IO) {
                            files.settle(scope, id, request.requestId,
                                if (progress.complete) TaskEditDraftOutcome.Completed else TaskEditDraftOutcome.Acknowledged)
                        }
                        transport.verified(request.requestId)
                        if (progress.complete) break
                    } catch (error: Exception) {
                        withContext(NonCancellable + Dispatchers.IO) {
                            if (error !is TaskEditException.Refused) runCatching { draft.holdUnknown() }
                            val pending = files.read(scope)
                            if (pending?.state == TaskEditDraftState.Pending && pending.pendingId == request.requestId) {
                                runCatching { files.settle(scope, id, request.requestId,
                                    if (error is TaskEditException.Refused) TaskEditDraftOutcome.Refused else TaskEditDraftOutcome.Unknown) }
                            }
                        }
                        throw error
                    }
                }
            } catch (error: Exception) {
                if (frozen) synchronized(lifecycle) { dispose = true }
                throw error
            } finally {
                val release = synchronized(lifecycle) {
                    busy = false
                    if (frozen) dispose = true
                    if (dispose) { closed = true; true } else false
                }
                if (release) closeHandles()
            }
        }
    }
    override fun close() {
        val release = synchronized(lifecycle) {
            if (closed) false else if (busy) { dispose = true; false } else { closed = true; true }
        }
        if (release) closeHandles()
    }
    private fun closeHandles() { draft.close(); qualification.close(); store.close() }

    companion object {
        suspend fun open(replicas: TaskReplicas, projectName: String, transport: TaskEditTransport, files: TaskEditDraftFiles): TaskEditSession {
            val scope = transport.scope
            withContext(Dispatchers.IO) { files.read(scope) }?.let { throw TaskEditDraftHeld(it) }
            val store = withContext(Dispatchers.IO) { replicas.store(transport.projectKey, projectName) }
            var qualification: TaskEditQualification? = null
            var accepted: TaskEditDraft? = null
            var saved: TaskEditSavedDraft? = null
            try {
                qualification = withContext(Dispatchers.IO) { store.qualifyEdit(scope.task, transport.route) }
                val original = checkNotNull(qualification)
                val request = original.request()
                saved = withContext(Dispatchers.IO) { files.create(scope, transport.route.machineId, transport.route.hostDevice,
                    request.requestId, original.editId(), TaskEditForm.initial(original.view()).json()) }
                val record = checkNotNull(saved)
                val proof = transport.proof(request) { files.reserve(scope, record.draftId, request.requestId, request.payload) }
                val draft = withContext(Dispatchers.IO) { original.accept(proof) }
                accepted = draft
                withContext(NonCancellable + Dispatchers.IO) { files.settle(scope, record.draftId, request.requestId, TaskEditDraftOutcome.Acknowledged) }
                transport.verified(request.requestId)
                return TaskEditSession(store, original, draft, transport, files, record)
            } catch (error: Exception) {
                withContext(NonCancellable + Dispatchers.IO) {
                    saved?.let { record ->
                        val pending = runCatching { files.read(scope) }.getOrNull()
                        if (pending?.state == TaskEditDraftState.Pending) runCatching {
                            files.settle(scope, record.draftId, pending.pendingId!!,
                                if (error is TaskEditException.Refused) TaskEditDraftOutcome.Refused else TaskEditDraftOutcome.Unknown)
                        }
                    }
                }
                accepted?.close(); qualification?.close(); store.close()
                throw error
            }
        }
    }
}
