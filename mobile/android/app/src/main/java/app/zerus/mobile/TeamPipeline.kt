package app.zerus.mobile

import org.json.JSONObject

/**
 * U36: a team task's status and its pipeline, over the task JSON of `hgs team status` (the shared
 * tests/fixtures/team-pipeline-tasks.json). After the implementer's report a task passes its
 * pipeline's stages - `review` and `test`, each done by a stage member - before the lead (or `auto`)
 * accepts it; a stage that returns it sends the findings back as a new attempt.
 *
 * The words and colors are the desktop's (design/agent-teams/pipelines): In review violet, In test
 * blue, the lead's acceptance green, a return or the lead's decision after the last round in the
 * warning color. The host computes every state; this file only presents it.
 */
enum class TeamTaskStatus(val wire: String, val label: String) {
    Draft("draft", "Draft"), Pending("pending", "Pending"), Assigned("assigned", "Assigned"),
    InProgress("in_progress", "In progress"), Submitted("submitted", "Submitted"),
    InReview("in_review", "In review"), InTest("in_test", "In test"),
    Accepted("accepted", "Accepted"), Rejected("rejected", "Rejected"), Cancelled("cancelled", "Cancelled");

    /** Reported and in one of its pipeline's stages. */
    val inStage: Boolean get() = this == InReview || this == InTest

    companion object {
        fun of(wire: String): TeamTaskStatus? = entries.firstOrNull { it.wire == wire }
    }
}

/** A stage run's outcome, as the host writes it; a state this client does not know is Unknown and closed, like the desktop's. */
enum class TeamStageState(val wire: String) {
    Waiting("waiting"), Delivered("delivered"), Passed("passed"), Returned("returned"), Skipped("skipped"), Revoked("revoked"), Unknown("");

    val open: Boolean get() = this == Waiting || this == Delivered

    companion object {
        fun of(wire: String): TeamStageState = entries.firstOrNull { it != Unknown && it.wire == wire } ?: Unknown
    }
}

/** The chip's and the verdict's color: one per stage kind, plus the warning and a quiet wait. */
enum class TeamStageTone { Review, Test, Accept, Warn, Quiet }

data class TeamStageRun(
    val id: String,
    val stage: Int,
    val kind: String,
    val round: Int,
    val attempt: Int,
    val member: String,
    val state: TeamStageState,
    val at: Double,
    val deliveredAt: Double?,
    val endedAt: Double?,
    val text: String,
    val findings: List<String>,
    /** Who ended it: the stage's role, Lead, You or Pipeline. */
    val by: String,
)

data class TeamPipelineStage(val kind: String, val role: String, val member: String)

/** The pipeline snapshot a task keeps from its assignment, with every stage run. */
data class TeamPipelineRun(
    val id: String,
    val name: String,
    val stages: List<TeamPipelineStage>,
    val maxRounds: Int,
    /** hold (the task's folder) or checkout (a detached checkout of the attempt). */
    val folder: String,
    /** lead, sub_lead or auto. */
    val accept: String,
    val round: Int,
    val runs: List<TeamStageRun>,
    /** #635 L2: the returns of the runs the host trimmed past its 64. */
    val trimmedReturns: Int = 0,
) {
    val openRun: TeamStageRun? get() = runs.lastOrNull { it.state.open }
    /** The rounds stage members returned, the trimmed ones included; the round limit counts these. */
    val returned: Int get() = trimmedReturns + runs.count { it.state == TeamStageState.Returned }
    /**
     * Every returned round's findings, which the lead decides with after the last round; a
     * command or land return counts at least one (#635 L3, the desktop's findingsCount).
     */
    val findings: Int get() = runs.filter { it.state == TeamStageState.Returned }
        .sumOf { if (TeamPipeline.runsCommand(it.kind)) maxOf(1, it.findings.size) else it.findings.size }
    /** Who accepts once the stages pass: auto, sub_lead or lead. */
    val acceptKind: String get() = if (accept == "auto" || accept == "sub_lead") accept else "lead"
}

/** Line 3 of a task card: the stage, its round and who works on it. */
data class TeamStageChip(val kind: String, val caption: String, val member: String, val tone: TeamStageTone, val outlined: Boolean)

/** One row of the inspector's Rounds: the stage, its round, its verdict and the findings. */
data class TeamStageVerdict(
    val title: String,
    val verdict: String,
    val tone: TeamStageTone,
    val member: String,
    val text: String,
    val findings: List<String>,
    val endedAt: Double?,
)

object TeamPipeline {
    /** At most this many runs per task, like the host (64). */
    const val MaxRuns = 64

    fun status(task: JSONObject): TeamTaskStatus? = TeamTaskStatus.of(task.string("status"))

    fun run(task: JSONObject): TeamPipelineRun? {
        val raw = task.optJSONObject("pipeline_run") ?: return null
        val maxRounds = raw.optInt("max_rounds").let { if (it > 0) it else 3 }
        return TeamPipelineRun(
            id = raw.string("id"),
            name = raw.string("name"),
            stages = raw.optJSONArray("stages")?.objects().orEmpty().map { TeamPipelineStage(it.string("kind"), it.string("role"), it.string("member")) },
            maxRounds = maxRounds,
            folder = raw.string("folder"),
            accept = raw.string("accept"),
            round = raw.optInt("round", 1).coerceAtLeast(1),
            runs = raw.optJSONArray("runs")?.objects().orEmpty().take(MaxRuns).map { stageRun(it) },
            trimmedReturns = raw.optInt("trimmed_returns").coerceAtLeast(0),
        )
    }

    /** A stage Zerus runs itself, with no member: a command or a land. */
    fun runsCommand(kind: String): Boolean = kind == "command" || kind == "land"

    private fun stageRun(raw: JSONObject): TeamStageRun = TeamStageRun(
        id = raw.string("id"),
        stage = raw.optInt("stage"),
        kind = raw.string("kind"),
        round = raw.optInt("round", 1),
        attempt = raw.optInt("attempt"),
        member = raw.string("member"),
        state = TeamStageState.of(raw.string("state")),
        at = raw.optDouble("at", 0.0).takeIf { it.isFinite() } ?: 0.0,
        deliveredAt = (raw.opt("delivered_at") as? Number)?.toDouble(),
        endedAt = (raw.opt("ended_at") as? Number)?.toDouble(),
        text = raw.string("text"),
        findings = raw.optJSONArray("findings")?.let { list -> (0 until list.length()).mapNotNull { list.opt(it) as? String } }.orEmpty(),
        by = raw.string("by"),
    )

    /** The desktop's stage name: "Review", "Test", "Sub-lead"..., or the kind as the host wrote it. */
    fun kindLabel(kind: String): String = when (kind) {
        "review" -> "Review"
        "test" -> "Test"
        "command" -> "Command"
        "land" -> "Land"
        "implement" -> "Implement"
        "auto" -> "Auto"
        "sub_lead" -> "Sub-lead"
        "lead" -> "Lead"
        else -> kind.replaceFirstChar { it.uppercase() }
    }

    fun kindTone(kind: String): TeamStageTone = when (kind) {
        "review" -> TeamStageTone.Review
        "test" -> TeamStageTone.Test
        else -> TeamStageTone.Quiet
    }

    /**
     * The card's stage chip, or null when the task has no pipeline or no stage to name:
     * - in a stage: "Review r1/3" with its member, or "Review waits" until a member takes it;
     *   a command or land stage "Command running" while Zerus runs it, "Command queued" before;
     * - back with the implementer after a return: "Review returned r1";
     * - reported after the last round returned it: "Lead decides · N findings" in the warning color;
     * - reported with the stages done: who accepts, "Lead", "Sub-lead" or "Auto".
     */
    fun chip(task: JSONObject): TeamStageChip? {
        val status = status(task) ?: return null
        val run = run(task) ?: return null
        val open = run.openRun
        if (status.inStage && open != null) {
            val label = kindLabel(open.kind)
            // #635 L4 (the desktop's #538 N8): Zerus runs a command or land stage itself, with no member.
            if (runsCommand(open.kind)) {
                val running = open.state == TeamStageState.Delivered
                return TeamStageChip(open.kind, if (running) "$label running" else "$label queued", "", kindTone(open.kind), outlined = !running)
            }
            return if (open.member.isEmpty()) TeamStageChip(open.kind, "$label waits", "", kindTone(open.kind), outlined = true)
            else TeamStageChip(open.kind, "$label r${open.round}/${run.maxRounds}", open.member, kindTone(open.kind), outlined = false)
        }
        val last = run.runs.lastOrNull() ?: return null
        return when {
            // #538 N10: only while the last round returned it; a later round that passed goes to the acceptance.
            status == TeamTaskStatus.Submitted && last.state == TeamStageState.Returned && run.returned >= run.maxRounds ->
                TeamStageChip("accept", if (run.findings == 1) "Lead decides · 1 finding" else "Lead decides · ${run.findings} findings", "", TeamStageTone.Warn, outlined = false)
            status == TeamTaskStatus.Submitted ->
                TeamStageChip("accept", kindLabel(run.acceptKind), "", TeamStageTone.Accept, outlined = false)
            last.state == TeamStageState.Returned && status in setOf(TeamTaskStatus.Pending, TeamTaskStatus.Assigned, TeamTaskStatus.InProgress) ->
                TeamStageChip(last.kind, "${kindLabel(last.kind)} returned r${last.round}", "", TeamStageTone.Warn, outlined = true)
            else -> null
        }
    }

    /** The verdict word of the inspector's round rows; none for an unknown state, as on the desktop. */
    fun verdict(state: TeamStageState): String = when (state) {
        TeamStageState.Waiting -> "WAITING"
        TeamStageState.Delivered -> "RUNNING"
        TeamStageState.Passed -> "PASS"
        TeamStageState.Returned -> "RETURN"
        TeamStageState.Skipped -> "SKIPPED"
        TeamStageState.Revoked -> "REVOKED"
        TeamStageState.Unknown -> ""
    }

    fun verdictTone(run: TeamStageRun): TeamStageTone = when (run.state) {
        TeamStageState.Passed -> TeamStageTone.Accept
        TeamStageState.Returned -> TeamStageTone.Warn
        TeamStageState.Delivered -> kindTone(run.kind)
        else -> TeamStageTone.Quiet
    }

    /** The inspector's Rounds, oldest first: "Review · round 1", its verdict, member, report and findings. */
    fun verdicts(task: JSONObject): List<TeamStageVerdict> = run(task)?.runs.orEmpty().map { run ->
        TeamStageVerdict(
            title = "${kindLabel(run.kind)} · round ${run.round}",
            verdict = verdict(run.state),
            tone = verdictTone(run),
            member = run.member.ifEmpty { run.by },
            text = run.text,
            findings = run.findings,
            endedAt = run.endedAt,
        )
    }

    /** The section's header: "Code: review, test, lead · round 2 of 3". */
    fun header(task: JSONObject): String = run(task)?.let { "${it.name.ifEmpty { it.id }} · round ${it.round} of ${it.maxRounds}" }.orEmpty()

    /** Foreground and background of a chip or verdict, in the desktop's tokens; the background is 13 % of the color. */
    fun colors(tone: TeamStageTone, dark: Boolean): DesktopBadgeColors {
        val foreground = when (tone) {
            TeamStageTone.Review -> if (dark) 0xFFB79CFF else 0xFF6F4FD8
            TeamStageTone.Test -> if (dark) 0xFF7FB6E0 else 0xFF2F6EA3
            TeamStageTone.Accept -> if (dark) 0xFF7DD3B0 else 0xFF167357
            TeamStageTone.Warn -> if (dark) 0xFFE8A75A else 0xFF9A5A0C
            TeamStageTone.Quiet -> if (dark) 0xFF9AA3AD else 0xFF5D6B7C
        }
        return DesktopBadgeColors(background = (foreground and 0x00FFFFFF) or 0x21000000, foreground = foreground)
    }

    /** The task status's own color: the stage's for In review and In test. */
    fun statusTone(status: TeamTaskStatus): TeamStageTone = when (status) {
        TeamTaskStatus.InReview -> TeamStageTone.Review
        TeamTaskStatus.InTest -> TeamStageTone.Test
        TeamTaskStatus.Accepted, TeamTaskStatus.Submitted -> TeamStageTone.Accept
        TeamTaskStatus.Rejected -> TeamStageTone.Warn
        else -> TeamStageTone.Quiet
    }
}
