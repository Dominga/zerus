package app.zerus.mobile

import app.zerus.mobile.replica.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SavedExecutionPresentationTest {
    private val cases = javaClass.classLoader!!.getResourceAsStream("task-saved-execution.json")!!
        .bufferedReader().use { JSONObject(it.readText()).getJSONArray("cases") }
    private fun evidence(e: JSONObject?): TaskExecutionInfo? = e?.let {
        val c = e.optJSONObject("current")
        TaskExecutionInfo(e.getLong("requirement").toULong(), e.getLong("attempts").toULong(), c?.let {
            val peer = c.optJSONObject("executor")
            TaskAttemptInfo(c.getString("id"), c.getLong("generation").toULong(), c.getString("origin"),
                peer?.let { SessionRef(it.getString("machine"), it.getString("session"), it.getString("session_id")) },
                c.getString("request_id"), c.getLong("requirement").toULong(), c.getString("phase"), c.getLong("at"),
                if (c.isNull("began_at")) null else c.getLong("began_at"), if (c.isNull("progress_at")) null else c.getLong("progress_at"), c.getBoolean("legacy")) },
            e.getBoolean("conflict"), e.getBoolean("has_unknown"), e.getBoolean("has_unexplained"))
    }
    private fun task(c: JSONObject) = TaskInfo("fixture-task", "T-a1b2c3", "project-1", c.getLong("schema"), false, 1, 999,
        TaskActor.Person, TaskStage.ACTIVE, TaskTarget.Session(SessionRef("wrong-machine", "wrong-target")), "V",
        SessionRef("wrong-machine", "wrong-current-session"), "workflow is not execution", null, "", "Task", emptyList(),
        execution = evidence(c.optJSONObject("execution")))
    private fun section(t: TaskInfo, project: String? = "project-1", stale: Boolean = false) =
        SavedExecutionPresentation.section(t, project, stale) { "saved:$it" }

    @Test fun sharedContractStaysSavedBoundedAndExplicitlyUnavailable() {
        for (i in 0 until cases.length()) {
            val c = cases.getJSONObject(i); val t = task(c); val state = section(t)
            assertEquals(c.getString("name"), c.getBoolean("available"), state.available)
            assertEquals(c.getString("name"), if (c.isNull("phase_label")) null else c.getString("phase_label"), state.phase)
            if (!state.available) assertTrue(state.rows.isEmpty())
            assertFalse(t.writable)
        }
    }
    @Test fun identityTimesAndRevisionsComeOnlyFromTheSavedCurrentAttempt() {
        val t = task(cases.getJSONObject(1)); val before = t.copy(); val state = section(t, stale = true)
        val rows = state.rows.associate { it.label to it.value }
        assertEquals("3", rows["Generation"])
        assertEquals("2", rows["Requirement revision"])
        assertEquals("4", rows["Current requirement revision"])
        assertEquals("11111111-1111-4111-8111-111111111111", rows["Attempt"])
        assertEquals("44444444-4444-4444-8444-444444444444", rows["Request"])
        assertEquals("22222222-2222-4222-8222-222222222222\ncodex/example/worker\nlaunch_id:33333333-3333-4333-8333-333333333333", rows["Recorded executor"])
        assertEquals("saved:1700000000000", rows["Recorded at"])
        assertEquals("saved:1700000001000", rows["Begin recorded at"])
        assertEquals("saved:1700000002000", rows["Progress recorded at"])
        assertTrue(state.notices.contains("This saved execution could not be refreshed."))
        assertFalse(section(t).notices.contains("This saved execution could not be refreshed."))
        assertEquals(before, t)
        assertFalse(section(t, "other-project").available)
        assertFalse(section(t, null).available)
        assertFalse(section(t.copy(execution = null)).available)
    }
    @Test fun unknownLegacyAndFutureTagsNeverBorrowAuthorityOrWorkflowFacts() {
        val older = section(task(cases.getJSONObject(10)))
        assertEquals("Begin recorded", older.phase)
        assertTrue(older.notices.contains("Some attempt delivery is unknown."))
        val legacy = section(task(cases.getJSONObject(8)))
        assertTrue(legacy.notices.contains("Legacy execution record"))
        assertFalse(legacy.rows.any { it.label == "Attempt" || it.label == "Generation" })
        assertNull(legacy.rows.single { it.label == "Recorded at" }.value)
        assertNull(legacy.rows.single { it.label == "Recorded executor" }.value)
        val none = section(task(cases.getJSONObject(9)))
        assertTrue(none.available); assertFalse(none.hasCurrent)
        assertEquals(listOf("Current requirement revision"), none.rows.map { it.label })
        assertEquals(3, none.notices.size)
        val t = task(cases.getJSONObject(0)); val e = t.execution!!
        assertFalse(section(t.copy(execution = e.copy(current = e.current!!.copy(phase = "future")))).available)
        assertFalse(section(t.copy(execution = e.copy(current = e.current!!.copy(origin = "future")))).available)
        val nameOnly = section(t.copy(execution = e.copy(current = e.current!!.copy(executor = SessionRef("saved-machine", "Not recorded")))))
        assertEquals("saved-machine\nNot recorded", nameOnly.rows.single { it.label == "Recorded executor" }.value)
        assertTrue(nameOnly.notices.contains("Executor identity unresolved"))
    }
}
