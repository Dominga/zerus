package app.zerus.mobile

/** A resolved empty inspection is content too: polling must not restore the cold placeholder. */
object ConversationLoadPresentation {
    data class Status(val text: String, val initial: Boolean, val spinner: Boolean)
    data class Banner(val text: String, val detail: String, val spinner: Boolean)

    fun status(hasContent: Boolean, detailLoading: Boolean, detailProgress: Boolean,
        historyLoading: Boolean, historyProgress: Boolean, demo: Boolean, resolved: Boolean = false,
        opening: Boolean = false): Status? {
        if (demo || !detailLoading && !historyLoading) return null
        val progress = opening || detailLoading && detailProgress || historyLoading && historyProgress
        val initial = !hasContent && !resolved
        if (!initial && !progress) return null
        val text = when {
            initial -> "Loading conversation…"
            historyLoading && detailLoading -> "Checking activity and messages…"
            historyLoading -> "Loading message history…"
            else -> "Checking latest activity…"
        }
        return Status(text, initial, progress)
    }

    /** Always occupies the same place above the history, including after a read settles. */
    fun banner(status: Status?, hasContent: Boolean, verified: Boolean, historyReady: Boolean,
        demo: Boolean): Banner? {
        if (demo) return null
        if (status != null) return Banner(status.text, when {
            !hasContent -> "Your conversation will appear here."
            !verified -> "Showing saved messages."
            else -> "Loaded messages remain available."
        }, status.spinner)
        return when {
            verified -> Banner("Latest activity checked", if (historyReady) "Message history loaded." else "Showing available messages.", false)
            hasContent -> Banner("Showing saved messages", "Refresh to check for updates.", false)
            else -> Banner("Conversation not yet loaded", "Refresh to check for updates.", false)
        }
    }

    fun readAllOutcome(marked: Int, skipped: Int, partial: Int): String = buildList {
        add(if (marked == 0) "No conversations marked read." else
            "${if (marked == 1) "1 conversation" else "$marked conversations"} marked read on this phone.")
        if (partial > 0) add("$partial marked loaded replies only.")
        if (skipped > 0) add("$skipped could not be checked.")
    }.joinToString(" ")
}
