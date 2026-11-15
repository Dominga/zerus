import Foundation
import XCTest
import ZerusReplica
@testable import ZerusKit

final class SavedExecutionPresentationTests: XCTestCase {
    private func cases() throws -> [[String: Any]] {
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: SharedFixture.url("task-saved-execution.json"))) as? [String: Any])
        return try XCTUnwrap(root["cases"] as? [[String: Any]])
    }
    private func task(_ c: [String: Any]) -> TaskInfo {
        let evidence = (c["execution"] as? [String: Any]).map { e in
            let current = (e["current"] as? [String: Any]).map { a in
                TaskAttemptInfo(id: a["id"] as! String, generation: (a["generation"] as! NSNumber).uint64Value, origin: a["origin"] as! String,
                    executor: (a["executor"] as? [String: Any]).map { SessionRef(machine: $0["machine"] as! String, session: $0["session"] as! String, sessionId: $0["session_id"] as! String) },
                    requestId: a["request_id"] as! String, requirement: (a["requirement"] as! NSNumber).uint64Value, phase: a["phase"] as! String,
                    at: (a["at"] as! NSNumber).int64Value, beganAt: (a["began_at"] as? NSNumber)?.int64Value,
                    progressAt: (a["progress_at"] as? NSNumber)?.int64Value, legacy: a["legacy"] as! Bool)
            }
            return TaskExecutionInfo(requirement: (e["requirement"] as! NSNumber).uint64Value, attempts: (e["attempts"] as! NSNumber).uint64Value,
                current: current, conflict: e["conflict"] as! Bool, hasUnknown: e["has_unknown"] as! Bool, hasUnexplained: e["has_unexplained"] as! Bool)
        }
        return TaskInfo(uuid: "fixture-task", id: "T-a1b2c3", project: "project-1", schema: (c["schema"] as! NSNumber).int64Value,
            writable: false, createdAt: 1, stageAt: 999, author: .person, stage: .active,
            target: .session(session: SessionRef(machine: "wrong-machine", session: "wrong-target")), rank: "V",
            session: SessionRef(machine: "wrong-machine", session: "wrong-current-session"), summary: "workflow is not execution",
            cancelledAt: nil, cancelReason: "", text: "Task", log: [], execution: evidence)
    }
    private func section(_ t: TaskInfo, project: String? = "project-1", stale: Bool = false) -> SavedExecutionSectionState {
        SavedExecutionPresentation.section(t, projectId: project, stale: stale) { "saved:\($0)" }
    }
    func testSharedContractStaysSavedBoundedAndExplicitlyUnavailable() throws {
        for c in try cases() {
            let t = task(c), state = section(t)
            XCTAssertEqual(state.available, c["available"] as! Bool, c["name"] as! String)
            XCTAssertEqual(state.phase, c["phase_label"] as? String, c["name"] as! String)
            if !state.available { XCTAssertTrue(state.rows.isEmpty) }
            XCTAssertFalse(t.writable)
        }
    }
    func testIdentityTimesAndRevisionsComeOnlyFromTheSavedCurrentAttempt() throws {
        let t = task(try cases()[1]), before = t, state = section(t, stale: true)
        let rows = Dictionary(uniqueKeysWithValues: state.rows.map { ($0.label, $0.value) })
        XCTAssertEqual(rows["Generation"]!, "3")
        XCTAssertEqual(rows["Requirement revision"]!, "2")
        XCTAssertEqual(rows["Current requirement revision"]!, "4")
        XCTAssertEqual(rows["Attempt"]!, "11111111-1111-4111-8111-111111111111")
        XCTAssertEqual(rows["Request"]!, "44444444-4444-4444-8444-444444444444")
        XCTAssertEqual(rows["Recorded executor"]!, "22222222-2222-4222-8222-222222222222\ncodex/example/worker\nlaunch_id:33333333-3333-4333-8333-333333333333")
        XCTAssertEqual(rows["Recorded at"]!, "saved:1700000000000")
        XCTAssertEqual(rows["Begin recorded at"]!, "saved:1700000001000")
        XCTAssertEqual(rows["Progress recorded at"]!, "saved:1700000002000")
        XCTAssertTrue(state.notices.contains("This saved execution could not be refreshed."))
        XCTAssertFalse(section(t).notices.contains("This saved execution could not be refreshed."))
        XCTAssertEqual(before, t)
        XCTAssertFalse(section(t, project: "other-project").available)
        XCTAssertFalse(section(t, project: nil).available)
        var absent = t; absent.execution = nil
        XCTAssertFalse(section(absent).available)
    }
    func testUnknownLegacyAndFutureTagsNeverBorrowAuthorityOrWorkflowFacts() throws {
        let cs = try cases(), older = section(task(cs[10]))
        XCTAssertEqual(older.phase, "Begin recorded")
        XCTAssertTrue(older.notices.contains("Some attempt delivery is unknown."))
        let legacy = section(task(cs[8]))
        XCTAssertTrue(legacy.notices.contains("Legacy execution record"))
        XCTAssertFalse(legacy.rows.contains { ["Attempt", "Generation"].contains($0.label) })
        XCTAssertNil(legacy.rows.first { $0.label == "Recorded at" }?.value)
        XCTAssertNil(legacy.rows.first { $0.label == "Recorded executor" }?.value)
        let none = section(task(cs[9]))
        XCTAssertTrue(none.available); XCTAssertFalse(none.hasCurrent)
        XCTAssertEqual(none.rows.map(\.label), ["Current requirement revision"])
        XCTAssertEqual(none.notices.count, 3)
        var t = task(cs[0]); t.execution!.current!.phase = "future"
        XCTAssertFalse(section(t).available)
        t = task(cs[0]); t.execution!.current!.origin = "future"
        XCTAssertFalse(section(t).available)
        t = task(cs[0]); t.execution!.current!.executor = SessionRef(machine: "saved-machine", session: "Not recorded")
        let nameOnly = section(t)
        XCTAssertEqual(nameOnly.rows.first { $0.label == "Recorded executor" }?.value, "saved-machine\nNot recorded")
        XCTAssertTrue(nameOnly.notices.contains("Executor identity unresolved"))
    }
}
