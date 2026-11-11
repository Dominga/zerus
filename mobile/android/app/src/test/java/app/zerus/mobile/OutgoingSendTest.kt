package app.zerus.mobile

import java.io.IOException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** zerus-a4mub.21: the outbox send entry settles from the same evidence it submits. */
class OutgoingSendTest {
    private class Result {
        var settled: JSONObject? = null
        var status: String? = null
        var message: String? = null
    }
    private fun send(attempt: SendAttempt = SendAttempt(),
        submit: suspend (SendAttempt) -> JSONObject,
        await: suspend (JSONObject) -> JSONObject = { it }): Pair<Result, Throwable?> {
        val result = Result()
        val thrown = runCatching { runBlocking {
            OutgoingSend(attempt, submit, await,
                settle = { result.settled = it },
                failed = { status, message -> result.status = status; result.message = message }).run()
        } }.exceptionOrNull()
        return result to thrown
    }

    @Test fun aStartedPostThatFailsSettlesUnknown() {
        // The wiring control: a POST that started (the caller's attempt is
        // marked by the evidence it passes) and then failed must settle
        // Unknown, never Not sent.
        val attempt = SendAttempt()
        val (result, thrown) = send(attempt, submit = { evidence ->
            assertSame(attempt, evidence)
            evidence.enqueued()
            throw IOException("lost response")
        })
        assertNull(thrown)
        assertNull(result.settled)
        assertEquals("uncertain", result.status)
        assertEquals("Delivery is unconfirmed. Check its receipt or dismiss the warning to continue.", result.message)
    }

    @Test fun aLocalPrePostFailureSettlesNotSent() {
        val (result, thrown) = send(submit = { throw IllegalStateException("route") })
        assertNull(thrown)
        assertEquals("failed", result.status)
        assertEquals("The saved message was not sent. Review it before trying again.", result.message)
        val (attachment, _) = send(submit = { throw AttachmentException("Could not read the attachment.") })
        assertEquals("failed", attachment.status)
        assertEquals("Could not read the attachment.", attachment.message)
    }

    @Test fun aRejectionSettlesNotSent() {
        val (result, _) = send(submit = { evidence ->
            evidence.enqueued()
            throw RelayException(409, "The computer rejected this message.")
        })
        assertEquals("failed", result.status)
        assertEquals("The computer rejected this message.", result.message)
    }

    @Test fun aConfirmedReceiptSettlesSent() {
        val receipt = JSONObject().put("state", "completed")
        val (result, thrown) = send(submit = { it.enqueued(); JSONObject() }, await = { receipt })
        assertNull(thrown)
        assertSame(receipt, result.settled)
        assertNull(result.status)
    }

    @Test fun aCallerCancellationAfterThePostStaysUnknownAndPropagates() {
        val (result, thrown) = send(submit = { evidence ->
            evidence.enqueued()
            throw CancellationException("canceled")
        })
        assertNotNull(thrown)
        assertTrue(thrown is CancellationException)
        assertEquals("uncertain", result.status)
    }
}
