import Foundation
import Synchronization
import XCTest
import ZerusReplica
@testable import ZerusKit

/// Ported from Android TaskActionsTest: Send now, Cancel, Done pages and fetches through a relay
/// whose computer answers from its replica.
final class TaskActionsTests: XCTestCase {
    private let key = ProjectKey(connectionId: "workspace-a", swarmId: "swarm-1", id: "project-1")
    private let connection = Connection(id: "workspace-a", label: "Workspace", url: "https://gateway.example", token: "synthetic")

    private func world() throws -> (FakeComputer, TaskReplicas, TaskSync) {
        let computer = FakeComputer()
        let replicas = try TaskReplicas(root: URL(fileURLWithPath: FakeComputer.folder()))
        return (computer, replicas, TaskSync(client: StubProtocol.client(), replicas: replicas))
    }

    func testASendFirstBringsThePhonesEdits() async throws {
        let (computer, replicas, sync) = try world()
        let task = try computer.store.filed("Add rate limiting")
        try await sync.exchangeNow(connection, key, name: "orchard", computerId: "computer-1")
        _ = try replicas.store(key, name: "orchard").editText(reference: task.uuid, text: "Add rate limiting per account")
        let textAtSend = Mutex("")
        let store = computer.store
        computer.update {
            $0.receipts["task_send"] = { payload in
                textAtSend.withLock { $0 = (try? store.get(reference: payload.string("task")).text) ?? "" }
                return ("completed", ["schema": 1, "ok": true, "task": .string(task.uuid), "stage": "active"], .null)
            }
            $0.envelopes = []
        }
        let outcome = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        XCTAssertEqual(outcome, .done)
        XCTAssertEqual(textAtSend.withLock { $0 }, "Add rate limiting per account")
        let operations = computer.operations
        XCTAssertEqual(operations.first, "task_exchange")
        XCTAssertEqual(operations.filter { $0 != "task_exchange" }, ["task_send"])
        XCTAssertEqual(operations.last, "task_exchange")
        let send = try XCTUnwrap(computer.envelopes.first { $0.string("operation") == "task_send" })
        XCTAssertEqual(send.string("session"), "")
        XCTAssertEqual(send.string("computer_id"), "computer-1")
        let payload = try XCTUnwrap(send.object("payload"))
        XCTAssertEqual([payload.string("project"), payload.string("task"), payload.string("session")], ["project-1", task.uuid, "codex/api"])
    }

    func testAFailedExchangeSendsNothing() async throws {
        let (computer, _, sync) = try world()
        let task = try computer.store.filed("Add rate limiting")
        computer.update { $0.refusal = "the project's tasks are busy; try again" }
        let outcome = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api",
                                      computerName: "Studio")
        guard case .refused(let reason) = outcome else { return XCTFail("\(outcome)") }
        XCTAssertTrue(reason.contains("busy"), reason)
        XCTAssertTrue(reason.hasPrefix("The tasks could not reach Studio"), reason)
        XCTAssertFalse(computer.operations.contains("task_send"))
    }

    func testAnUncertainSendSaysSo() async throws {
        let (computer, _, sync) = try world()
        let task = try computer.store.filed("Add rate limiting")
        let unrecorded = "the prompt was sent, but the task could not record it: disk full"
        computer.update { $0.receipts["task_send"] = { _ in ("uncertain", .null, .string(unrecorded)) } }
        let outcome = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        XCTAssertEqual(outcome, .uncertain(unrecorded))
    }

    func testALostAnswerToTheSendIsUncertainAndHoldsTheTask() async throws {
        let (computer, _, sync) = try world()
        let task = try computer.store.filed("Add rate limiting")
        // The relay may hold the request although its answer never arrived.
        computer.update { $0.raw["task_send"] = { .failure(.networkConnectionLost) } }
        let outcome = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        guard case .uncertain = outcome else { return XCTFail("\(outcome)") }
        let held = await sync.held(task.uuid)
        XCTAssertTrue(held)
    }

    func testARelayRefusalBeforeTheSendIsRefused() async throws {
        let (computer, _, sync) = try world()
        let task = try computer.store.filed("Add rate limiting")
        computer.update { $0.raw["task_send"] = { StubProtocol.json(400, #"{"error":"invalid task_send payload"}"#) } }
        let outcome = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        XCTAssertEqual(outcome, .refused("invalid task_send payload"))
        let held = await sync.held(task.uuid)
        XCTAssertFalse(held)
    }

    func testAnUncertainSendHoldsUntilAnExchangeWithThatComputerEnds() async throws {
        let (computer, replicas, sync) = try world()
        let task = try computer.store.filed("Add rate limiting")
        computer.update {
            $0.receipts["task_send"] = { [computer] _ in
                // Its copy cannot come back now either.
                computer.update { $0.refusal = "the project's tasks are busy; try again" }
                return ("uncertain", .null, "delivery uncertain")
            }
        }
        let outcome = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        XCTAssertEqual(outcome, .uncertain("delivery uncertain"))
        var held = await sync.held(task.uuid)
        XCTAssertTrue(held)
        // A sync that exchanges nothing shows no stage: the heartbeat names the phone's own digest.
        let mine = try replicas.store(key, name: "orchard").liveDigest()
        let failures = await sync.sync(connection, key, name: "orchard", peers: [TaskPeer(computerId: "computer-1", digest: mine)])
        XCTAssertEqual(failures, [:])
        held = await sync.held(task.uuid)
        XCTAssertTrue(held)
        await sync.settleSends(connection) { _ in "orchard" }
        held = await sync.held(task.uuid)
        XCTAssertTrue(held)
        computer.update { $0.refusal = nil }
        await sync.settleSends(connection) { _ in "orchard" }
        held = await sync.held(task.uuid)
        XCTAssertFalse(held)
    }

    func testARefusedSendGivesHgsReason() async throws {
        let (computer, _, sync) = try world()
        let task = try computer.store.filed("Add rate limiting")
        computer.update { $0.receipts["task_send"] = { _ in ("failed", .null, "codex/api cannot take it now: a question is open") } }
        let outcome = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        XCTAssertEqual(outcome, .refused("codex/api cannot take it now: a question is open"))
    }

    func testACancelCarriesItsReason() async throws {
        let (computer, _, sync) = try world()
        let task = try computer.store.filed("Add rate limiting")
        let reason = Mutex("")
        computer.update {
            $0.receipts["task_cancel"] = { payload in
                reason.withLock { $0 = payload.string("reason") }
                return ("completed", ["schema": 1, "ok": true, "stage": "review"], .null)
            }
        }
        let outcome = await sync.cancel(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, reason: "Wrong approach")
        XCTAssertEqual(outcome, .done)
        XCTAssertEqual(reason.withLock { $0 }, "Wrong approach")
    }

    func testDonePagesParseAndContinue() async throws {
        let (computer, _, sync) = try world()
        @Sendable func row(_ n: Int) -> JSON {
            ["uuid": .string("0b9d6c55-2f5e-4c39-9a51-0d6f3c2a7e1\(n)"), "id": .string("T-aaaaa\(n)"), "text": .string("Task \(n)"),
             "session": ["machine": "m", "session": "codex/api"], "accepted_at": .int(Int64(100 - n))]
        }
        computer.update {
            $0.receipts["task_done"] = { payload in
                let first = payload.object("before") == nil
                let rows: JSON = first ? [row(1), row(2)] : [row(3)]
                let next: JSON = first ? ["at": 98, "uuid": row(2).object?.value("uuid") ?? .null] : .null
                return ("completed", ["schema": 1, "tasks": rows, "next": next, "total": 3, "recent": 1, "damaged": 0], .null)
            }
        }
        let first = try await sync.donePage(connection, key, computerId: "computer-1", before: nil)
        XCTAssertEqual(first.rows.map(\.text), ["Task 1", "Task 2"])
        XCTAssertEqual([first.total, first.recent, first.damaged], [3, 1, 0])
        XCTAssertEqual(first.rows.first?.session?.session, "codex/api")
        let rest = try await sync.donePage(connection, key, computerId: "computer-1", before: first.next)
        XCTAssertEqual(rest.rows.map(\.text), ["Task 3"])
        XCTAssertNil(rest.next)
        let asked = try XCTUnwrap(computer.envelopes.last?.object("payload"))
        XCTAssertEqual(asked.object("before")?.int("at"), 98)
        XCTAssertEqual(asked.int("limit"), 50)
    }

    func testADonePageForOneSessionNamesIt() async throws {
        let (computer, _, sync) = try world()
        computer.update {
            $0.receipts["task_done"] = { _ in ("completed", ["schema": 1, "tasks": [], "next": .null, "total": 0, "recent": 0, "damaged": 0], .null) }
        }
        _ = try await sync.donePage(connection, key, computerId: "computer-1", before: nil, session: SessionRef(machine: "node-1", session: "codex/api"))
        let asked = try XCTUnwrap(computer.envelopes.last?.object("payload")?.object("session"))
        XCTAssertEqual([asked.string("machine"), asked.string("session")], ["node-1", "codex/api"])
    }

    func testOpeningADoneTaskFetchesIt() async throws {
        let (computer, replicas, sync) = try world()
        // Only hgs accepts a task into Done; the crate and bindings tests fetch a real one.
        let task = try computer.store.filed("Document the limits")
        try await sync.exchangeNow(connection, key, name: "orchard", computerId: "computer-1", fetch: [task.uuid])
        // The phone held nothing: its first request asks for that task by name.
        let first = try XCTUnwrap(computer.envelopes.first { $0.string("operation") == "task_exchange" })
        XCTAssertNotNil(first.object("payload")?.object("inventory")?.object("tasks")?.value(task.uuid))
        XCTAssertEqual(try replicas.store(key, name: "orchard").get(reference: task.uuid).text, "Document the limits")
    }

    func testEditsReachTheComputer() async throws {
        let (computer, replicas, sync) = try world()
        let task = try replicas.store(key, name: "orchard").filed("Filed on the phone")
        try await sync.exchangeNow(connection, key, name: "orchard", computerId: "computer-1")
        XCTAssertEqual(try computer.store.get(reference: task.uuid).text, "Filed on the phone")
    }
    func testDoneRowsRetainOpaqueIdentityAndMissingFieldsRemainLegacy() async throws {
        let (computer, _, sync) = try world()
        computer.update {
            $0.receipts["task_done"] = { _ in
                ("completed", ["schema": 1, "tasks": [
                    ["uuid": "task-1", "id": "T-first1", "text": "Original", "session": ["machine": "node-1", "session": "codex/old-label", "session_id": "launch_id:original"], "accepted_at": 2],
                    ["uuid": "task-2", "id": "T-second", "text": "Legacy", "session": ["machine": "node-1", "session": "codex/legacy"], "accepted_at": 1]
                ], "next": .null, "total": 2, "recent": 2, "damaged": 0], .null)
            }
        }
        let page = try await sync.donePage(connection, key, computerId: "computer-1", before: nil)
        XCTAssertEqual(page.rows[0].session?.sessionId, "launch_id:original")
        XCTAssertEqual(page.rows[1].session?.sessionId, "")
    }
    func testMalformedDoneIdentityCannotBecomeLegacy() async throws {
        let (computer, _, sync) = try world()
        computer.update {
            $0.receipts["task_done"] = { _ in
                ("completed", ["schema": 1, "tasks": [["uuid": "task-1", "id": "T-first1", "text": "Original", "session": ["machine": "node-1", "session": "codex/api", "session_id": 7], "accepted_at": 2]], "next": .null, "total": 1, "recent": 1, "damaged": 0], .null)
            }
        }
        do { _ = try await sync.donePage(connection, key, computerId: "computer-1", before: nil); XCTFail("Malformed identity must stay untrusted") }
        catch { }
    }

    private static func refusing(_ answer: JSONObject, _ documents: [String]) -> JSONObject {
        answer.putting("refused", .array(documents.map { .object(["uuid": .string($0), "error": "Forced private document refusal"]) }))
    }
    func testRefusedSelectedTaskCannotAuthorizeSend() async throws {
        let (computer, _, sync) = try world(); let task = try computer.store.filed("Selected requirement")
        computer.update {
            $0.modifyExchange = { Self.refusing($0, [task.uuid]) }
            $0.receipts["task_send"] = { _ in ("completed", [:], .null) }
        }
        let result = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        guard case .refused = result else { return XCTFail("A refused task cannot authorize Send") }
        XCTAssertFalse(computer.operations.contains("task_send"))
    }
    func testRefusedProjectCannotAuthorizeSend() async throws {
        let (computer, _, sync) = try world(); let task = try computer.store.filed("Selected requirement")
        computer.update {
            $0.modifyExchange = { Self.refusing($0, ["project"]) }
            $0.receipts["task_send"] = { _ in ("completed", [:], .null) }
        }
        let result = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        guard case .refused = result else { return XCTFail("A refused project cannot authorize Send") }
        XCTAssertFalse(computer.operations.contains("task_send"))
    }
    func testUnrelatedRefusedDocumentStillAllowsSelectedTask() async throws {
        let (computer, _, sync) = try world(); let task = try computer.store.filed("Selected requirement"); let other = try computer.store.filed("Unrelated requirement")
        computer.update {
            $0.modifyExchange = { Self.refusing($0, [other.uuid]) }
            $0.receipts["task_send"] = { _ in ("completed", [:], .null) }
        }
        let result = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        XCTAssertEqual(result, .done)
        XCTAssertEqual(computer.operations.filter { $0 == "task_send" }.count, 1)
    }
    func testLaterCleanPageCannotEraseSelectedTaskRefusal() async throws {
        let (computer, _, sync) = try world(); let task = try computer.store.filed("Selected requirement"); let pages = Mutex(0)
        computer.update {
            $0.modifyExchange = { answer in
                let first = pages.withLock { $0 += 1; return $0 == 1 }
                return first ? Self.refusing(answer, [task.uuid]).putting("more", true) : answer
            }
            $0.receipts["task_send"] = { _ in ("completed", [:], .null) }
        }
        let result = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        guard case .refused = result else { return XCTFail("An earlier refusal remains part of the exchange proof") }
        XCTAssertGreaterThanOrEqual(pages.withLock { $0 }, 2)
        XCTAssertFalse(computer.operations.contains("task_send"))
    }
    func testSelectedFetchRefusalCannotClaimFreshTaskState() async throws {
        let (computer, _, sync) = try world(); let task = try computer.store.filed("Selected requirement")
        computer.update { $0.modifyExchange = { Self.refusing($0, [task.uuid]) } }
        do { try await sync.exchangeNow(connection, key, name: "orchard", computerId: "computer-1", fetch: [task.uuid]); XCTFail("A refused selected fetch is not fresh state") }
        catch { }
    }
    func testSettlingUnknownHoldsKeepsRefusedTaskWithoutRepeatingSend() async throws {
        let (computer, _, sync) = try world(); let first = try computer.store.filed("First requirement"); let second = try computer.store.filed("Second requirement")
        computer.update { $0.receipts["task_send"] = { _ in ("uncertain", .null, "Unknown private outcome") } }
        for task in [first, second] {
            let result = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
            guard case .uncertain = result else { return XCTFail("Expected an original unknown outcome") }
        }
        computer.update { $0.modifyExchange = { Self.refusing($0, [first.uuid]) } }
        await sync.settleSends(connection, names: { _ in "orchard" })
        let firstHeld = await sync.held(first.uuid); let secondHeld = await sync.held(second.uuid)
        XCTAssertTrue(firstHeld); XCTAssertFalse(secondHeld)
        XCTAssertEqual(computer.operations.filter { $0 == "task_send" }.count, 2)
    }

    func testUnknownOriginalSendCannotBeRepeatedByCaller() async throws {
        let (computer, _, sync) = try world(); let task = try computer.store.filed("Original requirement")
        computer.update { $0.receipts["task_send"] = { _ in ("uncertain", .null, "Unknown private outcome") } }
        let first = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        guard case .uncertain = first else { return XCTFail("Expected an unknown original") }
        let second = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        guard case .refused = second else { return XCTFail("Unknown original delivery never authorizes another send") }
        XCTAssertEqual(computer.operations.filter { $0 == "task_send" }.count, 1)
    }

    private nonisolated static func awaitPrivateOriginal(_ gate: DispatchSemaphore) -> Bool { gate.wait(timeout: .now() + 10) == .success }
    func testCancellingAfterOriginalPostKeepsUnknownHold() async throws {
        let (computer, _, sync) = try world(); let task = try computer.store.filed("Original requirement")
        let entered = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0)
        computer.update { $0.receipts["task_send"] = { _ in
            entered.signal()
            guard Self.awaitPrivateOriginal(release) else { XCTFail("The private original-result gate did not settle"); return ("uncertain", .null, "Private gate timeout") }
            return ("completed", [:], .null)
        } }
        let connection = connection, key = key
        let original = Task { await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api") }
        let started = await Task.detached { Self.awaitPrivateOriginal(entered) }.value
        XCTAssertTrue(started); original.cancel(); release.signal(); _ = await original.value
        let held = await sync.held(task.uuid); XCTAssertTrue(held)
        let repeated = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
        guard case .refused = repeated else { return XCTFail("A cancelled posted original is not new Send authority") }
        XCTAssertEqual(computer.operations.filter { $0 == "task_send" }.count, 1)
    }
    func testBoundedUnknownSettlementAdvancesWhileNewHoldsArrive() async throws {
        let (computer, _, sync) = try world()
        let tasks = try (1...65).map { try computer.store.filed("Requirement \($0)") }
        computer.update { $0.receipts["task_send"] = { _ in ("uncertain", .null, "Unknown private outcome") } }
        for task in tasks {
            let result = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: task.uuid, session: "codex/api")
            guard case .uncertain = result else { return XCTFail("Expected an original unknown outcome") }
        }
        let ordered = tasks.map(\.uuid).sorted(), refused = Array(ordered.prefix(64)), last = try XCTUnwrap(ordered.last)
        computer.update { $0.modifyExchange = { Self.refusing($0, refused) } }
        await sync.settleSends(connection, names: { _ in "orchard" })
        let firstHeld = await sync.held(last); XCTAssertTrue(firstHeld)
        let arrival = try computer.store.filed("New requirement")
        let arrivalResult = await sync.send(connection, key, name: "orchard", computerId: "computer-1", task: arrival.uuid, session: "codex/api")
        guard case .uncertain = arrivalResult else { return XCTFail("Expected an arriving original unknown") }
        await sync.settleSends(connection, names: { _ in "orchard" })
        let lastHeld = await sync.held(last); XCTAssertFalse(lastHeld)
        await sync.settleSends(connection, names: { _ in "orchard" })
        let arrivalHeld = await sync.held(arrival.uuid); XCTAssertFalse(arrivalHeld)
        for task in refused { let held = await sync.held(task); XCTAssertTrue(held) }
        XCTAssertEqual(computer.operations.filter { $0 == "task_send" }.count, 66)
    }

}
