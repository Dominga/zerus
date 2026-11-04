package app.zerus.mobile

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** zerus-b4qk.1: a fork's history begins with the rows it inherited from its source. */
class ForkInheritedHistoryTest {
    private val target = Target("computer", "codex/project/branch", "run", "conversation", "workspace")
    private fun row(seq: Int, type: String, detail: String, extra: Map<String, Any> = emptyMap()) = JSONObject(mapOf(
        "type" to type, "detail" to detail, "at" to seq.toDouble(), "seq" to seq, "agent_id" to "",
        "history_id" to "id$seq", "history_cursor" to "cursor$seq", "history_stream" to "journal") + extra)
    private fun inherited(seq: Int, type: String, detail: String) =
        row(seq, "Inherited", detail, mapOf("inherited_type" to type, "inherited_from" to "codex/project/root"))
    private fun page(vararg rows: JSONObject) = JSONObject().put("history_epoch", "epoch").put("events", JSONArray(rows.toList()))

    @Test fun inheritedRowsParseAsReadOnlyHistoryOfTheirSource() {
        val events = NativeParser.events(page(
            inherited(1, "UserPromptSubmit", "source request"),
            inherited(2, "Stop", "source reply"),
            row(10, "UserPromptSubmit", "own request"),
            row(11, "Stop", "own reply", mapOf("incoming_seq" to 1)),
        ))
        assertEquals(listOf("source request", "source reply", "own request", "own reply"), events.map { it.text })
        assertEquals(listOf("codex/project/root", "codex/project/root", "", ""), events.map { it.inheritedFrom })
        // The original type decides the role; nothing acts as this session's own send.
        assertEquals(listOf("UserPromptSubmit", "Stop"), events.take(2).map { it.nativeType })
        assertEquals(listOf("You", "AgentMessage"), events.take(2).map { it.role })
        assertTrue(events.take(2).all { it.requestId.isEmpty() && it.delivery.isEmpty() })
    }

    @Test fun anInheritedRowWithoutASourceOrWithAnUnknownTypeIsDropped() {
        val events = NativeParser.events(page(
            row(1, "Inherited", "no source", mapOf("inherited_type" to "Stop")),
            row(2, "Inherited", "tool", mapOf("inherited_type" to "PreToolUse", "inherited_from" to "codex/project/root")),
            row(3, "Stop", "own reply"),
        ))
        assertEquals(listOf("own reply"), events.map { it.text })
    }

    @Test fun inheritedRepliesAreNeverIncomingOrUnread() {
        val events = NativeParser.events(page(inherited(1, "Stop", "source reply"), row(11, "Stop", "own reply", mapOf("incoming_seq" to 1))))
        val (source, own) = events
        assertFalse(ConversationReadPolicies.incoming(source))
        assertTrue(ConversationReadPolicies.incoming(own))
        val state = ConversationReadState().head(target, "epoch", 1, 1, true)
        assertFalse(ConversationReadPolicies.unread(state.record(target), source))
        assertEquals(1, ConversationReadPolicies.lowerBound(state.record(target), events))
        // Seeing an inherited reply records nothing for this session.
        val seen = ConversationReadState().visible(target, listOf(source))
        assertTrue(seen.record(target)?.seen.orEmpty().isEmpty())
    }

    @Test fun theDisplayCacheKeepsTheMarkAndReadsOlderRowsWithoutIt() {
        val events = NativeParser.events(page(inherited(1, "Stop", "source reply")))
        val decoded = EventHistoryCodec.decode(EventHistoryCodec.encode(events))
        assertEquals("codex/project/root", decoded.single().inheritedFrom)
        val older = EventHistoryCodec.encode(events).also { (it.getJSONObject(0)).remove("inherited_from") }
        assertEquals("", EventHistoryCodec.decode(older).single().inheritedFrom)
    }
}
