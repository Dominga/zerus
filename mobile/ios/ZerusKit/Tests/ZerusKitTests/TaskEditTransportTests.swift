import CryptoKit
import Foundation
import Synchronization
import XCTest
import ZerusReplica
@testable import ZerusKit

/// An actual isolated native endpoint, not a phone-side imitation of admission.
private final class PrivateTaskEndpoint: @unchecked Sendable {
    private let process = Process(), input = Pipe(), output = Pipe()
    private let lock = NSLock()
    private let root: URL
    init(root: URL, fixture: JSON) throws {
        self.root = root
        let fixtureFile = root.appendingPathComponent("public-keys.json")
        try fixture.data().write(to: fixtureFile)
        process.executableURL = try Self.binary()
        process.arguments = [fixtureFile.path]; process.currentDirectoryURL = root
        process.standardInput = input; process.standardOutput = output
        let errors = root.appendingPathComponent("native-errors.log")
        FileManager.default.createFile(atPath: errors.path, contents: nil)
        process.standardError = try FileHandle(forWritingTo: errors)
        var env = ProcessInfo.processInfo.environment.filter { !$0.key.hasPrefix("HGS_") }
        env["TMPDIR"] = root.path + "/"; process.environment = env
        try process.run()
    }
    private static let buildLock = NSLock()
    private static func binary() throws -> URL {
        if let path = ProcessInfo.processInfo.environment["ZERUS_TEST_TASK_FIXTURE"] { return URL(fileURLWithPath: path) }
        buildLock.lock(); defer { buildLock.unlock() }
        let repo = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let target = repo.appendingPathComponent("target/ios-native-fixture")
        let binary = target.appendingPathComponent("debug/examples/phone_fixture")
        if FileManager.default.isExecutableFile(atPath: binary.path) { return binary }
        let build = Process(); build.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        build.arguments = ["cargo", "+1.99.1", "build", "--locked", "--manifest-path", repo.appendingPathComponent("crates/mobile-task-edit/Cargo.toml").path,
                           "--example", "phone_fixture", "--target-dir", target.path, "-j", "4"]
        var env = ProcessInfo.processInfo.environment.filter { !$0.key.hasPrefix("HGS_") }
        env["CARGO_INCREMENTAL"] = "0"; env["CARGO_PROFILE_DEV_DEBUG"] = "0"; build.environment = env
        let done = DispatchSemaphore(value: 0); build.terminationHandler = { _ in done.signal() }
        try build.run()
        guard done.wait(timeout: .now() + 1200) == .success else { build.terminate(); throw ZerusError.invalid("The private native fixture build exceeded twenty minutes.") }
        guard build.terminationStatus == 0 else { throw ZerusError.invalid("The private native fixture failed to build.") }
        return binary
    }
    func call(_ command: String, _ fields: JSONObject = [:]) throws -> JSONObject {
        lock.lock(); defer { lock.unlock() }
        let request = fields.putting("command", .string(command))
        var bytes = try JSON.object(request).data(); bytes.append(10); try input.fileHandleForWriting.write(contentsOf: bytes)
        let done = DispatchSemaphore(value: 0), result = Mutex<Result<Data, Error>?>(nil), reader = output.fileHandleForReading
        DispatchQueue.global(qos: .utility).async {
            do {
                var line = Data()
                while let byte = try reader.read(upToCount: 1), !byte.isEmpty {
                    if byte[0] == 10 { result.withLock { $0 = .success(line) }; done.signal(); return }
                    guard line.count < 3 * 1024 * 1024 else { throw ZerusError.invalid("The private endpoint reply exceeded three MiB.") }
                    line.append(byte)
                }
                throw ZerusError.invalid("The private endpoint exited before its reply.")
            } catch { result.withLock { $0 = .failure(error) }; done.signal() }
        }
        guard done.wait(timeout: .now() + 15) == .success, let result = result.withLock({ $0 }) else { throw ZerusError.invalid("The private endpoint did not answer within fifteen seconds.") }
        guard let value = try JSON.parse(result.get()).object else { throw ZerusError.invalid("Invalid private endpoint reply.") }; return value
    }
    func close() throws {
        let done = DispatchSemaphore(value: 0); process.terminationHandler = { _ in done.signal() }
        try input.fileHandleForWriting.close()
        if process.isRunning && done.wait(timeout: .now() + 5) != .success { process.terminate(); _ = done.wait(timeout: .now() + 5); throw ZerusError.invalid("The private endpoint failed to settle after EOF.") }
        try output.fileHandleForReading.close()
        guard process.terminationStatus == 0 else { throw ZerusError.invalid("The private endpoint failed during cleanup.") }
    }
}

final class TaskEditTransportTests: XCTestCase {
    private final class World: @unchecked Sendable {
        let native: PrivateTaskEndpoint, root: URL, fixture: JSON, phone: WorkspaceIdentity, host: WorkspaceIdentity, membership: WorkspaceMembership
        let connection: Connection, key: ProjectKey, machine: Machine, replicas: TaskReplicas, store: TaskStore, task: String
        let journal: SQLiteWorkspaceJournal, files: TaskEditDraftFiles, client: RelayClient
        let responses = Mutex((posts: 0, corrupt: false))
        init() throws {
            root = FileManager.default.temporaryDirectory.appendingPathComponent("zerus-task-edit-" + UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
            fixture = try SharedFixture.json("encrypted-workspace-v1.json")
            let f = try XCTUnwrap(fixture.object)
            phone = try WorkspaceIdentity.parse(XCTUnwrap(f["phone_identity"]?.object)); host = try WorkspaceIdentity.parse(XCTUnwrap(f["gateway_identity"]?.object))
            membership = try WorkspaceMembership.parse(XCTUnwrap(f["final_state"]?.object))
            native = try PrivateTaskEndpoint(root: root, fixture: fixture)
            let seed = try native.call("seed"); task = seed.string("task")
            connection = Connection(id: "private-task-connection", label: "Fixture", url: "https://example.test", token: "fixture-token", protocolVersion: 2)
            key = ProjectKey(connectionId: connection.id, swarmId: "swarm-1", id: "project-1")
            machine = Machine(connectionId: connection.id, id: seed.string("machine"), name: "Private host", online: true, machineId: seed.string("machine"))
            journal = SQLiteWorkspaceJournal(directory: root.appendingPathComponent("journal"), testKeys: StaticKeyProvider(SymmetricKey(data: Data(repeating: 9, count: 32))))
            let bootstrap = try WorkspaceBootstrap.parse(XCTUnwrap(f["bootstrap_snapshot"]?.object))
            let approval = try WorkspaceApproval.parse(XCTUnwrap(f["approval"]?.object))
            let pending = try WorkspaceState(identity: phone, candidate: WorkspaceCandidate.parse(XCTUnwrap(f["candidate"]?.object)), issuer: host.device,
                                             pinnedRevision: 0, pinnedHead: bootstrap.state.head)
            try journal.create(connection, state: pending)
            var first = pending; first.membership = bootstrap.state; first.enrollment = approval
            try journal.update(connection, expected: pending, next: first)
            var active = first; active.membership = membership
            active.routes = [machine.id: WorkspaceRoute(computerId: machine.id, gatewayId: host.device.deviceId, routeId: host.device.deviceId, machineId: machine.machineId)]
            try journal.update(connection, expected: first, next: active, changes: [approval.membership])
            replicas = try TaskReplicas(root: root.appendingPathComponent("replicas")); store = replicas.store(key, name: "Fixture")
            let exchange = store.exchange()
            for _ in 0..<8 {
                guard let request = try exchange.nextRequest() else { break }
                let payload = try XCTUnwrap(JSON.parse(Data(request.utf8)).object)
                _ = try exchange.accept(answer: JSON.object(native.call("exchange", ["payload": .object(payload)])).compact)
            }
            files = TaskEditDraftFiles(directory: root.appendingPathComponent("drafts"), keys: StaticKeyProvider(SymmetricKey(data: Data(repeating: 7, count: 32))), syncDirectory: {})
            client = RelayClient(session: StubProtocol.client().session, workspaceJournal: journal)
            StubProtocol.reset { [self] request in
                do {
                    if request.url.path == "/v2/membership" {
                        return .response(status: 200, body: try JSON.object(["changes": [], "membership_revision": .int(membership.revision), "membership_head": .string(membership.head), "next_after": .null]).data())
                    }
                    guard request.method == "POST", request.url.path == "/v2/requests", let envelope = request.json?.object?["envelope"]?.object else { return StubProtocol.json(404, "{}") }
                    responses.withLock { $0.posts += 1 }
                    var result = try native.call("execute", ["envelope": .object(envelope)])
                    if responses.withLock({ state in let corrupt = state.corrupt; state.corrupt = false; return corrupt }), var body = result["envelope"]?.object {
                        body["signature"] = .string(WorkspaceProtocol.encode(Data(repeating: 0, count: 64))); result["envelope"] = .object(body)
                    }
                    return .response(status: 200, body: try JSON.object(result).data())
                } catch { return StubProtocol.json(500, #"{"error":"Private endpoint rejected the test envelope"}"#) }
            }
        }
        func transport(advertised: Bool = true) async throws -> TaskEditTransport {
            try await TaskEditTransport.open(client: client, connection: connection, key: key, task: task, computer: machine,
                features: [machine.key: advertised ? [TaskEditTransport.feature] : []])
        }
        func open() async throws -> TaskEditSession {
            try await TaskEditSession.open(replicas: replicas, projectName: "Fixture", transport: transport(), files: files) { try TaskEditForm($0).json() }
        }
        func close() throws { StubProtocol.reset { _ in StubProtocol.json(404, "{}") }; try native.close(); try FileManager.default.removeItem(at: root) }
    }
    func testUnadvertisedHostNeverPosts() async throws {
        let w = try World(); defer { XCTAssertNoThrow(try w.close()) }
        do { _ = try await w.transport(advertised: false); XCTFail("An unadvertised host accepted a qualified edit") } catch {}
        XCTAssertEqual(w.responses.withLock { $0.posts }, 0)
    }
    func testActualNativeCommentCommitsOnceAndSavedRecordCannotReopen() async throws {
        let w = try World(); defer { XCTAssertNoThrow(try w.close()) }
        let session = try await w.open()
        var form = TaskEditForm(session.view); form.comment = "Native phone comment"; form.sequence = 1
        try await session.save(sequence: 1, changes: form.json(), input: TaskEditInput(comment: form.comment))
        XCTAssertEqual(w.responses.withLock { $0.posts }, 2)
        XCTAssertTrue(try JSON.object(w.native.call("inspect")).compact.contains("Native phone comment"))
        do { _ = try await w.open(); XCTFail("Saved bytes recreated live authority") } catch is TaskEditDraftHeld {}
        XCTAssertEqual(w.responses.withLock { $0.posts }, 2)
        XCTAssertFalse(try w.store.get(reference: w.task).writable)
    }
    func testPostCommitCorruptProofIsUnknownAndCannotPostAgain() async throws {
        let w = try World(); defer { XCTAssertNoThrow(try w.close()) }
        let session = try await w.open(); let scope = session.scope
        w.responses.withLock { $0.corrupt = true }
        do { try await session.save(sequence: 1, changes: "retained form", input: TaskEditInput(comment: "Committed before bad proof")); XCTFail("Bad proof accepted") }
        catch TaskEditError.Unknown {}
        XCTAssertEqual(try w.files.read(scope)?.state, .unknown)
        XCTAssertTrue(try JSON.object(w.native.call("inspect")).compact.contains("Committed before bad proof"))
        do { try await session.save(sequence: 2, changes: "new", input: TaskEditInput(comment: "Must not post")); XCTFail("Unknown edit replayed") } catch {}
        XCTAssertEqual(w.responses.withLock { $0.posts }, 2)
    }
    func testActualNativeLargeMediaIsByteCompleteBeforeFinalEdit() async throws {
        let w = try World(); defer { XCTAssertNoThrow(try w.close()) }
        let session = try await w.open(), bytes = Data((0..<(800 * 1024)).map { UInt8($0 % 251) })
        let file = TaskMediaInput(name: "sample.bin", mime: "application/octet-stream", data: bytes, reference: "")
        try await session.save(sequence: 1, changes: "retained media form", input: TaskEditInput(content: TaskEditContent(title: "Reviewed title", text: "Reviewed requirement", kept: [], files: [file])))
        XCTAssertGreaterThan(w.responses.withLock { $0.posts }, 3)
        let result = try JSON.object(w.native.call("inspect")).compact
        XCTAssertTrue(result.contains(SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()))
        XCTAssertEqual(try w.files.read(session.scope)?.state, .completed)
    }
    func testForwardedRouteCannotOpenEvenWithAdvertisedFeature() async throws {
        let w = try World(); defer { XCTAssertNoThrow(try w.close()) }
        let before = try w.journal.load(w.connection)
        var next = before
        next.routes[w.machine.id] = WorkspaceRoute(computerId: w.machine.id, gatewayId: w.host.device.deviceId, routeId: "forwarded-route", machineId: w.machine.machineId)
        try w.journal.update(w.connection, expected: before, next: next)
        do { _ = try await w.transport(); XCTFail("A forwarded route gained local edit authority") } catch {}
        XCTAssertEqual(w.responses.withLock { $0.posts }, 0)
    }
    func testRouteReplacementBeforePostCannotReserveOrSubmit() async throws {
        let w = try World(); defer { XCTAssertNoThrow(try w.close()) }
        let transport = try await w.transport(), qualification = try w.store.qualifyEdit(reference: w.task, route: transport.route)
        let before = try w.journal.load(w.connection)
        var next = before
        next.routes[w.machine.id] = WorkspaceRoute(computerId: w.machine.id, gatewayId: w.host.device.deviceId, routeId: w.host.device.deviceId, machineId: "changed-machine")
        try w.journal.update(w.connection, expected: before, next: next)
        let reserved = Mutex(false)
        do { _ = try await transport.proof(qualification.request()) { reserved.withLock { $0 = true } }; XCTFail("Changed route posted") } catch {}
        XCTAssertFalse(reserved.withLock { $0 }); XCTAssertEqual(w.responses.withLock { $0.posts }, 0)
    }
    func testRetainedOriginalRequestCannotPostAgainAfterSettlement() async throws {
        let w = try World(); defer { XCTAssertNoThrow(try w.close()) }
        let transport = try await w.transport(), qualification = try w.store.qualifyEdit(reference: w.task, route: transport.route)
        let proof = try await transport.proof(qualification.request()) {}
        _ = try qualification.accept(proof: proof)
        try await transport.verified(qualification.request().requestId)
        let reserved = Mutex(false)
        do { _ = try await transport.proof(qualification.request()) { reserved.withLock { $0 = true } }; XCTFail("Retained UUID posted twice") } catch {}
        XCTAssertFalse(reserved.withLock { $0 })
        XCTAssertEqual(w.responses.withLock { $0.posts }, 1)
    }

    func testLocalValidationKeepsTheOriginalEditableAndDropsAbandonedFields() async throws {
        let w = try World(); defer { XCTAssertNoThrow(try w.close()) }
        let session = try await w.open()
        let abandoned = TaskEditContent(title: "Must not leak", text: "Abandoned replacement", kept: [], files: [])
        do { try await session.save(sequence: 1, changes: "invalid local form", input: TaskEditInput(content: abandoned, rank: "")); XCTFail("Invalid local rank accepted") } catch {}
        let editable = await session.editable
        XCTAssertTrue(editable)
        XCTAssertEqual(w.responses.withLock { $0.posts }, 1)
        try await session.save(sequence: 2, changes: "corrected local form", input: TaskEditInput(comment: "Only corrected comment"))
        let actual = try w.native.call("inspect")
        XCTAssertEqual(actual.string("text"), "Original requirement")
        XCTAssertTrue(JSON.object(actual).compact.contains("Only corrected comment"))
        XCTAssertEqual(w.responses.withLock { $0.posts }, 2)
    }

}
