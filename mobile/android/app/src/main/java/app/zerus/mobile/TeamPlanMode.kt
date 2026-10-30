package app.zerus.mobile

import org.json.JSONObject

/**
 * U59: how a team plans. `standard` phases are planned, approved, run and closed with their
 * integration and build (today's behaviour); a `free` phase never closes itself, takes tasks as
 * they come and is closed by the owner. Every field comes from the team JSON of `hgs team status`;
 * this file only presents it.
 */
enum class TeamPlanMode(val wire: String, val label: String) {
    Standard("standard", "Standard"),
    Free("free", "Free planning"),
    ;

    companion object {
        fun of(wire: String): TeamPlanMode = entries.firstOrNull { it.wire == wire } ?: Standard
    }
}

/** The free phase's state as the host computes it: open while work runs, idle when the queue empties. */
enum class TeamFreePhaseState(val wire: String) {
    Open("open"),
    Idle("idle"),
    ;

    companion object {
        fun of(wire: String): TeamFreePhaseState = entries.firstOrNull { it.wire == wire } ?: Open
    }
}

/** The open phase of a free team (the status's `free_phase`). */
data class TeamFreePhase(
    val phase: Int,
    val state: TeamFreePhaseState,
    val running: List<Int>,
    val accepted: Int,
    val acceptedSince: Int,
    val lastBuildAt: Double,
    val closePending: Int?,
) {
    /** No task of the phase is unfinished: the queue waits for more work, it is not done. */
    val idle: Boolean get() = state == TeamFreePhaseState.Idle
}

/** The plan-mode part of `hgs team status`: the mode, the lead's offer and the free phase. */
data class TeamPlan(
    val mode: TeamPlanMode,
    val offer: TeamPlanMode?,
    val reason: String,
    val freePhase: TeamFreePhase?,
)

/** The plan header's color: free violet, an idle queue blue, a waiting close amber, standard quiet. */
enum class TeamPlanTone { Standard, Free, Idle, Warn }

object TeamPlanStatus {
    /** The plan parts of a team's status JSON; a team stored before U59 reads standard. */
    fun of(status: JSONObject): TeamPlan {
        val offer = status.optString("plan_mode_offer").takeIf { it.isNotEmpty() }?.let(TeamPlanMode::of)
        return TeamPlan(
            mode = TeamPlanMode.of(status.optString("plan_mode")),
            offer = offer,
            reason = status.optString("plan_mode_reason"),
            freePhase = status.optJSONObject("free_phase")?.let { freePhase(it) },
        )
    }

    private fun freePhase(raw: JSONObject): TeamFreePhase = TeamFreePhase(
        phase = raw.optInt("phase"),
        state = TeamFreePhaseState.of(raw.optString("state")),
        running = raw.optJSONArray("running")?.let { list -> (0 until list.length()).mapNotNull { (list.opt(it) as? Number)?.toInt() } }.orEmpty(),
        accepted = raw.optInt("accepted"),
        acceptedSince = raw.optInt("accepted_since"),
        lastBuildAt = (raw.opt("last_build_at") as? Number)?.toDouble() ?: 0.0,
        closePending = (raw.opt("close_pending") as? Number)?.toInt(),
    )

    /** The mode beside the phase: "Free planning", the lead's offer, or nothing for standard. */
    fun badge(plan: TeamPlan): String? = when {
        plan.mode == TeamPlanMode.Free -> "Free planning"
        plan.offer == TeamPlanMode.Free -> "Free planning proposed"
        else -> null
    }

    /** The plan header: "Standard", or "Free planning · phase ...". */
    fun header(plan: TeamPlan): String {
        val phase = phaseLine(plan)
        return if (phase.isEmpty()) plan.mode.label else "${plan.mode.label} · $phase"
    }

    /**
     * The phase line: "" for a standard team; "Phase 2 · open · 5 accepted" while work runs;
     * "Phase 1 · idle, waiting for more work" when the queue is empty; a waiting close names its
     * build.
     */
    fun phaseLine(plan: TeamPlan): String {
        val phase = plan.freePhase ?: return ""
        val state = if (phase.idle) "idle, waiting for more work" else "open · ${phase.accepted} accepted"
        val line = "Phase ${phase.phase} · $state"
        return if (phase.closePending == null) line else "$line · close waits for build #${phase.closePending}"
    }

    /** The tone: the mode's, an idle queue's or a waiting close's. */
    fun tone(plan: TeamPlan): TeamPlanTone = when {
        plan.freePhase?.closePending != null -> TeamPlanTone.Warn
        plan.freePhase?.idle == true -> TeamPlanTone.Idle
        plan.mode == TeamPlanMode.Free -> TeamPlanTone.Free
        else -> TeamPlanTone.Standard
    }

    /** Foreground and background of the mode and phase line, in the desktop's tokens; 13 % background. */
    fun colors(tone: TeamPlanTone, dark: Boolean): DesktopBadgeColors {
        val foreground = when (tone) {
            TeamPlanTone.Standard -> if (dark) 0xFF9AA3AD else 0xFF5D6B7C
            TeamPlanTone.Free -> if (dark) 0xFFB79CFF else 0xFF6F4FD8
            TeamPlanTone.Idle -> if (dark) 0xFF7FB6E0 else 0xFF2F6EA3
            TeamPlanTone.Warn -> if (dark) 0xFFE8A75A else 0xFF9A5A0C
        }
        return DesktopBadgeColors(background = (foreground and 0x00FFFFFF) or 0x21000000, foreground = foreground)
    }
}
