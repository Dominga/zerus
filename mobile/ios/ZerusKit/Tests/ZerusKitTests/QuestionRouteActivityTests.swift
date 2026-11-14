import XCTest
@testable import ZerusKit

final class QuestionRouteActivityTests: XCTestCase {
    func testOutcomesFollowTheSharedContractAndNeverBecomeOutgoingReceipts() throws {
        let contract = try XCTUnwrap(SharedFixture.json("question-route-outcome-contract.json").object)
        for item in contract.objects("activity") {
            let line = try XCTUnwrap(item.object("line"))
            let label = QuestionRouteActivity.label(line,sessionId: contract.string("session_id"))
            XCTAssertEqual(label != nil,item.bool("accepted"),item.string("name"))
            if let label { XCTAssertEqual(label,item.string("label")) }
            let raw: JSONObject = ["session_id": contract["session_id"]!,"question_route_activity": .array([.object(line)])]
            let events = NativeEvents.events(raw)
            XCTAssertEqual(events.count,label == nil ? 0 : 1,item.string("name"))
            if let event = events.first {
                XCTAssertTrue(event.text.hasPrefix(item.string("label")))
                XCTAssertEqual(event.at,1794599000,accuracy: 0.001)
                XCTAssertEqual(event.role,"System")
                XCTAssertEqual(event.requestId,"")
                XCTAssertEqual(event.delivery,"")
                XCTAssertEqual(event.outgoingId,"")
                XCTAssertTrue(QuestionRouteActivity.events(raw.putting("agent_id",.string("worker"))).isEmpty)
                XCTAssertTrue(QuestionRouteActivity.events(raw.putting("archive_id",.string("archive"))).isEmpty)
            }
        }
    }
    func testFailedRoutesLeaveSupervisionAndNeedThePerson() throws {
        let contract = try XCTUnwrap(SharedFixture.json("question-route-outcome-contract.json").object)
        for item in contract.objects("decisions") {
            XCTAssertEqual(ChildSessions.quiet(try XCTUnwrap(item.object("row"))),item.bool("quiet"),item.string("state"))
        }
    }
    func testNoPartialListOrMoreThanTwentyLinesIsShown() throws {
        let contract = try XCTUnwrap(SharedFixture.json("question-route-outcome-contract.json").object)
        let good = try XCTUnwrap(contract.objects("activity").first?.object("line"))
        func raw(_ lines: [JSON]) -> JSONObject { ["session_id": contract["session_id"]!,"question_route_activity": .array(lines)] }
        XCTAssertTrue(QuestionRouteActivity.events(raw([.object(good),.object([:])])).isEmpty)
        XCTAssertTrue(QuestionRouteActivity.events(raw(Array(repeating: .object(good),count: 21))).isEmpty)
        XCTAssertEqual(QuestionRouteActivity.events(raw(Array(repeating: .object(good),count: 20))).count,20)
    }
}
