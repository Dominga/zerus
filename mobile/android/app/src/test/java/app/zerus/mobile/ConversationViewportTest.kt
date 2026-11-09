package app.zerus.mobile

import org.junit.Assert.*
import org.junit.Test

class ConversationViewportTest {
    private val keys = listOf("status", "event:first", "event:last", "tail")
    @Test fun firstOpenGoesDirectlyToTail() {
        val request = ConversationViewport.content(ConversationFollowState(), keys, "", null)
        assertEquals(keys.lastIndex, request.index)
        assertTrue(request.state.opened)
        assertTrue(request.state.following)
    }
    @Test fun reopeningRestoresExactMessageAndOffsetAcrossPrependedRows() {
        val anchor = ConversationAnchor("event:first", 42, false)
        val request = ConversationViewport.content(ConversationFollowState(following = false, anchor = anchor),
            listOf("earlier", "status", "event:first", "event:last", "tail"), "", null)
        assertEquals(2, request.index)
        assertEquals(42, request.offset)
        assertFalse(request.state.following)
    }
    @Test fun missingExpiredAnchorFallsBackToLatestRatherThanWrongOldIndex() {
        val request = ConversationViewport.content(ConversationFollowState(anchor = ConversationAnchor("expired", 42, false)), keys, "", null)
        assertEquals(keys.lastIndex, request.index)
        assertEquals(0, request.offset)
        assertTrue(request.state.following)
    }
    @Test fun IncomingMessagesDoNotMoveUserReadingOlderHistory() {
        val reading = ConversationFollowState(opened = true, following = false)
        assertNull(ConversationViewport.content(reading, keys + "new-tail", "", null).index)
        assertEquals(reading, ConversationViewport.content(reading, keys, "", null).state)
    }
    @Test fun ownSendForcesItsOptimisticMessageIntoViewEvenWhenReadingOlderHistory() {
        val reading = ConversationFollowState(opened = true, following = false, ownSendId = "old")
        val request = ConversationViewport.content(reading, keys, "new", "event:last")
        assertEquals(2, request.index)
        assertTrue(request.state.following)
        assertEquals("new", request.state.ownSendId)
    }
    @Test fun pendingOwnSendSignalWaitsForItsDurableOptimisticRow() {
        val reading = ConversationFollowState(opened = true, following = false, ownSendId = "old")
        val pending = ConversationViewport.content(reading, keys, "new", "not-yet-present")
        assertNull(pending.index)
        assertEquals("old", pending.state.ownSendId)
        assertNotNull(ConversationViewport.content(pending.state, keys + "not-yet-present", "new", "not-yet-present").index)
    }
    @Test fun jumpAndUserScrollSetFollowingIntentExplicitly() {
        val following = ConversationViewport.latest(ConversationFollowState(opened = true, following = false))
        assertTrue(following.following)
        assertNull(ConversationViewport.content(following, keys, "", null).index)
        assertFalse(ConversationViewport.userScrolled(following, false).following)
        assertTrue(ConversationViewport.userScrolled(following, true).following)
    }
    @Test fun metadataOnlyRefreshDoesNotRepeatTheOpeningOrOwnSendScroll() {
        val opened = ConversationViewport.content(ConversationFollowState(), keys, "", null)
        assertEquals(keys.lastIndex, opened.index)
        assertNull(ConversationViewport.content(opened.state, keys.toList(), "", null).index)
        val sent = ConversationViewport.content(opened.state, keys, "own", "event:last")
        assertEquals(2, sent.index)
        val acknowledged = ConversationViewport.content(sent.state, keys.toList(), "own", "event:last")
        assertNull(acknowledged.index)
        assertEquals(sent.state, acknowledged.state)
        assertTrue(acknowledged.state.following)
    }
    @Test fun appendedContentDefersFollowingUntilItsActualLayoutIsMeasured() {
        val following = ConversationFollowState(opened = true, following = true, ownSendId = "own")
        val expanded = listOf("status", "event:first", "event:last", "event:new", "tail")
        val update = ConversationViewport.content(following, expanded, "own", "event:last")
        assertNull(update.index)
        assertEquals(following, update.state)
        // A same-key text expansion has the same contract; content() cannot
        // know the new row height before LazyColumn measures it.
        assertNull(ConversationViewport.content(update.state, expanded.toList(), "own", "event:last").index)
    }
    @Test fun inheritedDividersUseTheSameIndicesAsTheRenderedMessagesAndTail() {
        val events = listOf(
            Event("a", "You", "Source A", "", inheritedFrom = "source-a"),
            Event("b", "AgentMessage", "Same source", "", inheritedFrom = "source-a"),
            Event("c", "You", "Source B", "", inheritedFrom = "source-b"),
            Event("d", "You", "Own message", "")
        )
        val rendered = ConversationViewport.keys(events, unreadBoundary = "b", hasWarning = true,
            hasBoundedNotice = true, queueVisible = true, questionsVisible = true)
        assertEquals(listOf("history:earlier", "history:status", "history:warning", "history:bounded",
            "history:missing-anchor", "history:inherited:a", "event:a", "history:unread", "event:b",
            "history:inherited:c", "event:c", "history:inherited:d", "event:d", "history:newer",
            "history:queue", "history:questions", "history:tail"), rendered)
        val own = ConversationViewport.content(ConversationFollowState(opened = true, following = false),
            rendered, "new-send", "event:d")
        assertEquals(12, own.index)
        assertEquals("event:d", rendered[own.index!!])
        assertEquals(rendered.lastIndex, ConversationViewport.loadedLatest(own.state, rendered, false, "").index)
    }
    @Test fun emptyAndOrdinaryRowsHaveNoPhantomInheritedSeparator() {
        assertEquals(listOf("history:earlier", "history:status", "history:missing-anchor",
            "history:empty", "history:newer", "history:tail"), ConversationViewport.keys(emptyList()))
        assertFalse(ConversationViewport.keys(emptyList(), conversationLoading = true).contains("history:empty"))
        val ordinary = ConversationViewport.keys(listOf(Event("own", "You", "Own message", "")))
        assertEquals(listOf("history:earlier", "history:status", "history:missing-anchor",
            "event:own", "history:newer", "history:tail"), ordinary)
    }
    @Test fun shortRestoredConversationFollowsWhenTailIsAlreadyFullyVisible() {
        val reading = ConversationFollowState(opened = true,following = false)
        assertTrue(ConversationViewport.settled(reading,true,false).following)
        assertFalse(ConversationViewport.settled(reading,true,true).following)
        assertFalse(ConversationViewport.settled(reading,false,false).following)
        assertFalse(ConversationViewport.settled(reading.copy(opened = false),true,false).following)
    }
    @Test fun emptyContentDoesNotConsumeFirstOpenOrSendSignal() {
        val waiting = ConversationFollowState()
        assertEquals(ConversationScroll(waiting), ConversationViewport.content(waiting, emptyList(), "new", "new"))
    }
    @Test fun completedExplicitLatestReadScrollsEvenWhenTheLoadedKeysDidNotChange() {
        val reading = ConversationFollowState(opened = true, following = false, ownSendId = "retained-send",
            anchor = ConversationAnchor("event:first", 42, false))
        assertNull(ConversationViewport.content(reading, keys, reading.ownSendId, null).index)
        val result = ConversationViewport.loadedLatest(reading, keys, false, "")
        assertEquals(keys.lastIndex, result.index)
        assertTrue(result.state.opened)
        assertTrue(result.state.following)
        assertNull(result.state.anchor)
        assertEquals("retained-send", result.state.ownSendId)
        assertEquals("", result.failure)
    }
    @Test fun cachedMiddlePageDoesNotPretendItReachedLatestOrDiscardReadingAnchor() {
        val reading = ConversationFollowState(opened = true, following = false,
            anchor = ConversationAnchor("event:first", 42, false))
        val result = ConversationViewport.loadedLatest(reading, keys, true, "")
        assertNull(result.index)
        assertEquals(reading, result.state)
        assertTrue(result.failure.contains("latest messages"))
        assertTrue(result.failure.contains("Refresh"))
    }
    @Test fun failedExplicitLatestReadKeepsItsSpecificFailureAndReadingPosition() {
        val reading = ConversationFollowState(opened = true, following = false,
            anchor = ConversationAnchor("event:first", 42, false))
        val failure = "Message history is still indexing. Try again shortly."
        val result = ConversationViewport.loadedLatest(reading, keys, true, failure)
        assertNull(result.index)
        assertEquals(reading, result.state)
        assertTrue(result.failure.endsWith(failure))
        val retried = ConversationViewport.loadedLatest(result.state, keys + "new-tail", false, "")
        assertEquals(keys.size, retried.index)
        assertTrue(retried.state.following)
    }
    @Test fun emptyLatestResultRemainsAnActionableFailureRatherThanConsumingOpening() {
        val unopened = ConversationFollowState()
        val result = ConversationViewport.loadedLatest(unopened, emptyList(), false, "")
        assertEquals(unopened, result.state)
        assertNull(result.index)
        assertTrue(result.failure.contains("No messages"))
    }
}
