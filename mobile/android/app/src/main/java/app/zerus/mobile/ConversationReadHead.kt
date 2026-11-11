package app.zerus.mobile

/** A later canonical page can identify rows already read from the bounded native inspection. */
internal object ConversationReadHead {
    fun observe(state: ConversationReadState, page: HistoryPage): ConversationReadState {
        val canonical = page.events.filter { it.historyEpoch == page.epoch && it.historyId.isNotBlank() && it.historyCursor.isNotBlank() }
        val aliases = HistoryAliases.exact(canonical, state.record(page.target)?.seen.orEmpty().toSet())
        val next = state.head(page.target, page.epoch, page.latestIncoming, page.totalIncoming, page.complete)
            .supersede(page.target, aliases)
        val record = next.record(page.target) ?: return next
        if (record.epoch.isBlank() || record.epoch != page.epoch) return next
        // An exact alias keeps a read identity, but it never grants the prefix
        // authority of on-screen canonical visibility (visible()). The watermark
        // advances only along consecutive incoming sequences this epoch has
        // already seen and stops at the first unseen, missing or duplicated
        // sequence, so a partial mark cannot acknowledge rows it never captured
        // (zerus-a4mub.3).
        val rows = canonical.filter(ConversationReadPolicies::incoming)
            .mapNotNull { row -> row.incomingSeq?.let { it to row.id } }
            .groupBy({ it.first }, { it.second })
        val seen = record.seen.toSet()
        var through = record.readThrough
        while (through < Long.MAX_VALUE) {
            val ids = rows[through + 1] ?: break
            if (ids.size != 1 || ids[0] !in seen) break
            through += 1
        }
        return next.acknowledged(page.target, through)
    }
}
