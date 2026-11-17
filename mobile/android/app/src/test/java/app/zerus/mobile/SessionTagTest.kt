package app.zerus.mobile

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SessionTagTest {
    @Test fun newTagsRejectShellSyntaxButLegacyTargetsStayUsable() {
        val names = JSONObject(javaClass.classLoader!!.getResource("session-tag-contract.json")!!.readText())
        val rejected = names.getJSONArray("rejected")
        for (i in 0 until rejected.length()) {
            val tag = rejected.getString(i)
            assertTrue(tag, LaunchPresentation.nameError(tag).isNotBlank())
            for ((op,key,value) in listOf(Triple("fork","tag",tag),Triple("rename","new_name","codex/project/$tag"))) {
                assertThrows(tag, IllegalArgumentException::class.java) { SessionActionPolicies.arguments(op,JSONObject().put(key,value)) }
            }
        }
        val accepted = names.getJSONArray("accepted")
        for (i in 0 until accepted.length()) assertEquals("", LaunchPresentation.nameError(accepted.getString(i)))
        val target = Target("computer","codex/project/old\$tag","run","conversation","workspace")
        assertEquals("", SessionActionPolicies.inspectReason(target,"rename",JSONObject().put("new_name","codex/project/safe"),JSONObject().put("allowed_actions",org.json.JSONArray().put("rename"))))
    }
}
