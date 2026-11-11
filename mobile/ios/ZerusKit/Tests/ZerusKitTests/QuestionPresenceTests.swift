import XCTest
@testable import ZerusKit

/// zerus-a4mub.25: iOS parity of the expanded question presence (Android zerus-a4mub.12, completeness zerus-a4mub.27).
/// Every row of the shared question-presence-contract.json, which Android QuestionPresencePolicies checks too,
/// then the iOS acceptance cases.
final class QuestionPresenceTests: XCTestCase {
    private let target = Target(computerId: "computer", session: "session", run: "run", conversation: "conversation", connectionId: "workspace")
    private let now: Int64 = 1_000_000_000

    private func question(_ id: String = "request", hash: String? = nil, canAnswer: Bool = true, optional: Bool = false,
                          reason: String = "") -> Question {
        Question(id: id, hash: hash ?? "hash-\(id)", prompts: [Prompt(id: "q", text: "Question", choices: [Choice(id: "first", label: "First", description: "")],
                                                                          multi: false, required: true, other: false, header: "", body: "",
                                                                          otherLabel: "", otherDescription: "")],
                 canAnswer: canAnswer, canSkip: true, optional: optional, delivery: "", createdAt: 1, source: "codex_async",
                 unavailableReason: reason, run: target.run, conversation: target.conversation)
    }
    private func resolve(_ display: QuestionDisplayState, _ questions: [Question], verified: Bool = true, available: Bool = true,
                         checking: Bool = false, checkedAgo: Int64? = 5_000, complete: Bool = true) -> QuestionPresenceView? {
        QuestionPresencePolicies.resolve(display, questions: questions, verified: verified, available: available, checking: checking,
                                         checkedAtMillis: checkedAgo.map { now - $0 } ?? 0, nowMillis: now, complete: complete)
    }
    private func expanded(_ question: Question) -> QuestionDisplayState { QuestionDisplayState(owner: target.key).open(question) }

    func testEverySharedContractRowResolvesExactly() throws {
        let contract = try XCTUnwrap(try SharedFixture.json("question-presence-contract.json").object)
        XCTAssertEqual(contract.int("schema"), 1)
        let cases = contract.objects("cases")
        XCTAssertGreaterThanOrEqual(cases.count, 15)
        for row in cases {
            let name = row.string("name")
            let questions = row.objects("questions").map {
                question($0.string("id"), hash: $0.string("hash"), canAnswer: $0.bool("can_answer"), optional: $0.bool("optional"),
                         reason: $0.string("unavailable_reason"))
            }
            let display = row.object("expanded").map { expanded(question($0.string("id"), hash: $0.string("hash"))) }
                ?? QuestionDisplayState(owner: target.key)
            let checkedAgo: Int64? = row.value("checked_ago_ms").flatMap { $0 == .null ? nil : row.int64("checked_ago_ms") }
            let view = resolve(display, questions, verified: row.bool("verified"), available: row.bool("available"),
                               checking: row.bool("checking"), checkedAgo: checkedAgo, complete: row.bool("complete", default: true))
            guard let expect = row.object("expect") else { XCTAssertNil(view, name); continue }
            let resolved = try XCTUnwrap(view, name)
            XCTAssertEqual(resolved.presence.rawValue, expect.string("presence"), name)
            XCTAssertEqual(resolved.title, expect.string("title"), name)
            XCTAssertEqual(resolved.detail, expect.string("detail"), name)
            XCTAssertEqual(resolved.age, expect.string("age"), name)
            XCTAssertEqual(resolved.modal, expect.bool("modal"), name)
            XCTAssertEqual(resolved.refresh, expect.bool("refresh"), name)
            let hash = expect.value("replacement_hash").flatMap { $0 == .null ? nil : expect.string("replacement_hash") }
            XCTAssertEqual(resolved.replacement?.hash, hash, name)
        }
    }

    func testEverySharedAgeAndReasonMatches() throws {
        let contract = try XCTUnwrap(try SharedFixture.json("question-presence-contract.json").object)
        for row in contract.objects("ages") {
            let ago: Int64? = row.value("checked_ago_ms").flatMap { $0 == .null ? nil : row.int64("checked_ago_ms") }
            XCTAssertEqual(QuestionPresencePolicies.age(ago.map { now - $0 } ?? 0, now: now), row.string("age"), "\(ago as Any)")
        }
        for row in contract.objects("reasons") {
            XCTAssertEqual(QuestionPresencePolicies.reason(row.string("raw"), optional: row.bool("optional")), row.string("reason"), row.string("raw"))
        }
    }

    func testAnAgedOptionalRequestStaysPendingAndUnanswerableWhileTheAgentWorks() throws {
        let request = question(canAnswer: false, optional: true)
        let view = try XCTUnwrap(resolve(expanded(request), [request], checkedAgo: 45 * 60_000))
        XCTAssertEqual(view.presence, .waiting)
        XCTAssertEqual(view.detail, "The agent is still working. This optional request stays open until it asks again.")
        XCTAssertEqual(view.age, "Checked 45m ago")
        // Its age never makes it answerable, closed or replaced.
        XCTAssertTrue(view.modal)
        XCTAssertNil(view.replacement)
    }

    func testARefreshRemovalWhileExpandedBecomesAnInlineClosure() throws {
        let request = question()
        let display = expanded(request)
        XCTAssertEqual(resolve(display, [request])?.presence, .answerable)
        // The refresh is in flight: the same exact expansion is checking, still inline.
        let checking = try XCTUnwrap(resolve(display, [], verified: false, available: false, checking: true))
        XCTAssertEqual(checking.presence, .checking)
        XCTAssertFalse(checking.modal)
        // The verified, complete projection no longer carries it: closed, inline, the identity kept.
        let closed = try XCTUnwrap(resolve(display, []))
        XCTAssertEqual(closed.presence, .closed)
        XCTAssertFalse(closed.modal)
        XCTAssertEqual(display.expandedId, request.id)
        XCTAssertEqual(display.expandedHash, request.hash)
    }

    func testRotationAndRestoreResolveTheSameExactExpansion() throws {
        let request = question()
        let display = expanded(request).withPage(request, 1)
        // A restored view keeps the value: the same identity, page and resolution, from a reloaded projection.
        let restored = display
        let reloaded = [question(), question("other")]
        XCTAssertEqual(restored, display)
        XCTAssertEqual(restored.page(request), 1)
        XCTAssertEqual(resolve(restored, reloaded)?.presence, .answerable)
        XCTAssertEqual(restored.resolve(reloaded), request)
    }

    func testObsoleteStatesStayInlineSoAnUnknownOutboxReviewIsReachable() throws {
        // Presence describes only the question source: every state without the exact request is inline
        // (no sheet), so an unrelated unknown-delivery Review in the conversation stays reachable.
        let display = expanded(question())
        let views = [resolve(display, []), resolve(display, [question(hash: "hash-2")]), resolve(display, [], complete: false),
                     resolve(display, [], verified: false, checking: true), resolve(display, [], verified: false)].compactMap { $0 }
        XCTAssertEqual(views.map(\.presence), [.closed, .replaced, .checking, .checking, .unavailable])
        XCTAssertTrue(views.allSatisfy { !$0.modal })
        XCTAssertTrue(views.allSatisfy { !$0.detail.contains("deliver") && !$0.title.contains("Unknown") })
    }

    func testAnIncompleteProjectionWhileIndexingNeverClosesOrReplaces() throws {
        let display = expanded(question())
        for questions in [[], [question(hash: "hash-2")]] {
            let view = try XCTUnwrap(resolve(display, questions, complete: false))
            XCTAssertEqual(view.presence, .checking)
            XCTAssertNil(view.replacement)
            XCTAssertTrue(view.refresh)
            XCTAssertTrue(view.detail.contains("indexing"))
        }
        // Complete again (or an older CLI): it closes or replaces.
        XCTAssertEqual(resolve(display, [])?.presence, .closed)
        XCTAssertEqual(resolve(display, [question(hash: "hash-2")])?.replacement?.hash, "hash-2")
    }

    func testAnAnswerIsNeverEnabledFromCacheAgeWorkingOrAPartialScan() {
        let request = question()
        let display = expanded(request)
        XCTAssertEqual(resolve(display, [request], verified: false, available: false)?.presence, .waiting)
        XCTAssertEqual(resolve(display, [request], available: false, checkedAgo: 7 * 86_400_000)?.presence, .waiting)
        XCTAssertEqual(resolve(display, [question(canAnswer: false, reason: "the agent is working")])?.presence, .waiting)
        for complete in [true, false] { XCTAssertNotEqual(resolve(display, [], verified: false, complete: complete)?.presence, .answerable) }
        XCTAssertNil(resolve(QuestionDisplayState(owner: target.key), [request]))
    }
}
