package app.zerus.mobile

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.graphics.Bitmap
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo
import kotlinx.coroutines.runBlocking
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** Cached opening, owned read phases and pending delivery UI; no real relay or native agent input. */
class ConversationFeedbackSmokeInstrumentation : Instrumentation() {
    override fun onCreate(arguments: Bundle?) { super.onCreate(arguments); start() }
    override fun onStart() {
        val result = Bundle()
        try {
            check(BuildConfig.DEBUG && Build.HARDWARE in setOf("ranchu", "goldfish"))
            smoke()
            result.putString("stream", "Conversation feedback passed: visible pinned read stages, parallel opening reads in both completion orders without duplicate history, stable retained-message position, settled/failed reads, retained next-draft editing, one sending label and actionable unknown delivery. Zero mutations.\n")
            finish(Activity.RESULT_OK, result)
        } catch (error: Throwable) {
            result.putString("stream", "Conversation feedback failed: ${error.stackTraceToString()}\n")
            finish(Activity.RESULT_CANCELED, result)
        }
    }
    private fun <T> main(action: () -> T): T {
        val value = AtomicReference<Result<T>>()
        runOnMainSync { value.set(runCatching(action)) }
        return value.get().getOrThrow()
    }
    private fun await(label: String, timeout: Long = 15_000, condition: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + timeout
        while (!condition()) {
            check(SystemClock.elapsedRealtime() < deadline) { "Timed out: $label" }
            SystemClock.sleep(30)
        }
    }
    private fun find(root: AccessibilityNodeInfo?, predicate: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? {
        if (root == null) return null
        root.refresh()
        if (root.isVisibleToUser && predicate(root)) return root
        for (i in 0 until root.childCount) find(root.getChild(i), predicate)?.let { return it }
        return null
    }
    private fun text(value: String) = find(uiAutomation.rootInActiveWindow) { it.text?.toString()?.contains(value) == true }
    private fun progressVisible() = find(uiAutomation.rootInActiveWindow) { it.className?.toString() == "android.widget.ProgressBar" } != null
    private fun refreshing(): Boolean {
        // Compose exposes the action label and state as separate virtual nodes.
        val root = uiAutomation.rootInActiveWindow
        return find(root) { it.contentDescription?.toString() == "Refresh" } != null &&
            find(root) { it.text?.toString() in setOf("Checking activity and messages…", "Loading message history…", "Checking latest activity…") } != null &&
            find(root) { it.className?.toString() == "android.widget.ProgressBar" } != null
    }
    private fun screenshot(name: String) {
        uiAutomation.takeScreenshot()?.let { bitmap ->
            File(targetContext.getExternalFilesDir(null), name).outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }
    private fun set(model: ZerusViewModel, name: String, value: Any) {
        model.javaClass.declaredMethods.single { it.name == name }.apply { isAccessible = true }.invoke(model, value)
    }
    private fun change(model: ZerusViewModel, reduce: (MessageState) -> MessageState) {
        model.javaClass.getDeclaredMethod("editState", kotlin.jvm.functions.Function1::class.java)
            .apply { isAccessible = true }.invoke(model, reduce)
    }
    private fun smoke() {
        val store = PrivateStore(targetContext)
        check(store.connections().isEmpty() && store.messageState() == MessageState()) { "Use fresh isolated emulator app data" }
        val target = Target("fixture-computer", "codex/fixture/feedback", "fixture-run", "fixture-conversation", "fixture-workspace")
        fun row(id: Int, body: String) = JSONObject().put("type", "AgentMessage").put("seq", id).put("at", id.toDouble())
            .put("detail", body).put("history_id", "fixture-$id").put("history_cursor", "fixture-cursor-$id").put("incoming_seq", id)
        val saved = "Saved response before refresh."
        val fresh = "Fresh response after the owned history read."
        val raw = JSONObject().put("name", target.session).put("run_id", target.run).put("conversation_id", target.conversation)
            .put("tracked", true).put("runtime_state", "live").put("process_state", "running")
            .put("activity", "idle").put("phase", "idle").put("agent", "codex").put("history_epoch", "fixture-epoch")
            .put("events", JSONArray(listOf(row(1, saved)))).put("reply_id", "fixture-reply").put("mobile_unread_reply", true)
        runBlocking { ConversationCache(targetContext).write(target, raw) }
        val second = target.copy(session = target.session + "-second", run = "second-run", conversation = "second-conversation")
        val secondRaw = JSONObject(raw.toString()).put("name", second.session).put("run_id", second.run).put("conversation_id", second.conversation)
        runBlocking { ConversationCache(targetContext).write(second, secondRaw) }
        val inspection = AtomicReference(CountDownLatch(1))
        val history = AtomicReference(CountDownLatch(1))
        val inspectionReads = AtomicInteger()
        val historyReads = AtomicInteger()
        val backgroundHeadReads = AtomicInteger()
        val mutations = AtomicInteger()
        val failInspection = AtomicBoolean(false)
        val client = OkHttpClient.Builder().addInterceptor { chain ->
            val request = chain.request()
            check(request.url.host == "relay.example.test")
            if (request.method == "GET") {
                check(request.url.encodedPath == "/v1/computers")
                val snapshot = JSONObject().put("sessions", JSONArray(listOf(raw))).put("mobile_capabilities",
                    JSONObject().put("protocol_version", 1).put("operations", JSONArray(listOf("history"))))
                val computer = JSONObject().put("id", target.computerId).put("name", "Fixture computer").put("online", true).put("snapshot", snapshot)
                return@addInterceptor Response.Builder().request(request).protocol(Protocol.HTTP_1_1).code(200).message("OK")
                    .body(JSONObject().put("computers", JSONArray(listOf(computer))).toString().toResponseBody("application/json".toMediaType())).build()
            }
            val input = JSONObject(okio.Buffer().also { request.body?.writeTo(it) }.readUtf8())
            val requested = if (input.string("session") == second.session) second else target
            val value = when (input.string("operation")) {
                "inspect" -> {
                    inspectionReads.incrementAndGet()
                    check(inspection.get().await(20, TimeUnit.SECONDS))
                    if (failInspection.get()) throw java.io.IOException("Synthetic inspection unavailable")
                    JSONObject((if (requested == second) secondRaw else raw).toString())
                }
                "history" -> {
                    val payload = input.getJSONObject("payload")
                    check(payload.string("expected_run_id") == requested.run && payload.string("expected_conversation_id") == requested.conversation)
                    if (payload.getInt("limit") > 1) { historyReads.incrementAndGet(); check(history.get().await(20, TimeUnit.SECONDS)) }
                    else backgroundHeadReads.incrementAndGet()
                    JSONObject().put("request_id", input.string("request_id")).put("name", requested.session)
                        .put("run_id", requested.run).put("conversation_id", requested.conversation).put("history_epoch", "fixture-epoch")
                        .put("events", JSONArray(listOf(row(1, saved), row(2, fresh))))
                        .put("has_more_before", false).put("has_more_after", false).put("next_before", "fixture-cursor-1").put("next_after", "fixture-cursor-2")
                        .put("head", JSONObject().put("complete", true).put("incoming_seq", 2).put("total_incoming", 2)).put("indexing", false)
                }
                "voice_profiles" -> JSONObject().put("schema", 1).put("computers", JSONArray())
                else -> { mutations.incrementAndGet(); error("Unexpected fixture operation ${input.string("operation")}") }
            }
            Response.Builder().request(request).protocol(Protocol.HTTP_1_1).code(200).message("OK")
                .body(JSONObject().put("request_id", input.string("request_id")).put("state", "completed").put("result", value)
                    .toString().toResponseBody("application/json".toMediaType())).build()
        }.build()
        val activity = startActivitySync(Intent(targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
        val model = main { activity.javaClass.getDeclaredMethod("getModel").apply { isAccessible = true }.invoke(activity) as ZerusViewModel }
        try {
            await("private storage") { main { model.storageReady } }
            await("read tracking ready") { main { model.javaClass.getDeclaredMethod("getReadTrackingReady").apply { isAccessible = true }.invoke(model) as Boolean } }
            main {
                model.javaClass.getDeclaredField("api").apply { isAccessible = true }.set(model, RelayApi(client))
                set(model, "setConnections", listOf(Connection(target.connectionId, "Fixture workspace", "https://relay.example.test", "fixture-token")))
                set(model, "setMachines", listOf(Machine(target.connectionId, target.computerId, "Fixture computer", true)))
                val session = Session(target, "Feedback fixture", "codex", "idle", "", "", 0, raw)
                set(model, "setSessions", listOf(session))
                set(model, "setRelayOperations", mapOf(target.connectionId to setOf("history")))
                set(model, "setMachineOperations", mapOf(MachineKey(target.connectionId, target.computerId) to setOf("history")))
                model.open(session)
            }
            await("both reads started while inspection and history are held") { inspectionReads.get() > 0 && historyReads.get() > 0 && text(saved) != null }
            // A catalog update during opening must not spend the serial relay queue on head probes.
            main { model.javaClass.getDeclaredMethod("watchHistoryHeads", Target::class.java).apply { isAccessible = true }.invoke(model, null) }
            try {
                await("opening indicator before five-second threshold", 1_500) { refreshing() }
            } catch (error: Throwable) {
                screenshot("e7et-opening-failed.png")
                throw IllegalStateException("Opening progress missing: " +
                    main { "busy=${model.detailBusy}, history=${model.olderLoading}, verified=${model.activityVerified}, events=${model.conversationEvents.size}" }, error)
            }
            check(text("Loading conversation…") == null)
            check(text("Showing saved messages.") != null)
            val before = android.graphics.Rect().also { checkNotNull(text(saved)).getBoundsInScreen(it) }
            screenshot("e7et-opening.png")
            inspection.get().countDown()
            await("owned history read") { historyReads.get() > 0 && main { !model.detailBusy && model.olderLoading } }
            check(refreshing() && text(saved) != null && text(fresh) == null)
            check(before == android.graphics.Rect().also { checkNotNull(text(saved)).getBoundsInScreen(it) }) { "Read phases moved the retained message" }
            val openingInspections = inspectionReads.get()
            SystemClock.sleep(5_500)
            check(inspectionReads.get() == openingInspections) { "Background polling repeated the opening inspection while history was pending" }
            check(backgroundHeadReads.get() == 0) { "Catalog history-head scans competed with the visible conversation" }
            history.get().countDown()
            await("fresh history and settled indicator") { text(fresh) != null && main { !model.detailBusy && !model.olderLoading && model.activityVerified } && !progressVisible() }
            check(historyReads.get() == 1) { "Opening fetched the complete page twice" }
            check(text("Message history loaded.") != null)
            failInspection.set(true)
            main { model.refreshActivity(explicit = true) }
            await("failed read keeps history and clears progress") { main { model.error.isNotBlank() && !model.detailBusy } && text(fresh) != null && !progressVisible() }
            failInspection.set(false)
            main { model.refreshActivity(explicit = true) }
            await("verified session after failure") { main { model.activityVerified && !model.detailBusy && !model.olderLoading } }
            main {
                model.edit(target, "One pending fixture message")
                val draft = model.draft(target)
                val requestId = "00000000-0000-4000-8000-000000000017"
                // Reserve the local in-flight identity without issuing a relay mutation.
                set(model, "setDeliveryFlights", setOf(requestId))
                change(model) { it.enqueue(draft, requestId, System.currentTimeMillis() / 1000.0) }
            }
            await("pending message footer") { text("Sending…") != null && text("One pending fixture message") != null }
            check(text("Sending… You can write the next message.") == null)
            check(text("Sending your previous message.") == null)
            check(text("Message") != null)
            val editor = checkNotNull(find(uiAutomation.rootInActiveWindow) { it.isEditable })
            check(editor.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, Bundle().apply {
                putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, "Next fixture draft")
            }))
            await("next draft remains editable") { text("Next fixture draft") != null &&
                main { model.draft(target).text == "Next fixture draft" && model.sendingBlocked(target) } }
            screenshot("e7et-sending.png")
            main { change(model) { state -> state.copy(outgoing = state.outgoing.map { it.copy(status = "uncertain", error = "Synthetic delivery unknown") }) } }
            await("actionable unknown delivery") {
                find(uiAutomation.rootInActiveWindow) { it.contentDescription?.toString() == "Check previous message delivery" } != null &&
                    find(uiAutomation.rootInActiveWindow) { it.contentDescription?.toString() == "Review previous message" } != null
            }
            check(text("Delivery unknown. Check the previous message before sending again.") == null)
            check(main { model.outgoing.single().status == "uncertain" })
            check(main { model.draft(target).text == "Next fixture draft" && model.sendingBlocked(target) })

            // Reverse the race on a new exact conversation. A page arriving before
            // inspection cannot verify activity or trigger another identical read.
            inspection.set(CountDownLatch(1)); history.set(CountDownLatch(1))
            val beforeHistory = historyReads.get()
            val beforeInspection = inspectionReads.get()
            main { model.open(Session(second, "Second fixture", "codex", "idle", "", "", 0, secondRaw)) }
            await("second opening starts both reads") { historyReads.get() > beforeHistory && inspectionReads.get() > beforeInspection }
            history.get().countDown()
            await("history precedes the held inspection") { main { model.historyComplete && !model.activityVerified && model.detailBusy } }
            check(refreshing())
            inspection.get().countDown()
            await("second conversation verified and history settled") { main { model.activityVerified && !model.detailBusy && !model.olderLoading } && text(fresh) != null && !progressVisible() }
            check(historyReads.get() == beforeHistory + 1) { "History-first opening fetched the complete page twice" }
            val headsBeforeBack = backgroundHeadReads.get()
            main { model.back() }
            await("catalog head scanning resumes after leaving the conversation") { backgroundHeadReads.get() > headsBeforeBack }
            check(mutations.get() == 0) { "The presentation fixture sent a mutation" }
        } finally {
            inspection.get().countDown(); history.get().countDown()
            main { activity.finish() }
        }
    }
}
