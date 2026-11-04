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
