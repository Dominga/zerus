import Foundation
import XCTest
@testable import ZerusKit

final class SavedExecutionTextTests: XCTestCase {
    func testPhasesMatchTheSharedSavedEvidenceContract() throws {
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: SharedFixture.url("task-saved-execution.json"))) as? [String: Any])
        for c in try XCTUnwrap(root["cases"] as? [[String: Any]]) where c["available"] as? Bool == true {
            guard let execution = c["execution"] as? [String: Any], let current = execution["current"] as? [String: Any] else { continue }
            XCTAssertEqual(SavedExecutionText.phase(try XCTUnwrap(current["phase"] as? String)), c["phase_label"] as? String, c["name"] as? String ?? "")
        }
        XCTAssertNil(SavedExecutionText.phase("future_phase"))
        XCTAssertNil(SavedExecutionText.phase(" running"))
    }

    func testUncertaintyAndFailedRefreshRemainVisibleBesideAnyPhase() {
        XCTAssertEqual(SavedExecutionText.notices(conflict: true, unknown: true, unexplained: true, legacy: true, stale: true),
                       ["Execution records conflict.", "Some attempt delivery is unknown.", "Some execution history is unexplained.",
                        "Legacy execution record", "This saved execution could not be refreshed."])
        XCTAssertTrue(SavedExecutionText.notices(conflict: false, unknown: false, unexplained: false, legacy: false, stale: false).isEmpty)
        XCTAssertEqual(SavedExecutionText.notices(conflict: false, unknown: true, unexplained: false, legacy: false, stale: false),
                       ["Some attempt delivery is unknown."])
        XCTAssertEqual(SavedExecutionText.phase("submitted"), "Submitted — begin unconfirmed")
        XCTAssertEqual(SavedExecutionText.phase("running"), "Begin recorded")
        XCTAssertNotEqual(SavedExecutionText.phase("submitted"), SavedExecutionText.phase("running"))
    }
}
