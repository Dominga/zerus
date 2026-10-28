package app.zerus.mobile

import org.json.JSONObject
import kotlin.math.max

/**
 * U30: the session states of the UI handoff's Status/SESSION-STATES.md over the fields the
 * host reports: `session_state` (working, build, user, deps, cooling, cold, stopped), `state_reason`
 * (the explicit reason behind it), `build`, `last_active` and `cache_ttl`. `offline` is this phone's
 * state for a machine that does not answer; the host never lists it.
 *
 * The host computes the order (SESSION-STATES.md section 4) and the texts; this file presents them
 * and keeps a fallback for hosts from before U30, so the list and the session screen read the same
 * whatever host answers.
 */
enum class SessionState(val wire: String) {
    Working("working"), Building("build"), User("user"), Deps("deps"),
    Cooling("cooling"), Cold("cold"), Stopped("stopped"), Offline("offline");

    companion object {
        fun of(wire: String): SessionState? = entries.firstOrNull { it.wire == wire }
    }
}

/** The Cold card's two actions: a prefilled New session, and Resume (the first message rebuilds the cache). */
enum class ColdAction { New, Resume }

/** What the card's badge, its line 3, the dot and the session screen's pill show for one session. */
data class SessionStateStatus(
    val state: SessionState,
    val caption: String,
    val detail: String,
    /** The full reason the host reported, for the badge's tooltip and the header pill. */
    val reason: String = "",
    /** Prompt-cache warmth left when the state is cooling: the countdown and the mini bar. */
    val warmSeconds: Double = 0.0,
    val ttlSeconds: Double = 0.0,
    val coldActions: List<ColdAction> = emptyList(),
)

object SessionStateContract {
    /** SESSION-STATES.md section 3: the mock fixes the prompt cache at 60 minutes; the host reports its own. */
    const val DefaultTtlSeconds = 3600.0

    private fun text(raw: JSONObject, key: String): String = raw.opt(key) as? String ?: ""
    private fun number(raw: JSONObject?, key: String): Double? =
        (raw?.opt(key) as? Number)?.toDouble()?.takeIf { it.isFinite() }

    /** The reason object's text, "Waiting for cargo test --workspace · long operation, 6m so far". */
    fun reason(raw: JSONObject): String = (raw.optJSONObject("state_reason"))?.optString("text").orEmpty()

    /** The reason's kind (subagents, long_operation, build, usage_limit, coordination, job, team_*, …). */
    fun reasonKind(raw: JSONObject): String = (raw.optJSONObject("state_reason"))?.optString("kind").orEmpty()

    /** The build Zerus runs for the session, when one is live. */
    fun build(raw: JSONObject): JSONObject? = raw.optJSONObject("build")?.takeIf { it.length() > 0 }

    fun ttlSeconds(raw: JSONObject): Double =
        number(raw.optJSONObject("cache_ttl"), "seconds")?.takeIf { it > 0 } ?: DefaultTtlSeconds

    fun keptWarm(raw: JSONObject): Boolean = raw.optJSONObject("cache_ttl")?.optBoolean("kept_warm") == true

    fun lastActive(raw: JSONObject): Double = number(raw, "last_active") ?: number(raw, "created") ?: 0.0

    /** Seconds since the session last worked; 0 when the host named no last activity. */
    fun idleSeconds(raw: JSONObject, now: Double): Double = lastActive(raw).let { if (it <= 0) 0.0 else max(0.0, now - it) }

    /** Prompt-cache warmth left: section 3's `ttl − idle`, never below zero. */
    fun warmSeconds(raw: JSONObject, now: Double): Double = max(0.0, ttlSeconds(raw) - idleSeconds(raw, now))

    /** The mini bar's fraction, at least 4 % while cooling (section 3). */
    fun warmFraction(raw: JSONObject, now: Double): Double {
        val warm = warmSeconds(raw, now)
        val ttl = ttlSeconds(raw)
        return if (warm <= 0 || ttl <= 0) 0.0 else max(0.04, warm / ttl)
    }

    /** Section 3: `fmtMin(ttl − idle)` in the badge, "41m", "1h 05m". */
    fun formatMinutes(seconds: Double): String {
        val total = max(0, (seconds / 60.0).toInt())
        return if (total < 60) "${total}m" else "%dh %02dm".format(total / 60, total % 60)
    }

    fun state(raw: JSONObject, online: Boolean, now: Double = System.currentTimeMillis() / 1000.0): SessionState {
        if (!online) return SessionState.Offline
        SessionState.of(text(raw, "session_state"))?.let { return it }
        return legacyState(raw, now)
    }

    /**
     * The older hosts' derivation (no session_state), as far as their fields allow: the same
     * precedence for the states they can express. A host that answers with session_state never
     * reaches it.
     */
    private fun legacyState(raw: JSONObject, now: Double): SessionState {
        val state = text(raw, "state").ifEmpty { text(raw, "display_state") }
        val phase = text(raw, "phase")
        val activity = text(raw, "activity")
        val paused = state == "paused" || state == "stopped" || text(raw, "process_state") == "exited"
        val attention = phase in setOf("approval", "input", "attention") || raw.optBoolean("needs_action") ||
            (raw.optBoolean("unread") && !paused)
        val busy = activity == "busy" || phase in setOf("tool", "working", "compacting")
        if (paused) return SessionState.Stopped
        if (busy) return SessionState.Working
        if (attention) return SessionState.User
        if (raw.optJSONObject("coordination_wait")?.optString("status") == "ok") return SessionState.Deps
        val ttl = ttlSeconds(raw)
        val idle = idleSeconds(raw, now)
        return if (idle >= ttl) SessionState.Cold else SessionState.Cooling
    }

    fun caption(raw: JSONObject, state: SessionState, now: Double): String = when (state) {
        SessionState.Working -> "Working"
        SessionState.Building -> "Building"
        SessionState.User -> reason(raw).ifEmpty { "Needs you" }
        SessionState.Deps -> "Waiting"
        SessionState.Cooling -> "Cooling ${formatMinutes(warmSeconds(raw, now))}"
        SessionState.Cold -> "Cold"
        SessionState.Stopped -> "Stopped"
        SessionState.Offline -> "Offline"
    }

    /** SESSION-STATES.md: the badge's tooltip; the concrete reason is in [line3]. */
    fun badgeTitle(state: SessionState): String = when (state) {
        SessionState.Working -> "Working — the model is writing code or running tools"
        SessionState.Building -> "Building — Zerus runs a build or a long test for the session"
        SessionState.User -> "Waiting for you — an answer, an approval or a reply to read"
        SessionState.Deps -> "Waiting for dependencies — subagents, a long operation, other members or the lead"
        SessionState.Cooling -> "Cooling — done and waiting for a new task; the prompt cache is still warm"
        SessionState.Cold -> "Cold — done and idle; the prompt cache expired, a new session is cheaper"
        SessionState.Stopped -> "Stopped by you"
        SessionState.Offline -> "The machine does not answer"
    }

    /** Section 2 and 3: line 3 always names what waits, how warm the cache is, or who stopped it. */
    fun line3(raw: JSONObject, state: SessionState, now: Double): String {
        val reason = reason(raw)
        return when (state) {
            SessionState.Working -> reason.ifEmpty { "Working on the current turn" }
            SessionState.Building -> reason.ifEmpty {
                build(raw)?.let { build -> "Running " + build.optString("label").ifEmpty { build.optString("command") } }.orEmpty()
            }
            SessionState.User -> reason.ifEmpty { "Waiting for you" }
            SessionState.Deps -> reason.ifEmpty { "Waiting" }
            SessionState.Cooling -> {
                val kept = if (keptWarm(raw)) " (kept warm)" else ""
                "Done · waiting for a new task · cache warm for ${formatMinutes(warmSeconds(raw, now))}$kept"
            }
            SessionState.Cold -> "Cache cold ${formatMinutes(idleSeconds(raw, now) - ttlSeconds(raw))}"
            SessionState.Stopped -> "Stopped by you · resume to continue"
            SessionState.Offline -> "The machine does not answer"
        }
    }

    fun status(raw: JSONObject, online: Boolean, now: Double = System.currentTimeMillis() / 1000.0): SessionStateStatus {
        val state = state(raw, online, now)
        val cold = if (state == SessionState.Cold) listOf(ColdAction.New, ColdAction.Resume) else emptyList()
        return SessionStateStatus(
            state = state,
            caption = caption(raw, state, now),
            detail = line3(raw, state, now),
            reason = reason(raw).ifEmpty { badgeTitle(state) },
            warmSeconds = if (state == SessionState.Cooling) warmSeconds(raw, now) else 0.0,
            ttlSeconds = ttlSeconds(raw),
            coldActions = cold,
        )
    }

    /** The session screen's pill and the card's badge read the same words. */
    fun pillText(raw: JSONObject, online: Boolean, now: Double = System.currentTimeMillis() / 1000.0): String =
        status(raw, online, now).caption

    /**
     * The reply badge waits while the session has real work of its own (a build or a job it waits on);
     * a job it only left running (a service) never defers it. The host withholds `unread_reply` while
     * it defers, so this reads the state it reported.
     */
    fun replyDeferred(raw: JSONObject, online: Boolean): Boolean {
        if (!online) return false
        return SessionState.of(text(raw, "session_state")) in setOf(SessionState.Building, SessionState.Deps)
    }

    /** Phones older than the host's fields keep working: a blank host state is not an error. */
    fun hasHostState(raw: JSONObject): Boolean = SessionState.of(text(raw, "session_state")) != null
}
