package app.zerus.mobile

import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class OutgoingDismissalTest {
    private val target = Target("computer", "session", "run", "conversation", "workspace")
    private val request = "11111111-1111-4111-8111-111111111111"
    private val nextRequest = "22222222-2222-4222-8222-222222222222"
    private val file = Attachment("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "fixture.txt", "text/plain", 7, "a".repeat(64))
    private val original = OutgoingMessage(request, target, "Original saved message", listOf(file), 100.0,
        status = "uncertain", error = "Delivery is unconfirmed.")
    private val editor = Draft(target, "Next distinct composition", attachments = listOf(file.copy(id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")))

    @Test fun dismissalRetainsUnknownPayloadAndLeavesEveryDraftUnchanged() {
        val question = Draft(target, "Saved answer", questionId = "question", questionHash = "hash")
        val detached = editor.copy(detachedId = "saved-composition")
        val unrelated = original.copy(requestId = nextRequest, target = target.copy(session = "other"))
        val state = MessageState(drafts = listOf(editor, question, detached), outgoing = listOf(original, unrelated), pickerDraft = detached)
        val dismissed = MessageChanges.dismissOutgoing(state, original)
        assertEquals(state.copy(outgoing = listOf(original.copy(deliveryDismissed = true), unrelated)), dismissed)
        assertEquals("uncertain", dismissed.outgoing.first().status)
        assertFalse(dismissed.outgoing.first().blocksSending)
        assertTrue(dismissed.outgoing.last().blocksSending)
        assertSame(dismissed, MessageChanges.dismissOutgoing(dismissed, dismissed.outgoing.first()))
    }

    @Test fun restartAllowsOnlyANewRequestAndRetainsTheOriginalTombstoneAndFiles() {
        val dismissed = MessageChanges.dismissOutgoing(MessageState(listOf(editor), listOf(original)), original)
        val restored = MessageCodec.state(JSONObject(MessageCodec.stateJson(dismissed).toString()))
        assertEquals(dismissed, restored)
        assertFalse(restored.outgoing.single().blocksSending)
        assertEquals(editor, restored.drafts.single())
        val next = restored.enqueue(editor, nextRequest, 200.0)
        assertEquals(listOf(request, nextRequest), next.outgoing.map { it.requestId })
        assertEquals(original.copy(deliveryDismissed = true), next.outgoing.first())
        assertEquals(editor.text, next.outgoing.last().text)
        assertEquals("sending", next.outgoing.last().status)
        assertFalse(next.outgoing.last().deliveryDismissed)
        assertTrue(next.outgoing.last().blocksSending)
        assertThrows(IllegalArgumentException::class.java) { restored.enqueue(editor, request, 200.0) }
    }

    @Test fun legacyAndMalformedDismissalFieldsNeverReleaseAnUnknownDelivery() {
        val legacy = MessageCodec.outgoingJson(original).also { it.remove("delivery_dismissed") }
        assertFalse(MessageCodec.outgoing(legacy).deliveryDismissed)
        assertTrue(MessageCodec.outgoing(legacy).blocksSending)
        for (invalid in listOf(JSONObject.NULL, "true", 1, JSONObject())) {
            val restored = MessageCodec.outgoing(JSONObject(legacy.toString()).put("delivery_dismissed", invalid))
            assertFalse(restored.deliveryDismissed)
            assertTrue(restored.blocksSending)
        }
        val dismissed = MessageCodec.outgoing(JSONObject(legacy.toString()).put("delivery_dismissed", true))
        assertTrue(dismissed.deliveryDismissed)
        assertFalse(dismissed.blocksSending)
    }

    @Test fun activeAndSettledAttemptsCannotBeDismissedEvenWithAStaleCapture() {
        for (status in listOf("sending", "submitted", "recorded", "reviewed", "unknown-future-state")) {
            val current = original.copy(status = status)
            val state = MessageState(listOf(editor), listOf(current))
            assertSame(state, MessageChanges.dismissOutgoing(state, current))
            assertSame(state, MessageChanges.dismissOutgoing(state, original))
        }
        val sending = original.copy(status = "sending", deliveryDismissed = true)
        assertTrue(sending.blocksSending)
        assertTrue(sending.recover().blocksSending)
        assertFalse(sending.recover().deliveryDismissed)
    }

    @Test fun anOrphanedSendingProjectionRequiresExplicitFlightRecoveryAndExactCapturedIdentity() {
        val interrupted = original.copy(status = "sending", error = "")
        val captured = interrupted.recover()
        val state = MessageState(listOf(editor), listOf(interrupted))
        assertSame(state, MessageChanges.dismissOutgoing(state, captured))
        assertSame(state, MessageChanges.dismissOutgoing(state, interrupted, recoverInterruptedSending = true))
        val dismissed = MessageChanges.dismissOutgoing(state, captured, recoverInterruptedSending = true)
        assertEquals(captured.copy(deliveryDismissed = true), dismissed.outgoing.single())
        assertEquals(editor, dismissed.drafts.single())
        assertFalse(dismissed.outgoing.single().blocksSending)
        assertSame(state, MessageChanges.dismissOutgoing(state, captured.copy(text = "Other payload"), recoverInterruptedSending = true))
    }

    @Test fun foreignChangedOrAmbiguousAttemptsNeverDismissTheSavedOriginal() {
        val state = MessageState(listOf(editor), listOf(original))
        val captures = listOf(original.copy(requestId = nextRequest), original.copy(requestId = ""),
            original.copy(target = target.copy(connectionId = "other-workspace")),
            original.copy(target = target.copy(computerId = "other-computer")),
            original.copy(target = target.copy(session = "other-session")),
            original.copy(target = target.copy(run = "other-run")),
            original.copy(target = target.copy(conversation = "other-conversation")),
            original.copy(text = "Changed payload"), original.copy(attachments = emptyList()),
            original.copy(createdAt = 101.0), original.copy(error = "New receipt error"))
        captures.forEach { assertSame(state, MessageChanges.dismissOutgoing(state, it)) }
        val ambiguous = state.copy(outgoing = listOf(original, original.copy(target = target.copy(run = "other"))))
        assertSame(ambiguous, MessageChanges.dismissOutgoing(ambiguous, original))
    }

    @Test fun aFailedMessageMayBeDismissedWithoutClaimingCancellationOrDelivery() {
        val failed = original.copy(status = "failed", error = "Not sent")
        val state = MessageState(listOf(editor), listOf(failed))
        assertEquals(state.copy(outgoing = listOf(failed.copy(deliveryDismissed = true))),
            MessageChanges.dismissOutgoing(state, failed))
    }

    @Test fun lateOriginalOutcomeStillSettlesAfterDismissalWithoutReplacingTheEditor() {
        val dismissed = MessageChanges.dismissOutgoing(MessageState(listOf(editor), listOf(original)), original)
        val unknownAgain = MessageChanges.result(dismissed, request) { it.copy(error = "Still unconfirmed") }
        assertFalse(unknownAgain.outgoing.single().blocksSending)
        val accepted = MessageChanges.result(unknownAgain, request) {
            it.copy(status = "submitted", submittedAt = 102.0, submittedText = it.text, error = "")
        }
        assertEquals(editor, accepted.drafts.single())
        assertTrue(accepted.outgoing.single().deliveryDismissed)
        assertEquals("submitted", accepted.outgoing.single().status)
        assertEquals(accepted, MessageCodec.state(MessageCodec.stateJson(accepted)))
    }

    @Test fun dismissedUnknownStillRequiresExactNativeProofAndNeverTextMatching() {
        val message = original.copy(deliveryDismissed = true)
        val provider = Event("provider", "You", original.text, "", original.createdAt, source = "provider")
        val echoed = ConversationMerge.merge(target, listOf(provider), listOf(message))
        assertTrue(echoed.recordedRequestIds.isEmpty())
        assertEquals(2, echoed.events.size)
        val exact = provider.copy(requestId = request, source = "hgs_delivery")
        val merged = ConversationMerge.merge(target, listOf(exact), listOf(message))
        assertEquals(setOf(request), merged.recordedRequestIds)
        assertEquals(1, merged.events.size)
        assertEquals("recorded", merged.events.single().delivery)
        val foreign = ConversationMerge.merge(target.copy(run = "other"), listOf(exact), listOf(message))
        assertTrue(foreign.recordedRequestIds.isEmpty())
    }

    @Test fun failedDurableDismissalKeepsTheLaneBlockedAndSuccessfulRetryKeepsTheEditor() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        try {
            val state = MessageState(listOf(editor), listOf(original))
            var fail = true
            val actor = OrderedStatePersistence(state, scope, { if (fail) throw IOException("synthetic storage failure") }, Dispatchers.Unconfined)
            assertTrue(runCatching { actor.durable({ MessageChanges.dismissOutgoing(it, original) }) }.isFailure)
            assertEquals(state, actor.state.value)
            assertEquals(state, actor.durableState)
            assertTrue(actor.state.value.outgoing.single().blocksSending)
            fail = false
            val written = actor.durable({ MessageChanges.dismissOutgoing(it, original) })
            assertTrue(written.outgoing.single().deliveryDismissed)
            assertFalse(written.outgoing.single().blocksSending)
            assertEquals(editor, written.drafts.single())
            assertEquals(written, actor.durableState)
        } finally { scope.cancel() }
    }
}
