package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class ConversationReadHeadTest {
    private val target = Target("computer", "session", "run", "conversation", "workspace")
    private val native = Event("provider:reply", "AgentMessage", "Latest reply", "", 1.0,
        source = "codex_transcript", nativeType = "AgentMessage")
    private fun canonical(sequence: Long = 74) = native.copy(id = "history:reply", historyId = "reply",
        historyCursor = "cursor:reply", historyEpoch = "epoch", incomingSeq = sequence,
        originalId = native.id, originalIds = listOf(native.id))
    private fun earlier(sequence: Long): Event {
        val nativeId = "provider:earlier-$sequence"
        return Event(nativeId, "AgentMessage", "Earlier reply $sequence", "", 1.0, source = "codex_transcript", nativeType = "AgentMessage")
            .copy(id = "history:earlier-$sequence", historyId = "earlier-$sequence", historyCursor = "cursor:earlier-$sequence",
                historyEpoch = "epoch", incomingSeq = sequence, originalId = nativeId, originalIds = listOf(nativeId))
    }
    private fun page(vararg rows: Event, total: Long = 74, target: Target = this.target) =
        HistoryPage(target, "epoch", rows.toList(), null, null, false, false, total, total, true, false, false, JSONObject())
    private fun readThrough63() = ConversationReadState().head(target, "epoch", 63, 63, true).visible(target, listOf(earlier(63)))

    @Test fun aPartialMarkedNativeReplyNeverExpandsTheCanonicalPrefix() {
        // zerus-a4mub.3: readThrough 63, and a non-authoritative Mark loaded
        // replies read captured only the loaded native reply 74. It must not
        // acknowledge the uncaptured 64..73 when the exact canonical alias
        // arrives; the alias itself keeps this reply read.
        val partial = ReadAttentionPolicies.mark(readThrough63(),
            ReadAttentionEvidence(target, "", "epoch", 63, false, setOf(native.id), null))
        assertEquals(63L, partial.record(target)!!.readThrough)
        assertTrue(partial.record(target)!!.seen.contains(native.id))
        val reconciled = ConversationReadHead.observe(partial, page(canonical()))
        assertEquals(63L, reconciled.record(target)!!.readThrough)
        assertTrue(reconciled.record(target)!!.seen.contains("history:reply"))
        assertEquals(11, ConversationReadPolicies.lowerBound(reconciled.record(target)!!, listOf(canonical())))
        assertEquals(reconciled, ConversationReadPolicies.decode(ConversationReadPolicies.encode(reconciled)))
    }

    @Test fun aViewedNativePositionCompletesOnlyAProvedPrefix() {
        // The exact native reply 74 was read on screen and 73 was canonically
        // visible: the consecutive prefix is proved and the alias completes it.
        val read = ConversationReadState().head(target, "epoch", 73, 73, true)
            .visible(target, listOf(earlier(73))).visible(target, listOf(native))
        assertEquals(74L, ConversationReadHead.observe(read, page(earlier(73), canonical())).record(target)!!.readThrough)
        // Without the proved 73 the same viewed position cannot invent 64..73.
        val unproved = ConversationReadState().head(target, "epoch", 63, 63, true)
            .visible(target, listOf(earlier(63))).visible(target, listOf(native))
        assertEquals(63L, ConversationReadHead.observe(unproved, page(canonical())).record(target)!!.readThrough)
    }

    @Test fun directCanonicalVisibilityStillAcknowledgesThePrecedingPrefix() {
        // docs/mobile-architecture 899-906: a visible canonical position
        // acknowledges the incoming rows before it, and reconciliation never
        // lowers that watermark.
        val read = readThrough63().visible(target, listOf(canonical()))
        assertEquals(74L, read.record(target)!!.readThrough)
        assertEquals(74L, ConversationReadHead.observe(read, page(canonical())).record(target)!!.readThrough)
    }

    @Test fun laterRepliesAndUnseenHeadsRemainUnread() {
        val read = ConversationReadState().visible(target, listOf(native))
        val reconciled = ConversationReadHead.observe(read, page(canonical(), total = 75))
        // No earlier reply was read on this phone: the whole head stays unread.
        assertEquals(75, ConversationReadPolicies.lowerBound(reconciled.record(target), emptyList()))
        assertEquals(74, ConversationReadPolicies.lowerBound(
            ConversationReadHead.observe(ConversationReadState(), page(canonical())).record(target), emptyList()))
    }

    @Test fun aDuplicateOrAmbiguousSequenceNeverProvesThePrefix() {
        // Two canonical rows claim the same sequence 64: the walk must stop
        // instead of choosing one of them.
        val first = earlier(64)
        val second = earlier(64).copy(id = "history:second", historyId = "second", historyCursor = "cursor:second",
            originalId = "provider:second", originalIds = listOf("provider:second"))
        val seen = readThrough63().viewed(target, setOf(first.originalId, second.originalId))
        assertEquals(63L, ConversationReadHead.observe(seen, page(first, second)).record(target)!!.readThrough)
    }

    @Test fun identicalTextOrAnAmbiguousAliasNeverProvesReading() {
        val read = ConversationReadState().visible(target, listOf(native))
        for (rows in listOf(listOf(canonical().copy(originalId = "", originalIds = emptyList())),
            listOf(canonical(), canonical().copy(id = "history:other", historyId = "other", historyCursor = "cursor:other")))) {
            assertEquals(0L, ConversationReadHead.observe(read, page(*rows.toTypedArray())).record(target)!!.readThrough)
        }
    }

    @Test fun foreignScopeOrNoncanonicalMetadataCannotTransferAReadPosition() {
        val read = ConversationReadState().visible(target, listOf(native))
        for (foreign in listOf(target.copy(connectionId = "other"), target.copy(conversation = "other"),
            target.copy(agentId = "child", parentConversation = target.conversation))) {
            assertEquals(0L, ConversationReadHead.observe(read, page(canonical(), target = foreign)).record(foreign)!!.readThrough)
        }
        for (row in listOf(canonical().copy(historyEpoch = "other"), canonical().copy(historyCursor = ""))) {
            assertEquals(0L, ConversationReadHead.observe(read, page(row)).record(target)!!.readThrough)
        }
    }
}
