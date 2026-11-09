package app.zerus.mobile

import org.junit.Assert.*
import org.junit.Test

class UnreadPresentationTest {
    private val keys = listOf("history:status","event:old","history:unread","event:new","history:tail")
    @Test fun exactTotalsAndKnownLowerBoundsAreDifferent() {
        assertEquals("12",UnreadPresentation.badge(12,3)!!.text)
        assertEquals("3+",UnreadPresentation.badge(null,3)!!.text)
        assertNull(UnreadPresentation.badge(0,3))
        assertNull(UnreadPresentation.badge(null,0))
        assertEquals("?",UnreadPresentation.badge(null,null,true)!!.text)
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
