package app.zerus.mobile

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class QuestionRouteActivityTest {
    private val contract = javaClass.classLoader!!.getResourceAsStream("question-route-outcome-contract.json")!!.bufferedReader().use { JSONObject(it.readText()) }
    @Test fun outcomesFollowTheSharedContractAndNeverBecomeOutgoingReceipts() {
        val cases = contract.getJSONArray("activity")
        for (i in 0 until cases.length()) {
            val case = cases.getJSONObject(i)
            val line = case.getJSONObject("line")
            val label = QuestionRouteActivity.label(line,contract.getString("session_id"))
            assertEquals(case.getString("name"),case.getBoolean("accepted"),label != null)
            if (label != null) assertEquals(case.getString("label"),label)
            val raw = JSONObject().put("session_id",contract.getString("session_id")).put("question_route_activity",JSONArray().put(line))
            val events = NativeParser.events(raw)
            assertEquals(case.getString("name"),if (label == null) 0 else 1,events.size)
            events.firstOrNull()?.let {
                assertTrue(it.text.startsWith(case.getString("label")))
                assertEquals(1794599000.0,it.at,0.001)
                assertEquals("System",it.role)
                assertEquals("",it.requestId)
                assertEquals("",it.receiptRequestId)
                assertEquals("",it.delivery)
                assertEquals("",it.outgoingId)
                assertTrue(QuestionRouteActivity.events(JSONObject(raw.toString()).put("agent_id","worker")).isEmpty())
                assertTrue(QuestionRouteActivity.events(JSONObject(raw.toString()).put("archive_id","archive")).isEmpty())
            }
        }
    }
    @Test fun failedRoutesLeaveSupervisionAndNeedThePerson() {
        val cases = contract.getJSONArray("decisions")
        for (i in 0 until cases.length()) {
            val case = cases.getJSONObject(i)
            assertEquals(case.getString("state"),case.getBoolean("quiet"),ChildSessions.quiet(case.getJSONObject("row")))
        }
    }
    @Test fun noPartialListOrMoreThanTwentyLinesIsShown() {
        val good = contract.getJSONArray("activity").getJSONObject(0).getJSONObject("line")
        fun raw(lines: JSONArray) = JSONObject().put("session_id",contract.getString("session_id")).put("question_route_activity",lines)
        assertTrue(QuestionRouteActivity.events(raw(JSONArray().put(good).put(JSONObject()))).isEmpty())
        assertTrue(QuestionRouteActivity.events(raw(JSONArray(List(21){ good }))).isEmpty())
        assertEquals(20,QuestionRouteActivity.events(raw(JSONArray(List(20){ good }))).size)
    }
}
