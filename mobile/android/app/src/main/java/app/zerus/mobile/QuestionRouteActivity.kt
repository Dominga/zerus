package app.zerus.mobile

import org.json.JSONObject

/** Read-only child route outcomes from the computer's bounded inspection.
 * Shared with desktop and iOS through question-route-outcome-contract.json.
 * No receipt, answer, retry or outgoing identity is created here. */
object QuestionRouteActivity {
    const val source = "question_route_outcome"
    private fun exact(value: JSONObject, vararg keys: String) = value.keys().asSequence().toSet() == keys.toSet()
    private fun uuid(value: Any?) = value is String && value != "00000000-0000-0000-0000-000000000000" &&
        value.matches(Regex("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"))
    private fun text(value: Any?, bytes: Int) = value is String && value.isNotEmpty() && value.toByteArray(Charsets.UTF_8).size <= bytes
    private fun number(value: Any?, min: Long, max: Long): Boolean = (value is Int || value is Long) && (value as Number).toLong() in min..max

    fun label(line: JSONObject, sessionId: String): String? {
        if (!exact(line,"request_id","at","role","child","parent","question","outcome") || line.opt("role") != "child" ||
            !uuid(line.opt("request_id")) || !number(line.opt("at"),1,9_007_199_254_740_991)) return null
        val child = line.optJSONObject("child") ?: return null
        val parent = line.optJSONObject("parent") ?: return null
        val question = line.optJSONObject("question") ?: return null
        if (!exact(child,"name","reference","session_id") || !text(child.opt("name"),512) || !uuid(child.opt("session_id")) ||
            child.opt("session_id") != sessionId || !text(child.opt("reference"),1024) || !child.string("reference").startsWith("zerus://session?") ||
            !exact(parent,"name","session_id") || !text(parent.opt("name"),512) || !uuid(parent.opt("session_id")) ||
            parent.opt("session_id") == child.opt("session_id") || !exact(question,"kind","questions","options","preview") ||
            question.opt("kind") !in setOf("single-choice","multiple-choice","free-text","multi-part") ||
            !number(question.opt("questions"),1,0xffffffffL) || !number(question.opt("options"),0,0xffffffffL)) return null
        val preview = question.opt("preview") as? String ?: return null
        if (preview.codePointCount(0,preview.length)>300 || preview.any(Char::isISOControl)) return null
        val name = parent.string("name").substringAfterLast('/')
        return when(line.opt("outcome")) {
            "not_sent" -> "Question not sent to $name"
            "uncertain" -> "Question delivery to $name uncertain"
            else -> null
        }
    }

    private fun submitted(line: JSONObject, sessionId: String): Boolean {
        val normalized = JSONObject(line.toString()).put("outcome","not_sent")
        if (line.opt("role") == "parent") {
            if (line.optJSONObject("parent")?.opt("session_id") != sessionId || !text(line.opt("text"),2048)) return false
            normalized.remove("text")
            normalized.put("role","child")
            return label(normalized,line.optJSONObject("child")?.string("session_id").orEmpty()) != null
        }
        return label(normalized,sessionId) != null
    }

    fun events(raw: JSONObject): List<Event> {
        if (raw.string("agent_id") !in listOf("","main") || raw.string("archive_id").isNotEmpty()) return emptyList()
        val lines = raw.optJSONArray("question_route_activity") ?: return emptyList()
        if (lines.length()>20) return emptyList()
        val parsed = (0 until lines.length()).map { i ->
            val line = lines.optJSONObject(i) ?: return emptyList()
            // Submitted routes keep their existing presentation; these are the
            // new failure outcomes only, never another session's message.
            if (!line.has("outcome")) {
                if (!submitted(line,raw.string("session_id"))) return emptyList()
                return@map null
            }
            val label = label(line,raw.string("session_id")) ?: return emptyList()
            val at = line.getDouble("at")/1000
            Event("route-${line.string("request_id")}", "System", listOf(label,line.getJSONObject("question").string("preview"),
                line.getJSONObject("child").string("reference")).filter(String::isNotEmpty).joinToString("\n"), at.toString(), at, source=source)
        }
        return parsed.filterNotNull()
    }
}
