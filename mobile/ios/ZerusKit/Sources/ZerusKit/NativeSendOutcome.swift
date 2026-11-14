import Foundation

/// A bounded original binding, without another copy of attachment bytes or text.
public struct NativeSendIntent: Codable, Hashable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public let canonical: String
    public let bindingSha256: String
    public let rowSha256: String
    public let compactionId: String?
    public var description: String { "NativeSendIntent(retained)" }
    public var debugDescription: String { description }

    public func value() -> JSONObject? {
        guard canonical.utf8.count <= 4096, let value = try? JSON.parse(canonical).object,
              NativeSendOutcome.validBinding(value), let encoded = try? SendReceiptRecovery.encode(.object(value)),
              SendReceiptRecovery.exact(canonical, encoded), SendReceiptRecovery.hash(canonical) == bindingSha256,
              NativeSendOutcome.hex(rowSha256), compactionId == nil || compactionId!.isEmpty || SendReceiptRecovery.uuid(compactionId!) else { return nil }
        return value
    }
    public func matches(_ row: OutgoingMessage) -> Bool {
        guard let v = value() else { return false }
        return row.target.agentId.isEmpty && row.target.archiveId.isEmpty && rowSha256 == OutboxRetention.binding(row) &&
            v.string("message_request_id") == row.requestId && v.string("computer_id") == row.target.computerId &&
            SendReceiptRecovery.exact(v.string("session"), row.target.session) && v.string("run_id") == row.target.run &&
            SendReceiptRecovery.exact(v.string("conversation_id"), row.target.conversation)
    }
}

public struct NativeSendProof: Codable, Hashable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    public let canonical: String
    public var description: String { "NativeSendProof(retained)" }
    public var debugDescription: String { description }
    public var value: JSONObject? { canonical.utf8.count <= 8192 ? try? JSON.parse(canonical).object : nil }
}

public enum NativeSendOutcome {
    public static let send = "send_outcome", read = "send_outcome_receipt"
    public static let operations: Set<String> = [send, read]
    public static func supported(relay: Set<String>?, native: Set<String>?) -> Bool {
        guard let relay, let native else { return false }
        return relay.isSuperset(of: operations) && native.isSuperset(of: operations)
    }
    static func hex(_ text: String) -> Bool { text.utf8.count == 64 && text.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) } }
    static func validBinding(_ v: JSONObject) -> Bool {
        guard Set(v.keys) == Set(["schema", "message_request_id", "computer_id", "machine_id", "session_id", "session", "provider", "run_id", "conversation_id", "native_encoding", "native_digest"]),
              v["schema"] == .int(2), ["claude", "codex", "kimi"].contains(v.string("provider")),
              ["compact", "python_default"].contains(v.string("native_encoding")), hex(v.string("native_digest")) else { return false }
        for key in ["message_request_id", "machine_id", "session_id"] {
            guard let s = SendReceiptRecovery.text(v, key), SendReceiptRecovery.uuid(s) else { return false }
        }
        for (key, maximum, empty) in [("computer_id", 36, false), ("session", 512, false), ("run_id", 128, false), ("conversation_id", 256, true)] {
            guard let s = SendReceiptRecovery.text(v, key), SendReceiptRecovery.identity(s, maximum, empty: empty) else { return false }
        }
        return true
    }
    public static func freeze(_ row: OutgoingMessage, payload: JSONObject, provider: String, machineId: String, sessionId: String, encoding: String) -> NativeSendIntent? {
        guard !payload.has("agent_id"), payload.string("request_id") == row.requestId,
              SendReceiptRecovery.exact(payload.string("text"), row.text), payload.string("expected_run_id") == row.target.run,
              SendReceiptRecovery.exact(payload.string("expected_conversation_id"), row.target.conversation),
              let files = payload["attachments"]?.array, files.count == row.attachments.count,
              let native = try? SendReceiptRecovery.encode(.object(payload), ascii: false, spaced: encoding == "python_default") else { return nil }
        let v: JSONObject = ["schema": 2, "message_request_id": .string(row.requestId), "computer_id": .string(row.target.computerId),
            "machine_id": .string(machineId), "session_id": .string(sessionId), "session": .string(row.target.session), "provider": .string(provider),
            "run_id": .string(row.target.run), "conversation_id": .string(row.target.conversation), "native_encoding": .string(encoding),
            "native_digest": .string(SendReceiptRecovery.hash(native))]
        guard validBinding(v), let canonical = try? SendReceiptRecovery.encode(.object(v)) else { return nil }
        let intent = NativeSendIntent(canonical: canonical, bindingSha256: SendReceiptRecovery.hash(canonical), rowSha256: OutboxRetention.binding(row),
                                      compactionId: payload.has("expected_compaction_id") ? payload.string("expected_compaction_id") : nil)
        return intent.matches(row) ? intent : nil
    }
    public static func request(_ id: String, intent: NativeSendIntent, payload: JSONObject? = nil) throws -> JSONObject {
        guard let binding = intent.value(), SendReceiptRecovery.uuid(id),
              payload == nil ? id != binding.string("message_request_id") : id == binding.string("message_request_id") else { throw ZerusError.invalid("The saved message details are unavailable. Review this message.") }
        var result: JSONObject = ["schema": 2, "request_id": .string(id), "binding": .object(binding), "binding_sha256": .string(intent.bindingSha256)]
        if let payload {
            guard let raw = try? SendReceiptRecovery.encode(.object(payload), ascii: false, spaced: binding.string("native_encoding") == "python_default"),
                  SendReceiptRecovery.hash(raw) == binding.string("native_digest") else { throw ZerusError.invalid("The saved message details are unavailable. Review this message.") }
            result["payload"] = .object(payload)
        }
        return result
    }
    public static func proof(_ id: String, intent: NativeSendIntent, result: JSONObject) -> NativeSendProof? {
        guard SendReceiptRecovery.uuid(id), intent.value() != nil, result["schema"] == .int(2), SendReceiptRecovery.text(result, "request_id") == id,
              result.string("binding_sha256") == intent.bindingSha256, let returned = result.object("binding"),
              let canonical = try? SendReceiptRecovery.encode(.object(returned)), SendReceiptRecovery.exact(canonical, intent.canonical) else { return nil }
        let base: Set<String> = ["schema", "request_id", "binding", "binding_sha256", "status", "proof"]
        switch result.string("status") {
        case "not_offered":
            guard Set(result.keys) == base.union(["reason"]), result["proof"] == .string("native_input_lock"),
                  let reason = SendReceiptRecovery.text(result, "reason"), SendReceiptRecovery.identity(reason, 4096) else { return nil }
        case "submitted":
            guard Set(result.keys) == base.union(["submitted_at", "queued"]), result["proof"] == .string("original_receipt"),
                  let at = result["submitted_at"]?.numberValue, at.isFinite, at >= 0, case .bool? = result["queued"] else { return nil }
        default: return nil
        }
        guard let canonical = try? JSON.object(result).text(), canonical.utf8.count <= 8192 else { return nil }
        return NativeSendProof(canonical: canonical)
    }
    public static func fromReceipt(_ id: String, intent: NativeSendIntent, receipt: JSONObject, lookup: Bool = false) -> NativeSendProof? {
        guard receipt.string("request_id") == id, let result = receipt.object("result"),
              let proof = proof(id, intent: intent, result: result) else { return nil }
        let expected = lookup || result.string("status") == "submitted" ? "completed" : "failed"
        return receipt.string("state") == expected ? proof : nil
    }
    public static func confirmed(_ row: OutgoingMessage) -> Bool {
        guard ["submitted", "recorded"].contains(row.status), let intent = row.nativeSendIntent, intent.matches(row),
              let result = row.nativeSendProof?.value, result.string("status") == "submitted" else { return false }
        return proof(result.string("request_id"), intent: intent, result: result) != nil
    }
    public static func settle(_ state: MessageState, captured: OutgoingMessage, proof: NativeSendProof, deadline: Double, now: Double) -> MessageState {
        guard now < deadline, let intent = captured.nativeSendIntent, intent.matches(captured), let result = proof.value,
              self.proof(result.string("request_id"), intent: intent, result: result) != nil,
              state.outgoing.filter({ $0.requestId == captured.requestId }).count == 1,
              let index = state.outgoing.firstIndex(where: { $0.requestId == captured.requestId }) else { return state }
        let current = state.outgoing[index]
        guard ["sending", "uncertain"].contains(current.status), current.target == captured.target,
              current.createdAt == captured.createdAt, current.nativeSendIntent == intent, intent.matches(current) else { return state }
        var row = current
        row.nativeSendProof = proof
        if result.string("status") == "not_offered" {
            row.status = "failed"; row.error = "The computer rejected this message. Review it before trying again."
        } else {
            row.status = "submitted"; row.submittedAt = result.number("submitted_at"); row.submittedText = row.text; row.error = ""
        }
        var next = state; next.outgoing[index] = row; return next
    }
    public static func settleReceipt(_ state: MessageState, captured: OutgoingMessage, receipt: JSONObject, deadline: Double, now: Double) -> MessageState {
        guard let intent = captured.nativeSendIntent else { return state }
        if let proof = fromReceipt(captured.requestId, intent: intent, receipt: receipt) {
            return settle(state, captured: captured, proof: proof, deadline: deadline, now: now)
        }
        guard now < deadline, state.outgoing.filter({ $0.requestId == captured.requestId }).count == 1,
              let index = state.outgoing.firstIndex(where: { $0.requestId == captured.requestId }),
              state.outgoing[index] == captured, ["sending", "uncertain"].contains(captured.status) else { return state }
        var next = state
        next.outgoing[index].status = "uncertain"
        next.outgoing[index].error = "Delivery is unconfirmed. Check its receipt before sending again."
        return next
    }
}
