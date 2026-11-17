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

/** Private local content drafts. Applying after a restart stays held; it never repeats a local creation. */
internal class TaskContentDraftFiles(private val root: File, private val key: () -> SecretKey,
    private val maxEntries: Int = 32, private val maxBytes: Long = 16L * 1024 * 1024,
    private val syncDirectory: () -> Unit = {}) {
    fun create(value: TaskContentDraft): TaskContentDraft = synchronized(LOCK) {
        check(read(value.scope, value.isNew) == null) { "Review the saved original task draft first." }
        write(value)
    }
    fun checkpoint(original: TaskContentDraft, sequence: Long, changes: String): TaskContentDraft = synchronized(LOCK) {
        val previous = original(original)
        check(previous.state == TaskContentDraft.State.Editing && sequence >= previous.form.sequence)
        check(sequence != previous.form.sequence || changes == previous.form.json())
        val form = TaskEditForm.read(changes, sequence)
        check(form.title == previous.form.title && form.originalTarget == previous.form.originalTarget && form.originalRank == previous.form.originalRank)
        write(previous.copy(form = form))
    }
    fun begin(value: TaskContentDraft): TaskContentDraft = synchronized(LOCK) {
        check(original(value) == value && value.state == TaskContentDraft.State.Editing)
        write(value.copy(state = TaskContentDraft.State.Applying))
    }
    fun completed(value: TaskContentDraft): TaskContentDraft = synchronized(LOCK) {
        val previous = original(value)
        check(previous == value.copy(state = TaskContentDraft.State.Applying))
        write(previous.copy(state = TaskContentDraft.State.Completed))
    }
    fun release(value: TaskContentDraft) = synchronized(LOCK) {
        val previous = original(value)
        check(previous.state != TaskContentDraft.State.Applying) { "The original save may have happened. Its draft stays held for review." }
        Files.delete(file(value.scope, value.isNew).toPath()); syncDirectory()
    }
    private fun original(value: TaskContentDraft) = checkNotNull(read(value.scope, value.isNew)).also {
        check(it.id == value.id && it.computer == value.computer && it.machine == value.machine && it.revision == value.revision && it.stage == value.stage)
    }
    fun read(scope: TaskEditDraftScope, isNew: Boolean): TaskContentDraft? = synchronized(LOCK) {
        scope.validate()
        if (!Files.exists(root.toPath(), LinkOption.NOFOLLOW_LINKS)) return@synchronized null
        ensureRoot()
        val file = file(scope, isNew)
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
        TaskContentDraft.parse(JSONObject(String(bytes, Charsets.UTF_8))).also { check(it.scope == scope && it.isNew == isNew) }
    }
    private fun file(scope: TaskEditDraftScope, isNew: Boolean): File {
        val frame = WorkspaceProtocol.frame("zerus-task-content-key-v1", *listOf(scope.connectionId, scope.swarmId, scope.project, scope.task)
            .map { it.toByteArray(Charsets.UTF_8) }.toTypedArray())
        val name = MessageDigest.getInstance("SHA-256").digest(frame).joinToString("") { "%02x".format(it) }
        return File(root, "${if (isNew) "new" else "task"}-$name.sealed")
    }
    private fun aad(file: File) = ("zerus-task-content-v1:" + file.name).toByteArray(Charsets.UTF_8)
    private fun ensureRoot() {
        if (!Files.exists(root.toPath(), LinkOption.NOFOLLOW_LINKS)) {
            Files.createDirectory(root.toPath(), PosixFilePermissions.asFileAttribute(DIRECTORY_MODE))
        }
        check(Files.isDirectory(root.toPath(), LinkOption.NOFOLLOW_LINKS)) { "Private task draft storage is unavailable." }
        Files.setPosixFilePermissions(root.toPath(), DIRECTORY_MODE)
    }
    private fun write(value: TaskContentDraft): TaskContentDraft {
        value.validate(); ensureRoot()
        val raw = value.json().toString().toByteArray(Charsets.UTF_8)
        check(raw.size <= MAX_RECORD) { "This task draft exceeds the private storage limit; no request was sent." }
        val destination = file(value.scope, value.isNew)
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
        private const val MAX_RECORD = 1024 * 1024
        private val FILE_MODE = setOf(PosixFilePermission.OWNER_READ, PosixFilePermission.OWNER_WRITE)
        private val DIRECTORY_MODE = FILE_MODE + PosixFilePermission.OWNER_EXECUTE
    }
}
