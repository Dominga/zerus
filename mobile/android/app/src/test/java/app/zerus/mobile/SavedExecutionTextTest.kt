package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SavedExecutionTextTest {
    private val contract = javaClass.classLoader!!.getResourceAsStream("task-saved-execution.json")!!
        .bufferedReader().use { JSONObject(it.readText()) }

    @Test fun phasesMatchTheSharedSavedEvidenceContract() {
        val cases = contract.getJSONArray("cases")
        for (i in 0 until cases.length()) {
            val c = cases.getJSONObject(i)
            if (!c.getBoolean("available")) continue
            val current = c.getJSONObject("execution").optJSONObject("current") ?: continue
            assertEquals(c.getString("name"), c.getString("phase_label"), SavedExecutionText.phase(current.getString("phase")))
        }
        assertNull(SavedExecutionText.phase("future_phase"))
        assertNull(SavedExecutionText.phase(" running"))
    }

    @Test fun uncertaintyAndFailedRefreshRemainVisibleBesideAnyPhase() {
        assertEquals(listOf("Execution records conflict.", "Some attempt delivery is unknown.",
            "Some execution history is unexplained.", "Legacy execution record", "This saved execution could not be refreshed."),
            SavedExecutionText.notices(true, true, true, true, true))
        assertTrue(SavedExecutionText.notices(false, false, false, false, false).isEmpty())
        assertEquals(listOf("Some attempt delivery is unknown."), SavedExecutionText.notices(false, true, false, false, false))
        assertEquals("Submitted — begin unconfirmed", SavedExecutionText.phase("submitted"))
        assertEquals("Begin recorded", SavedExecutionText.phase("running"))
        assertNotEquals(SavedExecutionText.phase("submitted"), SavedExecutionText.phase("running"))
    }
}
