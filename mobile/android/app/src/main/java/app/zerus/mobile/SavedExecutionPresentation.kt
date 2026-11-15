package app.zerus.mobile

import app.zerus.mobile.replica.TaskAttemptInfo
import app.zerus.mobile.replica.TaskInfo

/** Reads the selected local task only. Never resolves a runtime or offers an action. */
object SavedExecutionPresentation {
    fun section(task: TaskInfo, projectId: String?, stale: Boolean, time: (Long) -> String): SavedExecutionSectionState {
        val unavailable = SavedExecutionSectionState(false, notices = SavedExecutionText.notices(false, false, false, false, stale))
        if (projectId == null || task.project != projectId || task.schema !in 1L..8L) return unavailable
        val e = task.execution ?: return unavailable
        if (e.requirement > safe || e.attempts > 256uL || e.current?.let { !valid(it) } == true) return unavailable
        val c = e.current
        val notices = SavedExecutionText.notices(e.conflict, e.hasUnknown, e.hasUnexplained, c?.legacy == true, stale).toMutableList()
        val rows = mutableListOf(SavedExecutionRow("Current requirement revision", e.requirement.toString()))
        if (c != null) {
            if (!c.legacy) {
                rows += SavedExecutionRow("Attempt", c.id)
                rows += SavedExecutionRow("Generation", c.generation.toString())
            }
            rows += SavedExecutionRow("Requirement revision", c.requirement.toString())
            rows += SavedExecutionRow("Request", c.requestId.takeIf { it.isNotEmpty() })
            val executor = c.executor
            if (executor == null || executor.machine.isEmpty() || executor.session.isEmpty() || executor.sessionId.isEmpty()) notices += "Executor identity unresolved"
            rows += SavedExecutionRow("Recorded executor", executor?.let { listOf(it.machine, it.session, it.sessionId).filter(String::isNotEmpty).joinToString("\n").takeIf(String::isNotEmpty) })
            rows += SavedExecutionRow("Recorded at", if (c.legacy && c.at == 0L) null else time(c.at))
            rows += SavedExecutionRow("Begin recorded at", c.beganAt?.let(time))
            rows += SavedExecutionRow("Progress recorded at", c.progressAt?.let(time))
        }
        return SavedExecutionSectionState(true, c != null, c?.let { SavedExecutionText.phase(it.phase) }, notices, rows)
    }
    private val safe = 9_007_199_254_740_991uL
    private fun text(s: String) = s.toByteArray(Charsets.UTF_8).size <= 512 && s.none { it.code in 0..31 || it.code in 127..159 }
    private fun date(t: Long?) = t == null || t in -8_640_000_000_000_000L..8_640_000_000_000_000L
    private fun valid(c: TaskAttemptInfo): Boolean = text(c.id) && text(c.requestId) && c.generation <= safe && c.requirement <= safe &&
        c.origin in setOf("send", "start", "handoff", "reopen") && SavedExecutionText.phase(c.phase) != null &&
        date(c.at) && date(c.beganAt) && date(c.progressAt) && (c.phase != "running" || c.beganAt != null) &&
        (if (c.legacy) c.id.isEmpty() && c.generation == 0uL else c.id.matches(Regex("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")) && c.generation > 0uL) &&
        (c.executor?.let { text(it.machine) && text(it.session) && text(it.sessionId) } ?: c.legacy)
}
