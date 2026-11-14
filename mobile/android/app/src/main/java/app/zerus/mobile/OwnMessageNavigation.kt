package app.zerus.mobile

/** Source-scoped own-message navigation over the existing bounded history window. */
object OwnMessageNavigation {
    enum class Direction { Previous, Next }
    data class Selection(val cursor: String? = null, val viewport: String? = null, val following: Boolean = true)
    data class Page(val target: Target, val events: List<Event>, val epoch: String = "", val before: String? = null,
        val after: String? = null, val older: Boolean = false, val newer: Boolean = false)
    data class Directions(val previous: Boolean = false, val next: Boolean = false)
    class Availability {
        private var last = Directions()
        fun project(page: Page?, selection: Selection, canPage: Boolean, busy: Boolean, scrolling: Boolean): Directions {
            if (scrolling) return last
            return Directions(page != null && enabled(page,selection,Direction.Previous,canPage,busy),
                page != null && enabled(page,selection,Direction.Next,canPage,busy))
        }
        fun commit(value: Directions) { last = value }
    }
    private fun own(event: Event) = QuestionReplyPresentation.isOwnRole(event.role) && event.inheritedFrom.isBlank() &&
        event.origin !in setOf("peer_message", "subagent_report", "task_notification", "keep_warm")
    private fun index(events: List<Event>, id: String?) = if (id == null) -1 else events.indexOfFirst { id in HistoryInspection.identities(it) }
    fun candidate(page: Page, selection: Selection, direction: Direction): Event? {
        val cursor = index(page.events, selection.cursor)
        val viewport = if (selection.following) -1 else index(page.events, selection.viewport)
        val pivot = when { cursor >= 0 -> cursor; viewport >= 0 -> viewport; else -> page.events.size }
        var i = if (direction == Direction.Previous) pivot - 1 else if (cursor >= 0) pivot + 1 else pivot
        val step = if (direction == Direction.Previous) -1 else 1
        while (i in page.events.indices) {
            if (own(page.events[i])) return page.events[i]
            i += step
        }
        return null
    }
    fun enabled(page: Page, selection: Selection, direction: Direction, canPage: Boolean, busy: Boolean = false): Boolean =
        !busy && (candidate(page, selection, direction) != null || canPage && if (direction == Direction.Previous) page.older else page.newer)

    /** No accumulated transcript/cursor journal: each read replaces the same bounded window.
     * Losing the anchor to eviction is safe only within that window's unchanged epoch. */
    suspend fun find(target: Target, selection: Selection, direction: Direction, snapshot: () -> Page?,
        read: suspend (Direction) -> Boolean): Event? {
        var page = snapshot()?.takeIf { it.target == target } ?: return null
        candidate(page, selection, direction)?.let { return it }
        val anchor = when {
            index(page.events,selection.cursor) >= 0 -> selection.cursor
            !selection.following && index(page.events,selection.viewport) >= 0 -> selection.viewport
            else -> page.events.lastOrNull()?.id
        }
        val fromLatest = selection.cursor == null && selection.following && !page.newer
        var epoch = page.epoch
        while (if (direction == Direction.Previous) page.older else page.newer) {
            val previous = page
            if (!read(direction)) return null
            page = snapshot()?.takeIf { it.target == target } ?: return null
            if (epoch.isNotBlank() && page.epoch != epoch) return null
            // Recent inspection aliases must establish the original position in
            // canonical history before a newly indexed page can navigate from it.
            if (epoch.isBlank() && page.epoch.isNotBlank() && anchor != null && index(page.events, anchor) < 0 &&
                !(fromLatest && direction == Direction.Previous && !page.newer)) return null
            epoch = page.epoch
            if (page.before == previous.before && page.after == previous.after &&
                page.events.firstOrNull()?.id == previous.events.firstOrNull()?.id &&
                page.events.lastOrNull()?.id == previous.events.lastOrNull()?.id) return null
            val pivot = index(page.events, anchor)
            val nextSelection = if (pivot >= 0) Selection(cursor = anchor)
                else if (direction == Direction.Previous) Selection()
                else Selection(viewport = page.events.firstOrNull()?.id, following = false)
            candidate(page, nextSelection, direction)?.let { return it }
        }
        return null
    }
}
