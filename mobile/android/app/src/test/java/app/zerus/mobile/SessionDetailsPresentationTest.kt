package app.zerus.mobile

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SessionDetailsPresentationTest {
    private val target = Target("computer","provider/project/tag","run","conversation","workspace")
    @Test fun recordedAgentsAndCurrentCountsFollowTheSharedContract() {
        val contract = JSONObject(checkNotNull(javaClass.getResourceAsStream("/subagent-caption-contract.json")).bufferedReader().use { it.readText() })
        contract.getJSONArray("cases").objects().forEach { example ->
            val raw = JSONObject(example.getJSONObject("raw").toString()).put("subagents", contract.getJSONObject("children"))
            val selected = target.copy(archiveId = example.optString("archive_id"))
            val before = raw.toString()
            val agents = SessionDetailsPresentation.parse(raw, selected, example.getBoolean("online")).agents.sortedBy { it.id }
            val expected = if (example.getBoolean("current")) (0 until 8).map { contract.getJSONArray("live_details_captions").getString(it) } else List(8) { "Recorded" }
            assertEquals(example.getString("name"), expected, agents.map { SessionDetailsPresentation.agentStatus(it.state, it.recorded) })
            assertEquals(before, raw.toString())
        }
    }
    @Test fun cachedGroupCountsNeverBecomeCurrentWorkAndFreshInputStaysVisible() {
        val raw = JSONObject("""{"subagent_source":"hook_profiles","subagent_groups":{"profile":{"state":"working","active_count":2,"total_count":4}}}""")
        val live = SessionDetailsPresentation.parse(raw, target, true).agents.single()
        assertEquals(2L, SessionDetailsPresentation.agentActive(live, true))
        assertEquals(0L, SessionDetailsPresentation.agentActive(live, false))
        val recorded = SessionDetailsPresentation.parse(raw, target, false).agents.single()
        assertEquals(0L, SessionDetailsPresentation.agentActive(recorded, true))
        assertEquals(2L, recorded.active)
        assertEquals(4L, recorded.total)
        // A stale child run is recorded even in a fresh parent inspection.
        val approval = JSONObject("""{"subagents":{"child":{"state":"working","display_state":"approval","run_id":"old"}}}""")
        val child = SessionDetailsPresentation.parse(approval, target, true).agents.single()
        assertEquals("Recorded", SessionDetailsPresentation.agentStatus(child.state, child.recorded))
        approval.getJSONObject("subagents").getJSONObject("child").put("run_id", target.run)
        val fresh = SessionDetailsPresentation.parse(approval, target, true).agents.single()
        assertEquals("Needs approval", SessionDetailsPresentation.agentStatus(fresh.state, fresh.recorded))
        assertEquals("Needs input", SessionDetailsPresentation.agentStatus("input", false))
    }
    @Test fun commandPreviewsKeepProcessIdentityControlsAndNotes() {
        val contract = JSONObject(checkNotNull(javaClass.getResourceAsStream("/process-command-contract.json")).bufferedReader().use { it.readText() })
        val cases = contract.getJSONArray("cases")
        val selected = target.copy(run="run-a",conversation="conversation-a")
        for (index in 0 until cases.length()) {
            val processes = cases.getJSONObject(index).getJSONObject("processes")
            val wire = processes.getJSONArray("items").getJSONObject(0)
            val raw = JSONObject().put("processes",processes)
            val view = SessionDetailsPresentation.parse(raw,selected,true)
            val job = view.processes.single()
            assertEquals("job-a",job.id)
            assertEquals("run-a",job.run)
            assertEquals("conversation-a",job.conversation)
            assertEquals(wire.getString("command"),job.command)
            assertEquals("running",job.status)
            assertFalse(job.stale)
            assertTrue(job.output)
            assertFalse(job.stop)
            assertEquals(wire.getBoolean("command_truncated"),view.processNotes.isNotEmpty())
            if (wire.getBoolean("command_truncated")) assertTrue(view.processNotes.single().contains("displayed preview"))
            assertTrue(SessionDetailsPresentation.parse(raw,selected,false).processes.single().stale)
        }
    }
    @Test fun keepWarmIsShownReadOnlyWithTheHostsReason() {
        val now = 1_794_000_000.0
        fun warm(vararg extra: Pair<String, Any?>) = JSONObject().put("keep_warm", JSONObject().put("enabled", true).put("until", now + 2 * 3600 + 14 * 60 + 5)
            .put("revision", 2).put("state", "waiting").put("next_at", JSONObject.NULL).put("reason", "").put("turn_active", false).apply { extra.forEach { (k, v) -> put(k, v ?: JSONObject.NULL) } })
        fun fields(raw: JSONObject) = SessionDetailsPresentation.parse(raw, target, true, now).fields.associate { it.label to it.value }
        assertEquals("On, 2 h 14 m left", fields(warm())["Keep cache warm"])
        assertEquals("On, always", fields(warm("until" to null))["Keep cache warm"])
        assertEquals("Next ping in 10 m", fields(warm("next_at" to now + 600))["Keep-warm status"])
        assertEquals("Warming the cache now", fields(warm("turn_active" to true))["Keep-warm status"])
        val cold = "The cache went cold. Keeping it warm resumes after your next message."
        assertEquals(cold, fields(warm("state" to "cold", "reason" to cold))["Keep-warm status"])
        val off = fields(warm("enabled" to false, "state" to "off", "until" to null))
        assertEquals("Off", off["Keep cache warm"])
        assertFalse(off.containsKey("Keep-warm status"))
        val reason = "DeepSeek keeps its context cache for hours without pings."
        val unsupported = fields(warm("enabled" to false, "state" to "off", "supported" to false, "reason" to reason))
        assertEquals(listOf("Off", reason), listOf(unsupported["Keep cache warm"], unsupported["Keep-warm status"]))
        // An older computer reports nothing: no field rather than a guess.
        assertFalse(fields(JSONObject()).containsKey("Keep cache warm"))
    }

    @Test fun detailsExposeExactIdentityIncludingArchives() {
        val view = SessionDetailsPresentation.parse(null, target.copy(archiveId="archive-one"), false)
        assertEquals("conversation", view.fields.single { it.label == "Session ID" }.value)
        assertEquals("archive-one", view.fields.single { it.label == "Archive ID" }.value)
        assertFalse(view.fields.any { it.label == "Run ID" })
    }
    private fun task(run: String) = JSONObject().put("run_id",run).put("source","plan").put("items",JSONArray().put(JSONObject()
        .put("id","task").put("title","Native task").put("status","in_progress")))
    private fun model() = JSONObject().put("model","native-model").put("effort","high").put("settings_change_supported",true)
        .put("settings_apply_when","now").put("model_options",JSONArray().put(JSONObject().put("id","native-model")
            .put("label","Native model").put("effort_options",JSONArray(listOf("low","high")))))
    @Test fun taskOwnersUseNativeRosterAndWrongRunIsLastRecorded() {
        val raw = JSONObject().put("task_lists",JSONObject().put("main",task("old")).put("agent:child",task(target.run)))
            .put("subagents",JSONObject().put("child",JSONObject().put("label","Named child")))
        val view = SessionDetailsPresentation.parse(raw,target,true)
        assertEquals("main",view.tasks.first().owner)
        assertTrue(view.tasks.first().recorded)
        assertEquals("Named child",view.tasks[1].label)
        assertFalse(view.tasks[1].recorded)
        assertEquals("In progress",SessionDetailsPresentation.taskStatus(view.tasks[1].tasks.single().status))
    }
    @Test fun cachedAndArchivedTasksCannotAppearCurrent() {
        val raw = JSONObject().put("task_lists",JSONObject().put("main",task(target.run)))
        assertTrue(SessionDetailsPresentation.parse(raw,target,false).tasks.single().recorded)
        assertTrue(SessionDetailsPresentation.parse(raw,target.copy(archiveId="archive"),true).tasks.single().recorded)
    }
    @Test fun nativeReadyDoesNotCompleteActiveGoal() {
        val raw = JSONObject().put("activity","idle").put("phase","idle").put("goal",JSONObject()
            .put("objective","Finish native objective").put("status","active").put("tokens_used",0).put("token_budget",1000).put("time_used_seconds",61))
        val view = SessionDetailsPresentation.parse(raw,target,true)
        assertEquals("Pursuing goal",view.goal.first { it.label == "Status" }.value)
        assertEquals("0 / 1000",view.goal.first { it.label == "Tokens" }.value)
        assertEquals("1m",view.goal.first { it.label == "Recorded active time" }.value)
        assertTrue(view.goalNotice.contains("does not complete"))
    }
    @Test fun malformedGoalMetricsNeverBecomeNumbers() {
        val raw = JSONObject().put("goal",JSONObject().put("objective","Goal").put("status","mystery")
            .put("tokens_used","12").put("time_used_seconds",-1).put("token_budget",999))
        val view = SessionDetailsPresentation.parse(raw,target,true)
        assertEquals("Goal status unknown (mystery)",view.goal.first { it.label == "Status" }.value)
        assertFalse(view.goal.any { it.label in listOf("Tokens","Recorded active time") })
    }
    @Test fun hookProfilesRemainGroupsNotIndividualAgents() {
        val raw = JSONObject().put("subagent_source","hook_profiles")
            .put("subagent_groups",JSONObject().put("reviewer",JSONObject().put("label","Reviewer profile").put("active_count",2).put("total_count",4)))
            .put("subagents",JSONObject().put("fake-individual",JSONObject().put("state","input")))
        val view = SessionDetailsPresentation.parse(raw,target,true)
        assertEquals("reviewer",view.agents.single().id)
        assertTrue(view.agents.single().group)
        assertEquals(2L,view.agents.single().active)
        assertTrue(view.agentNotice.contains("Individual agent identities are not reported"))
    }
    @Test fun knownChildIdentityAndStateRemainNative() {
        val raw = JSONObject().put("subagents",JSONObject().put("exact-child",JSONObject().put("name","Worker").put("state","finished").put("run_id",target.run)))
        val child = SessionDetailsPresentation.parse(raw,target,true).agents.single()
        assertEquals("exact-child",child.id)
        assertEquals("Ready",SessionDetailsPresentation.agentStatus(child.state))
        assertFalse(child.group); assertFalse(child.recorded)
    }
    @Test fun processIdentityAndCapabilitiesDoNotInferStopFromRunning() {
        val p = JSONObject().put("id","exact").put("run_id",target.run).put("conversation_id","old").put("status","running")
            .put("capabilities",JSONObject().put("output",true).put("stop",false))
        val view = SessionDetailsPresentation.parse(JSONObject().put("generation","opaque-generation")
            .put("processes",JSONObject().put("items",JSONArray().put(p)).put("complete",false)),target,true)
        assertTrue(view.processes.single().stale)
        assertTrue(view.processes.single().output)
        assertFalse(view.processes.single().stop)
        assertEquals("opaque-generation",view.generation)
        assertFalse(view.processComplete)
    }
    @Test fun settingsSnapshotIgnoresHistoryAndTimestampPolls() {
        val original = model()
        val changed = JSONObject(original.toString()).put("last_event_at",123).put("journal",JSONArray().put(JSONObject().put("detail","next event")))
        assertEquals(SessionDetailsPresentation.settings(original).signature,SessionDetailsPresentation.settings(changed).signature)
    }
    @Test fun settingsNoticeIsReadAndInvalidatesSnapshot() {
        val raw = model()
        assertEquals("",SessionDetailsPresentation.settings(raw).notice)
        val noticed = JSONObject(raw.toString()).put("settings_change_notice","Claude re-reads the full history on the next message.")
        assertEquals("Claude re-reads the full history on the next message.",SessionDetailsPresentation.settings(noticed).notice)
        assertNotEquals(SessionDetailsPresentation.settings(raw).signature,SessionDetailsPresentation.settings(noticed).signature)
    }
    @Test fun settingsCatalogAndPendingIdentityChangesInvalidateSnapshot() {
        val raw = model()
        val before = SessionDetailsPresentation.settings(raw).signature
        val pending = JSONObject(raw.toString()).put("pending_settings_id","new-intent").put("pending_model","native-model").put("pending_effort","low")
        assertNotEquals(before,SessionDetailsPresentation.settings(pending).signature)
        raw.getJSONArray("model_options").getJSONObject(0).put("effort_options",JSONArray(listOf("low")))
        assertNotEquals(before,SessionDetailsPresentation.settings(raw).signature)
    }
    @Test fun unreportedFieldsAreHonestAndNoModelWindowIsInvented() {
        val view = SessionDetailsPresentation.parse(JSONObject(),target,false)
        assertTrue(view.recorded)
        assertEquals("Not reported",view.fields.first { it.label == "Process" }.value)
        assertTrue(view.goal.isEmpty())
        assertTrue(view.processes.isEmpty())
        assertEquals("No goal reported.",view.goalNotice)
    }
    @Test fun nestedGenerationAndUnfamiliarStatesKeepNativeEvidence() {
        val view = SessionDetailsPresentation.parse(JSONObject().put("generation","parent").put("processes",JSONObject().put("generation","process-generation")),target,true)
        assertEquals("process-generation",view.generation)
        assertEquals("Finished",SessionDetailsPresentation.agentStatus("completed"))
        assertEquals("native-custom-state",SessionDetailsPresentation.agentStatus("native-custom-state"))
        assertEquals("blocked",SessionDetailsPresentation.taskStatus("blocked"))
    }
}
