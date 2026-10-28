package app.zerus.mobile

import org.json.JSONObject

/** Declared source, independent of model/auth. Unknown data never becomes Work. */
object ConversationSource {
    private val kinds = setOf("chatgpt_chat", "chatgpt_work", "codex_chat")
    fun kind(agent: String, raw: JSONObject?): String {
        if (agent != "external") return ""
        val context = raw?.optJSONObject("client_context") ?: return ""
        val schema = context.opt("schema_version")
        if (schema !is Number || schema.toDouble() != 1.0 || context.opt("evidence") != "explicit_registration") return ""
        return (context.opt("kind") as? String).orEmpty().takeIf { it in kinds }.orEmpty()
    }
    fun label(agent: String, raw: JSONObject?): String = when (kind(agent, raw)) {
        "chatgpt_chat" -> "ChatGPT Chat"
        "chatgpt_work" -> "ChatGPT Work"
        "codex_chat" -> "Codex Chat"
        else -> if (agent == "external") "External - unknown source" else ""
    }
    fun cooperative(raw: JSONObject?): Boolean = raw?.opt("agent") == "external" && raw.opt("backend") == "external-workstream"
        && raw.opt("external_protocol") == "cooperative-inbox-v1" && raw.opt("send_delivery") == "cooperative_inbox"
        && raw.opt("external_cloud_control") == false && kind("external",raw).isNotEmpty()
        && raw.opt("tracked") == true && raw.opt("send_available") == true && raw.opt("send_supported") == true
        && (raw.opt("run_id") as? String).orEmpty().isNotEmpty() && (raw.opt("conversation_id") as? String).orEmpty().isNotEmpty()
    const val QUEUE_WARNING = "External inbox only. This does not send to or wake ChatGPT. The agent must poll and acknowledge the instruction through Desktop Commander."
    fun chatUrl(raw: JSONObject?): String {
        val value = (raw?.opt("external_url") as? String).orEmpty()
        return value.takeIf { it.matches(Regex("https://chatgpt[.]com/c/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")) }.orEmpty()
    }
}
