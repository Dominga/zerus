package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DesktopSessionPaletteTest {
    @Test fun backgroundWorkRetainsDistinctCaptionWithoutAnIntermediateUnreadMarker() {
        val row=session("""{"activity":"idle","phase":"idle","process_work":{"count":1},"mobile_unread_reply":true}""")
        val badge=DesktopSessionPalette.badge(row,true)
        assertEquals("Background",badge.caption)
        assertEquals(DesktopBadgeKind.Background,badge.kind)
        assertFalse(badge.additionalUnread)
        for (dark in listOf(false, true)) {
            assertNotEquals(DesktopSessionPalette.colors(DesktopBadgeKind.Working, dark),
                DesktopSessionPalette.colors(badge.kind, dark))
            assertEquals(DesktopSessionPalette.colors(badge.kind, dark).foreground,
                DesktopSessionPalette.detailColors(row, true, dark).foreground)
        }
        val finished = row.copy(raw = JSONObject(row.raw.toString()).put("process_work", JSONObject().put("count", 0)))
        assertEquals(DesktopBadgeKind.Unread, DesktopSessionPalette.badge(finished, true).kind)
    }
    private fun session(raw: String = "{}", pending: Int = 0, archive: String = "") = Session(
        Target("computer", "codex/project/task", "run", "conversation", "workspace", archive),
        "Task", "codex", "Untrusted display string", "Project", "Preview", pending, JSONObject(raw))

    @Test fun localReadReminderUsesDesktopPrecedence() {
        assertEquals(DesktopBadgeKind.Attention,DesktopSessionPalette.badge(session("""{"mobile_review_later":true}"""),false).kind)
        assertEquals(DesktopBadgeKind.Attention,DesktopSessionPalette.badge(
            session("""{"session_state":"cooling","mobile_review_later":true}"""),true).kind)
        assertEquals(DesktopBadgeKind.Unread,DesktopSessionPalette.badge(session("""{"activity":"idle","mobile_unread_reply":true}"""),true).kind)
        val busy=DesktopSessionPalette.badge(session("""{"activity":"busy","mobile_unread_reply":true}"""),true)
        assertEquals(DesktopBadgeKind.Working,busy.kind)
        assertFalse(busy.additionalUnread)
    }

    @Test fun desktopPairsAreExactForBothThemes() {
        // Named keys keep new states from silently shifting the old tokens.
        val expected = mapOf(
            DesktopBadgeKind.Neutral to (DesktopBadgeColors(0xFF2B343E, 0xFFB6C2D0) to DesktopBadgeColors(0xFFE7EDF2, 0xFF566575)),
            DesktopBadgeKind.Working to (DesktopBadgeColors(0xFF194D36, 0xFF97F0BA) to DesktopBadgeColors(0xFFCCEFDC, 0xFF145B37)),
            DesktopBadgeKind.Building to (DesktopBadgeColors(0xFF1B3A4A, 0xFF7FD3F2) to DesktopBadgeColors(0xFFDFF0F8, 0xFF17627F)),
            DesktopBadgeKind.Attention to (DesktopBadgeColors(0xFFFFDA76, 0xFF392900) to DesktopBadgeColors(0xFFF4CE65, 0xFF392900)),
            DesktopBadgeKind.Error to (DesktopBadgeColors(0xFFFFABB6, 0xFF46202A) to DesktopBadgeColors(0xFFB73750, 0xFFFFFFFF)),
            DesktopBadgeKind.Paused to (DesktopBadgeColors(0xFF443654, 0xFFDEC3FF) to DesktopBadgeColors(0xFFE8DDF5, 0xFF634187)),
            DesktopBadgeKind.Unread to (DesktopBadgeColors(0xFFFFDA76, 0xFF392900) to DesktopBadgeColors(0xFFF4CE65, 0xFF392900)),
            DesktopBadgeKind.Scheduled to (DesktopBadgeColors(0xFF173E48, 0xFF9FE0F0) to DesktopBadgeColors(0xFFD3EDF4, 0xFF165A6B)),
            DesktopBadgeKind.Background to (DesktopBadgeColors(0xFF273544, 0xFFAAC0DF) to DesktopBadgeColors(0xFFE9F0F8, 0xFF466B99)),
            DesktopBadgeKind.Waiting to (DesktopBadgeColors(0xFF443A25, 0xFFEDD096) to DesktopBadgeColors(0xFFF5E8C7, 0xFF795913)),
            DesktopBadgeKind.Deps to (DesktopBadgeColors(0xFF1B2A44, 0xFF6FA8FF) to DesktopBadgeColors(0xFFE6EFFB, 0xFF2F66C9)),
            DesktopBadgeKind.Cooling to (DesktopBadgeColors(0xFF12333B, 0xFF56C8E0) to DesktopBadgeColors(0xFFE1F3F8, 0xFF137F95)),
            DesktopBadgeKind.Cold to (DesktopBadgeColors(0xFF2A3547, 0xFF8FA6C4) to DesktopBadgeColors(0xFFE9EEF5, 0xFF55708F)))
        assertEquals(DesktopBadgeKind.entries.toSet(), expected.keys)
        for ((kind, colors) in expected) {
            assertEquals("$kind dark", colors.first, DesktopSessionPalette.colors(kind, true))
            assertEquals("$kind light", colors.second, DesktopSessionPalette.colors(kind, false))
        }
    }

    @Test fun scheduledAgentsShowTheirNextRunInsteadOfReady() {
        val next = System.currentTimeMillis() / 1000 + 600
        val raw = """{"activity":"idle","phase":"idle","process_state":"running","schedules":{"count":1,"recurring":1,"next_at":$next}}"""
        val badge = DesktopSessionPalette.badge(session(raw), true)
        assertEquals(DesktopBadgeKind.Scheduled, badge.kind)
        assertEquals("Scheduled · " + SessionFilters.nextScheduledRun(session(raw)), badge.caption)
        assertTrue(badge.caption.matches(Regex("""Scheduled · (\d{2}:\d{2}|\d{1,2} \S+ \d{2}:\d{2})""")))
        assertEquals("Scheduled · due", DesktopSessionPalette.badge(session(raw.replace("$next", "1")), true).caption)
        assertEquals("Scheduled", DesktopSessionPalette.badge(session(raw.replace("$next", "null")), true).caption)
        // Questions, work and a new reply outrank the schedule; offline and unknown are not scheduled.
        assertEquals(DesktopBadgeKind.Attention, DesktopSessionPalette.badge(session(raw.replace("\"phase\":\"idle\"", "\"phase\":\"input\"")), true).kind)
        assertEquals(DesktopBadgeKind.Unread, DesktopSessionPalette.badge(session(raw.replace("}}", "},\"mobile_unread_reply\":true}")), true).kind)
        assertEquals("Offline", DesktopSessionPalette.badge(session(raw), false).caption)
        assertEquals(DesktopBadgeKind.Neutral, DesktopSessionPalette.badge(session(raw.replace("\"count\":1", "\"count\":0")), true).kind)
        assertEquals("Ready", DesktopSessionPalette.badge(session("""{"activity":"idle","phase":"idle","process_state":"running","schedules":null}"""), true).caption)
    }

    @Test fun workingToolThinkingAndCompactionAreGreenButIdleIsNeutral() {
        listOf("working", "tool", "thinking", "compacting").forEach { phase ->
            val item = session("""{"activity":"busy","phase":"$phase","process_state":"running"}""")
            val badge = DesktopSessionPalette.badge(item, true)
            assertEquals(DesktopBadgeKind.Working, badge.kind)
            assertEquals(if (phase == "compacting") "Compacting" else "Working", badge.caption)
        }
        val ready = DesktopSessionPalette.badge(session("""{"activity":"idle","phase":"idle"}"""), true)
        assertEquals("Ready", ready.caption)
        assertEquals(DesktopBadgeKind.Neutral, ready.kind)
        assertNotEquals(DesktopSessionPalette.colors(ready.kind, true), DesktopSessionPalette.colors(DesktopBadgeKind.Working, true))
    }

    @Test fun nativeAttentionAndErrorOverrideBusyWithDesktopColors() {
        listOf("input", "approval", "error").forEach { phase ->
            val item = session("""{"activity":"busy","phase":"$phase"}""")
            assertEquals(if (phase == "error") DesktopBadgeKind.Error else DesktopBadgeKind.Attention,
                DesktopSessionPalette.badge(item, true).kind)
        }
        val acknowledged = session("""{"activity":"busy","phase":"error","attention_acknowledged":true}""")
        assertEquals("Error", DesktopSessionPalette.badge(acknowledged, true).caption)
        assertEquals(DesktopBadgeKind.Neutral, DesktopSessionPalette.badge(acknowledged, true).kind)
    }

    @Test fun scheduledRecoveryDoesNotTurnRedButBlockedRecoveryNeedsAttention() {
        listOf("waiting", "dispatching", "retrying").forEach { recovery ->
            val item = session("""{"activity":"idle","phase":"error","recovery":{"state":"$recovery"}}""")
            assertEquals(DesktopBadgeKind.Neutral, DesktopSessionPalette.badge(item, true).kind)
            assertEquals(0xFF9AA4B2, DesktopSessionPalette.detailColors(item, true, true).foreground)
        }
        assertEquals(DesktopBadgeKind.Attention, DesktopSessionPalette.badge(
            session("""{"activity":"idle","phase":"idle","recovery":{"state":"blocked"}}"""), true).kind)
    }

    @Test fun resumedRecoveryShowsTheWorkingTurn() {
        val retrying = session("""{"tracked":true,"activity":"busy","phase":"tool","recovery":{"state":"retrying","acknowledged":true}}""")
        assertEquals("Retrying", SessionFilters.status(retrying, true))
        assertEquals(0xFF9AA4B2, DesktopSessionPalette.detailColors(retrying, true, true).foreground)
        val resumed = session("""{"tracked":true,"activity":"busy","phase":"tool","recovery":{"state":"retrying","acknowledged":true,"resumed_at":125}}""")
        assertEquals("Using a tool", SessionFilters.status(resumed, true))
        assertEquals(DesktopBadgeKind.Working, DesktopSessionPalette.badge(resumed, true).kind)
        assertEquals(0xFF55E39A, DesktopSessionPalette.detailColors(resumed, true, true).foreground)
    }

    @Test fun pausedPurpleAndStoppedArchiveOfflineNeutralIgnoreStaleBusy() {
        assertEquals(DesktopBadgeKind.Paused, DesktopSessionPalette.badge(session("""{"state":"paused","activity":"busy"}"""), true).kind)
        listOf(
            session("""{"state":"stopped","activity":"busy"}"""),
            session("""{"state":"archived","activity":"busy"}""", archive = "archive-id"),
            session("""{"process_state":"exited","activity":"busy"}"""),
            session("""{"conversation_state":"ended","activity":"busy"}"""),
            session("""{"activity":"unknown"}"""))
            .forEach { assertEquals(DesktopBadgeKind.Neutral, DesktopSessionPalette.badge(it, true).kind) }
        val offline = DesktopSessionPalette.badge(session("""{"activity":"busy","phase":"approval"}"""), false)
        assertEquals("Offline", offline.caption)
        assertEquals(DesktopBadgeKind.Neutral, offline.kind)
    }

    @Test fun unreadFollowsWorkingAndAttentionButPrecedesPaused() {
        val ready = DesktopSessionPalette.badge(session("""{"activity":"idle","unread_reply":true}"""), true)
        assertEquals("New reply", ready.caption)
        assertEquals(DesktopBadgeKind.Unread, ready.kind)
        assertFalse(ready.additionalUnread)
        val working = DesktopSessionPalette.badge(session("""{"activity":"busy","unread_reply":true}"""), true)
        assertEquals(DesktopBadgeKind.Working, working.kind)
        assertFalse(working.additionalUnread)
        val attention = DesktopSessionPalette.badge(session("""{"activity":"busy","phase":"input","unread_reply":true}"""), true)
        assertEquals(DesktopBadgeKind.Attention, attention.kind)
        assertFalse(attention.additionalUnread)
        assertEquals(DesktopBadgeKind.Unread, DesktopSessionPalette.badge(session("""{"state":"paused","unread_reply":true}"""), true).kind)
    }

    @Test fun childAttentionRespectsCurrentRosterAndAcknowledgement() {
        val child = """{"activity":"busy","subagents":{"child":{"display_state":"input"}}}"""
        assertEquals(DesktopBadgeKind.Attention, DesktopSessionPalette.badge(session(child), true).kind)
        val legacy = JSONObject(child).put("subagent_source", "hook_profiles")
        assertEquals(DesktopBadgeKind.Working, DesktopSessionPalette.badge(session(legacy.toString()), true).kind)
        val acknowledged = JSONObject(child).put("attention_acknowledged", true)
        assertEquals(DesktopBadgeKind.Working, DesktopSessionPalette.badge(session(acknowledged.toString()), true).kind)
        val ended = JSONObject(child).put("conversation_state", "ended")
        assertEquals(DesktopBadgeKind.Neutral, DesktopSessionPalette.badge(session(ended.toString()), true).kind)
        // A pending connector request affects filtering; desktop badges still follow the actual row state.
        assertEquals(DesktopBadgeKind.Working, DesktopSessionPalette.badge(session("""{"activity":"busy"}""", pending = 1), true).kind)
    }

    @Test fun reviewLaterRemainsAttentionOfflineAndDetailUsesSeparateDesktopTones() {
        assertEquals(DesktopBadgeKind.Attention, DesktopSessionPalette.badge(session("""{"review_later":true}"""), false).kind)
        val ready = session("""{"activity":"idle"}""")
        val working = session("""{"activity":"busy"}""")
        assertEquals(DesktopBadgeColors(0xFF2B323D, 0xFF78DABB), DesktopSessionPalette.detailColors(ready, true, true))
        assertEquals(DesktopBadgeColors(0xFF2B323D, 0xFF55E39A), DesktopSessionPalette.detailColors(working, true, true))
        assertEquals(DesktopBadgeColors(0xFFEDF1F5, 0xFF11775D), DesktopSessionPalette.detailColors(ready, true, false))
        assertEquals(DesktopBadgeColors(0xFFEDF1F5, 0xFF12834E), DesktopSessionPalette.detailColors(working, true, false))
    }
}
