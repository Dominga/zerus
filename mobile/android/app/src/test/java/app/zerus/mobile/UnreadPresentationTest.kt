package app.zerus.mobile

import org.junit.Assert.*
import org.junit.Test

class UnreadPresentationTest {
    @Test fun openingRestoresCanonicalDataAnchorsThroughRetainedPresentationAliases() {
        val native = Event("native", "Agent", "Reply", "")
        val canonical = native.copy(id = "history:canonical", originalId = native.id)
        val previous = ConversationRowKeys.reconcile("target", null, listOf(native))
        val rows = ConversationRowKeys.reconcile("target", previous, listOf(canonical))
        val keys = ConversationViewport.keys(listOf(canonical), eventKey = { rows.key(it.id) })
        val saved = SavedConversationViewport(canonical.id, 37)
        val memory = ConversationAnchor("event:" + canonical.id, 37, false)
        for (remembered in listOf(null, memory)) {
            val opened = UnreadPresentation.opening(remembered, saved, "", keys, rows::key)
            assertFalse(opened.unavailableSaved)
            assertEquals(ConversationAnchor("event:native", 37, false), opened.anchor)
        }
        assertEquals(canonical.id, saved.eventId)
    }

    private val keys = listOf("history:status","event:old","history:unread","event:new","history:tail")
    @Test fun exactTotalsAndKnownLowerBoundsAreDifferent() {
        assertEquals("12",UnreadPresentation.badge(12,3)!!.text)
        assertEquals("3+",UnreadPresentation.badge(null,3)!!.text)
        assertNull(UnreadPresentation.badge(0,3))
        assertNull(UnreadPresentation.badge(null,0))
        assertEquals("?",UnreadPresentation.badge(null,null,true)!!.text)
    }
    @Test fun recentReadsDoNotReuseAnUnverifiedHistoricalCount() {
        val target=Target("computer","session","run","conversation","workspace")
        val reply=Event("native:shown","AgentMessage","Synthetic reply","",nativeType="AgentMessage")
        val before=ConversationReadState().head(target,"canonical-epoch",34,34,true)
        val read=before.visible(target,listOf(reply))
        val record=read.record(target)!!
        assertEquals(0L,record.readThrough)
        assertEquals(34L,record.headIncoming)
        assertEquals(34,ConversationReadPolicies.lowerBound(record,listOf(reply)))
        val loaded=listOf(reply).count { ConversationReadPolicies.unread(record,it) }
        assertEquals(0,loaded)
        assertNull(UnreadPresentation.conversationBadge(null,loaded))
        // Presentation does not acknowledge historical messages or manufacture a watermark.
        assertEquals(0L,read.record(target)!!.readThrough)
    }
    @Test fun genuinelyUnseenLoadedRepliesKeepTheirLowerBoundNearTheTail() {
        val badge=UnreadPresentation.conversationBadge(null,34)!!
        assertEquals("34",badge.text)
        assertEquals("34 unread in loaded messages",badge.label)
        assertTrue(badge.description.contains("conversation total is not verified"))
        assertEquals("34+",UnreadPresentation.badge(null,34)!!.text)
        assertEquals("Latest",UnreadPresentation.latest().text)
        assertEquals("Jump to latest messages",UnreadPresentation.latest().description)
        assertFalse(UnreadPresentation.latest().description.contains(badge.text))
    }
    @Test fun currentCanonicalEvidenceSupersedesLoadedSubsetCounts() {
        assertNull(UnreadPresentation.conversationBadge(0,3))
        assertEquals("2",UnreadPresentation.conversationBadge(2,0)!!.text)
        assertNull(UnreadPresentation.conversationBadge(null,0))
        assertNull(UnreadPresentation.conversationBadge(null,null))
        assertEquals("Loading latest…",UnreadPresentation.latest(true).text)
    }
    @Test fun savedReaderAnchorHasPriorityOverUnreadBoundary() {
        val position = UnreadPresentation.opening(null,SavedConversationViewport("old",37),"new",keys)
        assertEquals("event:old",position.anchor!!.itemKey)
        assertEquals(37,position.anchor.offset)
        assertFalse(position.unavailableSaved)
    }
    @Test fun firstUnreadIsShownWhenNoReadingAnchorExists() {
        assertEquals("history:unread",UnreadPresentation.opening(null,null,"new",keys).anchor!!.itemKey)
        assertNull(UnreadPresentation.opening(null,null,"",keys.filterNot { it == "history:unread" }).anchor)
    }
    @Test fun deliberateMemoryPositionWinsOverDurableOlderPosition() {
        val memory = ConversationAnchor("event:new",17,false)
        assertEquals(memory,UnreadPresentation.opening(memory,SavedConversationViewport("old",10),"new",keys).anchor)
    }
    @Test fun followingSavedTailDoesNotRestoreAnOldOrMissingMessageAnchor() {
        for(id in listOf("old","missing")) {
            val saved=SavedConversationViewport(id,30,"old-cursor","epoch",following=true)
            val opening=UnreadPresentation.opening(null,saved,"",keys)
            assertNull(opening.anchor)
            assertFalse(opening.unavailableSaved)
            assertEquals("history:unread",UnreadPresentation.opening(null,saved,"new",keys).anchor!!.itemKey)
        }
        val deliberate=ConversationAnchor("event:old",7,false)
        assertEquals(deliberate,UnreadPresentation.opening(deliberate,SavedConversationViewport("new",0,following=true),"",keys).anchor)
    }
    @Test fun missingSavedPositionIsExplicitAndNeverFallsToTail() {
        val withUnread = UnreadPresentation.opening(null,SavedConversationViewport("missing",10),"new",keys)
        assertTrue(withUnread.unavailableSaved)
        assertEquals("history:unread",withUnread.anchor!!.itemKey)
        val noUnread = UnreadPresentation.opening(null,SavedConversationViewport("missing",10),"",keys.filterNot { it == "history:unread" })
        assertEquals("event:old",noUnread.anchor!!.itemKey)
        assertTrue(noUnread.unavailableSaved)
    }
}
