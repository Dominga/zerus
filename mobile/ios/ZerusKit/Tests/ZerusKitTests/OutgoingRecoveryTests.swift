import CryptoKit
import XCTest
@testable import ZerusKit

final class OutgoingRecoveryTests: XCTestCase {
    private let target = Target(computerId: "computer", session: "session", run: "run", conversation: "conversation", connectionId: "workspace")
    private let originalId = "00000000-0000-4000-8000-000000000001"
    private let nextId = "00000000-0000-4000-8000-000000000002"
    private var attachment: Attachment {
        Attachment(id: "00000000-0000-4000-8000-000000000003", name: "fixture.txt", mime: "text/plain", bytes: 4, sha256: String(repeating: "a", count: 64))
    }
    private var message: OutgoingMessage {
        OutgoingMessage(requestId: originalId, target: target, text: "Original payload", attachments: [attachment], createdAt: 1,
                        status: "uncertain", submittedAt: 0, submittedText: "", error: "Original unconfirmed result")
    }
    private var editor: Draft { Draft(target: target, text: "A different new composition", revision: "current", generation: "new-editor") }

    func testDismissalSurvivesSealedRestartAndRetainsOriginalEvidence() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let key = SymmetricKey(size: .bits256)
        let store = SealedFileStore(directory: directory, keys: StaticKeyProvider(key))
        let dismissed = OutgoingRecoveryPolicies.dismiss(MessageState(drafts: [editor], outgoing: [message]), message)
        try store.write("message_state", dismissed)
        let reopened = SealedFileStore(directory: directory, keys: StaticKeyProvider(key))
        let restored = try XCTUnwrap(reopened.read("message_state", as: MessageState.self)).recovered()
        XCTAssertEqual(restored, dismissed)
        XCTAssertEqual(restored.outgoing[0].status, "uncertain")
        XCTAssertTrue(restored.outgoing[0].deliveryDismissed)
        XCTAssertFalse(restored.outgoing[0].blocksSending)
        XCTAssertTrue(restored.outgoing[0].recoverable)
        XCTAssertEqual(restored.outgoing[0].attachments, [attachment])
        XCTAssertEqual(restored.drafts, [editor])
    }

    func testLegacyMessagesDefaultToUndismissedAndMalformedFlagsRefuse() throws {
        var legacy = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(message)) as? [String: Any])
        legacy.removeValue(forKey: "deliveryDismissed")
        let decoded = try JSONDecoder().decode(OutgoingMessage.self, from: JSONSerialization.data(withJSONObject: legacy))
        XCTAssertEqual(decoded, message)
        XCTAssertFalse(decoded.deliveryDismissed)
        XCTAssertTrue(decoded.blocksSending)
        legacy["deliveryDismissed"] = "true"
        XCTAssertThrowsError(try JSONDecoder().decode(OutgoingMessage.self, from: JSONSerialization.data(withJSONObject: legacy)))
    }

    func testLocalDismissalChangesOnlyItsFlagAndKeepsEveryComposition() {
        var parked = editor
        parked.detachedId = "parked"
        let state = MessageState(drafts: [editor, parked], outgoing: [message], pickerDraft: editor, pickerId: "picker")
        let changed = OutgoingRecoveryPolicies.dismiss(state, message)
        var expected = message
        expected.deliveryDismissed = true
        XCTAssertEqual(changed.outgoing, [expected])
        XCTAssertEqual(changed.drafts, state.drafts)
        XCTAssertEqual(changed.pickerDraft, state.pickerDraft)
        XCTAssertEqual(changed.pickerId, state.pickerId)
        XCTAssertEqual(OutgoingRecoveryPolicies.dismiss(changed, message), changed)
    }

    func testActiveSendingAndStaleOrAmbiguousCapturesCannotBeDismissed() {
        let state = MessageState(drafts: [editor], outgoing: [message])
        var sending = message
        sending.status = "sending"
        XCTAssertTrue(sending.blocksSending)
        XCTAssertEqual(OutgoingRecoveryPolicies.dismiss(state.update(sending), message), state.update(sending))
        XCTAssertEqual(OutgoingRecoveryPolicies.dismiss(state.update(sending), sending), state.update(sending))
        var foreign = message
        foreign.target = target.with(connectionId: "other-workspace")
        var changedText = message
        changedText.text = "Changed payload"
        var changedFile = message
        changedFile.attachments = []
        var changedTime = message
        changedTime.createdAt = 2
        for captured in [foreign, changedText, changedFile, changedTime] {
            XCTAssertEqual(OutgoingRecoveryPolicies.dismiss(state, captured), state)
        }
        let ambiguous = MessageState(outgoing: [message, message])
        XCTAssertEqual(OutgoingRecoveryPolicies.dismiss(ambiguous, message), ambiguous)
    }

    func testASettledOrphanedSendingRowCanRecoverWithoutReplacingTheEditor() {
        var original = message
        original.status = "sending"
        let state = MessageState(drafts: [editor], outgoing: [original])
        let captured = original.recover() // Same presentation as normalizedOutgoing after its flight ends.
        XCTAssertEqual(OutgoingRecoveryPolicies.dismiss(state, captured), state)
        let recovered = OutgoingRecoveryPolicies.dismiss(state, captured, recoverInterruptedSending: true)
        XCTAssertEqual(recovered.outgoing[0].status, "uncertain")
        XCTAssertTrue(recovered.outgoing[0].deliveryDismissed)
        XCTAssertFalse(recovered.outgoing[0].blocksSending)
        XCTAssertEqual(recovered.outgoing[0].requestId, original.requestId)
        XCTAssertEqual(recovered.outgoing[0].target, original.target)
        XCTAssertEqual(recovered.outgoing[0].text, original.text)
        XCTAssertEqual(recovered.outgoing[0].attachments, original.attachments)
        XCTAssertEqual(recovered.drafts, [editor])
        XCTAssertEqual(OutgoingRecoveryPolicies.dismiss(state, original, recoverInterruptedSending: true), state)
    }

    func testDismissalPermitsANewDistinctSendWithoutReusingTheOriginalRequest() throws {
        let initial = MessageState(drafts: [editor], outgoing: [message])
        XCTAssertThrowsError(try initial.enqueue(editor, requestId: nextId, at: 2))
        let dismissed = OutgoingRecoveryPolicies.dismiss(initial, message)
        XCTAssertThrowsError(try dismissed.enqueue(editor, requestId: originalId, at: 2))
        let written = try dismissed.enqueue(editor, requestId: nextId, at: 2)
        XCTAssertEqual(written.outgoing.count, 2)
        XCTAssertEqual(written.outgoing[0], dismissed.outgoing[0])
        XCTAssertEqual(written.outgoing[1].requestId, nextId)
        XCTAssertEqual(written.outgoing[1].text, editor.text)
        XCTAssertEqual(written.outgoing[1].status, "sending")
        XCTAssertFalse(written.outgoing[1].deliveryDismissed)
        XCTAssertTrue(written.outgoing[1].blocksSending)
    }

    func testLateOriginalReceiptPreservesDismissalNewSendAndNewerEditor() throws {
        let dismissed = OutgoingRecoveryPolicies.dismiss(MessageState(drafts: [editor], outgoing: [message]), message)
        let sent = try dismissed.enqueue(editor, requestId: nextId, at: 2)
        let newer = Draft(target: target, text: "An even newer composition", revision: "newer", generation: "newer-editor")
        let current = MessageChanges.draft(sent, newer)
        let settled = MessageChanges.result(current, originalId) { original in
            var updated = original
            updated.status = "submitted"
            updated.submittedAt = 3
            updated.submittedText = original.text
            updated.error = ""
            return updated
        }
        XCTAssertEqual(settled.outgoing[0].status, "submitted")
        XCTAssertTrue(settled.outgoing[0].deliveryDismissed)
        XCTAssertEqual(settled.outgoing[0].attachments, message.attachments)
        XCTAssertEqual(settled.outgoing[1], sent.outgoing[1])
        XCTAssertEqual(settled.drafts, [newer])
    }

    func testLateUnknownReadDoesNotRestoreTheDismissedSendingBlock() {
        let dismissed = OutgoingRecoveryPolicies.dismiss(MessageState(drafts: [editor], outgoing: [message]), message)
        let checked = MessageChanges.result(dismissed, originalId) { existing in
            var updated = existing
            updated.status = "uncertain"
            updated.error = "A later check is still unconfirmed"
            return updated
        }
        XCTAssertTrue(checked.outgoing[0].deliveryDismissed)
        XCTAssertFalse(checked.outgoing[0].blocksSending)
        XCTAssertEqual(checked.outgoing[0].requestId, originalId)
        XCTAssertEqual(checked.outgoing[0].text, message.text)
        XCTAssertEqual(checked.drafts, [editor])
    }

    func testDismissedUnknownStillChecksAndReconcilesAnExactNativeReceipt() {
        let dismissed = OutgoingRecoveryPolicies.dismiss(MessageState(drafts: [editor], outgoing: [message]), message)
        var probe = OutgoingReceiptProbe()
        XCTAssertTrue(probe.claim(target, navigation: 1, message: dismissed.outgoing[0], busy: false))
        XCTAssertFalse(probe.claim(target, navigation: 1, message: dismissed.outgoing[0], busy: false))
        let receipt = Event(id: "receipt:original", role: "You", text: message.text, time: "", at: 2,
                            source: "hgs_delivery", requestId: originalId)
        var unrelated = receipt
        unrelated.requestId = nextId
        XCTAssertTrue(ConversationMerge.merge(target: target, native: [unrelated], outgoing: dismissed.outgoing).recordedRequestIds.isEmpty)
        let merged = ConversationMerge.merge(target: target, native: [receipt], outgoing: dismissed.outgoing)
        XCTAssertEqual(merged.recordedRequestIds, [originalId])
        let recorded = MessageChanges.recorded(dismissed, merged.recordedRequestIds)
        XCTAssertEqual(recorded.outgoing[0].status, "recorded")
        XCTAssertTrue(recorded.outgoing[0].deliveryDismissed)
        XCTAssertEqual(recorded.outgoing[0].requestId, originalId)
        XCTAssertEqual(recorded.outgoing[0].attachments, [attachment])
        XCTAssertEqual(recorded.drafts, [editor])
    }
}
