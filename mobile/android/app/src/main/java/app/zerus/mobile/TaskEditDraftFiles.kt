package app.zerus.mobile

import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.StandardCopyOption
import java.nio.file.attribute.PosixFilePermission
import java.nio.file.attribute.PosixFilePermissions
import java.security.MessageDigest
import java.util.UUID
import javax.crypto.Cipher
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Immutable routing namespace. Labels, the current screen and the relay do not rehome a draft. */
internal data class TaskEditDraftScope(val connectionId: String, val swarmId: String, val project: String, val task: String) {
    fun json() = JSONObject().put("connection", connectionId).put("swarm", swarmId).put("project", project).put("task", task)
    fun validate() {
        listOf(connectionId, swarmId, project).forEach { require(it.toByteArray(Charsets.UTF_8).size in 1..512 && it.none(Char::isISOControl)) }
        taskDraftUuid(task)
    }
    companion object {
        fun parse(value: JSONObject): TaskEditDraftScope {
            WorkspaceProtocol.exactFields(value, setOf("connection", "swarm", "project", "task"))
            return TaskEditDraftScope(WorkspaceProtocol.string(value, "connection"), WorkspaceProtocol.string(value, "swarm"),
                WorkspaceProtocol.string(value, "project"), WorkspaceProtocol.string(value, "task")).also { it.validate() }
        }
    }
}
internal enum class TaskEditDraftState { Editing, Frozen, Pending, Unknown, Completed, Refused }
internal enum class TaskEditDraftOutcome { Acknowledged, Completed, Refused, Unknown }

/** Saved presentation and transport evidence only; this record cannot reconstruct a native edit handle. */
internal data class TaskEditSavedDraft(
    val scope: TaskEditDraftScope, val draftId: String, val machineId: String, val hostDevice: String,
    val qualificationId: String, val editId: String, val original: String,
    val changes: String = original, val changeSequence: Long = 0,
    val state: TaskEditDraftState = TaskEditDraftState.Editing, val qualified: Boolean = false,
    val manifest: String? = null, val pendingId: String? = null, val pendingPayload: String? = null,
    val usedRequests: List<String> = emptyList()
) {
    override fun toString() = "TaskEditSavedDraft(id=$draftId, state=$state)"
    fun json() = JSONObject().put("version", 1).put("scope", scope.json()).put("draft_id", draftId)
        .put("machine_id", machineId).put("host_device", hostDevice).put("qualification_id", qualificationId).put("edit_id", editId)
        .put("original", original).put("changes", changes).put("change_sequence", changeSequence)
        .put("state", state.name).put("qualified", qualified)
        .put("manifest", manifest ?: JSONObject.NULL).put("pending_id", pendingId ?: JSONObject.NULL)
        .put("pending_payload", pendingPayload ?: JSONObject.NULL).put("used_requests", JSONArray(usedRequests))
    fun validate() {
        scope.validate()
        listOf(draftId, machineId, qualificationId, editId).forEach(::taskDraftUuid)
        WorkspaceProtocol.identifier(hostDevice)
        require(qualificationId != editId && usedRequests.size <= 256 && usedRequests.distinct().size == usedRequests.size)
        require(changeSequence >= 0)
        usedRequests.forEach(::taskDraftUuid)
        require(!qualified || qualificationId in usedRequests)
        require(manifest == null || qualified && manifest.isNotEmpty())
        require((pendingId == null) == (pendingPayload == null))
        pendingId?.let { taskDraftUuid(it); require(it in usedRequests && !pendingPayload.isNullOrEmpty()) }
        when (state) {
            TaskEditDraftState.Editing -> require(manifest == null && pendingId == null)
            TaskEditDraftState.Frozen -> require(qualified && manifest != null && pendingId == null)
            TaskEditDraftState.Pending, TaskEditDraftState.Unknown, TaskEditDraftState.Refused -> require(pendingId != null)
            TaskEditDraftState.Completed -> require(qualified && manifest != null && pendingId == editId)
        }
        if (!qualified && pendingId != null) require(pendingId == qualificationId && manifest == null)
    }
    companion object {
        fun parse(value: JSONObject): TaskEditSavedDraft {
            WorkspaceProtocol.exactFields(value, setOf("version", "scope", "draft_id", "machine_id", "host_device", "qualification_id",
                "edit_id", "original", "changes", "change_sequence", "state", "qualified", "manifest", "pending_id", "pending_payload", "used_requests"))
            require(WorkspaceProtocol.number(value, "version") == 1L)
            require(value.get("qualified") is Boolean)
            val used = value.getJSONArray("used_requests")
            require(used.length() <= 256)
            fun nullable(name: String) = if (value.isNull(name)) null else WorkspaceProtocol.string(value, name)
            return TaskEditSavedDraft(TaskEditDraftScope.parse(value.getJSONObject("scope")), WorkspaceProtocol.string(value, "draft_id"),
                WorkspaceProtocol.string(value, "machine_id"), WorkspaceProtocol.string(value, "host_device"),
                WorkspaceProtocol.string(value, "qualification_id"), WorkspaceProtocol.string(value, "edit_id"), WorkspaceProtocol.string(value, "original"),
                changes = WorkspaceProtocol.string(value, "changes"), changeSequence = WorkspaceProtocol.number(value, "change_sequence"),
                state = TaskEditDraftState.valueOf(WorkspaceProtocol.string(value, "state")), qualified = value.getBoolean("qualified"),
                manifest = nullable("manifest"), pendingId = nullable("pending_id"), pendingPayload = nullable("pending_payload"),
                usedRequests = (0 until used.length()).map { used.get(it) as? String ?: error("Invalid saved task draft identity.") })
                .also { it.validate() }
        }
    }
}
private fun taskDraftUuid(value: String) { require(UUID.fromString(value).toString() == value) { "Invalid saved task draft identity." } }

/**
 * Bounded encrypted drafts. Unlike presentation caches, they have no expiry or eviction.
 * Every state transition is a synchronized atomic, fsynced replacement before transport.
 * On restart Pending is held for read-only outcome review; callers never reconstruct a
 * live FFI handle or call reserve again from saved bytes.
 */
internal class TaskEditDraftFiles(
    private val root: File, private val key: () -> SecretKey,
    private val maxEntries: Int = 32, private val maxBytes: Long = 256L * 1024 * 1024,
    private val syncDirectory: () -> Unit = {}
) {
    fun create(scope: TaskEditDraftScope, machine: String, host: String, qualification: String, edit: String,
               original: String): TaskEditSavedDraft = synchronized(LOCK) {
        scope.validate(); ensureRoot()
        check(!Files.exists(file(scope).toPath(), LinkOption.NOFOLLOW_LINKS)) { "This task already has a saved draft. Review it before creating another." }
        write(TaskEditSavedDraft(scope, UUID.randomUUID().toString(), machine, host, qualification, edit, original))
    }
    fun read(scope: TaskEditDraftScope): TaskEditSavedDraft? = synchronized(LOCK) {
        scope.validate()
        if (!Files.exists(root.toPath(), LinkOption.NOFOLLOW_LINKS)) return@synchronized null
        ensureRoot()
        val file = file(scope)
        if (!Files.exists(file.toPath(), LinkOption.NOFOLLOW_LINKS)) return@synchronized null
        check(Files.isRegularFile(file.toPath(), LinkOption.NOFOLLOW_LINKS) && file.length() in 29..(MAX_RECORD.toLong() + 28)) {
            "The saved task draft is unreadable. Its bytes were kept for review."
        }
        val packed = ByteArray(file.length().toInt())
        file.inputStream().use { input ->
            var offset = 0
            while (offset < packed.size) {
                val count = input.read(packed, offset, packed.size - offset)
                check(count > 0) { "The saved task draft changed while reading. Its bytes were kept." }
                offset += count
            }
            check(input.read() == -1) { "The saved task draft changed while reading. Its bytes were kept." }
        }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, packed.copyOfRange(0, 12)))
        cipher.updateAAD(aad(file))
        val bytes = cipher.doFinal(packed.copyOfRange(12, packed.size))
        check(bytes.size <= MAX_RECORD)
        TaskEditSavedDraft.parse(JSONObject(String(bytes, Charsets.UTF_8))).also { check(it.scope == scope) }
    }
    fun edit(scope: TaskEditDraftScope, draftId: String, sequence: Long, changes: String): TaskEditSavedDraft = synchronized(LOCK) {
        val previous = original(scope, draftId)
        check(previous.state == TaskEditDraftState.Editing && previous.qualified && sequence > previous.changeSequence) {
            "This task draft changed or transport already started. The newer draft was kept."
        }
        write(previous.copy(changes = changes, changeSequence = sequence))
    }
    fun freeze(scope: TaskEditDraftScope, draftId: String, manifest: String, expectedSequence: Long = 0): TaskEditSavedDraft = synchronized(LOCK) {
        val previous = original(scope, draftId)
        check(previous.state == TaskEditDraftState.Editing && previous.qualified && previous.manifest == null && manifest.isNotEmpty() &&
            previous.changeSequence == expectedSequence) {
            "The original task draft cannot be replaced or changed after transport starts."
        }
        write(previous.copy(state = TaskEditDraftState.Frozen, manifest = manifest))
    }
    fun reserve(scope: TaskEditDraftScope, draftId: String, requestId: String, payload: String): TaskEditSavedDraft = synchronized(LOCK) {
        val previous = original(scope, draftId)
        taskDraftUuid(requestId)
        check(previous.state in setOf(TaskEditDraftState.Editing, TaskEditDraftState.Frozen) &&
            requestId !in previous.usedRequests && payload.isNotEmpty()) {
            "The original task request is held for review and will not be submitted again."
        }
        check(if (!previous.qualified) requestId == previous.qualificationId else previous.manifest != null && requestId != previous.qualificationId) {
            "This request does not belong to the original qualified task draft."
        }
        write(previous.copy(state = TaskEditDraftState.Pending, pendingId = requestId, pendingPayload = payload,
            usedRequests = previous.usedRequests + requestId))
    }
    /** Only the caller that verified the exact FFI proof may supply an acknowledged outcome. */
    fun settle(scope: TaskEditDraftScope, draftId: String, requestId: String, outcome: TaskEditDraftOutcome): TaskEditSavedDraft = synchronized(LOCK) {
        val previous = original(scope, draftId)
        check(previous.state == TaskEditDraftState.Pending && previous.pendingId == requestId) {
            "The returned outcome does not belong to the original pending task request."
        }
        val next = when (outcome) {
            TaskEditDraftOutcome.Acknowledged -> {
                check(requestId != previous.editId) { "The final edit requires its verified completion." }
                previous.copy(state = if (previous.manifest == null) TaskEditDraftState.Editing else TaskEditDraftState.Frozen,
                    qualified = true, pendingId = null, pendingPayload = null)
            }
            TaskEditDraftOutcome.Completed -> {
                check(requestId == previous.editId && previous.manifest != null)
                previous.copy(state = TaskEditDraftState.Completed)
            }
            TaskEditDraftOutcome.Refused -> previous.copy(state = TaskEditDraftState.Refused)
            TaskEditDraftOutcome.Unknown -> previous.copy(state = TaskEditDraftState.Unknown)
        }
        write(next)
    }
    /** Known completion may release a draft; pending, refused and unknown bytes stay for review. */
    fun releaseCompleted(scope: TaskEditDraftScope, draftId: String) = synchronized(LOCK) {
        val previous = original(scope, draftId)
        check(previous.state == TaskEditDraftState.Completed) { "Only this task's verified completed edit can release its draft." }
        Files.delete(file(scope).toPath())
        syncDirectory()
    }
    /** Explicit person-confirmed discard only. An unconfirmed delivery remains held. */
    fun discardUnsubmitted(scope: TaskEditDraftScope, draftId: String) = synchronized(LOCK) {
        val previous = original(scope, draftId)
        check(previous.state in setOf(TaskEditDraftState.Editing, TaskEditDraftState.Refused) ||
            previous.state == TaskEditDraftState.Frozen && previous.editId !in previous.usedRequests) {
            "This task edit may have been submitted. Its draft must stay for review."
        }
        Files.delete(file(scope).toPath())
        syncDirectory()
    }
    private fun original(scope: TaskEditDraftScope, id: String): TaskEditSavedDraft =
        checkNotNull(read(scope)) { "The original task draft is unavailable." }.also { check(it.draftId == id) { "The original task draft identity changed." } }
    private fun file(scope: TaskEditDraftScope): File {
        val frame = WorkspaceProtocol.frame("zerus-task-draft-key-v1", *listOf(scope.connectionId, scope.swarmId, scope.project, scope.task)
            .map { it.toByteArray(Charsets.UTF_8) }.toTypedArray())
        val name = MessageDigest.getInstance("SHA-256").digest(frame).joinToString("") { "%02x".format(it) }
        return File(root, "$name.sealed")
    }
    private fun aad(file: File) = ("zerus-task-edit-draft-v1:" + file.name).toByteArray(Charsets.UTF_8)
    private fun ensureRoot() {
        if (!Files.exists(root.toPath(), LinkOption.NOFOLLOW_LINKS)) {
            Files.createDirectory(root.toPath(), PosixFilePermissions.asFileAttribute(DIRECTORY_MODE))
        }
        check(Files.isDirectory(root.toPath(), LinkOption.NOFOLLOW_LINKS)) { "Private task draft storage is unavailable." }
        Files.setPosixFilePermissions(root.toPath(), DIRECTORY_MODE)
    }
    private fun write(value: TaskEditSavedDraft): TaskEditSavedDraft {
        value.validate(); ensureRoot()
        val raw = value.json().toString().toByteArray(Charsets.UTF_8)
        check(raw.size <= MAX_RECORD) { "This task draft exceeds the private storage limit; no request was sent." }
        val destination = file(value.scope)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key()); cipher.updateAAD(aad(destination))
        val packed = cipher.iv + cipher.doFinal(raw)
        var count = 0; var bytes = 0L
        Files.newDirectoryStream(root.toPath()).use { rows ->
            for (path in rows) {
                check(++count <= maxEntries && Files.isRegularFile(path, LinkOption.NOFOLLOW_LINKS)) { "Private task draft storage is full or needs review. Saved drafts were kept." }
                if (path != destination.toPath()) bytes += Files.size(path)
                check(bytes <= maxBytes) { "Private task draft storage is full. Saved drafts were kept." }
            }
        }
        check((Files.exists(destination.toPath(), LinkOption.NOFOLLOW_LINKS) || count < maxEntries) && bytes + packed.size <= maxBytes) {
            "Private task draft storage is full. Saved drafts were kept."
        }
        val temporary = Files.createTempFile(root.toPath(), ".task-draft-", ".part", PosixFilePermissions.asFileAttribute(FILE_MODE)).toFile()
        try {
            FileOutputStream(temporary).use { it.write(packed); it.fd.sync() }
            Files.move(temporary.toPath(), destination.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            syncDirectory()
        } finally { temporary.delete() }
        return value
    }
    companion object {
        private val LOCK = Any()
        private const val MAX_RECORD = 64 * 1024 * 1024
        private val FILE_MODE = setOf(PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE)
        private val DIRECTORY_MODE = FILE_MODE + PosixFilePermission.OWNER_EXECUTE
    }
}
