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
        assertEquals(TeamStageChip("accept", "Lead decides · 3 rounds", "", TeamStageTone.Warn, outlined = false), chips[3])
        // A member that took the stage: its round of the maximum, and who.
        val delivered = JSONObject(tasks[1].toString())
        delivered.getJSONObject("pipeline_run").getJSONArray("runs").getJSONObject(1).put("state", "delivered").put("member", "qa")
        assertEquals(TeamStageChip("test", "Test r1/3", "qa", TeamStageTone.Test, outlined = false), TeamPipeline.chip(delivered))
        // Reported with every stage passed: the lead's decision, or Auto.
        val passed = JSONObject(tasks[1].toString()).put("status", "submitted")
        passed.getJSONObject("pipeline_run").getJSONArray("runs").getJSONObject(1).put("state", "passed")
        assertEquals(TeamStageChip("accept", "Lead decides", "", TeamStageTone.Accept, outlined = false), TeamPipeline.chip(passed))
        passed.getJSONObject("pipeline_run").put("accept", "auto")
        assertEquals("Auto", TeamPipeline.chip(passed)!!.caption)
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
        assertEquals(listOf("WAITING", "RUNNING", "PASS", "RETURN", "SKIPPED", "ENDED"), TeamStageState.entries.map { TeamPipeline.verdict(it) })
    }
}
