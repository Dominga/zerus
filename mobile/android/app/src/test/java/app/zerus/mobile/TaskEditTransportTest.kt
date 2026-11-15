package app.zerus.mobile

import app.zerus.mobile.replica.TaskEditException
import app.zerus.mobile.replica.TaskEditQualification
import app.zerus.mobile.replica.TaskMediaInput
import app.zerus.mobile.replica.TaskStore
import app.zerus.mobile.replica.taskEditRankBetween
import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import javax.crypto.KeyGenerator

/** The actual private native Endpoint answers this phone's signed, encrypted frames. */
class TaskEditTransportTest {
    @get:Rule val folders = TemporaryFolder()
    private val fixture = JSONObject(javaClass.classLoader!!.getResource("encrypted-workspace-v1.json")!!.readText())
    private val host = WorkspaceIdentity.parse(fixture.getJSONObject("gateway_identity"))
    private val phone = WorkspaceIdentity.parse(fixture.getJSONObject("phone_identity"))
    private val membership = WorkspaceMembership.parse(fixture.getJSONObject("final_state"))

    private class NativeFixture(binary: String, fixture: File, root: File) : AutoCloseable {
        private val errorLog = File(root, "native-stderr.txt")
        private val process = ProcessBuilder(binary, fixture.path).directory(root).redirectError(errorLog).apply {
            environment().keys.filter { it.startsWith("HGS_") }.forEach { environment().remove(it) }
            val temp = File(root, "tmp").apply { mkdir() }
            environment()["TMPDIR"] = temp.path
        }.start()
        private val writer = process.outputStream.bufferedWriter()
        private val reader = process.inputStream.bufferedReader()
        private val worker = Executors.newSingleThreadExecutor { action -> Thread(action, "private-task-fixture").apply { isDaemon = true } }
        @Synchronized fun call(command: String, payload: JSONObject? = null): JSONObject {
            val request = JSONObject().put("command", command)
            payload?.keys()?.forEach { request.put(it, payload.get(it)) }
            writer.write(request.toString()); writer.newLine(); writer.flush()
            val reply = worker.submit<String> {
                val text = StringBuilder()
                while (true) {
                    val value = reader.read()
                    check(value >= 0) { "The private native task fixture exited before its reply: ${errorLog.readText().takeLast(2048)}" }
                    if (value == '\n'.code) break
                    check(text.length < 3 * 1024 * 1024) { "Private native task reply exceeded its bound." }
                    text.append(value.toChar())
                }
                text.toString()
            }
            return try { JSONObject(reply.get(15, TimeUnit.SECONDS)) }
            catch (error: java.util.concurrent.TimeoutException) {
                throw IllegalStateException("Private native task fixture did not answer $command within 15 seconds; child alive=${process.isAlive}; stderr=${errorLog.readText().takeLast(2048)}", error)
            }
        }
        override fun close() {
            writer.close()
            val exited = process.waitFor(5, TimeUnit.SECONDS)
            if (!exited) { process.destroyForcibly(); check(process.waitFor(5, TimeUnit.SECONDS)) }
            reader.close(); worker.shutdownNow()
            check(worker.awaitTermination(5, TimeUnit.SECONDS)) { "The private native fixture reader did not settle." }
            check(exited && process.exitValue() == 0) { "The private native fixture failed or cleanup timed out: ${errorLog.readText().takeLast(2048)}" }
        }
    }
    private data class World(val native: NativeFixture, val server: MockWebServer, val api: RelayApi, val connection: Connection,
                             val key: ProjectKey, val machine: Machine, val replicas: TaskReplicas, val store: TaskStore, val task: String,
                             val files: TaskEditDraftFiles, val scope: TaskEditDraftScope, var posts: Int = 0, var corruptNextResult: Boolean = false)
    private fun world(syncDirectory: () -> Unit = {}, block: suspend (World) -> Unit) = runBlocking {
        val publicFixture = folders.newFile("workspace-fixture.json").apply { writeText(fixture.toString()) }
        val binary = checkNotNull(System.getProperty("zerus.taskFixtureBinary")) { "Build the private native task fixture before the client tests." }
        NativeFixture(binary, publicFixture, folders.newFolder()).use { native ->
            val seed = native.call("seed"); val computer = seed.getString("machine"); val task = seed.getString("task")
            MockWebServer().use { server ->
                server.start()
                val connection = Connection("private-task-connection", "Fixture", server.url("/").toString().trimEnd('/'), "fixture-token", 2)
                val key = ProjectKey(connection.id, "swarm-1", "project-1")
                val journal = MemoryWorkspaceJournal()
                journal.create(connection, WorkspaceState(phone, WorkspaceCandidate.parse(fixture.getJSONObject("candidate")), host.device, 0,
                    fixture.getJSONObject("bootstrap_snapshot").getJSONObject("state").getString("head"), membership,
                    mapOf(computer to WorkspaceRoute(computer, host.device.deviceId, host.device.deviceId, computer)),
                    WorkspaceApproval.parse(fixture.getJSONObject("approval"))))
                val replicas = TaskReplicas(folders.newFolder())
                replicas.store(key, "Fixture").use { store ->
                    store.exchange().use { exchange ->
                        repeat(8) {
                            val request = exchange.nextRequest() ?: return@repeat
                            exchange.accept(native.call("exchange", JSONObject().put("payload", JSONObject(request))).toString())
                        }
                    }
                    assertFalse(store.get(task).writable)
                    val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
                    val world = World(native, server, RelayApi(workspace = journal), connection, key,
                        Machine(connection.id, computer, "Fixture host", true, machineId = computer), replicas, store, task,
                        TaskEditDraftFiles(folders.newFolder(), { secret }, syncDirectory = syncDirectory), TaskEditDraftScope(connection.id, key.swarmId, key.id, task))
                    server.dispatcher = object : Dispatcher() {
                        override fun dispatch(request: RecordedRequest): MockResponse {
                            if (request.path!!.startsWith("/v2/membership")) return MockResponse().setBody(JSONObject().put("changes", JSONArray())
                                .put("membership_revision", membership.revision).put("membership_head", membership.head).put("next_after", JSONObject.NULL).toString())
                            if (request.method == "POST" && request.path == "/v2/requests") {
                                world.posts++
                                val body = request.body.readUtf8()
                                assertFalse(body.contains("signing_private_key")); assertFalse(body.contains("encryption_private_key"))
                                val result = native.call("execute", JSONObject().put("envelope", JSONObject(body).getJSONObject("envelope")))
                                if (world.corruptNextResult) {
                                    world.corruptNextResult = false
                                    result.getJSONObject("envelope").put("signature", WorkspaceProtocol.encode(ByteArray(64)))
                                }
                                return MockResponse().setBody(result.toString())
                            }
                            return MockResponse().setResponseCode(404).setBody("{}")
                        }
                    }
                    block(world)
                }
            }
        }
    }
    private suspend fun transport(w: World, advertised: Boolean = true) = TaskEditTransport.open(w.api, w.connection, w.key, w.task, w.machine,
        mapOf(MachineKey(w.connection.id, w.machine.id) to if (advertised) setOf("task_admitted_edit_v1") else emptySet()))
    private suspend fun qualify(w: World, transport: TaskEditTransport, qualification: TaskEditQualification): Pair<app.zerus.mobile.replica.TaskEditDraft, TaskEditSavedDraft> {
        val request = qualification.request()
        val saved = w.files.create(w.scope, w.machine.machineId, host.device.deviceId, request.requestId, qualification.editId(), w.store.get(w.task).text)
        val proof = transport.proof(request) { w.files.reserve(w.scope, saved.draftId, request.requestId, request.payload) }
        val draft = qualification.accept(proof)
        w.files.settle(w.scope, saved.draftId, request.requestId, TaskEditDraftOutcome.Acknowledged)
        transport.verified(request.requestId)
        return draft to saved
    }

    @Test fun anUnadvertisedHostRefusesBeforeAnyRequest() = world { w ->
        assertTrue(runCatching { transport(w, advertised = false) }.isFailure)
        assertEquals(0, w.posts)
        assertEquals("Original requirement", w.store.get(w.task).text)
    }
    @Test fun anActualNativeCommentCommitsOnceAndThePhoneCacheWaitsForDocumentSync() = world { w ->
        val transport = transport(w)
        w.store.qualifyEdit(w.task, transport.route).use { qualification ->
            val (draft, saved) = qualify(w, transport, qualification)
            draft.use {
                draft.comment("One explicit phone comment", emptyList())
                draft.rank(taskEditRankBetween(null, "V"))
                w.files.freeze(w.scope, saved.draftId, draft.freeze())
                val request = checkNotNull(draft.nextRequest())
                val proof = transport.proof(request) { w.files.reserve(w.scope, saved.draftId, request.requestId, request.payload) }
                val result = draft.accept(proof)
                assertTrue(result.complete); assertTrue(result.edited)
                w.files.settle(w.scope, saved.draftId, request.requestId, TaskEditDraftOutcome.Completed)
                transport.verified(request.requestId)
                assertNull(draft.nextRequest())
                assertEquals(2, w.posts)
                assertEquals(1, w.native.call("inspect").getJSONArray("log").objects().count { it.string("text") == "One explicit phone comment" })
                assertTrue(w.native.call("inspect").getString("rank") < "V")
                assertFalse(w.store.get(w.task).writable)
                assertEquals(TaskEditDraftState.Completed, w.files.read(w.scope)!!.state)
            }
        }
    }
    @Test fun corruptProofAfterARealHostCommitStaysUnknownAndCannotPostAgain() = world { w ->
        val transport = transport(w)
        w.store.qualifyEdit(w.task, transport.route).use { qualification ->
            val (draft, saved) = qualify(w, transport, qualification)
            draft.use {
                draft.comment("Committed before the result was lost", emptyList())
                val frozen = draft.freeze(); w.files.freeze(w.scope, saved.draftId, frozen)
                val request = checkNotNull(draft.nextRequest()); w.corruptNextResult = true
                val proof = transport.proof(request) { w.files.reserve(w.scope, saved.draftId, request.requestId, request.payload) }
                val failure = runCatching { draft.accept(proof) }.exceptionOrNull()
                assertTrue(failure.toString(), failure is TaskEditException.Unknown)
                draft.holdUnknown(); w.files.settle(w.scope, saved.draftId, request.requestId, TaskEditDraftOutcome.Unknown)
                assertEquals(1, w.native.call("inspect").getJSONArray("log").objects().count { it.string("text") == "Committed before the result was lost" })
                assertTrue(runCatching { draft.nextRequest() }.isFailure)
                assertTrue(runCatching { transport.proof(request) { w.files.reserve(w.scope, saved.draftId, request.requestId, request.payload) } }.isFailure)
                assertEquals(2, w.posts)
                assertEquals(frozen, w.files.read(w.scope)!!.manifest)
                assertEquals(TaskEditDraftState.Unknown, w.files.read(w.scope)!!.state)
            }
        }
    }
    @Test fun everyMediaByteArrivesBeforeTheOriginalFinalEditCompletes() = world { w ->
        val transport = transport(w)
        w.store.qualifyEdit(w.task, transport.route).use { qualification ->
            val (draft, saved) = qualify(w, transport, qualification)
            draft.use {
                val bytes = ByteArray(800 * 1024).also { java.util.Random(7).nextBytes(it) }
                draft.content("Reviewed title", "Reviewed requirement", emptyList(), listOf(TaskMediaInput("fixture.bin", "application/octet-stream", bytes, "")))
                w.files.freeze(w.scope, saved.draftId, draft.freeze())
                var complete = false
                var parts = 0
                while (!complete) {
                    check(++parts <= 16)
                    val request = checkNotNull(draft.nextRequest())
                    val proof = transport.proof(request) { w.files.reserve(w.scope, saved.draftId, request.requestId, request.payload) }
                    val progress = draft.accept(proof)
                    complete = progress.complete
                    w.files.settle(w.scope, saved.draftId, request.requestId,
                        if (complete) TaskEditDraftOutcome.Completed else TaskEditDraftOutcome.Acknowledged)
                    transport.verified(request.requestId)
                    val native = w.native.call("inspect")
                    if (!complete) assertEquals("Original requirement", native.string("text"))
                    else {
                        assertEquals(qualification.editId(), request.requestId)
                        val file = native.getJSONArray("attachments").getJSONObject(0)
                        assertEquals(bytes.size.toLong(), file.getLong("size"))
                        assertEquals(MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }, file.getString("sha256"))
                    }
                }
                assertTrue(parts > 1)
                assertEquals("Original requirement", w.store.get(w.task).text)
            }
        }
    }
    @Test fun theSessionWrapperPersistsTheOriginalFormAndCompletesOnlyItsOwnTask() = world { w ->
        val transport = transport(w)
        TaskEditSession.open(w.replicas, "Fixture", transport, w.files).use { session ->
            assertEquals(w.scope, session.scope)
            val form = TaskEditForm.initial(session.view).copy(comment = "From the original editor", sequence = 1)
            session.persist(form.sequence, form.json())
            session.save(form.sequence, form.json(), TaskEditInput(comment = form.comment))
            assertFalse("A completed live edit handle remains editable", session.editable)
            assertEquals(TaskEditDraftState.Completed, w.files.read(w.scope)!!.state)
            assertEquals(form.comment, TaskEditForm.read(w.files.read(w.scope)!!.changes, 1).comment)
            assertEquals(1, w.native.call("inspect").getJSONArray("log").objects().count { it.string("text") == form.comment })
            assertEquals(2, w.posts)
        }
    }
    @Test fun theSessionWrapperKeepsAnUnverifiablePostCommitResultWithoutAnyRetry() = world { w ->
        TaskEditSession.open(w.replicas, "Fixture", transport(w), w.files).use { session ->
            val form = TaskEditForm.initial(session.view).copy(comment = "Keep this uncertain edit", sequence = 1)
            w.corruptNextResult = true
            val error = runCatching { session.save(1, form.json(), TaskEditInput(comment = form.comment)) }.exceptionOrNull()
            assertTrue(error.toString(), error is TaskEditException.Unknown)
            assertEquals(TaskEditDraftState.Unknown, w.files.read(w.scope)!!.state)
            assertTrue(runCatching { session.save(2, form.json(), TaskEditInput(comment = form.comment)) }.isFailure)
            assertEquals(2, w.posts)
            assertEquals(1, w.native.call("inspect").getJSONArray("log").objects().count { it.string("text") == form.comment })
        }
    }
    @Test fun aForwardedRouteCannotOpenEvenWhenTheHostAdvertisesTheFeature() = world { w ->
        val journal = checkNotNull(w.api.workspace)
        val before = journal.load(w.connection)
        val original = checkNotNull(before.routes[w.machine.id])
        journal.update(w.connection, before, before.withRoutes(mapOf(w.machine.id to original.copy(routeId = "forwarded-route"))))
        assertTrue(runCatching { transport(w) }.isFailure)
        assertEquals(0, w.posts)
    }
    @Test fun aRouteReplacementBetweenOpenAndPostRefusesBeforeInput() = world { w ->
        val original = transport(w)
        w.store.qualifyEdit(w.task, original.route).use { qualification ->
            val journal = checkNotNull(w.api.workspace)
            val before = journal.load(w.connection)
            val route = checkNotNull(before.routes[w.machine.id])
            journal.update(w.connection, before, before.withRoutes(mapOf(w.machine.id to route.copy(machineId = "changed-machine"))))
            var reserved = false
            assertTrue(runCatching { original.proof(qualification.request()) { reserved = true } }.isFailure)
            assertFalse(reserved)
            assertEquals(0, w.posts)
        }
    }
    @Test fun aRetainedOriginalRequestCannotBePostedAgainAfterVerifiedSettlement() = world { w ->
        val transport = transport(w)
        w.store.qualifyEdit(w.task, transport.route).use { qualification ->
            val (draft, _) = qualify(w, transport, qualification)
            draft.use {
                var reserved = false
                assertTrue(runCatching { transport.proof(qualification.request()) { reserved = true } }.isFailure)
                assertFalse(reserved)
                assertEquals(1, w.posts)
            }
        }
    }
    @Test fun aDraftDurabilityFailurePreventsEveryNativePost() = world(syncDirectory = { error("Forced private-directory sync failure") }) { w ->
        val failure = runCatching { TaskEditSession.open(w.replicas, "Fixture", transport(w), w.files) }.exceptionOrNull()
        assertNotNull(failure)
        assertEquals(0, w.posts)
        assertEquals("Original requirement", w.native.call("inspect").string("text"))
    }

    @Test fun aRequestReservationSyncFailurePreventsTheFirstPost() {
        val syncs = java.util.concurrent.atomic.AtomicInteger()
        world(syncDirectory = { if (syncs.incrementAndGet() == 2) error("Forced request reservation sync failure") }) { w ->
            assertTrue(runCatching { TaskEditSession.open(w.replicas, "Fixture", transport(w), w.files) }.isFailure)
            assertEquals(0, w.posts)
            assertTrue(w.files.read(w.scope)!!.state in setOf(TaskEditDraftState.Pending, TaskEditDraftState.Unknown))
        }
    }

    @Test fun aLocalValidationFailureKeepsTheOriginalEditableAndDropsAbandonedFields() = world { w ->
        TaskEditSession.open(w.replicas, "Fixture", transport(w), w.files).use { session ->
            val abandoned = TaskEditContent("Must not leak", "Abandoned replacement", emptyList(), emptyList())
            assertTrue(runCatching { session.save(1, "invalid local form", TaskEditInput(content = abandoned, rank = "")) }.isFailure)
            assertTrue(session.editable)
            assertEquals(1, w.posts)
            session.save(2, "corrected local form", TaskEditInput(comment = "Only corrected comment"))
            val actual = w.native.call("inspect")
            assertEquals("Original requirement", actual.string("text"))
            assertTrue(actual.toString().contains("Only corrected comment"))
            assertEquals(2, w.posts)
        }
    }

}
