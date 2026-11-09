package app.zerus.mobile

data class UnreadBadge(val text: String, val description: String, val label: String = "$text unread")
data class LatestNavigation(val text: String, val description: String)
data class OpeningReadingPosition(val anchor: ConversationAnchor?, val unavailableSaved: Boolean = false)

object UnreadPresentation {
    fun badge(total: Int?, loaded: Int?, nativeUnread: Boolean = false): UnreadBadge? = when {
        total != null && total > 0 -> UnreadBadge(total.toString(), "$total unread messages in this conversation")
        total != null -> null
        loaded != null && loaded > 0 -> UnreadBadge("$loaded+", "At least $loaded unread messages in the available activity; the conversation total is not verified")
        nativeUnread -> UnreadBadge("?", "The computer reports an unread reply; the total is not reported")
        else -> null
    }
    /** Loaded unread rows are an exact subset count, never the conversation total or a scroll distance. */
    fun conversationBadge(total: Int?, loaded: Int?): UnreadBadge? = if (total != null) badge(total, loaded)
        else loaded?.takeIf { it > 0 }?.let {
            UnreadBadge(it.toString(), "$it unread messages in loaded activity; the conversation total is not verified", "$it unread in loaded messages")
        }
    /** Navigation never turns a conversation-wide unread count into a distance below the viewport. */
    fun latest(loading: Boolean = false) = if (loading) LatestNavigation("Loading latest…", "Loading latest messages")
        else LatestNavigation("Latest", "Jump to latest messages")
    fun opening(memory: ConversationAnchor?, saved: SavedConversationViewport?, boundary: String, keys: List<String>): OpeningReadingPosition {
        val remembered = memory?.takeIf { !it.following } ?: saved?.takeUnless { it.following }?.let { ConversationAnchor("event:" + it.eventId,it.offset,false) }
        if(remembered != null && remembered.itemKey in keys) return OpeningReadingPosition(remembered)
        if(boundary.isNotBlank() && "history:unread" in keys) return OpeningReadingPosition(ConversationAnchor("history:unread",0,false),remembered != null)
        // A missing saved position never silently drops the reader at the newest message.
        return if(remembered != null) OpeningReadingPosition(keys.firstOrNull { it.startsWith("event:") }?.let { ConversationAnchor(it,0,false) },true)
            else OpeningReadingPosition(null)
    }
}
