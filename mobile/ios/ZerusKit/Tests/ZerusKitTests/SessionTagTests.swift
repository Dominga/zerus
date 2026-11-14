import XCTest
@testable import ZerusKit

final class SessionTagTests: XCTestCase {
    func testNewTagsRejectShellSyntaxWithoutChangingLegacySelectors() throws {
        let names = try XCTUnwrap(try SharedFixture.json("session-tag-contract.json").object)
        for tag in names.strings("rejected") {
            XCTAssertFalse(LaunchPresentation.nameError(tag).isEmpty, tag)
            XCTAssertThrowsError(try SessionActionPolicies.arguments("fork", ["tag": .string(tag)]), tag)
            XCTAssertThrowsError(try SessionActionPolicies.arguments("rename", ["new_name": .string("codex/project/" + tag)]), tag)
        }
        for tag in names.strings("accepted") { XCTAssertEqual("", LaunchPresentation.nameError(tag), tag) }
        let target = Target(computerId: "computer", session: "codex/project/old$tag", run: "run", conversation: "conversation", connectionId: "workspace")
        XCTAssertEqual("", SessionActionPolicies.inspectReason(target, "rename", ["new_name": "codex/project/safe"], ["allowed_actions": ["rename"]]))
    }
}
