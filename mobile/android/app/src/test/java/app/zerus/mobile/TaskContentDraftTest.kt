package app.zerus.mobile

import app.zerus.mobile.replica.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class TaskContentDraftTest {
    @get:Rule val folders = TemporaryFolder()
    private val key = ProjectKey("connection", "swarm", "project")
    private fun store() = TaskStore(folders.newFolder().path, key.id, "Fixture")
    @Test fun staleBodyCannotOverwriteNewContentAndKeepsTheOriginalDraft() = store().use { store ->
        val task = store.add("Original", TaskStage.TODO, TaskTarget.AnySession, TaskPosition.End)
        val draft = TaskContentDraft.existing(key, "computer", task).let { it.copy(form = it.form.copy(text = "Private draft")) }
        store.editText(task.uuid, "Concurrent change")
        assertTrue(runCatching { draft.apply(store, emptyList()) }.isFailure)
        assertEquals("Concurrent change", store.get(task.uuid).text)
        assertEquals("Private draft", draft.form.text)
    }
    @Test fun retainedTokensAndNewBytesAreAppliedWithoutLosingConcurrentHistory() = store().use { store ->
        val task = store.addWithMedia("Original", TaskStage.TODO, TaskTarget.AnySession, TaskPosition.End,
            listOf(TaskMediaInput("first.txt", "text/plain", byteArrayOf(1))))
        val draft = TaskContentDraft.existing(key, "computer", task).let { it.copy(form = it.form.copy(text = "Reviewed", kept = emptyList())) }
        store.comment(task.uuid, "Unrelated arrival")
        val bytes = byteArrayOf(0, -1, 13, 10)
        val saved = draft.apply(store, listOf(TaskMediaInput("new.txt", "text/plain", bytes)))
        assertEquals(listOf("new.txt"), saved.attachments.map { it.name })
        assertTrue(saved.log.any { it.text == "Unrelated arrival" })
        assertArrayEquals(bytes, store.readMedia(task.uuid, null, 0u, saved.attachments.single().token).data)
    }

    @Test fun pickerRequiresWholeOriginalDraftAndFreshToken() {
        val draft = TaskContentDraft.new(key, "computer", TaskStage.TODO, TaskTarget.AnySession)
        val ticket = TaskContentPicker(draft = draft)
        assertTrue(ticket.matches(draft)); assertNotEquals(ticket.token, TaskContentPicker(draft = draft).token)
        assertFalse(ticket.matches(draft.copy(form = draft.form.copy(sequence = 1))))
        assertFalse(ticket.matches(draft.copy(form = draft.form.copy(target = TaskTarget.Session(SessionRef("other", "label", "launch_id:original"))))))
        assertFalse(ticket.matches(draft.copy(state = TaskContentDraft.State.Applying)))
        assertFalse(ticket.matches(draft.copy(scope = draft.scope.copy(connectionId = "foreign"))))
        assertFalse(ticket.matches(draft.copy(scope = draft.scope.copy(project = "foreign"))))
        assertFalse(ticket.matches(draft.copy(computer = "foreign")))
        assertFalse(ticket.matches(draft.copy(id = java.util.UUID.randomUUID().toString())))
    }
    private val secret = javax.crypto.KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    private fun drafts(root: java.io.File, entries: Int = 32, sync: () -> Unit = {}) =
        TaskContentDraftFiles(root, { secret }, maxEntries = entries, syncDirectory = sync)
    @Test fun applyingSurvivesRestartAndCannotBeRecreatedReleasedOrEdited() {
        val root = folders.newFolder(); val files = drafts(root)
        val draft = files.create(TaskContentDraft.new(key, "computer", TaskStage.TODO, TaskTarget.AnySession, "Kept"))
        val held = files.begin(draft)
        assertEquals(held, drafts(root).read(draft.scope, true))
        assertTrue(runCatching { files.create(draft) }.isFailure)
        assertTrue(runCatching { files.begin(draft) }.isFailure)
        assertTrue(runCatching { files.checkpoint(draft, 1, draft.form.json()) }.isFailure)
        assertTrue(runCatching { files.release(draft) }.isFailure)
        files.completed(draft); files.release(draft)
        assertNull(files.read(draft.scope, true))
    }
    @Test fun monotonicCheckpointAndQuotaKeepPreviousBytes() {
        val root = folders.newFolder(); val files = drafts(root, entries = 1)
        val draft = files.create(TaskContentDraft.new(key, "computer", TaskStage.TODO, TaskTarget.AnySession))
        val next = files.checkpoint(draft, 4, draft.form.copy(text = "Latest").json())
        assertTrue(runCatching { files.checkpoint(draft, 3, draft.form.json()) }.isFailure)
        assertTrue(runCatching { files.checkpoint(draft, 4, draft.form.json()) }.isFailure)
        assertTrue(runCatching { files.create(draft.copy(scope = draft.scope.copy(project = "other"))) }.isFailure)
        assertEquals(next, files.read(draft.scope, true))
    }
    @Test fun failedFsyncIsVisibleAndNeverAuthorizesAnApply() {
        val root = folders.newFolder(); val normal = drafts(root)
        val draft = normal.create(TaskContentDraft.new(key, "computer", TaskStage.TODO, TaskTarget.AnySession))
        assertTrue(runCatching { drafts(root, sync = { error("forced private sync refusal") }).begin(draft) }.isFailure)
        assertEquals(TaskContentDraft.State.Applying, normal.read(draft.scope, true)?.state)
        assertTrue(runCatching { normal.begin(draft) }.isFailure)
    }
}
