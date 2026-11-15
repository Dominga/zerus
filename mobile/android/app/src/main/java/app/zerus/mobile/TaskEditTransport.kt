package app.zerus.mobile

import app.zerus.mobile.replica.TaskEditException
import app.zerus.mobile.replica.TaskEditProof
import app.zerus.mobile.replica.TaskEditRequest
import app.zerus.mobile.replica.TaskEditRoute
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import org.json.JSONObject
import java.security.MessageDigest

/** One original authenticated same-computer route. No plaintext/forwarded fallback. */
internal class TaskEditTransport private constructor(
    private val api: RelayApi, private val connection: Connection, private val key: ProjectKey, private val task: String,
    private val originalRoute: WorkspaceRoute, private val phone: WorkspaceDevice, private val host: WorkspaceDevice,
    val route: TaskEditRoute
) {
    val projectKey: ProjectKey get() = key
    val scope: TaskEditDraftScope get() = TaskEditDraftScope(key.connectionId, key.swarmId, key.id, task)
    /** [reserve] durably records the original request before the first possible POST. */
    suspend fun proof(request: TaskEditRequest, reserve: () -> Unit): TaskEditProof {
        var posted = false
        try {
            return withTimeout(90_000) { withContext(Dispatchers.IO) {
                val journal = checkNotNull(api.workspace)
                val state = api.synchronizeWorkspace(connection)
                val membership = checkNotNull(state.membership)
                check(state.routes[originalRoute.computerId] == originalRoute && state.identity.device == phone &&
                    membership.members[phone.deviceId] == phone && membership.members[host.deviceId] == host) {
                    "The original task computer or workspace keys changed. Your draft was kept."
                }
                val payload = JSONObject(request.payload)
                check(payload.getInt("schema") == 1 && payload.get("project") == key.id)
                val updates = payload.getJSONArray("updates")
                check(updates.length() == 1 && updates.getJSONObject(0).get("task") == task)
                check(journal.recordedRequest(connection, request.requestId) == null) {
                    "This original task request is already retained; only its result may be reviewed."
                }
                reserve()
                val body = JSONObject().put("request_id", request.requestId).put("computer_id", originalRoute.computerId)
                    .put("session", "").put("operation", "task_exchange").put("payload", payload)
                    .put("encrypted_transport", JSONObject().put("gateway_id", originalRoute.gatewayId)
                        .put("route_id", originalRoute.routeId).put("machine_id", originalRoute.machineId))
                val bytes = body.toString().toByteArray(Charsets.UTF_8)
                check(bytes.size <= 1024 * 1024)
                val header = WorkspaceHeader(membership.workspaceId, phone.deviceId, host.deviceId, phone.keyGeneration,
                    host.keyGeneration, originalRoute.gatewayId, originalRoute.routeId, request.requestId,
                    "request", "mutation", System.currentTimeMillis() / 1000 + 300, membership.revision, membership.head)
                val encrypted = WorkspaceEnvelope.seal(state.identity, host, header, bytes)
                val hash = WorkspaceProtocol.encode(MessageDigest.getInstance("SHA-256").digest(bytes))
                val original = journal.reserve(connection, request.requestId, hash, encrypted)
                posted = true
                var response = api.call(connection.url, connection.token, "/v2/requests",
                    JSONObject().put("envelope", original.json()), attempt = SendAttempt())
                while (response.isNull("envelope")) {
                    check(response.string("request_id") == request.requestId && response.string("state") in setOf("queued", "claimed")) {
                        "No original native result is available."
                    }
                    delay(400)
                    response = api.call(connection.url, connection.token, "/v2/requests/${request.requestId}")
                }
                check(response.string("request_id") == request.requestId)
                val result = WorkspaceEnvelope.parse(response.getJSONObject("envelope"))
                val fresh = api.synchronizeWorkspace(connection)
                val current = checkNotNull(fresh.membership)
                val requestHead = checkNotNull(journal.head(connection, original.header.membershipRevision))
                val resultHead = checkNotNull(journal.head(connection, result.header.membershipRevision))
                // Private identity goes only to the in-process FFI verifier. Do
                // not log, retain in a UI model, or send this record to a relay.
                TaskEditProof(connection.id, fresh.identity.privateJson().toString(), current.json().toString(),
                    requestHead, resultHead, original.json().toString(), result.json().toString())
            } }
        } catch (error: TimeoutCancellationException) {
            if (posted) throw TaskEditException.Unknown("The original task edit exceeded its response deadline. Your saved draft is held and will not be sent again. Refresh this task to review it.")
            throw TaskEditException.Failed("This task computer could not be verified before the deadline. No task edit was submitted.")
        } catch (error: CancellationException) { throw error }
        catch (error: Exception) {
            if (posted) throw TaskEditException.Unknown("The original task edit result could not be verified. Your saved draft is held and will not be sent again. Refresh this task to review it.")
            throw error
        }
    }

    /** Called only after the live FFI handle verified the exact native proof. */
    suspend fun verified(requestId: String) = withContext(Dispatchers.IO) {
        checkNotNull(api.workspace).settle(connection, requestId)
    }

    companion object {
        const val FEATURE = "task_admitted_edit_v1"
        suspend fun open(api: RelayApi, connection: Connection, key: ProjectKey, task: String, computer: Machine,
                         features: Map<MachineKey, Set<String>>): TaskEditTransport {
            check(connection.protocolVersion == 2 && connection.id == key.connectionId && computer.connectionId == connection.id &&
                computer.online && !computer.lastKnown && computer.machineId.isNotEmpty() &&
                FEATURE in features[MachineKey(connection.id, computer.id)].orEmpty()) {
                "Update Zerus on this task's computer to change it. Your draft was kept."
            }
            val state = api.synchronizeWorkspace(connection)
            val membership = checkNotNull(state.membership)
            val route = checkNotNull(state.routes[computer.id]) { "Refresh this task computer's encrypted route. Your draft was kept." }
            check(route.computerId == computer.id && route.machineId == computer.machineId && route.gatewayId == route.routeId) {
                "This forwarded task computer does not yet support qualified edits. Your draft was kept."
            }
            val host = checkNotNull(membership.members[route.gatewayId]); val phone = state.identity.device
            check(host.role == "computer" && phone.role == "phone" && membership.members[phone.deviceId] == phone)
            return TaskEditTransport(api, connection, key, task, route, phone, host,
                TaskEditRoute(connection.id, computer.machineId, host.deviceId, phone.deviceId, membership.json().toString()))
        }
    }
}
