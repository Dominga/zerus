package app.zerus.mobile

import java.io.IOException
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/**
 * This phone's observed outcome for one conversation's shared read acknowledgement
 * (`mark_read`). Presentation only: zerus-a4mub.9 owns when and what may be
 * acknowledged; its paths report their observed outcomes through
 * [ZerusViewModel.noteSharedRead]. Times are wall-clock milliseconds.
 */
sealed interface SharedReadStatus {
    data object None : SharedReadStatus
    /** Sent and not yet confirmed; [retryAt] when the computer completed it without applying. */
    data class Pending(val since: Long, val retryAt: Long? = null) : SharedReadStatus
    data class Confirmed(val at: Long) : SharedReadStatus
    data class Failed(val reason: String, val at: Long) : SharedReadStatus
}

/**
 * One compact, time-bound status for the open conversation, with details on demand
 * (zerus-a4mub.10). Its facts are independent: what the shown messages are and when
 * they were observed, the read in flight, connectivity, the recent window, history
 * pagination, the viewport and shared read acknowledgement. It never says the
 * conversation is current, and it grants no authority: answering, sending and read
 * marks keep their own verified-frame checks.
 */
object ConversationSyncStatus {
    enum class Tone { Quiet, Good, Busy, Warning }
    enum class History { Unknown, Loading, MoreAvailable, Truncated, Complete, Indexing, Failed }

    data class Facts(
        val now: Long,
        val demo: Boolean = false,
        val archived: Boolean = false,
        /** Messages, questions or a resolved empty read are shown. */
        val loaded: Boolean,
        /** The shown messages come from a read this view completed, not only from this phone's copy. */
        val verified: Boolean,
        /** When the shown messages were observed; null when a saved copy has no time. */
        val observedAt: Long?,
        val refreshing: Boolean = false,
        /** An explicit refresh at once; an automatic one only after its delay, so polls never flicker. */
        val refreshVisible: Boolean = false,
        /** This conversation's last read failure. */
        val failure: String = "",
        /** The failure was a lost connection, or the computer is reported offline. */
        val offline: Boolean = false,
        /** The loaded window is known to miss newer or recent messages. */
        val tailPartial: Boolean = false,
        val history: History = History.Unknown,
        val historyMessage: String = "",
        val atLatest: Boolean = true,
        val unreadLabel: String = "",
        val read: SharedReadStatus = SharedReadStatus.None,
    )
    data class Primary(val text: String, val tone: Tone, val progress: Boolean)
    data class Detail(val label: String, val value: String, val tone: Tone = Tone.Quiet)
    data class Presentation(val primary: Primary, val details: List<Detail>)

    private const val MINUTE = 60_000L

    fun age(now: Long, at: Long): String {
        val elapsed = (now - at).coerceAtLeast(0)
        return when {
            elapsed < MINUTE -> "just now"
            elapsed < 60 * MINUTE -> "${elapsed / MINUTE} min ago"
            elapsed < 24 * 60 * MINUTE -> "${elapsed / (60 * MINUTE)} h ago"
            else -> "${elapsed / (24 * 60 * MINUTE)} d ago"
        }
    }

    private val clockFormat = DateTimeFormatter.ofPattern("HH:mm")
    /** "10:42" in this phone's time zone. */
    fun clock(at: Long): String = clockFormat.format(Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()))

    fun present(facts: Facts, time: (Long) -> String = ::clock): Presentation =
        Presentation(primary(facts), details(facts, time))

    private fun primary(f: Facts): Primary {
        if (f.demo) return Primary("Preview", Tone.Quiet, false)
        if (f.archived) return Primary("Archived · read-only", Tone.Quiet, false)
        if (!f.loaded) return when {
            f.refreshing -> Primary("Loading…", Tone.Busy, f.refreshVisible)
            f.failure.isNotBlank() -> Primary(if (f.offline) "Offline · nothing saved" else "Couldn't load", Tone.Warning, false)
            else -> Primary("Not loaded", Tone.Quiet, false)
        }
        if (f.refreshing && f.refreshVisible) return Primary("Refreshing…", Tone.Busy, true)
        val updated = f.observedAt?.let { "updated ${age(f.now, it)}" } ?: "saved copy"
        val base = when {
            f.failure.isNotBlank() -> Primary(if (f.offline) "Offline · $updated" else "Couldn't refresh · $updated", Tone.Warning, false)
            f.offline -> Primary("Computer offline · $updated", Tone.Warning, false)
            !f.verified -> Primary(f.observedAt?.let { "Saved copy · ${age(f.now, it)}" } ?: "Saved copy", Tone.Quiet, false)
            else -> {
                val at = f.observedAt ?: f.now
                Primary("Updated ${age(f.now, at)}", if (f.now - at < 2 * MINUTE) Tone.Good else Tone.Quiet, false)
            }
        }
        // The problem leads, so a narrow strip truncates the age, not the failure. A connection
        // warning keeps the line; the read failure then stays in the details.
        return if (f.read is SharedReadStatus.Failed && base.tone != Tone.Warning)
            Primary("Read not synced · ${base.text.replaceFirstChar { it.lowercase() }}", Tone.Warning, false) else base
    }

    private fun details(f: Facts, time: (Long) -> String): List<Detail> = buildList {
        val observed = f.observedAt?.let { "at ${time(it)} (${age(f.now, it)})" }
        add(Detail("Messages", when {
            f.demo -> "Preview content"
            !f.loaded -> "Not loaded yet"
            f.verified -> "Read from the computer ${observed ?: "in this view"}"
            observed != null -> "Saved on this phone; last read from the computer $observed"
            else -> "Saved on this phone; when it was last read is unknown"
        }))
        add(when {
            f.refreshing -> Detail("Connection", if (f.refreshVisible) "Refreshing now" else "Checking for new activity", Tone.Busy)
            f.failure.isNotBlank() -> Detail("Connection", f.failure, Tone.Warning)
            f.offline -> Detail("Connection", "The computer is reported offline", Tone.Warning)
            f.verified -> Detail("Connection", "Last read succeeded")
            else -> Detail("Connection", "Not read from the computer yet")
        })
        add(when {
            f.tailPartial -> Detail("Recent messages", "Showing part of the conversation; use Latest or the history controls for more", Tone.Warning)
            f.verified -> Detail("Recent messages", "Latest messages loaded")
            else -> Detail("Recent messages", "May have changed since it was saved")
        })
        when (f.history) {
            History.Unknown -> {}
            History.Loading -> add(Detail("History", "Loading earlier messages…", Tone.Busy))
            History.MoreAvailable -> add(Detail("History", "Earlier messages available"))
            History.Truncated -> add(Detail("History", "Older messages may be outside the available history"))
            History.Complete -> add(Detail("History", "Beginning of the conversation is loaded"))
            History.Indexing -> add(Detail("History", "History is still indexing; unread totals may be unavailable"))
            History.Failed -> add(Detail("History", f.historyMessage.ifBlank { "Earlier messages could not be loaded" }, Tone.Warning))
        }
        add(Detail("Position", if (f.atLatest) "At the latest message" else
            listOf("Reading earlier messages", f.unreadLabel).filter { it.isNotBlank() }.joinToString(" · ")))
        add(when (val read = f.read) {
            SharedReadStatus.None -> Detail("Read status", "Nothing waiting to sync")
            is SharedReadStatus.Pending -> Detail("Read status", read.retryAt?.let { "The computer has not applied it yet; trying again at ${time(it)}" }
                ?: "Waiting for the computer to confirm (sent ${time(read.since)})", Tone.Busy)
            is SharedReadStatus.Confirmed -> Detail("Read status", "Confirmed by the computer at ${time(read.at)}")
            is SharedReadStatus.Failed -> Detail("Read status", "Not confirmed: ${read.reason}", Tone.Warning)
        })
    }

    /**
     * The observed outcome of one existing `mark_read` call. [retryAt] is the wall time of the
     * bounded retry the caller scheduled for a completed but unapplied mark, null when none.
     */
    fun sharedReadOutcome(now: Long, failure: Throwable?, completed: Boolean, applied: Boolean, retryAt: Long?): SharedReadStatus = when {
        failure != null -> SharedReadStatus.Failed(if (failure is IOException) "No connection" else failure.message.orEmpty().ifBlank { "The read mark failed" }, now)
        !completed -> SharedReadStatus.Failed("The computer did not complete the read mark", now)
        applied -> SharedReadStatus.Confirmed(now)
        retryAt != null -> SharedReadStatus.Pending(now, retryAt)
        else -> SharedReadStatus.Failed("The computer did not apply the read mark", now)
    }
}
