package app.zerus.mobile
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
class ConversationSourceTest {
    private fun row(kind: String) = JSONObject().put("client_context",JSONObject().put("schema_version",1).put("kind",kind).put("evidence","explicit_registration"))
    @Test fun declaredSurfacesHaveDifferentLabels() {
        assertEquals(setOf("ChatGPT Chat","ChatGPT Work","Codex Chat"),setOf("chatgpt_chat","chatgpt_work","codex_chat").map { ConversationSource.label("external",row(it)) }.toSet())
    }
    @Test fun nativeCodexIsNotGuessedFromModelName() {
        assertEquals("",ConversationSource.label("codex",row("chatgpt_work").put("model","codex")))
        assertEquals("External - unknown source",ConversationSource.label("external",JSONObject().put("model","gpt-6")))
    }
    @Test fun malformedAndFutureMetadataFailsClosed() {
        val value=row("chatgpt_chat")
        for(bad in listOf<Any>(true,2,"1")) {value.getJSONObject("client_context").put("schema_version",bad);assertEquals("",ConversationSource.kind("external",value))}
    }
    @Test fun unsafeLinksAreRefused() {
        for(bad in listOf("file:///private","javascript:alert(1)","https://chatgpt.com.evil.test/c/11111111-2222-4333-8444-555555555555","https://chatgpt.com/c/11111111-2222-4333-8444-555555555555?token=x"))
            assertEquals("",ConversationSource.chatUrl(JSONObject().put("external_url",bad)))
        assertTrue(ConversationSource.chatUrl(JSONObject().put("external_url","https://chatgpt.com/c/11111111-2222-4333-8444-555555555555")).isNotEmpty())
    }
    @Test fun queueNeverClaimsProviderDelivery() {
        assertTrue(ConversationSource.QUEUE_WARNING.contains("does not send"));assertFalse(ConversationSource.cooperative(JSONObject().put("agent","external")))
        val verified=row("chatgpt_chat").put("agent","external").put("backend","external-workstream").put("external_protocol","cooperative-inbox-v1")
            .put("send_delivery","cooperative_inbox").put("external_cloud_control",false).put("tracked",true)
            .put("send_available",true).put("send_supported",true).put("run_id","exact-run").put("conversation_id","exact-conversation")
        assertTrue(ConversationSource.cooperative(verified));assertEquals("",NativeParser.messageBlockReason(verified))
        verified.put("external_protocol","future")
        assertFalse(ConversationSource.cooperative(verified));assertTrue(NativeParser.messageBlockReason(verified).contains("external inbox"))
    }
}
