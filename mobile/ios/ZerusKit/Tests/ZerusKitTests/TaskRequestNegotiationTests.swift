import CryptoKit
import Foundation
import Synchronization
import XCTest
import ZerusReplica
@testable import ZerusKit

/// Real encrypted client transport; this private peer scripts outcomes, never native input.
final class TaskRequestNegotiationTests: XCTestCase {
    private let hostKey = MachineKey(connectionId: "workspace-a", computerId: "computer-1")
    private final class World: Sendable {
        let connection: Connection, key: ProjectKey, sync: TaskSync, task: String
        let calls = Mutex<[JSONObject]>([]), outcome = Mutex("completed")
        init(_ connection: Connection, _ key: ProjectKey, _ sync: TaskSync, _ task: String) {
            self.connection = connection; self.key = key; self.sync = sync; self.task = task
        }
        func payloads(_ operation: String) -> [JSONObject] { calls.withLock { $0.filter { $0.string("operation") == operation }.compactMap { $0.object("payload") } } }
    }
    private func world(features: [MachineKey: Set<String>]? = nil, protocolVersion: Int = 2,
                       _ body: (World) async throws -> Void) async throws {
        let fixture = try XCTUnwrap(SharedFixture.json("encrypted-workspace-v1.json").object)
        let host = try WorkspaceIdentity.parse(XCTUnwrap(fixture["gateway_identity"]?.object))
        let phone = try WorkspaceIdentity.parse(XCTUnwrap(fixture["phone_identity"]?.object))
        let roster = try WorkspaceMembership.parse(XCTUnwrap(fixture["final_state"]?.object))
        let candidate = try WorkspaceCandidate.parse(XCTUnwrap(fixture["candidate"]?.object))
        let bootstrap = try WorkspaceBootstrap.parse(XCTUnwrap(fixture["bootstrap_snapshot"]?.object))
        let approval = try WorkspaceApproval.parse(XCTUnwrap(fixture["approval"]?.object))
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        defer { XCTAssertNoThrow(try FileManager.default.removeItem(at: root)) }
        let connection = Connection(id: "workspace-a", label: "Fixture", url: "https://gateway.example", token: "synthetic", protocolVersion: protocolVersion)
        let key = ProjectKey(connectionId: connection.id, swarmId: "swarm-1", id: "project-1")
        let journal = SQLiteWorkspaceJournal(directory: root.appendingPathComponent("journal"), testKeys: StaticKeyProvider(SymmetricKey(data: Data(repeating: 9, count: 32))))
        let pending = WorkspaceState(identity: phone, candidate: candidate, issuer: host.device, pinnedRevision: 0, pinnedHead: bootstrap.state.head)
        try journal.create(connection, state: pending)
        var first = pending; first.membership = bootstrap.state; first.enrollment = approval
        try journal.update(connection, expected: pending, next: first)
        var active = first; active.membership = roster
        active.routes = ["computer-1": WorkspaceRoute(computerId: "computer-1", gatewayId: host.device.deviceId, routeId: "route-fixture", machineId: "computer-1")]
        try journal.update(connection, expected: first, next: active, changes: [approval.membership])
        let computer = TaskStore(root: root.appendingPathComponent("computer").path, project: key.id, projectName: "Fixture")
        let task = try computer.filed("Private requirement")
        let replicas = try TaskReplicas(root: root.appendingPathComponent("phone"))
        let advertised = features ?? [hostKey: ["task_session_identity_v2"]]
        let sync = TaskSync(client: RelayClient(session: StubProtocol.client().session, workspaceJournal: journal), replicas: replicas, capabilities: { advertised })
        let world = World(connection, key, sync, task.uuid)
        StubProtocol.reset { request in
            do {
                if request.url.path == "/v2/membership" {
                    return .response(status: 200, body: try JSON.object(["changes": [], "membership_revision": .int(roster.revision), "membership_head": .string(roster.head), "next_after": .null]).data())
                }
                guard request.method == "POST", let outer = request.json?.object else { return StubProtocol.json(404, "{}") }
                let encrypted = protocolVersion == 2 ? try WorkspaceEnvelope.parse(XCTUnwrap(outer["envelope"]?.object)) : nil
                let clear: JSONObject
                if let encrypted {
                    XCTAssertEqual(encrypted.header.routeId, "route-fixture")
                    let bytes = try encrypted.open(identity: host, sender: phone.device, expected: encrypted.header)
                    clear = try XCTUnwrap(JSON.parse(bytes).object)
                } else { clear = outer }
                world.calls.withLock { $0.append(clear) }
                let operation = clear.string("operation"), payload = clear.object("payload") ?? [:]
                let state = operation == "task_send" ? world.outcome.withLock { $0 } : "completed"
                let result: JSON
                switch operation {
                case "task_exchange": result = try JSON.parse(computer.answerExchange(request: JSON.object(payload).compact))
                case "task_done": result = ["schema": 1, "tasks": [], "next": .null, "total": 0, "recent": 0, "damaged": 0]
                default: result = .null
                }
                let id = encrypted?.header.requestId ?? clear.string("request_id")
                let reply: JSONObject = ["request_id": .string(id), "state": .string(state), "result": result, "error": state == "completed" ? .null : "Private action outcome"]
                guard let encrypted else { return .response(status: 200, body: try JSON.object(reply).data()) }
                let h = encrypted.header
                let header = try WorkspaceHeader(workspaceId: h.workspaceId, senderDeviceId: host.device.deviceId, recipientDeviceId: phone.device.deviceId,
                    senderKeyGeneration: host.device.keyGeneration, recipientKeyGeneration: phone.device.keyGeneration, gatewayId: h.gatewayId, routeId: h.routeId,
                    requestId: id, kind: "result", operationClass: h.operationClass, expiresAt: h.expiresAt, membershipRevision: h.membershipRevision, membershipHead: h.membershipHead)
                let answer = try WorkspaceEnvelope.seal(identity: host, recipient: phone.device, header: header, plaintext: JSON.object(reply).data())
                return .response(status: 200, body: try JSON.object(["request_id": .string(id), "state": .string(state), "envelope": .object(answer.json)]).data())
            } catch { XCTFail("Private encrypted fixture failed: \(error)"); return StubProtocol.json(500, "{}") }
        }
        try await body(world)
    }
    private func send(_ world: World) async -> TaskOutcome {
        await world.sync.send(world.connection, world.key, name: "Fixture", computerId: "computer-1", task: world.task,
                              session: "codex/current-label", sessionId: "launch_id: original ")
    }
    func testOriginalAdvertisedHostReceivesSchemaTwoAndOpaqueKey() async throws {
        try await world { w in
            let result = await send(w); XCTAssertEqual(result, .done)
            let payload = try XCTUnwrap(w.payloads("task_send").only)
            XCTAssertEqual(payload.int("schema"), 2); XCTAssertEqual(payload.string("session_id"), "launch_id: original ")
        }
    }
    func testForeignAdvertisementCannotOptInOriginalHost() async throws {
        for features: [MachineKey: Set<String>] in [[:], [MachineKey(connectionId: "workspace-a", computerId: "other"): ["task_session_identity_v2"]], [MachineKey(connectionId: "other", computerId: "computer-1"): ["task_session_identity_v2"]]] {
            try await world(features: features) { w in
                let result = await send(w); XCTAssertEqual(result, .done)
                let payload = try XCTUnwrap(w.payloads("task_send").only)
                XCTAssertEqual(payload.int("schema"), 1); XCTAssertNil(payload["session_id"])
            }
        }
    }
    func testLegacyTransportChoosesSchemaOneUpFront() async throws {
        try await world(protocolVersion: 1) { w in
            let result = await send(w); XCTAssertEqual(result, .done)
            let payload = try XCTUnwrap(w.payloads("task_send").only)
            XCTAssertEqual(payload.int("schema"), 1); XCTAssertNil(payload["session_id"])
        }
    }
    func testSchemaTwoRefusalNeverFallsBack() async throws {
        try await world { w in
            w.outcome.withLock { $0 = "failed" }
            let result = await send(w); guard case .refused = result else { return XCTFail("Expected a refused original") }
            XCTAssertEqual(w.payloads("task_send").count, 1); XCTAssertEqual(w.payloads("task_send").first?.int("schema"), 2)
        }
    }
    func testSchemaTwoUnknownNeverFallsBackOrRepeats() async throws {
        try await world { w in
            w.outcome.withLock { $0 = "uncertain" }
            let result = await send(w); guard case .uncertain = result else { return XCTFail("Expected an unknown original") }
            let repeated = await send(w); guard case .refused = repeated else { return XCTFail("An unknown original is held") }
            XCTAssertEqual(w.payloads("task_send").count, 1); XCTAssertEqual(w.payloads("task_send").first?.int("schema"), 2)
        }
    }
    func testIdentityBoundDoneFilterUsesAdvertisedSchema() async throws {
        try await world { w in
            _ = try await w.sync.donePage(w.connection, w.key, computerId: "computer-1", before: nil,
                session: SessionRef(machine: "computer-1", session: "codex/old-label", sessionId: "launch_id:original"))
            let payload = try XCTUnwrap(w.payloads("task_done").only)
            XCTAssertEqual(payload.int("schema"), 2); XCTAssertEqual(payload.object("session")?.string("session_id"), "launch_id:original")
        }
    }
}
private extension Array { var only: Element? { count == 1 ? first : nil } }
