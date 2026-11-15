package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.util.UUID
import javax.crypto.KeyGenerator

class TaskEditDraftFilesTest {
    @get:Rule val temporary = TemporaryFolder()
    private val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    private val scope = TaskEditDraftScope("connection", "swarm", "project", UUID.randomUUID().toString())
    private val machine = UUID.randomUUID().toString()
    private val qualification = UUID.randomUUID().toString()
    private val edit = UUID.randomUUID().toString()
    private val original = JSONObject().put("title", "Private original requirement").put("text", "Keep this draft").toString()
    private fun files(root: File, entries: Int = 32, bytes: Long = 256L * 1024 * 1024, sync: () -> Unit = {}) =
        TaskEditDraftFiles(root, { secret }, maxEntries = entries, maxBytes = bytes, syncDirectory = sync)
    private fun create(files: TaskEditDraftFiles, key: TaskEditDraftScope = scope) =
        files.create(key, machine, "host-device", qualification, edit, original)
    private fun qualified(files: TaskEditDraftFiles): TaskEditSavedDraft {
        val draft = create(files)
        files.reserve(scope, draft.draftId, qualification, "qualification bytes")
        return files.settle(scope, draft.draftId, qualification, TaskEditDraftOutcome.Acknowledged)
    }
    private fun frozen(files: TaskEditDraftFiles): TaskEditSavedDraft {
        val draft = qualified(files)
        return files.freeze(scope, draft.draftId, "exact frozen manifest")
    }
    private fun refuses(block: () -> Unit) {
        val result = runCatching(block)
        assertTrue("The original draft must remain held", result.isFailure)
    }

    @Test fun originalSourceAndFrozenBytesSurviveRestartInEncryptedStorage() {
        val root = temporary.newFolder()
        val store = files(root)
        val draft = frozen(store)
        val read = files(root).read(scope)!!
        assertEquals(draft, read)
        assertEquals(original, read.original)
        assertEquals("exact frozen manifest", read.manifest)
        assertEquals(machine, read.machineId)
        assertEquals("host-device", read.hostDevice)
        assertEquals(qualification, read.qualificationId)
        assertEquals(edit, read.editId)
        val cipher = root.listFiles()!!.single().readBytes().toString(Charsets.ISO_8859_1)
        assertFalse(cipher.contains("Private original requirement"))
        assertFalse(cipher.contains("exact frozen manifest"))
    }

    @Test fun aReservedRequestCannotPostTwiceOrChangeIdentityEvenAfterRestart() {
        val root = temporary.newFolder()
        val store = files(root)
        val draft = frozen(store)
        store.reserve(scope, draft.draftId, edit, "exact request payload")
        val reopened = files(root)
        val retained = reopened.read(scope)!!
        assertEquals(TaskEditDraftState.Pending, retained.state)
        assertEquals(edit, retained.pendingId)
        assertEquals("exact request payload", retained.pendingPayload)
        refuses { reopened.reserve(scope, draft.draftId, edit, "exact request payload") }
        refuses { reopened.reserve(scope, draft.draftId, UUID.randomUUID().toString(), "changed") }
        refuses { reopened.create(scope, machine, "host-device", qualification, edit, original) }
        assertEquals(retained, reopened.read(scope))
    }

    @Test fun anUnknownOutcomeKeepsTheOriginalDraftAndForbidsAnyNewRequest() {
        val root = temporary.newFolder()
        val store = files(root)
        val draft = frozen(store)
        store.reserve(scope, draft.draftId, edit, "payload")
        store.settle(scope, draft.draftId, edit, TaskEditDraftOutcome.Unknown)
        val reopened = files(root)
        assertEquals(TaskEditDraftState.Unknown, reopened.read(scope)!!.state)
        refuses { reopened.reserve(scope, draft.draftId, UUID.randomUUID().toString(), "new payload") }
        refuses { reopened.freeze(scope, draft.draftId, "replacement manifest") }
        assertEquals(original, reopened.read(scope)!!.original)
        assertEquals("exact frozen manifest", reopened.read(scope)!!.manifest)
    }

    @Test fun onlyTheExactPendingIdentityCanAdvanceAndTheFinalIdentityCompletesOnce() {
        val root = temporary.newFolder()
        val store = files(root)
        val draft = frozen(store)
        val part = UUID.randomUUID().toString()
        store.reserve(scope, draft.draftId, part, "first part")
        refuses { store.settle(scope, draft.draftId, edit, TaskEditDraftOutcome.Acknowledged) }
        refuses { store.settle(scope, draft.draftId, UUID.randomUUID().toString(), TaskEditDraftOutcome.Acknowledged) }
        refuses { store.settle(scope, draft.draftId, part, TaskEditDraftOutcome.Completed) }
        store.settle(scope, draft.draftId, part, TaskEditDraftOutcome.Acknowledged)
        refuses { store.reserve(scope, draft.draftId, part, "first part") }
        store.reserve(scope, draft.draftId, edit, "last part")
        store.settle(scope, draft.draftId, edit, TaskEditDraftOutcome.Completed)
        assertEquals(TaskEditDraftState.Completed, files(root).read(scope)!!.state)
        refuses { store.reserve(scope, draft.draftId, UUID.randomUUID().toString(), "another input") }
    }

    @Test fun qualificationAndFreezeCannotReplaceAnEarlierReviewedBinding() {
        val root = temporary.newFolder()
        val store = files(root)
        val draft = create(store)
        refuses { store.reserve(scope, draft.draftId, edit, "no qualification yet") }
        refuses { store.freeze(scope, draft.draftId, "not qualified") }
        store.reserve(scope, draft.draftId, qualification, "qualification")
        store.settle(scope, draft.draftId, qualification, TaskEditDraftOutcome.Acknowledged)
        store.freeze(scope, draft.draftId, "first manifest")
        refuses { store.freeze(scope, draft.draftId, "second manifest") }
        assertEquals("first manifest", store.read(scope)!!.manifest)
    }

    @Test fun sourceScopesAndDraftIdentitiesCannotAlias() {
        val root = temporary.newFolder()
        val store = files(root)
        val draft = frozen(store)
        for (other in listOf(scope.copy(connectionId = "other"), scope.copy(swarmId = "other"),
            scope.copy(project = "other"), scope.copy(task = UUID.randomUUID().toString()))) {
            assertNull(store.read(other))
            refuses { store.reserve(other, draft.draftId, edit, "payload") }
        }
        refuses { store.reserve(scope, UUID.randomUUID().toString(), edit, "payload") }
        assertEquals(draft, store.read(scope))
    }

    @Test fun aCorruptOrReplayedFileStaysVisibleAsAnErrorAndIsNeverRecreated() {
        val root = temporary.newFolder()
        val store = files(root)
        frozen(store)
        val file = root.listFiles()!!.single()
        val good = file.readBytes()
        file.writeBytes(good.copyOf(20))
        refuses { files(root).read(scope) }
        refuses { files(root).create(scope, machine, "host-device", qualification, edit, original) }
        assertArrayEquals(good.copyOf(20), file.readBytes())
        file.writeBytes(good)
        val other = scope.copy(task = UUID.randomUUID().toString())
        create(store, other)
        val destination = root.listFiles()!!.single { it != file }
        destination.writeBytes(good)
        refuses { files(root).read(other) }
    }

    @Test fun fullStorageRefusesWithoutEvictingAnyOriginalDraft() {
        val root = temporary.newFolder()
        val store = files(root, entries = 1)
        val draft = frozen(store)
        refuses { create(store, scope.copy(task = UUID.randomUUID().toString())) }
        assertEquals(draft, store.read(scope))
        assertEquals(1, root.listFiles()!!.size)
        refuses { files(root, bytes = 30).reserve(scope, draft.draftId, edit, "payload") }
        assertEquals(draft, store.read(scope))
    }

    @Test fun aDurabilityErrorIsVisibleBeforeTheCallerMaySend() {
        val root = temporary.newFolder()
        val store = files(root)
        val draft = frozen(store)
        var sent = false
        val result = runCatching {
            files(root, sync = { error("forced directory sync failure") }).reserve(scope, draft.draftId, edit, "payload")
            sent = true
        }
        assertTrue(result.isFailure)
        assertFalse(sent)
        // The rename may already have committed. Preserve the reservation,
        // rather than turning a failed sync into a second submission.
        assertEquals(TaskEditDraftState.Pending, store.read(scope)!!.state)
        refuses { store.reserve(scope, draft.draftId, edit, "payload") }
    }
}
