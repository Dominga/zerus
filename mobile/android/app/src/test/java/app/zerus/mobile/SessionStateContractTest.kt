package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

/**
 * U30: the shared session-state contract (mobile/android/app/src/test/resources/session-state-contract.json)
 * as the phone presents it. The host computes `session_state` and `state_reason` (SESSION-STATES.md
 * section 4); the list and the session screen must show those words, the countdown and the Cold actions.
 */
class SessionStateContractTest {
    private val contract: JSONObject by lazy {
        val text = javaClass.classLoader!!.getResourceAsStream("session-state-contract.json")!!
            .bufferedReader().use { it.readText() }
        JSONObject(text)
    }
    private val now = contract.getDouble("now")

    /** One case's row as the host would report it: its raw input plus the state and reason it computed. */
    private fun output(caseName: String): JSONObject {
        val cases = contract.getJSONArray("cases")
        for (i in 0 until cases.length()) {
            val case = cases.getJSONObject(i)
            if (case.optString("name") != caseName) continue
            val expect = case.getJSONObject("expect")
            val raw = JSONObject(case.getJSONObject("raw").toString())
            raw.put("session_state", expect.getString("session_state"))
            if (expect.has("kind")) {
                val reason = JSONObject(expect.toString())
                reason.remove("session_state")
                if (expect.has("text_prefix")) {
                    // A prefix is a Rust assertion, not a wire field. The host
                    // sends complete text with its local reset clock; model a
                    // UTC host deterministically instead of a missing reason.
                    check(expect.getString("kind") == "usage_limit")
                    val reset = raw.getJSONObject("provider_error").getDouble("reset_at")
                    val clock = DateTimeFormatter.ofPattern("HH:mm").withZone(ZoneOffset.UTC)
                        .format(Instant.ofEpochSecond(reset.toLong()))
                    reason.put("text", expect.getString("text_prefix") + clock)
                    reason.remove("text_prefix")
                }
                raw.put("state_reason", reason)
            }
            return raw
        }
        throw AssertionError("no case $caseName in the shared contract")
    }

    private fun cases(): List<String> {
        val list = mutableListOf<String>()
        val cases = contract.getJSONArray("cases")
        for (i in 0 until cases.length()) list += cases.getJSONObject(i).optString("name")
        return list
    }

    @Test
    fun everyContractCaseShowsItsStateAndItsTexts() {
        // Every case of the shared fixture, not a subset: the phone reads what the host decided.
        val names = cases()
        assertEquals("the contract's cases", 44, names.size)
        for (name in names) {
            val raw = output(name)
            val expected = contract.getJSONArray("cases").let { array ->
                (0 until array.length()).map { array.getJSONObject(it) }.first { it.optString("name") == name }
            }.getJSONObject("expect")
            val status = SessionStateContract.status(raw, true, now)
            assertEquals("$name state", expected.getString("session_state"), status.state.wire)
            assertTrue("$name caption", status.caption.isNotBlank())
            assertTrue("$name line 3", status.detail.isNotBlank())
            // Section 2: deps always names its reason, in the host's exact words; so does a build.
            if (expected.optString("session_state") in setOf("deps", "build")) {
                if (expected.has("text")) {
                    assertEquals("$name reason", expected.getString("text"), status.detail)
                } else if (expected.has("text_prefix")) {
                    assertTrue("$name reason", status.detail.startsWith(expected.getString("text_prefix")))
                    assertEquals("$name host text", raw.getJSONObject("state_reason").getString("text"), status.detail)
                } else if (status.state == SessionState.Building) {
                    // Partial fixture expectations omit the host's text. The
                    // phone still names the build from its structured fields.
                    val build = raw.getJSONObject("build")
                    assertTrue("$name build", status.detail.contains(build.optString("label").ifEmpty { build.getString("command") }))
                }
            }
            assertTrue("$name badge title", status.reason.isNotBlank())
        }
    }

    @Test
    fun coolingCountsDownAndColdOffersItsTwoActions() {
        // Section 3: cooling keeps the countdown and the warmth bar, cold names how long past the TTL
        // and carries + New and Resume.
        val cooling = SessionStateContract.status(output("dev-server-after-the-turn-is-cooling"), true, now)
        assertEquals(SessionState.Cooling, cooling.state)
        assertTrue("warmth left", cooling.warmSeconds > 0)
        assertTrue("countdown ${cooling.caption}", cooling.caption.startsWith("Cooling "))
        assertTrue("countdown minutes", cooling.caption.endsWith("m"))
        assertTrue("cache line ${cooling.detail}", cooling.detail.startsWith("Done · waiting for a new task · cache warm for "))
        assertTrue(SessionStateContract.warmFraction(output("dev-server-after-the-turn-is-cooling"), now) >= 0.04)
        assertTrue(cooling.coldActions.isEmpty())

        val cold = SessionStateContract.status(output("dev-server-after-the-ttl-is-cold"), true, now)
        assertEquals(SessionState.Cold, cold.state)
        assertEquals(0.0, cold.warmSeconds, 0.0)
        assertTrue("cold line ${cold.detail}", cold.detail.startsWith("Cache cold "))
        assertEquals(listOf(ColdAction.New, ColdAction.Resume), cold.coldActions)

        // One hour of cache keeps the session cooling, and past it cold, from the host's own TTL.
        val hour = SessionStateContract.status(output("one-hour-cache-still-cooling"), true, now)
        assertEquals(SessionState.Cooling, hour.state)
        assertTrue("hour warmth ${hour.warmSeconds}", hour.warmSeconds > 0)
        assertEquals(SessionState.Cold, SessionStateContract.status(output("one-hour-cache-cold"), true, now).state)
    }

    @Test
    fun aServiceNeverDefersTheReplyWhileABuildDoes() {
        // The Background rules of CLAUDE.md: a job nothing waits for is a service; a build Zerus runs
        // is work and its reply badge waits.
        val service = output("dev-server-after-the-turn-is-cooling")
        assertFalse(SessionStateContract.replyDeferred(service, true))
        val replyBesideService = output("reply-not-deferred-by-a-service")
        assertEquals(SessionState.User, SessionStateContract.status(replyBesideService, true, now).state)
        assertFalse(SessionStateContract.replyDeferred(replyBesideService, true))
        val build = output("zerus-build-defers-the-reply")
        assertTrue(SessionStateContract.replyDeferred(build, true))
        assertEquals(SessionState.Building, SessionStateContract.status(build, true, now).state)
        // The live build names itself on the card.
        val running = SessionStateContract.status(output("zerus-build-running"), true, now)
        assertEquals("Running cargo test · 6m so far", running.detail)
        assertEquals(SessionState.Building, running.state)
    }

    @Test
    fun olderHostsFallBackToTheSameStates() {
        // A host from before U30 has no session_state: the phone derives the state it can from the
        // fields it knows, so an update of the phone keeps working against an older computer.
        fun row(vararg pairs: Pair<String, Any>): JSONObject = JSONObject().apply {
            pairs.forEach { (key, value) -> put(key, value) }
        }
        assertEquals(SessionState.Working, SessionStateContract.state(row("activity" to "busy", "phase" to "tool"), true, now))
        assertEquals(SessionState.Stopped, SessionStateContract.state(row("state" to "paused"), true, now))
        assertEquals(SessionState.User, SessionStateContract.state(row("phase" to "approval"), true, now))
        assertEquals(SessionState.Deps, SessionStateContract.state(
            row("activity" to "idle", "coordination_wait" to JSONObject().put("status", "ok")), true, now))
        assertEquals(SessionState.Cold, SessionStateContract.state(
            row("activity" to "idle", "last_active" to now - 4000.0, "cache_ttl" to JSONObject().put("seconds", 300.0)), true, now))
        assertEquals(SessionState.Cooling, SessionStateContract.state(
            row("activity" to "idle", "last_active" to now - 60.0, "cache_ttl" to JSONObject().put("seconds", 300.0)), true, now))
        assertEquals(SessionState.Offline, SessionStateContract.state(row("activity" to "idle"), false, now))
        assertFalse(SessionStateContract.hasHostState(row("activity" to "idle")))
    }

    @Test
    fun bothThemesReadTheStatesAndTheMinutesAsTheDesignSays() {
        // Section 6: deps, cooling and cold have their own tokens in both themes, and the new build
        // badge is not one of them.
        for (kind in listOf(DesktopBadgeKind.Deps, DesktopBadgeKind.Cooling, DesktopBadgeKind.Cold, DesktopBadgeKind.Building)) {
            assertNotEquals("$kind dark and light", DesktopSessionPalette.colors(kind, true), DesktopSessionPalette.colors(kind, false))
        }
        assertEquals("41m", SessionStateContract.formatMinutes(41 * 60 + 20.0))
        assertEquals("0m", SessionStateContract.formatMinutes(-5.0))
        assertEquals("1h 05m", SessionStateContract.formatMinutes(65 * 60.0))
        // The badge and the header pill read the same words.
        val raw = output("zerus-build-running")
        assertEquals(SessionStateContract.status(raw, true, now).caption, SessionStateContract.pillText(raw, true, now))
    }
}
