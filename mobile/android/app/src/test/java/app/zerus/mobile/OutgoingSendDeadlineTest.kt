package app.zerus.mobile

import java.util.Collections
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/**
 * zerus-a4mub.13: the outbox send entry acquires the receipt inside its timer
 * and settles outside it. A receipt that arrives just before the deadline must
 * stay submitted even when the accepted durable write finishes after the
 * deadline: timing the settlement too would let the failure path overwrite the
 * accepted "submitted" with "uncertain".
 */
class OutgoingSendDeadlineTest {
    private val target = Target("computer", "session", "run", "conversation", "workspace")
    private val request = "11111111-1111-4111-8111-111111111111"
    private val original = "Original message"
    private val newer = "The next message kept in the editor"

    private fun receipt() = JSONObject().put("request_id", request).put("state", "completed")
        .put("result", JSONObject().put("status", "submitted").put("submitted_at", 2.0).put("submitted_text", original))

    /** The production settlement mutation, as ZerusViewModel.settleOutgoing applies it. */
    private fun submitted(state: MessageState) = MessageChanges.result(state, request) {
        it.copy(status = "submitted", submittedAt = 2.0, submittedText = original, error = "")
    }
    private fun uncertain(state: MessageState, message: String) = MessageChanges.result(state, request) {
        it.copy(status = "uncertain", error = message)
    }

    @Test fun aReceiptBeforeTheDeadlineWithItsAcceptedWriteAcrossItStaysSubmitted() = runBlocking {
        val deadline = 3_000L
        var state = MessageState(listOf(Draft(target, newer, revision = "newer-editor")),
            listOf(OutgoingMessage(request, target, original, emptyList(), 1.0)))
        val order = Collections.synchronizedList(mutableListOf<String>())
        var posts = 0
        var failure: Pair<String, String>? = null
        var writtenAfterDeadline = false
        val started = System.nanoTime()
        val send = OutgoingSend(SendAttempt(), timeoutMillis = deadline,
            submit = { evidence -> posts += 1; evidence.enqueued(); JSONObject().put("request_id", request) },
            await = { _ -> delay(deadline - 600); receipt() },
            settle = { value ->
                // The accepted durable save is NonCancellable and its write
                // finishes across the receipt deadline.
                withContext(NonCancellable) {
                    delay(900)
                    writtenAfterDeadline = (System.nanoTime() - started) / 1_000_000 >= deadline
                    state = submitted(state)
                    order.add("written")
                }
            },
            failed = { status, message ->
                failure = status to message
                state = uncertain(state, message)
                order.add("uncertain")
            })
        try {
            send.run()
        } finally {
            // The ViewModel releases the delivery flight after the entry returns.
            order.add("released")
        }
        assertNull("a settlement failure was recorded", failure)
        assertTrue("the accepted write did not cross the deadline", writtenAfterDeadline)
        assertEquals(listOf("written", "released"), order)
        assertEquals("submitted", state.outgoing.single().status)
        assertTrue(state.outgoing.none { it.status == "uncertain" })
        val message = state.outgoing.single()
        assertEquals(request, message.requestId)
        assertEquals(original, message.text)
        assertEquals(original, message.submittedText)
        assertEquals(2.0, message.submittedAt, 0.0)
        assertEquals(newer, state.drafts.single().text)
        assertEquals(1, posts)
    }

    @Test fun aReceiptThatArrivesAfterTheDeadlineIsTheAcquisitionTimeoutControl() = runBlocking {
        var state = MessageState(listOf(Draft(target, newer)),
            listOf(OutgoingMessage(request, target, original, emptyList(), 1.0)))
        var posts = 0
        var failure: Pair<String, String>? = null
        var settled = false
        val send = OutgoingSend(SendAttempt(), timeoutMillis = 150,
            submit = { evidence -> posts += 1; evidence.enqueued(); JSONObject().put("request_id", request) },
            await = { _ -> delay(2_000); receipt() },
            settle = { settled = true },
            failed = { status, message -> failure = status to message; state = uncertain(state, message) })
        send.run()
        assertFalse("a timed-out receipt was settled", settled)
        assertEquals("uncertain", failure?.first)
        assertEquals("uncertain", state.outgoing.single().status)
        assertEquals(1, posts)
    }

    @Test fun aCallerCancellationDuringAcquisitionSettlesUncertainAndPropagates() = runBlocking {
        var failure: Pair<String, String>? = null
        val posted = CompletableDeferred<Unit>()
        val job = async {
            OutgoingSend(SendAttempt(), timeoutMillis = 10_000,
                submit = { evidence -> evidence.enqueued(); posted.complete(Unit); JSONObject() },
                await = { _ -> delay(10_000); JSONObject() },
                settle = { fail("a cancelled acquisition was settled") },
                failed = { status, message -> failure = status to message }).run()
        }
        posted.await()
        job.cancelAndJoin()
        assertEquals("uncertain", failure?.first)
        assertTrue(job.isCancelled)
    }
}
