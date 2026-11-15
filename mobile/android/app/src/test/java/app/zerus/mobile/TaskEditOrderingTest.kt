package app.zerus.mobile

import org.junit.Assert.*
import org.junit.Test

class TaskEditOrderingTest {
    private val original = ProjectKey("connection", "swarm", "original")
    @Test fun changingProjectsCannotStageTheDefaultRankOnTheOriginalTask() {
        val other = original.copy(id = "other")
        assertNull(TaskEditOrdering.top(original, other, "task", listOf("neighbor" to "8")))
        assertNull(TaskEditOrdering.top(original, null, "task", emptyList()))
    }
    @Test fun theOriginalProjectStillOrdersBeforeItsActualNeighbor() {
        val rank = checkNotNull(TaskEditOrdering.top(original, original, "task", listOf("task" to "V", "neighbor" to "8")))
        assertTrue(rank < "8")
        assertEquals("V", TaskEditOrdering.top(original, original, "task", emptyList()))
    }
}
