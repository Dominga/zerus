package app.zerus.mobile

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class ReceiptPollingTest {
    private fun row(state: String) = JSONObject().put("request_id", "fixture-request").put("state", state)

    @Test fun readResponsiveWindowReturnsToOrdinaryCadenceAndRetainsExactIdentity() = runBlocking {
        val pauses = mutableListOf<Long>(); val identities = mutableListOf<String>()
        val result = ReceiptPolling.await(row("queued"),35,800,100,pauses::add) {
            identities += it; row(if (identities.size < 10) "claimed" else "completed")
        }
        assertEquals(listOf(100L,200L,200L,200L,200L,200L,200L,200L,800L,800L),pauses)
        assertEquals(List(10) { "fixture-request" },identities)
        assertEquals("completed",result.string("state"))
    }
    @Test fun unresolvedReceiptsStayBoundedAndTerminalUncertaintyIsNotReplayed() = runBlocking {
        var polls = 0
        val unresolved = ReceiptPolling.await(row("queued"),3,800,100,{}) { polls++;row("claimed") }
        assertEquals(3,polls);assertEquals("claimed",unresolved.string("state"))
        for (state in listOf("completed","failed","uncertain")) {
            assertEquals(state,ReceiptPolling.await(row(state),35,800,100,{ error("No sleep") }) { error("No poll") }.string("state"))
        }
    }
    @Test fun ordinaryCadenceAndCancellationRemainUnchanged() = runBlocking {
        val pauses = mutableListOf<Long>()
        ReceiptPolling.await(row("queued"),1,800,800,pauses::add) { row("completed") }
        assertEquals(listOf(800L),pauses)
        val cancelled = CancellationException("fixture navigation")
        try {
            ReceiptPolling.await(row("queued"),35,800,100,{ throw cancelled }) { error("Cancelled wait must not poll") }
            fail("Cancellation must propagate")
        } catch (actual: CancellationException) { assertSame(cancelled,actual) }
    }
}
