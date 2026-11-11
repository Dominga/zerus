package app.zerus.mobile

import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class ArchiveBrowsingTest {
    private val first = "10000000-0000-4000-8000-000000000001"
    private val second = "10000000-0000-4000-8000-000000000002"
    private val live = Target("computer", "codex/project/reused", "run", "conversation", "workspace")
    private val connection = Connection("workspace", "Test", "https://gateway.example", "synthetic")
    @Test fun liveKeyAndPrivateSerializationRemainExactlyCompatible() {
        assertEquals("[\"workspace\",\"computer\",\"codex/project/reused\",\"run\",\"conversation\"]", live.key)
        assertFalse(MessageCodec.targetJson(live).has("archive"))
        assertEquals(live, MessageCodec.target(MessageCodec.targetJson(live)))
        val archive = live.copy(archiveId = first)
        assertNotEquals(live.key, archive.key)
        assertNotEquals(archive.key, archive.copy(archiveId = second).key)
        assertEquals(archive, MessageCodec.target(MessageCodec.targetJson(archive)))
    }
    @Test fun archiveCannotResolveFirstLiveConversationOrEnterOutgoingQueue() {
        val initial = live.copy(conversation = "")
        assertTrue(initial.resolvesTo(live))
        assertFalse(initial.resolvesTo(live.copy(archiveId = first)))
        assertFalse(initial.copy(archiveId = first).resolvesTo(live))
        assertThrows(IllegalArgumentException::class.java) { live.copy(archiveId = first).json() }
        assertThrows(IllegalArgumentException::class.java) { MessageState().enqueue(Draft(live.copy(archiveId = first), "Cannot send"), "request", 1.0) }
        assertFalse(OutgoingMessage("request", initial, "Hello", emptyList(), 1.0).belongsTo(live.copy(archiveId = first)))
    }
    @Test fun parserRetainsValidArchiveAndLiveWithSameNameButRejectsUnaddressableArchives() {
        val rows = JSONArray().put(JSONObject().put("name", live.session).put("run_id", live.run).put("conversation_id", live.conversation))
            .put(JSONObject().put("name", live.session).put("state", "archived").put("archive_id", first))
            .put(JSONObject().put("name", live.session).put("archive_id", second))
            .put(JSONObject().put("name", "unsafe").put("state", "archived"))
            .put(JSONObject().put("name", "malformed").put("archive_id", "1-1-1-1-1"))
        val machine = JSONObject().put("id", "computer").put("snapshot", JSONObject().put("sessions", rows))
        val sessions = NativeParser.sessions(connection, machine)
        assertEquals(3, sessions.size)
        assertEquals(3, sessions.map { it.target.key }.distinct().size)
        assertEquals(1, sessions.count { SessionFilters.matches(it, SessionFilter.All, true) })
        assertEquals(2, sessions.count { SessionFilters.matches(it, SessionFilter.Archive, true) })
    }
    @Test fun projectMembershipUsesArchiveUuidRatherThanReusedLiveName() {
        fun group(id: String, names: List<String>, archives: List<String>) = JSONObject().put("id", id).put("name", id)
            .put("sessions", JSONArray(names)).put("archives", JSONArray(archives))
        val machine = JSONObject().put("id", "computer").put("snapshot", JSONObject().put("mobile_projects", JSONObject()
            .put("schema", 1).put("available", true).put("swarm_id", "swarm").put("projects", JSONArray()
                .put(group("Live", listOf(live.session), emptyList())).put(group("History", emptyList(), listOf(first, second))))))
        val sessions = listOf(Session(live, "Live", "codex", "", "", "", 0, JSONObject()),
            Session(live.copy(archiveId = first), "Old", "codex", "", "", "", 0, JSONObject()),
            Session(live.copy(archiveId = second), "Older", "codex", "", "", "", 0, JSONObject()))
        val result = ProjectParser.parse(connection, listOf(machine), sessions)
        assertEquals(listOf("Live", "History", "History"), result.sessions.map { it.projectKey!!.id })
        assertEquals(listOf("Live", "History"), result.projects.map { it.key.id })
    }
    private suspend fun inspectResponse(target: Target, raw: JSONObject): Pair<Result<JSONObject>, JSONObject> {
        MockWebServer().use { server ->
            server.dispatcher = object : Dispatcher() {
                override fun dispatch(request: RecordedRequest): MockResponse {
                    val envelope = JSONObject(request.body.clone().readUtf8())
                    return MockResponse().setBody(JSONObject().put("request_id", envelope.getString("request_id")).put("state", "completed").put("result", raw).toString())
                }
            }
            server.start()
            val testConnection = connection.copy(url = server.url("/").toString().trimEnd('/'))
            val result = runCatching { RelayApi().inspect(testConnection, target) }
            val payload = JSONObject(server.takeRequest().body.readUtf8()).getJSONObject("payload")
            return result to payload
        }
    }
    @Test fun archiveInspectCarriesExplicitUuidAndRequiresSameNameAndUuid() = runBlocking {
        val archived = live.copy(archiveId = first)
        val (result, payload) = inspectResponse(archived, JSONObject().put("name", live.session).put("archive_id", first))
        assertTrue(result.isSuccess)
        assertEquals(first, payload.getString("archive_id"))
        assertTrue(inspectResponse(archived, JSONObject().put("name", live.session).put("archive_id", second)).first.isFailure)
        assertTrue(inspectResponse(archived, JSONObject().put("name", "different").put("archive_id", first)).first.isFailure)
        assertTrue(inspectResponse(archived, JSONObject().put("name", live.session)).first.isFailure)
    }
    @Test fun liveInspectRejectsArchiveOrWrongSessionWithoutInstallingItsContent() = runBlocking {
        assertTrue(inspectResponse(live, JSONObject().put("name", live.session)).first.isSuccess)
        assertTrue(inspectResponse(live, JSONObject().put("name", live.session).put("state", "archived")).first.isFailure)
        assertTrue(inspectResponse(live, JSONObject().put("name", live.session).put("archive_id", first)).first.isFailure)
        assertTrue(inspectResponse(live, JSONObject().put("name", "other")).first.isFailure)
        assertEquals(0, inspectResponse(live, JSONObject().put("name", live.session)).second.length())
    }
    @Test fun mutationsOfAnArchiveAreRejectedBeforeAnyHttpRequest() = runBlocking {
        MockWebServer().use { server ->
            server.start()
            val testConnection = connection.copy(url = server.url("/").toString().trimEnd('/'))
            val archived = live.copy(archiveId = first)
            listOf("send", "answer", "interrupt").forEach { operation ->
                assertTrue(runCatching { RelayApi().submit(testConnection, archived, operation, JSONObject(), "request", SendAttempt()) }.isFailure)
            }
            assertTrue(runCatching { RelayApi().submit(testConnection, archived, "inspect", JSONObject(), "request", SendAttempt()) }.isFailure)
            assertEquals(0, server.requestCount)
        }
    }
}
