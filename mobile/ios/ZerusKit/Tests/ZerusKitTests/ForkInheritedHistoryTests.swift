import XCTest
@testable import ZerusKit

/// zerus-b4qk.1: a fork's history begins with the rows it inherited from its source.
final class ForkInheritedHistoryTests: XCTestCase {
    private let target = Target(computerId: "computer", session: "codex/project/branch", run: "run", conversation: "conversation", connectionId: "workspace")

    private func row(_ seq: Int, _ type: String, _ detail: String, _ extra: JSONObject = [:]) -> JSON {
        var value: JSONObject = ["type": .string(type), "detail": .string(detail), "at": .double(Double(seq)), "seq": .int(Int64(seq)),
                                 "agent_id": "", "history_id": .string("id\(seq)"), "history_cursor": .string("cursor\(seq)"),
                                 "history_stream": "journal"]
        for (key, item) in extra { value[key] = item }
        return .object(value)
    }
    private func inherited(_ seq: Int, _ type: String, _ detail: String) -> JSON {
        row(seq, "Inherited", detail, ["inherited_type": .string(type), "inherited_from": "codex/project/root"])
    }
    private func page(_ rows: JSON...) -> JSONObject { ["history_epoch": "epoch", "events": .array(rows)] }

    func testInheritedRowsParseAsReadOnlyHistoryOfTheirSource() {
        let events = NativeEvents.events(page(
            inherited(1, "UserPromptSubmit", "source request"),
            inherited(2, "Stop", "source reply"),
            row(10, "UserPromptSubmit", "own request"),
            row(11, "Stop", "own reply", ["incoming_seq": .int(1)])))
        XCTAssertEqual(events.map(\.text), ["source request", "source reply", "own request", "own reply"])
        XCTAssertEqual(events.map { $0.inheritedFrom ?? "" }, ["codex/project/root", "codex/project/root", "", ""])
        // The original type decides the role; nothing acts as this session's own send.
        XCTAssertEqual(events.prefix(2).map(\.nativeType), ["UserPromptSubmit", "Stop"])
        XCTAssertEqual(events.prefix(2).map(\.role), ["You", "AgentMessage"])
        XCTAssertTrue(events.prefix(2).allSatisfy { $0.requestId.isEmpty && $0.deliveryState == nil })
    }

    func testInheritedRowsNeverCarryTheSourcesSendIdentitiesOrVoice() {
        // The same rows as this session's own prove each field parses; inherited
        // copies keep only their text.
        let delivery = "6f1c2a0e-3b4d-4c5e-8f60-7a8b9c0d1e2f", opencode = "0a1b2c3d-4e5f-4061-8728-394a5b6c7d8e"
        let voiceId = "9e8d7c6b-5a49-4382-9170-6f5e4d3c2b1a"
        let voice = VoiceMessage(id: voiceId, runId: "source-run", conversationId: "source-conversation", createdAt: 50, revision: 3,
                                 state: "submitted", audio: DisplayedAttachment(name: "voice.wav", mime: "audio/wav", bytes: 6444,
                                                                                messageId: voiceId, index: 0),
                                 durationMs: 200, transcript: "voice transcript")
        func rows(_ seq: Int, _ wrap: (Int, String, String, JSONObject) -> JSON) -> [JSON] {
            [wrap(seq, "UserPromptSubmit", "delivered request", ["source": "hgs_delivery", "message_id": .string(delivery),
                                                                 "delivery_state": "queued", "submitted_text": "delivered request"]),
             wrap(seq + 1, "UserPromptSubmit", "opencode request", ["source": "opencode_native", "request_id": .string(opencode)]),
             wrap(seq + 2, "UserMessage", "voice transcript", ["source": "hgs_voice", "message_id": .string(voiceId),
                                                               "voice": .object(VoiceProtocol.json(voice))])]
        }
        let inheritedRows = rows(1) { seq, type, detail, extra in
            var marked = extra
            marked["inherited_type"] = .string(type)
            marked["inherited_from"] = "codex/project/root"
            return self.row(seq, "Inherited", detail, marked)
        }
        let events = NativeEvents.events(["history_epoch": "epoch",
                                          "events": .array(inheritedRows + rows(10) { self.row($0, $1, $2, $3) })])
        let inherited = events.filter { $0.inheritedFrom != nil }, own = events.filter { $0.inheritedFrom == nil }
        XCTAssertEqual(inherited.map(\.text), ["delivered request", "opencode request", "voice transcript"])
        XCTAssertEqual(own.map(\.text), inherited.map(\.text))
        XCTAssertEqual(own.map(\.requestId), [delivery, opencode, voiceId])
        XCTAssertEqual(own.map(\.deliveryState), ["queued", nil, nil])
        XCTAssertEqual(own.last?.voice?.id, voiceId)
        XCTAssertTrue(inherited.allSatisfy { $0.role == "You" && $0.requestId.isEmpty && $0.deliveryState == nil && $0.voice == nil })
    }

    func testALocalSendIsNeverAdoptedByAnInheritedRowOfEqualText() {
        // #1283 R1: a short source turn, a fork within seconds, and the same
        // text sent from the fork within 10 s of the source's prompt.
        let sent = OutgoingMessage(requestId: "1c2d3e4f-5a6b-4c7d-8e9f-0a1b2c3d4e5f", target: target, text: "yes", attachments: [],
                                   createdAt: 104, status: "recorded", submittedAt: 105)
        let source = NativeEvents.events(page(inherited(100, "UserPromptSubmit", "yes")))[0]
        let alone = ConversationMerge.merge(target: target, native: [source], outgoing: [sent])
        XCTAssertEqual(alone.events.first, source)
        XCTAssertEqual(alone.events.dropFirst().map(\.source), ["mobile_outbox"])
        XCTAssertTrue(alone.recordedRequestIds.isEmpty)
        // The fork's own row arrives: it is the only candidate and records the send.
        let own = NativeEvents.events(page(row(106, "UserPromptSubmit", "yes")))[0]
        let both = ConversationMerge.merge(target: target, native: [source, own], outgoing: [sent])
        XCTAssertEqual(both.events.map(\.id), [source.id, "outgoing:\(sent.requestId)"])
        XCTAssertEqual(both.events.first, source)
        XCTAssertEqual(both.recordedRequestIds, [sent.requestId])
        XCTAssertEqual(both.events.last?.delivery, "recorded")
    }

    func testAnAttachmentReceiptNeverStandsForAnInheritedRow() {
        // #1283 N1: a receipt of this session with the same text, within 10 s.
        let delivery = "6f1c2a0e-3b4d-4c5e-8f60-7a8b9c0d1e2f"
        let receipt: JSON = ["type": "UserPromptSubmit", "source": "hgs_delivery", "message_id": .string(delivery), "detail": "yes",
                             "submitted_text": "yes", "at": 101, "agent_id": ""]
        var raw = page(inherited(100, "UserPromptSubmit", "yes"))
        raw["attachment_messages"] = [receipt]
        let events = NativeEvents.events(raw)
        XCTAssertEqual(events.map { $0.inheritedFrom ?? "" }, ["codex/project/root", ""])
        XCTAssertEqual(events.map(\.requestId), ["", delivery])
    }

    func testAnInheritedRowWithoutASourceOrWithAnUnknownTypeIsDropped() {
        let events = NativeEvents.events(page(
            row(1, "Inherited", "no source", ["inherited_type": "Stop"]),
            row(2, "Inherited", "tool", ["inherited_type": "PreToolUse", "inherited_from": "codex/project/root"]),
            row(3, "Stop", "own reply")))
        XCTAssertEqual(events.map(\.text), ["own reply"])
    }

    func testInheritedRepliesAreNeverIncomingOrUnread() {
        let events = NativeEvents.events(page(inherited(1, "Stop", "source reply"), row(11, "Stop", "own reply", ["incoming_seq": .int(1)])))
        XCTAssertFalse(ConversationReadPolicies.incoming(events[0]))
        XCTAssertTrue(ConversationReadPolicies.incoming(events[1]))
        XCTAssertFalse(ConversationReadPolicies.unread(nil, events[0]))
    }

    func testTheDisplayCacheKeepsTheMarkAndReadsOlderRowsWithoutIt() throws {
        let events = NativeEvents.events(page(inherited(1, "Stop", "source reply")))
        let decoded = try JSONDecoder().decode([Event].self, from: JSONEncoder().encode(events))
        XCTAssertEqual(decoded.first?.inheritedFrom, "codex/project/root")
        var older = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(events)) as? [[String: Any]])
        older[0].removeValue(forKey: "inheritedFrom")
        let legacy = try JSONDecoder().decode([Event].self, from: JSONSerialization.data(withJSONObject: older))
        XCTAssertNil(legacy.first?.inheritedFrom)
    }
}
