import CryptoKit
import Foundation
import Synchronization
import ZerusReplica

private let taskEditQueue = DispatchQueue(label: "app.zerus.task-edit-io", qos: .utility, attributes: .concurrent)
func taskEditIO<T: Sendable>(_ body: @escaping @Sendable () throws -> T) async throws -> T {
    try await withCheckedThrowingContinuation { continuation in
        taskEditQueue.async { continuation.resume(with: Result { try body() }) }
    }
}

/// One captured, authenticated same-computer route. Never forwards or downgrades.
public struct TaskEditTransport: Sendable {
    public static let feature = "task_admitted_edit_v1"
    public let key: ProjectKey
    public let task: String
    public let route: TaskEditRoute
    private let client: RelayClient
    private let connection: Connection
    private let originalRoute: WorkspaceRoute
    private let phone: WorkspaceDevice
    private let host: WorkspaceDevice
    public var scope: TaskEditDraftScope { TaskEditDraftScope(connectionId: key.connectionId, swarmId: key.swarmId, project: key.id, task: task) }

    public static func open(client: RelayClient, connection: Connection, key: ProjectKey, task: String, computer: Machine,
                            features: [MachineKey: Set<String>]) async throws -> Self {
        guard connection.protocolVersion == 2, connection.id == key.connectionId, computer.connectionId == connection.id,
              computer.online, !computer.lastKnown, !computer.machineId.isEmpty,
              features[MachineKey(connectionId: connection.id, computerId: computer.id)]?.contains(feature) == true else {
            throw ZerusError.invalid("Update Zerus on this task's computer to change it. Your draft was kept.")
        }
        let state = try await client.synchronizeWorkspace(connection), membership = try state.active()
        guard let original = state.routes[computer.id], original.computerId == computer.id, original.machineId == computer.machineId,
              original.gatewayId == original.routeId, let host = membership.members[original.gatewayId], host.role == "computer",
              state.identity.device.role == "phone", membership.members[state.identity.device.deviceId] == state.identity.device else {
            throw ZerusError.mismatch("Refresh this task computer's encrypted route. Forwarded computers cannot yet accept qualified edits. Your draft was kept.")
        }
        let route = TaskEditRoute(connectionId: connection.id, machineId: computer.machineId, hostDevice: host.deviceId,
                                  phoneDevice: state.identity.device.deviceId, membershipJson: String(decoding: try JSON.object(membership.json).data(), as: UTF8.self))
        return Self(key: key, task: task, route: route, client: client, connection: connection, originalRoute: original, phone: state.identity.device, host: host)
    }

    /// The private record and original encrypted request are durable before the only possible POST.
    public func proof(_ request: TaskEditRequest, reserve: @escaping @Sendable () throws -> Void) async throws -> TaskEditProof {
        let posted = Mutex(false)
        do {
            return try await Deadline.within(.seconds(90)) {
                let state = try await client.synchronizeWorkspace(connection), membership = try state.active(), journal = try client.journal()
                guard state.routes[originalRoute.computerId] == originalRoute, state.identity.device == phone,
                      membership.members[phone.deviceId] == phone, membership.members[host.deviceId] == host else {
                    throw ZerusError.mismatch("The original task computer or workspace keys changed. Your draft was kept.")
                }
                let original = try await taskEditIO {
                    guard let payload = try JSON.parse(Data(request.payload.utf8)).object,
                          payload["schema"] == 1, payload.string("project") == key.id,
                          let updates = payload["updates"]?.array, updates.count == 1, updates[0].object?.string("task") == task,
                          try journal.recordedRequest(connection, id: request.requestId) == nil else {
                        throw ZerusError.invalid("This original task request is unavailable or already retained; it will not be submitted again.")
                    }
                    try reserve()
                    let body: JSONObject = ["request_id": .string(request.requestId), "computer_id": .string(originalRoute.computerId),
                        "session": "", "operation": "task_exchange", "payload": .object(payload), "encrypted_transport": .object([
                            "gateway_id": .string(originalRoute.gatewayId), "route_id": .string(originalRoute.routeId), "machine_id": .string(originalRoute.machineId)])]
                    let clear = try JSON.object(body).data()
                    guard clear.count <= WorkspaceProtocol.maxPayload else { throw ZerusError(.limit, "The task request exceeds its bounded transport limit.") }
                    let header = try WorkspaceHeader(workspaceId: membership.workspaceId, senderDeviceId: phone.deviceId, recipientDeviceId: host.deviceId,
                        senderKeyGeneration: phone.keyGeneration, recipientKeyGeneration: host.keyGeneration, gatewayId: originalRoute.gatewayId,
                        routeId: originalRoute.routeId, requestId: request.requestId, kind: "request", operationClass: "mutation",
                        expiresAt: RelayClient.nowSeconds + 300, membershipRevision: membership.revision, membershipHead: membership.head)
                    let envelope = try WorkspaceEnvelope.seal(identity: state.identity, recipient: host, header: header, plaintext: clear)
                    return try journal.reserve(connection, id: request.requestId, plainHash: WorkspaceProtocol.encode(Data(SHA256.hash(data: clear))), envelope: envelope)
                }
                posted.withLock { $0 = true }
                var response = try await client.call(connection.url, token: connection.token, path: "/v2/requests", body: ["envelope": .object(original.json)])
                while response["envelope"]?.isNull == true {
                    guard response.string("request_id") == request.requestId, ["queued", "claimed"].contains(response.string("state")) else {
                        throw ZerusError.mismatch("No original native result is available.")
                    }
                    try await Task.sleep(for: .milliseconds(400))
                    response = try await client.call(connection.url, token: connection.token, path: "/v2/requests/\(request.requestId)")
                }
                guard response.string("request_id") == request.requestId, let raw = response["envelope"]?.object else { throw ZerusError.mismatch("No original native result is available.") }
                let result = try WorkspaceEnvelope.parse(raw)
                let fresh = try await client.synchronizeWorkspace(connection), current = try fresh.active()
                return try await taskEditIO {
                    guard let requestHead = try journal.head(connection, revision: original.header.membershipRevision),
                          let resultHead = try journal.head(connection, revision: result.header.membershipRevision) else {
                        throw ZerusError.mismatch("The original task result history is unavailable.")
                    }
                    // Private identity stays in-process, solely for the FFI verifier.
                    func text(_ value: JSONObject) throws -> String { String(decoding: try JSON.object(value).data(), as: UTF8.self) }
                    return try TaskEditProof(connectionId: connection.id, identityJson: text(fresh.identity.privateJSON), membershipJson: text(current.json),
                        requestHead: requestHead, resultHead: resultHead, originalRequest: text(original.json), result: text(result.json))
                }
            }
        } catch {
            if posted.withLock({ $0 }) {
                throw TaskEditError.Unknown(message: "The original task edit result could not be verified. Your saved draft is held and will not be sent again. Refresh this task to review it.")
            }
            throw error
        }
    }
    /// Only after the live FFI handle verifies the exact native proof.
    public func verified(_ id: String) async throws { try await taskEditIO { try client.journal().settle(connection, id: id) } }
}
