package app.zerus.mobile

import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test

class RelayReadTimingTest {
    private class ReadFailure(val marker: Any) : IllegalStateException("fixture read failure")
    @Test fun optionalObservationPreservesReadValuesAndTheOriginalFailure() = runBlocking {
        // With -PzerusReadTiming=true the SDK-free Android Log stub throws.
        // This observer failure must not turn an authenticated read into failure.
        assertEquals(42, RelayReadTiming.read(RelayReadTiming.Operation.Inspect) { 42 })
        RelayReadTiming.point(RelayReadTiming.Stage.OpenConversation)
        val failure = ReadFailure(Any())
        try {
            RelayReadTiming.read(RelayReadTiming.Operation.History) { throw failure }
            fail("Read failure must propagate")
        } catch (actual: IllegalStateException) { assertSame(failure, actual) }
    }
}
