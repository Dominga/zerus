package app.zerus.mobile

import java.io.IOException
import java.net.SocketTimeoutException
import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** zerus-a4mub.6: settlement decides from transport evidence, never from an exception type. */
class SendTransportTest {
    private fun rejected(status: Int = 409) = RelayException(status, "The computer rejected this message.")

    @Test fun aFailureBeforeTheMutationPostIsNotSent() {
        // Route missing, seal failure, reservation failure and a cancel before
        // the POST all leave the stage Local: nothing left the phone.
        val attempt = SendAttempt()
        assertEquals(TransportStage.Local, attempt.stage)
        assertEquals("failed", SendSettlement.status(attempt.stage, null))
        assertEquals("The saved message was not sent. Review it before trying again.",
            SendSettlement.message(attempt.stage, null, null))
        assertEquals("Could not read the attachment.", SendSettlement.message(attempt.stage, null, "Could not read the attachment."))
    }

    @Test fun aPostThatMayHaveLeftThePhoneStaysUnknown() {
        val attempt = SendAttempt()
        attempt.enqueued()
        assertEquals(TransportStage.Enqueued, attempt.stage)
        assertEquals("uncertain", SendSettlement.status(attempt.stage, null))
        assertEquals("Delivery is unconfirmed. Check its receipt or dismiss the warning to continue.",
            SendSettlement.message(attempt.stage, null, null))
    }

    @Test fun anHttpRejectionIsNotSentEvenAfterThePost() {
        val attempt = SendAttempt()
        attempt.enqueued()
        assertEquals("failed", SendSettlement.status(attempt.stage, rejected()))
        assertEquals("The computer rejected this message.", SendSettlement.message(attempt.stage, rejected(), null))
    }

    @Test fun theSameExceptionSettlesByStageNotByType() {
        // A generic IOException, an HTTP timeout and a caller cancellation are
        // not evidence on their own: the stage decides no-delivery vs unknown.
        val local = SendAttempt()
        val sent = SendAttempt().also { it.enqueued() }
        for (failure in listOf(IOException("lost"), SocketTimeoutException("timeout"))) {
            assertEquals("failed", SendSettlement.status(local.stage, null))
            assertEquals("uncertain", SendSettlement.status(sent.stage, null))
            assertNotNull(failure)
        }
        // A cancellation before the POST is local; once enqueued it is unknown.
        assertEquals("failed", SendSettlement.status(SendAttempt().stage, null))
        assertEquals("uncertain", SendSettlement.status(SendAttempt().also { it.enqueued() }.stage, null))
    }

    @Test fun aLegacyLocalValidationFailureStaysLocal()=runBlocking {
        // A plain (v1) submit validates target and payload locally before any
        // HTTP call; the evidence stays Local and settlement is Not sent.
        val connection=Connection("plain","Fixture","http://127.0.0.1:1","token",1)
        val archived=Target("machine","session","run","conversation",connection.id,archiveId="archive-1")
        val attempt=SendAttempt()
        val failure=runCatching { RelayApi().submit(connection,archived,"send",JSONObject().put("archive_id",""),"request-one",attempt) }.exceptionOrNull()
        assertNotNull(failure)
        assertEquals(TransportStage.Local,attempt.stage)
        assertEquals("failed",SendSettlement.status(attempt.stage,null))
    }

    @Test fun aLegacyPostMarksTheSameEvidence()=runBlocking {
        MockWebServer().use { server ->
            server.enqueue(MockResponse().setBody("""{"request_id":"request-one","state":"queued"}"""))
            server.start()
            val connection=Connection("plain","Fixture",server.url("/").toString().trimEnd('/'),"token",1)
            val attempt=SendAttempt()
            val receipt=RelayApi().submit(connection,Target("machine","session","run","conversation",connection.id),"send",JSONObject().put("text","private message"),"request-one",attempt)
            assertEquals("queued",receipt.string("state"))
            assertEquals(TransportStage.Enqueued,attempt.stage)
            assertEquals(1,server.requestCount)
            assertEquals("uncertain",SendSettlement.status(attempt.stage,null))
        }
    }

    @Test fun evidenceOnlyMovesForwardAndNeverBackToLocal() {
        val attempt = SendAttempt()
        attempt.enqueued()
        attempt.enqueued()
        assertEquals(TransportStage.Enqueued, attempt.stage)
        // A new attempt starts local again; evidence is per send, never shared.
        assertEquals(TransportStage.Local, SendAttempt().stage)
    }
}
