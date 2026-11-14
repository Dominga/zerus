import Foundation
import XCTest
@testable import ZerusKit

final class NativeSendOutcomeTests: XCTestCase {
    private func vectors() throws -> [JSONObject] {
        let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../../../../services/mobile/vectors/send_outcome/outcomes.json")
        return try XCTUnwrap(JSON.parse(Data(contentsOf: file)).object?.array("vectors")).compactMap(\.object)
    }
    private func fixture(_ v: JSONObject) throws -> (OutgoingMessage, JSONObject, NativeSendIntent) {
        let send = try XCTUnwrap(v.object("send")?.object("payload")), binding = try XCTUnwrap(send.object("binding")), p = try XCTUnwrap(send.object("payload"))
        let target = Target(computerId: binding.string("computer_id"), session: binding.string("session"), run: binding.string("run_id"), conversation: binding.string("conversation_id"), connectionId: "gateway")
        let payloadFiles = try XCTUnwrap(p.array("attachments"))
        let files: [Attachment] = payloadFiles.isEmpty ? [] : [Attachment(id: "55555555-5555-4555-8555-555555555555", name: "note.txt", mime: "text/plain", bytes: 5, sha256: SendReceiptRecovery.hash("hello"))]
        var row = OutgoingMessage(requestId: binding.string("message_request_id"), target: target, text: p.string("text"), attachments: files, createdAt: 1, status: "uncertain")
        let intent = try XCTUnwrap(NativeSendOutcome.freeze(row, payload: p, provider: binding.string("provider"), machineId: binding.string("machine_id"), sessionId: binding.string("session_id"), encoding: binding.string("native_encoding")))
        row.nativeSendIntent = intent
        return (row, p, intent)
    }
    func testSharedVectorsFreezeExactTextAndAttachmentDigestsAndBothEncodings() throws {
        let all = try vectors(); XCTAssertEqual(all.count, 4)
        for v in all {
            let (row, p, intent) = try fixture(v)
            XCTAssertEqual(try NativeSendOutcome.request(row.requestId, intent: intent, payload: p), v.object("send")?.object("payload"))
            XCTAssertLessThanOrEqual(intent.canonical.utf8.count, 4096)
            XCTAssertFalse(intent.canonical.contains("data_base64"))
            for key in ["not_sent", "read_not_sent", "read_submitted"] {
                let result = try XCTUnwrap(v.object(key))
                XCTAssertNotNil(NativeSendOutcome.proof(result.string("request_id"), intent: intent, result: result), key)
            }
            let restored = try JSONDecoder().decode(OutgoingMessage.self, from: JSONEncoder().encode(row))
            XCTAssertEqual(restored.nativeSendIntent, intent); XCTAssertTrue(intent.matches(restored))
            XCTAssertEqual(OutboxRetention.normalize(MessageState(outgoing: [row])).outgoing, [row])
        }
    }
    func testOnlyExactClosedNegativeUnblocksTheLaneAndAnExplicitNewUuidCanSend() throws {
        for v in try vectors() {
            let (row, _, intent) = try fixture(v), result = try XCTUnwrap(v.object("not_sent"))
            let receipt: JSONObject = ["request_id": .string(row.requestId), "state": "failed", "result": .object(result)]
            let state = MessageState(outgoing: [row])
            let settled = NativeSendOutcome.settleReceipt(state, captured: row, receipt: receipt, deadline: 100, now: 99)
            let kept = try XCTUnwrap(settled.outgoing.first)
            XCTAssertEqual(kept.status, "failed"); XCTAssertFalse(kept.blocksSending)
            XCTAssertEqual(kept.requestId, row.requestId); XCTAssertEqual(kept.text, row.text); XCTAssertEqual(kept.attachments, row.attachments)
            XCTAssertEqual(kept.nativeSendIntent, intent); XCTAssertNotNil(kept.nativeSendProof)
            let draft = Draft(target: row.target, text: row.text, attachments: row.attachments)
            XCTAssertThrowsError(try settled.enqueue(draft, requestId: row.requestId, at: 2))
            let next = try settled.enqueue(draft, requestId: "66666666-6666-4666-8666-666666666666", at: 2)
            XCTAssertEqual(next.outgoing.count, 2); XCTAssertEqual(next.outgoing[0], kept)
            XCTAssertEqual(NativeSendOutcome.settleReceipt(state, captured: row, receipt: receipt, deadline: 99, now: 99), state)
            let positive = try XCTUnwrap(v.object("read_submitted"))
            let proof = try XCTUnwrap(NativeSendOutcome.proof(positive.string("request_id"), intent: intent, result: positive))
            let sent = NativeSendOutcome.settle(state, captured: row, proof: proof, deadline: 100, now: 99).outgoing[0]
            XCTAssertTrue(NativeSendOutcome.confirmed(sent)); XCTAssertEqual(sent.recover().status, "submitted")
        }
    }
    func testMissingForeignCorruptAndOldNegativesStayUnknown() throws {
        let v = try XCTUnwrap(vectors().first), (row, _, intent) = try fixture(v), good = try XCTUnwrap(v.object("not_sent"))
        for key in ["schema", "request_id", "binding", "binding_sha256", "status", "proof", "reason"] {
            var bad = good; bad[key] = key == "reason" ? .null : .string("changed")
            XCTAssertNil(NativeSendOutcome.proof(row.requestId, intent: intent, result: bad), key)
        }
        for key in intent.value()!.keys {
            var bad = good, binding = intent.value()!
            binding[key] = .string("foreign"); bad["binding"] = .object(binding)
            bad["binding_sha256"] = .string(SendReceiptRecovery.hash(try SendReceiptRecovery.encode(.object(binding))))
            XCTAssertNil(NativeSendOutcome.proof(row.requestId, intent: intent, result: bad), key)
        }
        let state = MessageState(outgoing: [row])
        for result: JSON in [.null, .object(["status": "refused_before_input"]), .object(["status": "not_offered", "reason": "not offered"])] {
            let receipt: JSONObject = ["request_id": .string(row.requestId), "state": "failed", "result": result]
            let next = NativeSendOutcome.settleReceipt(state, captured: row, receipt: receipt, deadline: 100, now: 99)
            XCTAssertEqual(next.outgoing[0].status, "uncertain"); XCTAssertTrue(next.outgoing[0].blocksSending)
            XCTAssertEqual(next.outgoing[0].requestId, row.requestId)
        }
        let proof = try XCTUnwrap(NativeSendOutcome.proof(row.requestId, intent: intent, result: good))
        var changed = row; changed.text += "changed"
        let moved = MessageState(outgoing: [changed])
        XCTAssertEqual(NativeSendOutcome.settle(moved, captured: row, proof: proof, deadline: 100, now: 99), moved)
        XCTAssertEqual(WorkspaceProtocol.operationClass(NativeSendOutcome.read), "read")
        XCTAssertEqual(WorkspaceProtocol.operationClass(NativeSendOutcome.send), "mutation")
        XCTAssertFalse(NativeSendOutcome.supported(relay: [NativeSendOutcome.send], native: NativeSendOutcome.operations))
    }
    func testLostOrPlainFailedOriginalUsesOnlyFreshReadWithTheSavedBinding() async throws {
        let v = try XCTUnwrap(vectors().first), (row, _, intent) = try fixture(v), result = try XCTUnwrap(v.object("read_not_sent"))
        let original = row.requestId
        StubProtocol.reset { request in
            if request.method == "GET" { return StubProtocol.json(200, JSON.object(["request_id": .string(original), "state": "failed", "error": "not offered"]).compact) }
            let id = request.json!.string("request_id")
            return StubProtocol.json(200, JSON.object(["request_id": .string(id), "state": "completed", "result": .object(result.putting("request_id", .string(id)))]).compact)
        }
        let connection = Connection(id: "gateway", label: "Fixture", url: "https://gateway.example.test", token: "synthetic")
        guard case .unconfirmed = try await StubProtocol.client().checkDelivery(connection, message: row, intent: nil, limit: .seconds(1)) else { return XCTFail("Plain failed receipt became Not sent") }
        XCTAssertEqual(StubProtocol.requests.count, 1)
        guard case .outcome(let proof, _) = try await StubProtocol.client().checkDelivery(connection, message: row, intent: nil, limit: .seconds(1), nativeOutcomeSupported: true) else { return XCTFail("Exact negative was not recovered") }
        XCTAssertEqual(proof.value?.string("status"), "not_offered")
        let post = try XCTUnwrap(StubProtocol.requests.last?.json)
        XCTAssertEqual(post.string("operation"), NativeSendOutcome.read); XCTAssertNotEqual(post.string("request_id"), original)
        XCTAssertEqual(post.object("payload")?.object("binding"), intent.value())
        XCTAssertEqual(StubProtocol.requests.filter { $0.method == "POST" }.count, 1)
    }
    func testHistoryEchoCannotSettleAnUnknownNegotiatedSend() throws {
        let v = try XCTUnwrap(vectors().first), (row, _, _) = try fixture(v)
        let event = Event(id: "echo", role: "You", text: row.text, time: "", at: 2, requestId: row.requestId, source: "hgs_delivery")
        XCTAssertTrue(ConversationMerge.merge(target: row.target, native: [event], outgoing: [row]).recordedRequestIds.isEmpty)
        let state = MessageState(outgoing: [row])
        XCTAssertEqual(OutboxRetention.recorded(state, [row.requestId], exact: [:]), state)
        var legacy = row; legacy.nativeSendIntent = nil
        XCTAssertTrue(ConversationMerge.merge(target: row.target, native: [event], outgoing: [legacy]).recordedRequestIds.contains(row.requestId))
        var corrupt = row; corrupt.status = "submitted"
        XCTAssertEqual(corrupt.recover().status, "uncertain")
    }
}
