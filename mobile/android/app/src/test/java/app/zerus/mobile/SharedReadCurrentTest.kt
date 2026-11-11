package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/**
 * zerus-a4mub.9: the open conversation acknowledges the current shared reply and attention from its
 * verified inspection's `current_read`, never from a catalog row, and only what it showed. The cases are
 * shared with ZerusKit (mobile/fixtures/current-read.json).
 */
class SharedReadCurrentTest {
    private fun fixture():JSONObject {
        val file=generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it,"mobile/fixtures/current-read.json") }.firstOrNull { it.isFile }
        assertNotNull("The shared fixture is part of this repository",file)
        return JSONObject(file!!.readText())
    }
    private fun target(value:JSONObject)=Target("computer","claude/project/session",value.getString("run"),value.getString("conversation"),
        "gateway",value.getString("archive_id"),value.getString("agent_id"),if(value.getString("agent_id").isBlank()) "" else value.getString("conversation"))
    private fun row(target:Target,value:JSONObject?)=value?.let {
        Session(target,"Session","claude","idle","project","",0,JSONObject().put("run_id",target.run).put("conversation_id",target.conversation)
            .put("state",it.getString("state")).put("read",it.opt("read") ?: JSONObject.NULL))
    }
    private fun parsed(current:CurrentRead?)=current?.let {
        JSONObject().put("reply_id",it.reply ?: JSONObject.NULL).put("attention_signature",it.attention ?: JSONObject.NULL)
            .put("attention_complete",it.attentionComplete)
    }
    /** Android's org.json has no toMap: keys with JSON null as Kotlin null. */
    private fun flat(value:JSONObject)=value.keys().asSequence().associateWith { key -> value.opt(key).takeUnless { it==JSONObject.NULL } }
    private fun same(expected:Any?,actual:JSONObject?,name:String) {
        if(expected==null || expected==JSONObject.NULL) return assertNull(name,actual)
        assertNotNull(name,actual)
        assertEquals(name,flat(JSONObject(expected.toString())),flat(actual!!))
    }

    @Test fun everySharedCaseParsesAndAcknowledgesExactlyWhatWasShown() {
        val fixture=fixture()
        assertEquals(1,fixture.getInt("schema"))
        val cases=fixture.getJSONArray("cases")
        assertTrue(cases.length()>=30)
        for(i in 0 until cases.length()) {
            val case=cases.getJSONObject(i);val name=case.getString("name")
            val target=target(case.getJSONObject("target"))
            val current=SharedReadPolicies.current(case.getJSONObject("inspection"),target,case.getBoolean("verified"))
            same(case.opt("current"),parsed(current),name)
            val payload=current?.let { SharedReadPolicies.currentPayload(row(target,case.optJSONObject("row")),target,it,
                case.getBoolean("reply_shown"),case.getBoolean("attention_shown")) }
            same(case.opt("payload"),payload,name)
            payload?.let { assertEquals(name,setOf("run_id","conversation_id","reply_id","attention_signature"),it.keys().asSequence().toSet()) }
        }
    }

    @Test fun everySharedViewportShowsExactlyItsReplyAndAttention() {
        val viewports=fixture().getJSONArray("viewports")
        assertTrue(viewports.length()>=8)
        for(i in 0 until viewports.length()) {
            val case=viewports.getJSONObject(i)
            val shown=SharedReadPolicies.currentShown(case.getBoolean("latest"),case.getBoolean("tail_visible"),case.getBoolean("newest_visible"),
                case.getBoolean("questions_listed"),case.getBoolean("questions_visible"))
            assertEquals(case.getString("name"),case.getBoolean("reply_shown") to case.getBoolean("attention_shown"),shown)
        }
    }

    @Test fun aCatalogRowNeverStandsInForTheDisplayedInspection() {
        val target=Target("computer","claude/project/session","run-1","conversation-1","gateway")
        // The catalog row already names a newer reply and request; the inspection on screen does not.
        val catalog=Session(target,"Session","claude","input","project","",0,JSONObject().put("run_id","run-1").put("conversation_id","conversation-1")
            .put("reply_id","8:200.5").put("attention_signature","b".repeat(64)).put("read",JSONObject.NULL))
        val inspection=JSONObject().put("run_id","run-1").put("conversation_id","conversation-1")
        assertNull(SharedReadPolicies.current(inspection,target,true))
        inspection.put("current_read",JSONObject().put("schema",1).put("run_id","run-1").put("conversation_id","conversation-1")
            .put("reply_id","7:100.5").put("attention_signature",JSONObject.NULL).put("attention_complete",true))
        val payload=SharedReadPolicies.currentPayload(catalog,target,SharedReadPolicies.current(inspection,target,true)!!,true,true)!!
        assertEquals("7:100.5",payload.getString("reply_id"))
        assertTrue(payload.isNull("attention_signature"))
        // The canonical path is unchanged: without a fresh head the catalog row sends nothing.
        assertNull(SharedReadPolicies.payload(catalog,false,null))
    }
}
