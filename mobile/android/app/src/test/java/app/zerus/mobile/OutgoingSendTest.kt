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
        await: suspend (JSONObject) -> JSONObject = { it },
        release: suspend () -> Unit = {}): Pair<Result, Throwable?> {
        val result = Result()
        val thrown = runCatching { runBlocking {
            OutgoingSend(attempt, submit, await,
                settle = { result.settled = it },
                failed = { status, message -> result.status = status; result.message = message },
                release = release).run()
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

    @Test fun aLocalPrePostFailureReleasesOnlyThatReservation() {
        // zerus-a4mub.22: the reserve happens before the mutation POST, so a
        // local failure after it must release exactly that reservation; a
        // settlement after the marker must never release.
        val reserved = mutableSetOf("request-one")
        var releases = 0
        val (local, _) = send(submit = {
            throw IllegalStateException("Refresh this computer's encrypted route before sending.")
        }, release = { releases += 1; reserved.remove("request-one") })
        assertEquals("failed", local.status); assertEquals(1, releases); assertTrue(reserved.isEmpty())
        val (sent, _) = send(submit = { evidence -> evidence.enqueued(); throw IOException("lost response") },
            release = { releases += 1; reserved.remove("request-one") })
        assertEquals("uncertain", sent.status); assertEquals(1, releases); assertTrue(reserved.isEmpty())
        val (rejected, _) = send(submit = { evidence -> evidence.enqueued(); throw RelayException(409, "refused") },
            release = { releases += 1 })
        assertEquals("failed", rejected.status); assertEquals(1, releases)
    }

    @Test fun anAllowlistedLocalReasonReachesTheSettlement() {
        for (reason in listOf(
            "Refresh this computer's encrypted route before sending.",
            "Encrypted outgoing storage is full. Check outstanding deliveries.",
            "The session changed before transport. Review the saved message.",
            "Compact continuation was canceled. The captured message was not sent and remains recoverable.")) {
            val (result, _) = send(submit = { throw IllegalStateException(reason) })
            assertEquals("failed", result.status)
            assertEquals(reason, result.message)
        }
    }

    @Test fun anUnknownLocalFailureKeepsTheGenericMessage() {
        for (message in listOf(null, "", "java.net.SocketException: broken pipe", "Refresh this computer's encrypted route")) {
            val (result, _) = send(submit = { throw IllegalStateException(message) })
            assertEquals("failed", result.status)
            assertEquals("The saved message was not sent. Review it before trying again.", result.message)
        }
        // A reason-looking message after the marker never becomes a local reason.
        val (sent, _) = send(submit = { a -> a.enqueued(); throw IllegalStateException("Encrypted outgoing storage is full. Check outstanding deliveries.") })
        assertEquals("uncertain", sent.status)
        assertEquals("Delivery is unconfirmed. Check its receipt or dismiss the warning to continue.", sent.message)
    }

    @Test fun twoPreparedResendsReleasingLocallyNeverAccumulateReservations() {
        // zerus-a4mub.22: repeated Prepare again after local failures must not
        // grow the retained outbox.
        val reserved = mutableSetOf<String>()
        var next = 0
        repeat(2) {
            val id = "request-${++next}"
            reserved += id
            val (result, _) = send(submit = { throw IllegalStateException("Encrypted outgoing storage is full. Check outstanding deliveries.") },
                release = { reserved.remove(id) })
            assertEquals("failed", result.status)
        }
        assertTrue(reserved.isEmpty())
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
