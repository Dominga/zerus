package app.zerus.mobile

import app.zerus.mobile.replica.*
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import javax.crypto.KeyGenerator

class TaskContentIdentityTest {
    @get:Rule val folders = TemporaryFolder()
    @Test fun restartKeepsAnySessionDraftHeldWhenRelayComputerIdNamesAReplacementMachine() = runBlocking {
        val key = ProjectKey("connection", "swarm", "project")
        val connection = Connection(key.connectionId, "Fixture", "https://relay.example.test", "private")
        val old = Machine(key.connectionId, "same-relay-computer", "Original", true, machineId = "original-native-machine")
        val replacement = old.copy(machineId = "replacement-native-machine", name = "Replacement")
        val files = TaskContentDraftFiles(folders.newFolder(), { secret })
        val original = files.create(TaskContentDraft.new(key, old.id, old.machineId, TaskStage.TODO, TaskTarget.AnySession, "Original private draft"))
        val owner = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val model = TaskContentModel({ "message:$it" }, owner, TaskReplicas(folders.newFolder()), files,
            read = { byteArrayOf() }, remove = { _, _ -> fail("original files must stay") }, upload = { _, _ -> true }, changed = { _, _ -> fail("replacement completion") })
        try {
            val context = TaskEditContext(connection, key, "Fixture", replacement, emptyMap())
            model.open(context, TaskContentDraft.new(key, replacement.id, replacement.machineId, TaskStage.TODO, TaskTarget.AnySession))
            val ready = withTimeoutOrNull(10_000) { while (model.state?.draft == null) delay(5); true }
            assertEquals("original draft must open: " + model.state?.error, true, ready)
            assertEquals(original.id, model.state!!.draft!!.id)
            assertTrue("A reused relay computer ID must not rehome an Any-session draft", model.state!!.held)
            model.change(model.state!!.token) { it.copy(text = "Must not replace original") }
            model.save(model.state!!.token)
            assertEquals("Original private draft", files.read(original.scope, true)!!.form.text)
        } finally { owner.cancel() }
    }
    private val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
}
