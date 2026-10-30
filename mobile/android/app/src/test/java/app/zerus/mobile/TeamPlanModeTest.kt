package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** U59: the plan mode, the lead's offer and the free phase's state over the shared fixture. */
class TeamPlanModeTest {
    private val teams = JSONObject(checkNotNull(javaClass.getResourceAsStream("/team-plan-mode.json")).bufferedReader().use { it.readText() })
        .getJSONArray("teams").objects()

    private fun team(name: String): JSONObject = checkNotNull(teams.firstOrNull { it.optString("name") == name }) { "no team $name" }

    @Test fun fixtureMatchesTheSharedCopy() {
        val shared = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "tests/fixtures/team-plan-mode.json") }.firstOrNull { it.isFile }
        org.junit.Assume.assumeTrue("The shared fixture is outside this checkout", shared != null)
        assertArrayEquals(shared!!.readBytes(), checkNotNull(javaClass.getResourceAsStream("/team-plan-mode.json")).use { it.readBytes() })
    }

    @Test fun aStandardTeamShowsTheModeAndNoFreePhase() {
        val plan = TeamPlanStatus.of(team("standard"))
        assertEquals(TeamPlanMode.Standard, plan.mode)
        assertNull(plan.offer)
        assertEquals("", plan.reason)
        assertNull(plan.freePhase)
        assertNull(TeamPlanStatus.badge(plan))
        assertEquals("", TeamPlanStatus.phaseLine(plan))
        assertEquals("Standard", TeamPlanStatus.header(plan))
        assertEquals(TeamPlanTone.Standard, TeamPlanStatus.tone(plan))
        // A team stored before U59 sends no plan_mode and reads standard too.
        assertEquals(TeamPlanMode.Standard, TeamPlanStatus.of(JSONObject()).mode)
    }

    @Test fun aFreeOpenPhaseShowsTheModeAndItsWork() {
        val plan = TeamPlanStatus.of(team("free-open"))
        assertEquals(TeamPlanMode.Free, plan.mode)
        val phase = checkNotNull(plan.freePhase)
        assertEquals(2, phase.phase)
        assertFalse(phase.idle)
        assertEquals(listOf(7, 8), phase.running)
        assertEquals(5, phase.accepted)
        assertEquals(2, phase.acceptedSince)
        assertEquals(1793300000.0, phase.lastBuildAt, 0.0)
        assertNull(phase.closePending)
        assertEquals("Free planning", TeamPlanStatus.badge(plan))
        assertEquals("Phase 2 · open · 5 accepted", TeamPlanStatus.phaseLine(plan))
        assertEquals("Free planning · Phase 2 · open · 5 accepted", TeamPlanStatus.header(plan))
        assertEquals(TeamPlanTone.Free, TeamPlanStatus.tone(plan))
        // The desktop's tokens: free violet, 13 % background.
        assertEquals(DesktopBadgeColors(0x21B79CFF, 0xFFB79CFF), TeamPlanStatus.colors(TeamPlanTone.Free, dark = true))
        assertEquals(DesktopBadgeColors(0x216F4FD8, 0xFF6F4FD8), TeamPlanStatus.colors(TeamPlanTone.Free, dark = false))
    }

    @Test fun anIdleOpenPhaseWaitsForMoreWork() {
        val plan = TeamPlanStatus.of(team("free-idle"))
        val phase = checkNotNull(plan.freePhase)
        assertTrue(phase.idle)
        assertTrue(phase.running.isEmpty())
        assertEquals(9, phase.accepted)
        assertEquals("Phase 1 · idle, waiting for more work", TeamPlanStatus.phaseLine(plan))
        assertEquals("Free planning · Phase 1 · idle, waiting for more work", TeamPlanStatus.header(plan))
        assertEquals(TeamPlanTone.Idle, TeamPlanStatus.tone(plan))
        assertEquals(0xFF7FB6E0, TeamPlanStatus.colors(TeamPlanTone.Idle, dark = true).foreground)
    }

    @Test fun theLeadProposesAModeWithItsReason() {
        val plan = TeamPlanStatus.of(team("free-offer"))
        assertEquals(TeamPlanMode.Standard, plan.mode)
        assertEquals(TeamPlanMode.Free, plan.offer)
        assertEquals("a stream of small support requests", plan.reason)
        assertEquals("Free planning proposed", TeamPlanStatus.badge(plan))
        assertEquals("Standard", plan.mode.label)
        assertEquals(TeamPlanTone.Standard, TeamPlanStatus.tone(plan))
    }

    @Test fun aWaitingCloseNamesItsBuild() {
        val plan = TeamPlanStatus.of(team("free-pending-close"))
        val phase = checkNotNull(plan.freePhase)
        assertEquals(3, phase.closePending)
        assertFalse(phase.idle)
        assertEquals(1793200000.0, phase.lastBuildAt, 0.0)
        assertEquals("Phase 3 · open · 4 accepted · close waits for build #3", TeamPlanStatus.phaseLine(plan))
        assertEquals(TeamPlanTone.Warn, TeamPlanStatus.tone(plan))
        assertEquals(0xFFE8A75A, TeamPlanStatus.colors(TeamPlanTone.Warn, dark = true).foreground)
        assertEquals(0xFF9A5A0C, TeamPlanStatus.colors(TeamPlanTone.Warn, dark = false).foreground)
    }

    @Test fun theModeAndStateAreBounded() {
        // Unknown words read standard and open; a free team without a phase still shows its mode.
        assertEquals(TeamPlanMode.Standard, TeamPlanMode.of("nonsense"))
        assertEquals(TeamFreePhaseState.Open, TeamFreePhaseState.of("nonsense"))
        val free = TeamPlanStatus.of(JSONObject().put("plan_mode", "free"))
        assertEquals(TeamPlanMode.Free, free.mode)
        assertNull(free.freePhase)
        assertEquals("Free planning", TeamPlanStatus.header(free))
    }
}
