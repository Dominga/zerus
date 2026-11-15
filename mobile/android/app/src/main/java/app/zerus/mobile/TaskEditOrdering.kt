package app.zerus.mobile

import app.zerus.mobile.replica.taskEditRankBetween

/** Presentation-only ordering; no store or canonical neighboring task is written. */
internal object TaskEditOrdering {
    fun top(original: ProjectKey, current: ProjectKey?, task: String, neighbors: List<Pair<String, String>>): String? {
        if (current != original) return null
        val first = neighbors.filter { it.first != task }.minOfOrNull { it.second }
        return taskEditRankBetween(null, first)
    }
}
