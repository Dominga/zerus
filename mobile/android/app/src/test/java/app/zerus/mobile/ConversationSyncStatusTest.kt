package app.zerus.mobile

import app.zerus.mobile.ConversationSyncStatus.Facts
import app.zerus.mobile.ConversationSyncStatus.History
import app.zerus.mobile.ConversationSyncStatus.Tone
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

/** zerus-a4mub.10: one truthful, time-bound status for an open conversation. */
class ConversationSyncStatusTest {
    private val now = 1_800_000_000_000L
    private val minute = 60_000L
    private fun present(facts: Facts) = ConversationSyncStatus.present(facts) { "T${(it - now) / minute}" }
    private fun verified(ago: Long = 10_000L) = Facts(now = now, loaded = true, verified = true, observedAt = now - ago)
    private fun saved(ago: Long? = 2 * 60 * minute) = Facts(now = now, loaded = true, verified = false, observedAt = ago?.let { now - it })
    private fun detail(facts: Facts, label: String) = present(facts).details.single { it.label == label }

    @Test fun a_verified_read_reports_its_age_and_never_claims_to_be_current() {
        val fresh = present(verified())
        assertEquals("Updated just now", fresh.primary.text)
        assertEquals(Tone.Good, fresh.primary.tone)
        val aged = present(verified(3 * minute))
        assertEquals("Updated 3 min ago", aged.primary.text)
        assertEquals(Tone.Quiet, aged.primary.tone)
        assertEquals("Updated 2 h ago", present(verified(2 * 60 * minute)).primary.text)
        assertEquals("Updated 3 d ago", present(verified(3 * 24 * 60 * minute)).primary.text)
        for (facts in listOf(verified(), verified(3 * minute), saved(), saved(null))) {
            val text = present(facts).primary.text + present(facts).details.joinToString { it.value }
            for (claim in listOf("current", "Connected", "checked", "Latest activity")) assertFalse("$claim in $text", text.contains(claim, ignoreCase = true))
        }
        assertEquals("Read from the computer at T0 (just now)", detail(verified(), "Messages").value)
        assertEquals("10:42", ConversationSyncStatus.clock(java.time.ZonedDateTime.of(2026, 11, 11, 10, 42, 7, 0, java.time.ZoneId.systemDefault()).toInstant().toEpochMilli()))
    }

    @Test fun the_age_keeps_growing_in_the_background_without_a_new_read() {
        val facts = verified(10_000L)
        assertEquals("Updated just now", present(facts).primary.text)
        assertEquals("Updated 45 min ago", present(facts.copy(now = now + 45 * minute)).primary.text)
        // A clock that moved backwards never shows a negative age.
        assertEquals("Updated just now", present(facts.copy(observedAt = now + 5 * minute)).primary.text)
    }

    @Test fun an_aged_saved_copy_stays_honest_while_an_automatic_refresh_is_still_hidden() {
        val waiting = saved().copy(refreshing = true, refreshVisible = false)
        assertEquals(ConversationSyncStatus.Primary("Saved copy · 2 h ago", Tone.Quiet, false), present(waiting).primary)
        assertEquals("Checking for new activity", detail(waiting, "Connection").value)
        assertEquals("Saved on this phone; last read from the computer at T-120 (2 h ago)", detail(waiting, "Messages").value)
        // Past the delay (or at once for an explicit refresh) the same read becomes visible progress.
        assertEquals(ConversationSyncStatus.Primary("Refreshing…", Tone.Busy, true), present(waiting.copy(refreshVisible = true)).primary)
        assertEquals("Saved copy", present(saved(null)).primary.text)
        assertEquals("Saved on this phone; when it was last read is unknown", detail(saved(null), "Messages").value)
    }

    @Test fun an_explicit_refresh_shows_progress_immediately_over_an_earlier_failure() {
        val retrying = saved().copy(failure = "No connection.", offline = true, refreshing = true, refreshVisible = true)
        assertEquals(ConversationSyncStatus.Primary("Refreshing…", Tone.Busy, true), present(retrying).primary)
        assertEquals("Refreshing now", detail(retrying, "Connection").value)
    }

    @Test fun a_failed_refresh_keeps_the_saved_messages_and_says_why() {
        val offline = saved(5 * minute).copy(failure = "No connection. Showing saved conversation. Refresh to try again.", offline = true)
        assertEquals(ConversationSyncStatus.Primary("Offline · updated 5 min ago", Tone.Warning, false), present(offline).primary)
        assertEquals(ConversationSyncStatus.Detail("Connection", "No connection. Showing saved conversation. Refresh to try again.", Tone.Warning),
            detail(offline, "Connection"))
        val refused = saved(5 * minute).copy(failure = "The session changed.")
        assertEquals(ConversationSyncStatus.Primary("Couldn't refresh · updated 5 min ago", Tone.Warning, false), present(refused).primary)
        assertEquals("Offline · saved copy", present(saved(null).copy(failure = "No connection.", offline = true)).primary.text)
        val computer = verified(2 * minute).copy(offline = true)
        assertEquals(ConversationSyncStatus.Primary("Computer offline · updated 2 min ago", Tone.Warning, false), present(computer).primary)
    }

    @Test fun a_successful_read_after_reconnecting_clears_the_warning() {
        val failed = saved(5 * minute).copy(failure = "No connection.", offline = true)
        val reconnected = failed.copy(verified = true, observedAt = now - 1_000L, failure = "", offline = false)
        assertEquals(Tone.Warning, present(failed).primary.tone)
        assertEquals(ConversationSyncStatus.Primary("Updated just now", Tone.Good, false), present(reconnected).primary)
        assertEquals("Last read succeeded", detail(reconnected, "Connection").value)
    }

    @Test fun recent_window_and_history_pagination_are_separate_details() {
        assertEquals("Latest messages loaded", detail(verified(), "Recent messages").value)
        assertEquals("May have changed since it was saved", detail(saved(), "Recent messages").value)
        val partial = verified().copy(tailPartial = true)
        assertEquals(ConversationSyncStatus.Detail("Recent messages", "Showing part of the conversation; use Latest or the history controls for more", Tone.Warning),
            detail(partial, "Recent messages"))
        // A partial window is a detail, not a new primary claim.
        assertEquals("Updated just now", present(partial).primary.text)
        assertEquals("Loading earlier messages…", detail(verified().copy(history = History.Loading), "History").value)
        assertEquals("Earlier messages available", detail(verified().copy(history = History.MoreAvailable), "History").value)
        assertEquals("Beginning of the conversation is loaded", detail(verified().copy(history = History.Complete), "History").value)
        assertEquals("History is still indexing; unread totals may be unavailable", detail(verified().copy(history = History.Indexing), "History").value)
        assertEquals(ConversationSyncStatus.Detail("History", "Older messages could not be loaded.", Tone.Warning),
            detail(verified().copy(history = History.Failed, historyMessage = "Older messages could not be loaded."), "History"))
        assertEquals("Older messages may be outside the available history", detail(verified().copy(history = History.Truncated), "History").value)
        assertTrue(present(verified()).details.none { it.label == "History" })
    }

    @Test fun the_viewport_position_is_its_own_detail() {
        assertEquals("At the latest message", detail(verified(), "Position").value)
        assertEquals("Reading earlier messages · 12 unread", detail(verified().copy(atLatest = false, unreadLabel = "12 unread"), "Position").value)
        assertEquals("Reading earlier messages", detail(verified().copy(atLatest = false), "Position").value)
    }

    @Test fun shared_read_acknowledgement_is_pending_confirmed_or_failed() {
        assertEquals("Nothing waiting to sync", detail(verified(), "Read status").value)
        val pending = verified().copy(read = SharedReadStatus.Pending(now - minute))
        assertEquals("Updated just now", present(pending).primary.text)
        assertEquals("Waiting for the computer to confirm (sent T-1)", detail(pending, "Read status").value)
        val retrying = verified().copy(read = SharedReadStatus.Pending(now - minute, retryAt = now + 2 * minute))
        assertEquals("The computer has not applied it yet; trying again at T2", detail(retrying, "Read status").value)
        assertEquals("Confirmed by the computer at T0", detail(verified().copy(read = SharedReadStatus.Confirmed(now)), "Read status").value)
        val failed = verified(3 * minute).copy(read = SharedReadStatus.Failed("No connection", now))
        // The problem leads, so a narrow strip truncates the age rather than the failure.
        assertEquals(ConversationSyncStatus.Primary("Read not synced · updated 3 min ago", Tone.Warning, false), present(failed).primary)
        assertEquals(ConversationSyncStatus.Detail("Read status", "Not confirmed: No connection", Tone.Warning), detail(failed, "Read status"))
        assertEquals("Read not synced · saved copy · 2 h ago", present(saved().copy(read = SharedReadStatus.Failed("No connection", now))).primary.text)
        // A connection warning keeps the primary line; the read failure stays in the details.
        val offline = failed.copy(failure = "No connection.", offline = true, verified = false)
        assertEquals("Offline · updated 3 min ago", present(offline).primary.text)
        assertEquals(Tone.Warning, detail(offline, "Read status").tone)
    }

    @Test fun shared_read_outcomes_come_only_from_the_existing_receipt() {
        assertEquals(SharedReadStatus.Failed("No connection", now), ConversationSyncStatus.sharedReadOutcome(now, IOException("x"), false, false, null))
        assertEquals(SharedReadStatus.Failed("refused", now), ConversationSyncStatus.sharedReadOutcome(now, IllegalStateException("refused"), false, false, null))
        assertEquals(SharedReadStatus.Failed("The computer did not complete the read mark", now), ConversationSyncStatus.sharedReadOutcome(now, null, false, false, null))
        assertEquals(SharedReadStatus.Confirmed(now), ConversationSyncStatus.sharedReadOutcome(now, null, true, true, null))
        assertEquals(SharedReadStatus.Pending(now, now + 120_000), ConversationSyncStatus.sharedReadOutcome(now, null, true, false, now + 120_000))
        assertEquals(SharedReadStatus.Failed("The computer did not apply the read mark", now), ConversationSyncStatus.sharedReadOutcome(now, null, true, false, null))
    }

    @Test fun preview_archive_and_first_load_states() {
        assertEquals(ConversationSyncStatus.Primary("Preview", Tone.Quiet, false), present(verified().copy(demo = true)).primary)
        assertEquals(ConversationSyncStatus.Primary("Archived · read-only", Tone.Quiet, false), present(saved().copy(archived = true)).primary)
        val first = Facts(now = now, loaded = false, verified = false, observedAt = null, refreshing = true)
        assertEquals(ConversationSyncStatus.Primary("Loading…", Tone.Busy, false), present(first).primary)
        assertEquals(ConversationSyncStatus.Primary("Loading…", Tone.Busy, true), present(first.copy(refreshVisible = true)).primary)
        assertEquals(ConversationSyncStatus.Primary("Offline · nothing saved", Tone.Warning, false),
            present(first.copy(refreshing = false, failure = "No connection.", offline = true)).primary)
        assertEquals(ConversationSyncStatus.Primary("Couldn't load", Tone.Warning, false),
            present(first.copy(refreshing = false, failure = "Refused.")).primary)
        assertEquals("Not loaded yet", detail(first.copy(refreshing = false), "Messages").value)
    }

    @Test fun the_status_has_no_authority_output() {
        // Presentation only: its outputs are text, tone and progress. Answering, sending and
        // read marks keep their own verified-frame checks (activityVerified, contextBlocked, ...).
        val fields = ConversationSyncStatus.Presentation::class.java.declaredFields.map { it.name }.filterNot { it.startsWith("$") }.sorted()
        assertEquals(listOf("details", "primary"), fields)
        assertEquals(listOf("progress", "text", "tone"), ConversationSyncStatus.Primary::class.java.declaredFields.map { it.name }.filterNot { it.startsWith("$") }.sorted())
        assertNull(ConversationSyncStatus::class.java.declaredMethods.firstOrNull { it.name.contains("allow", true) || it.name.contains("enabled", true) })
    }
}
