package app.zerus.mobile

import app.zerus.mobile.replica.SessionRef
import app.zerus.mobile.replica.TaskMediaInput
import app.zerus.mobile.replica.TaskPosition
import app.zerus.mobile.replica.TaskStage
import app.zerus.mobile.replica.TaskStore
import app.zerus.mobile.replica.TaskTarget
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
import java.util.Collections

/** Real signed/encrypted client transport; the private peer scripts action results, not native input. */
class TaskRequestNegotiationTest {
    @get:Rule val folders = TemporaryFolder()
    private val fixture = JSONObject(javaClass.classLoader!!.getResource("encrypted-workspace-v1.json")!!.readText())
    private val host = WorkspaceIdentity.parse(fixture.getJSONObject("gateway_identity"))
    private val phone = WorkspaceIdentity.parse(fixture.getJSONObject("phone_identity"))
    private val roster = WorkspaceMembership.parse(fixture.getJSONObject("final_state"))
    private val hostKey = MachineKey("workspace-a", "computer-1")
    private data class World(val connection: Connection, val project: ProjectKey, val sync: TaskSync, val task: String,
                             val calls: MutableList<JSONObject>, val computer: TaskStore, val replicas: TaskReplicas,
                             @Volatile var outcome: String = "completed", @Volatile var beforeSend: () -> Unit = {},
                             @Volatile var modifyExchange: (JSONObject) -> Unit = {})
    private fun world(features: Map<MachineKey, Set<String>> = mapOf(hostKey to setOf("task_session_identity_v2")),
                      protocol: Int = 2, block: suspend (World) -> Unit) = runBlocking {
        MockWebServer().use { server ->
            server.start()
            val connection = Connection("workspace-a", "Fixture", server.url("/").toString().trimEnd('/'), "fixture-token", protocol)
            val key = ProjectKey(connection.id, "swarm-1", "project-1")
            val journal = MemoryWorkspaceJournal()
            journal.create(connection, WorkspaceState(phone, WorkspaceCandidate.parse(fixture.getJSONObject("candidate")), host.device, 0,
                fixture.getJSONObject("bootstrap_snapshot").getJSONObject("state").getString("head"), roster,
                mapOf("computer-1" to WorkspaceRoute("computer-1", host.device.deviceId, "route-fixture", "computer-1")),
                WorkspaceApproval.parse(fixture.getJSONObject("approval"))))
            TaskStore(folders.newFolder().path, key.id, "Fixture").use { computer ->
                val task = computer.add("Private requirement", TaskStage.TODO, TaskTarget.AnySession, TaskPosition.End)
                val replicas = TaskReplicas(folders.newFolder())
                val world = World(connection, key, TaskSync({ RelayApi(workspace = journal) }, replicas, capabilities = { features }),
                    task.uuid, Collections.synchronizedList(mutableListOf()), computer, replicas)
                server.dispatcher = object : Dispatcher() {
                    override fun dispatch(request: RecordedRequest): MockResponse {
                        if (request.path!!.startsWith("/v2/membership")) return MockResponse().setBody(JSONObject().put("changes", JSONArray())
                            .put("membership_revision", roster.revision).put("membership_head", roster.head).put("next_after", JSONObject.NULL).toString())
                        if (request.method != "POST") return MockResponse().setResponseCode(404).setBody("{}")
                        val outer = JSONObject(request.body.readUtf8())
                        val encrypted = if (protocol == 2) WorkspaceEnvelope.parse(outer.getJSONObject("envelope")) else null
                        val clear = encrypted?.let {
                            assertEquals(host.device.deviceId, it.header.recipientDeviceId)
                            assertEquals("route-fixture", it.header.routeId)
                            JSONObject(String(it.open(host, phone.device, it.header), Charsets.UTF_8))
                        } ?: outer
                        world.calls += clear
                        val operation = clear.getString("operation"); val payload = clear.getJSONObject("payload")
                        if (operation == "task_send") world.beforeSend()
                        val state = if (operation == "task_send") world.outcome else "completed"
                        val result: Any = when (operation) {
                            "task_exchange" -> JSONObject(computer.answerExchange(payload.toString())).also { world.modifyExchange(it) }
                            "task_done" -> JSONObject().put("schema", 1).put("tasks", JSONArray()).put("next", JSONObject.NULL)
                                .put("total", 0).put("recent", 0).put("damaged", 0)
                            else -> JSONObject.NULL
                        }
                        val id = encrypted?.header?.requestId ?: clear.getString("request_id")
                        val reply = JSONObject().put("request_id", id).put("state", state).put("result", result)
                            .put("error", if (state == "completed") JSONObject.NULL else "Private action outcome")
                        if (encrypted == null) return MockResponse().setBody(reply.toString())
                        val header = encrypted.header.copy(senderDeviceId = host.device.deviceId, recipientDeviceId = phone.device.deviceId,
                            senderKeyGeneration = host.device.keyGeneration, recipientKeyGeneration = phone.device.keyGeneration, kind = "result")
                        return MockResponse().setBody(JSONObject().put("request_id", id).put("state", state)
                            .put("envelope", WorkspaceEnvelope.seal(host, phone.device, header, reply.toString().toByteArray()).json()).toString())
                    }
                }
                block(world)
            }
        }
    }
    private suspend fun send(world: World) = world.sync.send(world.connection, world.project, "Fixture", "computer-1", world.task,
        "codex/current-label", sessionId = "launch_id: original ")
    @Test fun theOriginalAdvertisedHostReceivesSchemaTwoWithTheExactOpaqueKey() = world { w ->
        assertEquals(TaskOutcome.Done, send(w))
        val payload = w.calls.single { it.getString("operation") == "task_send" }.getJSONObject("payload")
        assertEquals(2, payload.getInt("schema")); assertEquals("launch_id: original ", payload.getString("session_id"))
    }
    @Test fun aForeignHostOrConnectionAdvertisementCannotOptInThisHost() {
        for (features in listOf(emptyMap(), mapOf(MachineKey("workspace-a", "other") to setOf("task_session_identity_v2")),
            mapOf(MachineKey("other-workspace", "computer-1") to setOf("task_session_identity_v2")))) world(features) { w ->
            assertEquals(TaskOutcome.Done, send(w))
            val payload = w.calls.single { it.getString("operation") == "task_send" }.getJSONObject("payload")
            assertEquals(1, payload.getInt("schema")); assertFalse(payload.has("session_id"))
        }
    }
    @Test fun theLegacyTransportSelectsSchemaOneBeforeAnyRequest() = world(protocol = 1) { w ->
        assertEquals(TaskOutcome.Done, send(w))
        val payload = w.calls.single { it.getString("operation") == "task_send" }.getJSONObject("payload")
        assertEquals(1, payload.getInt("schema")); assertFalse(payload.has("session_id"))
    }
    @Test fun aSchemaTwoRefusalNeverFallsBackToAnotherRequest() = world { w ->
        w.outcome = "failed"; assertTrue(send(w) is TaskOutcome.Refused)
        val requests = w.calls.filter { it.getString("operation") == "task_send" }
        assertEquals(1, requests.size); assertEquals(2, requests.single().getJSONObject("payload").getInt("schema"))
    }
    @Test fun aSchemaTwoUnknownNeverFallsBackOrRepeatsTheOriginal() = world { w ->
        w.outcome = "uncertain"; assertTrue(send(w) is TaskOutcome.Uncertain); assertTrue(send(w) is TaskOutcome.Refused)
        val requests = w.calls.filter { it.getString("operation") == "task_send" }
        assertEquals(1, requests.size); assertEquals(2, requests.single().getJSONObject("payload").getInt("schema"))
    }
    @Test fun anIdentityBoundDoneFilterUsesTheSameAdvertisedSchema() = world { w ->
        w.sync.donePage(w.connection, w.project, "computer-1", null, session = SessionRef("computer-1", "codex/old-label", "launch_id:original"))
        val payload = w.calls.single { it.getString("operation") == "task_done" }.getJSONObject("payload")
        assertEquals(2, payload.getInt("schema")); assertEquals("launch_id:original", payload.getJSONObject("session").getString("session_id"))
    }
    private suspend fun largeMedia(w: World): ByteArray {
        w.sync.exchangeNow(w.connection, w.project, "Fixture", "computer-1")
        val data = ByteArray(900 * 1024).also { java.util.Random(7).nextBytes(it) }
        w.replicas.store(w.project, "Fixture").use { phone ->
            val original = phone.get(w.task)
            phone.editContent(w.task, original.text, emptyList(), listOf(TaskMediaInput("fixture.png", "image/png", data)), original.contentRevision)
        }
        w.calls.clear()
        return data
    }
    private fun uploadFrames(w: World) = w.calls.filter { it.getString("operation") == "task_exchange" }.flatMap { call ->
        call.getJSONObject("payload").optJSONArray("updates")?.objects().orEmpty().map { java.util.Base64.getDecoder().decode(it.getString("bytes")) }
    }.filter { it.size >= 8 && it.copyOfRange(0, 8).contentEquals("ZERUSUP1".toByteArray()) }
    private fun verifyMedia(w: World, data: ByteArray) {
        val media = w.computer.media(w.task, null).single()
        assertArrayEquals(data, w.computer.readMedia(w.task, null, 0u, media.token).data)
    }
    @Test fun advertisedUploadDeliversEveryByteBeforeTheNativeAction() = world(mapOf(hostKey to setOf("task_media_upload_v1"))) { w ->
        val data = largeMedia(w); val before = java.util.concurrent.atomic.AtomicInteger()
        w.beforeSend = { verifyMedia(w, data); before.incrementAndGet() }
        assertEquals(TaskOutcome.Done, send(w)); assertEquals(1, before.get())
        assertTrue(uploadFrames(w).size >= 2); verifyMedia(w, data)
    }
    @Test fun aDownlevelOrForeignUploadAdvertisementKeepsMediaAndSendsNothing() {
        for (features in listOf(emptyMap(), mapOf(MachineKey("workspace-a", "other") to setOf("task_media_upload_v1")),
            mapOf(MachineKey("other", "computer-1") to setOf("task_media_upload_v1")))) world(features) { w ->
            largeMedia(w); val result = send(w)
            assertTrue(result.toString(), result is TaskOutcome.Refused)
            assertTrue(w.calls.none { it.getString("operation") == "task_send" }); assertTrue(uploadFrames(w).isEmpty())
            assertTrue(w.computer.media(w.task, null).isEmpty())
            w.replicas.store(w.project, "Fixture").use { assertEquals(1, it.media(w.task, null).size) }
        }
    }
    @Test fun anUploadAcknowledgementMismatchNeverFallsBackOrRunsAnAction() = world(mapOf(hostKey to setOf("task_media_upload_v1"))) { w ->
        largeMedia(w)
        w.modifyExchange = { answer -> answer.optJSONObject("upload")?.let { it.put("offset", it.getLong("offset") + 1) } }
        assertTrue(send(w) is TaskOutcome.Refused)
        assertEquals(1, uploadFrames(w).size); assertTrue(w.calls.none { it.getString("operation") == "task_send" })
        assertTrue(w.computer.media(w.task, null).isEmpty())
    }
    @Test fun uploadModeStaysBoundToItsOriginalAdvertisedExchange() {
        val advertised = java.util.concurrent.ConcurrentHashMap<MachineKey, Set<String>>()
        advertised[hostKey] = setOf("task_media_upload_v1")
        world(advertised) { w ->
            val data = largeMedia(w)
            w.modifyExchange = { if (it.has("upload")) advertised.clear() }
            w.beforeSend = { verifyMedia(w, data) }
            assertEquals(TaskOutcome.Done, send(w)); assertTrue(uploadFrames(w).size >= 2)
            assertTrue(advertised.isEmpty())
        }
    }

}
