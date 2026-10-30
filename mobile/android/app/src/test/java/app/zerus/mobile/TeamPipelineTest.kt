package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** U36: the stage statuses, chips and verdicts over the host's shared fixture (team-pipeline-tasks.json). */
class TeamPipelineTest {
    private val tasks = JSONObject(checkNotNull(javaClass.getResourceAsStream("/team-pipeline-tasks.json")).bufferedReader().use { it.readText() })
        .getJSONArray("tasks").objects()

    @Test fun fixtureMatchesTheSharedCopy() {
        val shared = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "tests/fixtures/team-pipeline-tasks.json") }.firstOrNull { it.isFile }
        org.junit.Assume.assumeTrue("The shared fixture is outside this checkout", shared != null)
        assertArrayEquals(shared!!.readBytes(), checkNotNull(javaClass.getResourceAsStream("/team-pipeline-tasks.json")).use { it.readBytes() })
    }

    @Test fun statusesUseTheDesktopsWordsAndColors() {
        assertEquals(listOf(TeamTaskStatus.InReview, TeamTaskStatus.InTest, TeamTaskStatus.Pending, TeamTaskStatus.Submitted), tasks.map { TeamPipeline.status(it) })
        assertEquals(listOf("In review", "In test", "Pending", "Submitted"), tasks.map { TeamPipeline.status(it)!!.label })
        assertTrue(TeamTaskStatus.InReview.inStage && TeamTaskStatus.InTest.inStage && !TeamTaskStatus.Submitted.inStage)
        assertEquals(TeamStageTone.Review, TeamPipeline.statusTone(TeamTaskStatus.InReview))
        assertEquals(TeamStageTone.Test, TeamPipeline.statusTone(TeamTaskStatus.InTest))
        // design/agent-teams/pipelines: review violet, test blue, accept green, warning; 13 % backgrounds.
        assertEquals(DesktopBadgeColors(0x21B79CFF, 0xFFB79CFF), TeamPipeline.colors(TeamStageTone.Review, dark = true))
        assertEquals(DesktopBadgeColors(0x216F4FD8, 0xFF6F4FD8), TeamPipeline.colors(TeamStageTone.Review, dark = false))
        assertEquals(0xFF7FB6E0, TeamPipeline.colors(TeamStageTone.Test, dark = true).foreground)
        assertEquals(0xFF2F6EA3, TeamPipeline.colors(TeamStageTone.Test, dark = false).foreground)
        assertEquals(0xFF167357, TeamPipeline.colors(TeamStageTone.Accept, dark = false).foreground)
        assertEquals(0xFFE8A75A, TeamPipeline.colors(TeamStageTone.Warn, dark = true).foreground)
        assertNull(TeamTaskStatus.of("unknown"))
    }

    @Test fun thePipelineSnapshotParses() {
        val run = checkNotNull(TeamPipeline.run(tasks[0]))
        assertEquals("code", run.id); assertEquals("Code: review, test, lead", run.name)
        assertEquals(listOf("review" to "reviewer", "test" to "tester"), run.stages.map { it.kind to it.role })
        assertEquals(3, run.maxRounds); assertEquals("hold", run.folder); assertEquals("lead", run.accept); assertEquals(1, run.round)
        assertEquals("review" to 1, run.openRun!!.kind to run.openRun!!.round)
        assertEquals(listOf(0, 0, 1, 3), tasks.map { TeamPipeline.run(it)!!.returned })
        assertEquals("Code: review, test, lead · round 1 of 3", TeamPipeline.header(tasks[0]))
        assertEquals("Code: review, test, lead · round 3 of 3", TeamPipeline.header(tasks[3]))
        assertNull(TeamPipeline.run(JSONObject().put("status", "in_progress")))
    }

    @Test fun eachTaskHasItsStageChip() {
        val chips = tasks.map { TeamPipeline.chip(it) }
        assertEquals(TeamStageChip("review", "Review waits", "", TeamStageTone.Review, outlined = true), chips[0])
        assertEquals(TeamStageChip("test", "Test waits", "", TeamStageTone.Test, outlined = true), chips[1])
        assertEquals(TeamStageChip("review", "Review returned r1", "", TeamStageTone.Warn, outlined = true), chips[2])
        assertEquals(TeamStageChip("accept", "Lead decides · 3 findings", "", TeamStageTone.Warn, outlined = false), chips[3])
        // A member that took the stage: its round of the maximum, and who.
        val delivered = JSONObject(tasks[1].toString())
        delivered.getJSONObject("pipeline_run").getJSONArray("runs").getJSONObject(1).put("state", "delivered").put("member", "qa")
        assertEquals(TeamStageChip("test", "Test r1/3", "qa", TeamStageTone.Test, outlined = false), TeamPipeline.chip(delivered))
        // Reported with every stage passed: the lead's decision, or Auto.
        val passed = JSONObject(tasks[1].toString()).put("status", "submitted")
        passed.getJSONObject("pipeline_run").getJSONArray("runs").getJSONObject(1).put("state", "passed")
        assertEquals(TeamStageChip("accept", "Lead", "", TeamStageTone.Accept, outlined = false), TeamPipeline.chip(passed))
        passed.getJSONObject("pipeline_run").put("accept", "auto")
        assertEquals("Auto", TeamPipeline.chip(passed)!!.caption)
        // #538 L8: the desktop's acceptance names (TeamStages.cpp:238 with TeamModel.cpp stageName).
        passed.getJSONObject("pipeline_run").put("accept", "sub_lead")
        assertEquals(TeamStageChip("accept", "Sub-lead", "", TeamStageTone.Accept, outlined = false), TeamPipeline.chip(passed))
        // No pipeline, or a finished task: no chip.
        assertNull(TeamPipeline.chip(JSONObject().put("status", "in_progress")))
        assertNull(TeamPipeline.chip(JSONObject(tasks[1].toString()).put("status", "accepted")))
    }

    @Test fun theInspectorListsEachRoundsVerdict() {
        assertEquals(listOf("Review · round 1" to "WAITING"), TeamPipeline.verdicts(tasks[0]).map { it.title to it.verdict })
        val test = TeamPipeline.verdicts(tasks[1])
        assertEquals(listOf("Review · round 1" to "PASS", "Test · round 1" to "WAITING"), test.map { it.title to it.verdict })
        assertEquals(TeamStageVerdict("Review · round 1", "PASS", TeamStageTone.Accept, "rev", "Read it", emptyList(), 1800000800.0), test[0])
        val returned = TeamPipeline.verdicts(tasks[2]).single()
        assertEquals("RETURN", returned.verdict); assertEquals(TeamStageTone.Warn, returned.tone)
        assertEquals(listOf("Page size is unbounded"), returned.findings)
        val limit = TeamPipeline.verdicts(tasks[3])
        assertEquals(listOf("Review · round 1", "Review · round 2", "Review · round 3"), limit.map { it.title })
        assertTrue(limit.all { it.verdict == "RETURN" && it.findings == listOf("Page size is unbounded") })
        assertEquals(listOf(1800000800.0, 1800001800.0, 1800002800.0), limit.map { it.endedAt })
        assertEquals(listOf("WAITING", "RUNNING", "PASS", "RETURN", "SKIPPED", "REVOKED", ""), TeamStageState.entries.map { TeamPipeline.verdict(it) })
    }

    /** #538 N10: the round-limit chip only while the last round returned the task (desktop TeamStages.cpp:181). */
    @Test fun aRoundThatPassedAfterTheLimitIsNotOutOfRounds() {
        val passed = JSONObject(tasks[3].toString())
        val runs = passed.getJSONObject("pipeline_run").put("round", 4).getJSONArray("runs")
        runs.put(JSONObject(runs.getJSONObject(2).toString()).put("id", "00000000-0000-4000-9000-000000004004")
            .put("attempt", 4).put("round", 4).put("state", "passed").put("findings", org.json.JSONArray()))
        assertEquals(3, TeamPipeline.run(passed)!!.returned)
        assertEquals(TeamStageChip("accept", "Lead", "", TeamStageTone.Accept, outlined = false), TeamPipeline.chip(passed))
    }

    /** #538 L8: the limit chip counts the findings, "1 finding" in the singular (TeamStages.cpp:228). */
    @Test fun theLimitChipCountsFindings() {
        val one = JSONObject(tasks[3].toString())
        val runs = one.getJSONObject("pipeline_run").getJSONArray("runs")
        for (i in 0 until 2) runs.getJSONObject(i).put("findings", org.json.JSONArray())
        assertEquals("Lead decides · 1 finding", TeamPipeline.chip(one)!!.caption)
        runs.getJSONObject(0).put("findings", org.json.JSONArray(listOf("a", "b")))
        assertEquals("Lead decides · 3 findings", TeamPipeline.chip(one)!!.caption)
    }

    /** #538 L8: a state this client does not know is closed, never a waiting stage (TeamModel.h:228). */
    @Test fun anUnknownStageStateIsClosed() {
        assertEquals(TeamStageState.Unknown, TeamStageState.of("paused"))
        assertEquals(TeamStageState.Unknown, TeamStageState.of(""))
        assertFalse(TeamStageState.Unknown.open)
        val unknown = JSONObject(tasks[0].toString())
        unknown.getJSONObject("pipeline_run").getJSONArray("runs").getJSONObject(0).put("state", "paused")
        assertNull(TeamPipeline.run(unknown)!!.openRun)
        assertNull(TeamPipeline.chip(unknown))
        assertEquals(listOf("Review · round 1" to ""), TeamPipeline.verdicts(unknown).map { it.title to it.verdict })
        assertEquals(TeamStageTone.Quiet, TeamPipeline.verdicts(unknown).single().tone)
    }
}
