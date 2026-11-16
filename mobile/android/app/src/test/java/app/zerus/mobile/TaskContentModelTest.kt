package app.zerus.mobile

import app.zerus.mobile.replica.*
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.util.UUID
import javax.crypto.KeyGenerator

class TaskContentModelTest {
    @get:Rule val folders = TemporaryFolder()
    private val key = ProjectKey("connection", "swarm", "project")
    private val connection = Connection("connection", "Fixture", "https://relay.example.test", "private")
    private val context = TaskEditContext(connection, key, "Fixture", Machine("connection", "computer", "Fixture", true), emptyMap())
    private val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    private suspend fun await(model: TaskContentModel, description: String, predicate: (TaskContentState) -> Boolean) {
        val ended = withTimeoutOrNull(10_000) { while (model.state?.let(predicate) != true) delay(5); true }
        assertEquals(description + ": " + model.state?.error, true, ended)
    }
    @Test fun projectSwitchAndLatePickerCannotRehomeOriginalSaveOrConsumeTwice() = runBlocking {
        val replicas = TaskReplicas(folders.newFolder()); val files = TaskContentDraftFiles(folders.newFolder(), { secret })
        val owner = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val completions = mutableListOf<Pair<ProjectKey, String>>()
        var upload = true; var removed = 0
        val model = TaskContentModel({ "message:$it" }, owner, replicas, files, read = { byteArrayOf(0, -1) }, remove = { _, _ -> removed++ },
            upload = { _, _ -> upload }, changed = { original, task -> completions += original.key to task })
        try {
            val proposed = TaskContentDraft.new(key, "computer", TaskStage.TODO, TaskTarget.AnySession, "Original project")
            model.open(context, proposed); await(model, "draft open") { !it.loading && it.draft != null }
            var ticket: TaskContentPicker? = null
            model.picker(model.state!!.token) { ticket = it }; await(model, "original picker") { ticket != null }
            val imported = Attachment(UUID.randomUUID().toString(), "fixture.txt", "text/plain", 2, "ab".repeat(32))
            model.picked(ticket!!, listOf({ imported })); await(model, "private import") { !it.busy && it.draft!!.form.files.size == 1 }
            var replayImports = 0
            model.picked(ticket!!, listOf({ replayImports++; imported })); assertEquals(0, replayImports)
            val foreign = ProjectKey("other-connection", "other-swarm", "other-project")
            model.open(context.copy(key = foreign), TaskContentDraft.new(foreign, "computer", TaskStage.TODO, TaskTarget.AnySession))
            // The open editor remains the original even when another project's UI asks to open one.
            assertEquals(key.id, model.state!!.draft!!.scope.project)
            model.save(model.state!!.token); await(model, "original save complete") { it.complete }
            assertEquals(key, completions.single().first)
            replicas.store(key, "Fixture").use { store ->
                val task = store.get(completions.single().second); val file = task.attachments.single()
                assertArrayEquals(byteArrayOf(0, -1), store.readMedia(task.uuid, null, 0u, file.token).data)
            }
            replicas.store(foreign, "Foreign").use { assertTrue(it.list().tasks.isEmpty()) }
            model.save(model.state!!.token); assertEquals(1, completions.size)
        } finally { owner.cancel() }
    }
    @Test fun unsupportedPickerAndCapabilityLossKeepBytesWithoutApplying() = runBlocking {
        val replicas = TaskReplicas(folders.newFolder()); val files = TaskContentDraftFiles(folders.newFolder(), { secret })
        val owner = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        var upload = false; var opened = 0; var completed = 0
        val model = TaskContentModel({ "message:$it" }, owner, replicas, files, read = { byteArrayOf(1) }, remove = { _, _ -> },
            upload = { _, _ -> upload }, changed = { _, _ -> completed++ })
        try {
            model.open(context, TaskContentDraft.new(key, "computer", TaskStage.TODO, TaskTarget.AnySession, "Kept"))
            await(model, "draft open") { !it.loading && it.draft != null }
            val token = model.state!!.token
            model.picker(token) { opened++ }; assertEquals(0, opened); assertFalse(model.state!!.busy)
            upload = true; var ticket: TaskContentPicker? = null
            model.picker(token) { opened++; ticket = it }; await(model, "picker open") { ticket != null }
            model.picked(ticket!!, listOf({ Attachment(UUID.randomUUID().toString(), "a.txt", "text/plain", 1, "ab".repeat(32)) }))
            await(model, "import saved") { !it.busy && it.draft!!.form.files.size == 1 }
            upload = false; model.save(token)
            await(model, "save refusal settled") { !it.busy }
            assertEquals(0, completed); assertEquals(1, model.state!!.draft!!.form.files.size)
            replicas.store(key, "Fixture").use { assertTrue(it.list().tasks.isEmpty()) }
        } finally { owner.cancel() }
    }
    @Test fun foreignContextCannotCreateOrOverwriteAnOriginalDraft() = runBlocking {
        val owner = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val files = TaskContentDraftFiles(folders.newFolder(), { secret })
        val model = TaskContentModel({ "message:$it" }, owner, TaskReplicas(folders.newFolder()), files,
            read = { byteArrayOf() }, remove = { _, _ -> }, upload = { _, _ -> true }, changed = { _, _ -> fail("foreign completion") })
        try {
            val draft = TaskContentDraft.new(key.copy(id = "foreign"), "computer", TaskStage.TODO, TaskTarget.AnySession)
            model.open(context, draft); await(model, "foreign draft refused") { !it.loading && it.error.isNotEmpty() }
            assertNull(files.read(draft.scope, true))
        } finally { owner.cancel() }
    }

    @Test fun cancelledPickerAndPartialImportFailureNeverConsumeAgainOrLoseOriginalFiles() = runBlocking {
        val owner = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val removed = mutableListOf<String>()
        val model = TaskContentModel({ "message:$it" }, owner, TaskReplicas(folders.newFolder()), TaskContentDraftFiles(folders.newFolder(), { secret }),
            read = { byteArrayOf(1) }, remove = { id, _ -> removed += id }, upload = { _, _ -> true }, changed = { _, _ -> fail("no save requested") })
        try {
            model.open(context, TaskContentDraft.new(key, "computer", TaskStage.TODO, TaskTarget.AnySession, "Kept"))
            await(model, "draft open") { !it.loading && it.draft != null }
            val token = model.state!!.token; var first: TaskContentPicker? = null
            model.picker(token) { first = it }; await(model, "first picker") { first != null }
            model.picked(first!!, emptyList()); assertFalse(model.state!!.busy)
            var imports = 0
            model.picked(first!!, listOf({ imports++; error("cancelled callback must not import") }))
            delay(20); assertEquals(0, imports)
            var second: TaskContentPicker? = null
            model.picker(token) { second = it }; await(model, "second picker") { second != null }
            val file = Attachment(UUID.randomUUID().toString(), "first.txt", "text/plain", 1, "ab".repeat(32))
            model.picked(second!!, listOf({ file }, { error("forced provider refusal") }))
            await(model, "partial failure settled") { !it.busy && it.error.contains("forced provider refusal") }
            assertTrue(model.state!!.draft!!.form.files.isEmpty())
            assertEquals(listOf(file.id), removed)
            assertEquals("Kept", model.state!!.draft!!.form.text)
        } finally { owner.cancel() }
    }
}
