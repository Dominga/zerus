package app.zerus.mobile

import org.junit.Assert.*
import org.junit.Test

class ConversationRowKeysTest {
    private fun row(id: String, text: String = "Repeated text") = Event(id, "AgentMessage", text, "", at = 1.0)

    @Test fun demonstratedHookToProviderReplacementKeepsItsCompositionKey() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("hook")))
        val replacement = row("provider", "Full provider reply")
        val next = ConversationRowKeys.reconcile("target", initial, listOf(replacement), mapOf("hook" to "provider"))
        assertEquals("event:hook", next.key("provider"))
        assertEquals("provider", next.eventIdByKey["event:hook"])
        assertEquals(setOf("provider"), next.keyByEventId.keys)
        assertEquals("provider", replacement.id)
    }

    @Test fun canonicalOriginalIdentitiesKeepTheExistingRowAcrossFurtherRefreshes() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("native")))
        for (canonical in listOf(row("history:canonical").copy(originalId = "native"),
            row("history:canonical").copy(originalIds = listOf("native")))) {
            val next = ConversationRowKeys.reconcile("target", initial, listOf(canonical))
            assertEquals("event:native", next.key(canonical.id))
            val refreshed = ConversationRowKeys.reconcile("target", next, listOf(canonical.copy(text = "Expanded reply")))
            assertEquals(next, refreshed)
        }
    }

    @Test fun combinedHookProviderCanonicalChainKeepsTheOriginalCompositionKey() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("hook")))
        val next = ConversationRowKeys.reconcile("target", initial, listOf(row("history:canonical")),
            mapOf("hook" to "provider", "provider" to "history:canonical"))
        assertEquals("event:hook", next.key("history:canonical"))
        assertEquals(mapOf("event:hook" to "history:canonical"), next.eventIdByKey)
    }

    @Test fun cyclicAndOverlongAliasChainsDoNotEstablishPresentationIdentity() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("start")))
        val cyclic = ConversationRowKeys.reconcile("target", initial, listOf(row("middle")),
            mapOf("start" to "middle", "middle" to "start"))
        assertEquals("event:middle", cyclic.key("middle"))
        val selfCycle = ConversationRowKeys.reconcile("target", initial, listOf(row("other")), mapOf("start" to "start"))
        assertEquals("event:other", selfCycle.key("other"))
        val chain = (0..ConversationRowKeys.maxRetained).associate { index ->
            (if (index == 0) "start" else "step-$index") to "step-${index + 1}"
        }
        val last = "step-${ConversationRowKeys.maxRetained + 1}"
        val overlong = ConversationRowKeys.reconcile("target", initial, listOf(row(last)), chain)
        assertEquals("event:$last", overlong.key(last))
    }

    @Test fun currentIdentityWinsOverAliasesAndCannotBeStolenByAnotherRow() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("old")))
        val canonical = ConversationRowKeys.reconcile("target", initial, listOf(row("canonical")), mapOf("old" to "canonical"))
        val next = ConversationRowKeys.reconcile("target", canonical,
            listOf(row("canonical"), row("other").copy(originalId = "canonical")), mapOf("canonical" to "other"))
        assertEquals("event:old", next.key("canonical"))
        assertEquals("event:other", next.key("other"))
        assertEquals(2, next.eventIdByKey.size)
    }

    @Test fun ambiguousSourcesAndSuccessorsNeverMergeDistinctMessages() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("a"), row("b")))
        val combined = ConversationRowKeys.reconcile("target", initial,
            listOf(row("combined").copy(originalIds = listOf("a", "b"))))
        assertEquals("event:combined", combined.key("combined"))
        val divided = ConversationRowKeys.reconcile("target", initial,
            listOf(row("first").copy(originalId = "a"), row("second").copy(originalIds = listOf("a"))))
        assertEquals("event:first", divided.key("first"))
        assertEquals("event:second", divided.key("second"))
        assertEquals(2, divided.eventIdByKey.size)
    }

    @Test fun repeatedTextAndTimeDoNotEstablishAnyAlias() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("old")))
        val next = ConversationRowKeys.reconcile("target", initial, listOf(row("new-a"), row("new-b")))
        assertEquals("event:new-a", next.key("new-a"))
        assertEquals("event:new-b", next.key("new-b"))
    }

    @Test fun anotherTargetDoesNotReuseKeysEvenWhenOriginalIdsArePresent() {
        val initial = ConversationRowKeys.reconcile("target-a", null, listOf(row("old")))
        val next = ConversationRowKeys.reconcile("target-b", initial,
            listOf(row("new").copy(originalId = "old")), mapOf("old" to "new"))
        assertEquals("event:new", next.key("new"))
        assertEquals(mapOf("event:new" to "new"), next.eventIdByKey)
    }

    @Test fun removedRowsAreDroppedAndOverflowRowsStayVisibleWithoutGrowingMaps() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("removed"), row("kept")))
        val next = ConversationRowKeys.reconcile("target", initial, listOf(row("kept")))
        assertEquals(setOf("kept"), next.keyByEventId.keys)
        assertNull(next.eventIdByKey["event:removed"])
        val rows = (0..ConversationRowKeys.maxRetained).map { row("row-$it") }
        val bounded = ConversationRowKeys.reconcile("target", next, rows, maxRetained = Int.MAX_VALUE)
        assertEquals(ConversationRowKeys.maxRetained, bounded.keyByEventId.size)
        assertEquals(ConversationRowKeys.maxRetained, bounded.eventIdByKey.size)
        assertEquals("event:row-${ConversationRowKeys.maxRetained}", bounded.key(rows.last().id))
    }

    @Test fun aReappearingCanonicalIdCannotCollideWithAnOldPresentationAlias() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("old")))
        val canonical = ConversationRowKeys.reconcile("target", initial, listOf(row("new")), mapOf("old" to "new"))
        val next = ConversationRowKeys.reconcile("target", canonical, listOf(row("new"), row("old")), maxRetained = 1)
        assertEquals("event:new", next.key("new"))
        assertEquals("event:old", next.key("old"))
        assertEquals(mapOf("event:new" to "new"), next.eventIdByKey)
    }

    @Test fun forkDividersAndNumericPositionsUseTheSameStablePresentationIdentity() {
        val initial = ConversationRowKeys.reconcile("target", null, listOf(row("native")))
        val canonical = row("history:canonical").copy(originalId = "native", inheritedFrom = "source")
        val state = ConversationRowKeys.reconcile("target", initial, listOf(canonical))
        val keys = ConversationViewport.keys(listOf(canonical), unreadBoundary = canonical.id,
            eventKey = { state.key(it.id) })
        assertEquals(listOf("history:earlier", "history:status", "history:missing-anchor",
            "history:inherited:native", "history:unread", "event:native", "history:newer", "history:tail"), keys)
        assertEquals("history:inherited:native", ConversationViewport.inheritedKey(state.key(canonical.id)))
        assertEquals("history:canonical", state.eventIdByKey[keys[5]])
    }
}
