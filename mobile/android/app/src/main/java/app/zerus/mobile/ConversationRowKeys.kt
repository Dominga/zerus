package app.zerus.mobile

/** Bounded presentation identities only; native/read/receipt identities stay unchanged. */
object ConversationRowKeys {
    const val maxRetained = 2048

    data class State internal constructor(val targetKey: String, val keyByEventId: Map<String, String>,
        /** Full LazyColumn key to the current native/display event ID. */
        val eventIdByKey: Map<String, String>) {
        /** Rows beyond the retained bound remain visible under their canonical ID. */
        fun key(eventId: String): String = "event:" + (keyByEventId[eventId] ?: eventId)
    }

    fun reconcile(targetKey: String, previousState: State?, events: List<Event>,
        supersededIds: Map<String, String> = emptyMap(), maxRetained: Int = ConversationRowKeys.maxRetained): State {
        require(maxRetained >= 0)
        val previous = previousState?.takeIf { targetKey.isNotBlank() && it.targetKey == targetKey }
        val currentIds = events.mapTo(mutableSetOf()) { it.id }
        val retained = events.distinctBy { it.id }.take(maxRetained.coerceAtMost(ConversationRowKeys.maxRetained))
        val assigned = linkedMapOf<String, String>()
        val claimed = mutableSetOf<String>()
        // A canonical fallback must never collide with another row's retained
        // key, including a row outside the retained bound.
        fun usable(id: String, key: String) = key == id || key !in currentIds
        val direct = retained.mapNotNull { event -> previous?.keyByEventId?.get(event.id)?.let { event.id to it } }
        val directClaims = direct.groupingBy { it.second }.eachCount()
        direct.forEach { (id, key) ->
            if (usable(id, key) && directClaims[key] == 1) {
                assigned[id] = key
                claimed += key
            }
        }
        fun successor(source: String): String? {
            var id = source
            var steps = 0
            val visited = mutableSetOf<String>()
            val bound = minOf(supersededIds.size, ConversationRowKeys.maxRetained)
            while (true) {
                val next = supersededIds[id] ?: return id.takeIf { it != source }
                if (steps >= bound || !visited.add(id)) return null
                id = next
                steps++
            }
        }
        // Only prior retained rows can supply a composition key. Resolve their
        // proven chains with a bound; cyclic/overlong paths supply no alias.
        val sourcesBySuccessor = previous?.keyByEventId?.keys.orEmpty().take(ConversationRowKeys.maxRetained)
            .mapNotNull { source -> successor(source)?.let { it to source } }
            .groupBy({ it.first }, { it.second })
        val candidates = linkedMapOf<String, String>()
        retained.filter { it.id !in assigned && previous?.keyByEventId?.containsKey(it.id) != true }.forEach { event ->
            val sources = event.originalIds + event.originalId + sourcesBySuccessor[event.id].orEmpty()
            val keys = sources.mapNotNull { previous?.keyByEventId?.get(it) }.toSet()
            val key = keys.singleOrNull()
            if (key != null && usable(event.id, key) && key !in claimed) candidates[event.id] = key
        }
        // One old row claimed by several successors is not an identity proof
        // for either successor. Keep both messages under fresh canonical keys.
        val aliasClaims = candidates.values.groupingBy { it }.eachCount()
        retained.forEach { event ->
            if (event.id !in assigned) {
                val alias = candidates[event.id]?.takeIf { aliasClaims[it] == 1 && it !in claimed }
                val key = alias ?: event.id
                assigned[event.id] = key
                claimed += key
            }
        }
        return State(targetKey, assigned.toMap(), assigned.entries.associate { (id, key) -> "event:$key" to id })
    }
}
