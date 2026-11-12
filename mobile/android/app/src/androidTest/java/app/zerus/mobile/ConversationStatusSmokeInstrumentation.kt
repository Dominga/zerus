package app.zerus.mobile

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Rect
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/**
 * zerus-a4mub.10: the conversation status strip in a real conversation against a synthetic
 * relay: delayed, failing and refused refreshes, reconnecting, a truncated history, shared read
 * outcomes reported through the status seam, background and foreground, and the keyboard. It
 * keeps the shown messages and the editor in place. Zero deliveries; run on a disposable emulator
 * with fresh app data, with `-e pass NAME` naming the screenshots (for example `narrow`).
 */
class ConversationStatusSmokeInstrumentation : Instrumentation() {
    private var pass = "default"
    override fun onCreate(arguments: Bundle?) { pass = arguments?.getString("pass").orEmpty().ifBlank { "default" }; super.onCreate(arguments); start() }
    override fun onStart() {
        val result = Bundle()
        try {
            check(BuildConfig.DEBUG && Build.HARDWARE in setOf("ranchu", "goldfish"))
            val store = PrivateStore(targetContext)
            require(store.connections().isEmpty() && store.messageState() == MessageState() && store.readState() == ConversationReadState()) {
                "Use fresh isolated emulator app data; this runner never clears existing state."
            }
            val report = smoke()
            result.putString("stream", "Conversation status ($pass) passed: $report. Zero deliveries.\n")
            finish(Activity.RESULT_OK, result)
        } catch (error: Throwable) {
            result.putString("stream", "Conversation status ($pass) failed: ${error.stackTraceToString()}\n")
            finish(Activity.RESULT_CANCELED, result)
        }
    }
    private fun <T> main(action: () -> T): T {
        val result = AtomicReference<Result<T>>()
        runOnMainSync { result.set(runCatching(action)) }
        return result.get().getOrThrow()
    }
    private fun await(description: String, timeout: Long = 15_000, condition: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + timeout
        while (!condition()) {
            check(SystemClock.elapsedRealtime() < deadline) { "Timed out: $description" }
            SystemClock.sleep(40)
        }
    }
    private fun find(root: AccessibilityNodeInfo?, predicate: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? {
        if (root == null) return null
        root.refresh()
        if (predicate(root)) return root
        for (i in 0 until root.childCount) find(root.getChild(i), predicate)?.let { return it }
        return null
    }
    private fun root() = uiAutomation.rootInActiveWindow
    /** Sheets and the keyboard are separate windows; search every interactive window. */
    private fun anyWindow(predicate: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? =
        uiAutomation.windows.mapNotNull { it.root }.firstNotNullOfOrNull { find(it, predicate) } ?: find(root(), predicate)
    private fun text(value: String) = anyWindow { it.isVisibleToUser && it.text?.toString()?.contains(value) == true }
    private fun bounds(node: AccessibilityNodeInfo) = Rect().also(node::getBoundsInScreen)
    /** The strip's status, from its accessibility description. */
    private fun status(): String? = anyWindow { it.contentDescription?.toString()?.startsWith("Conversation status: ") == true }
        ?.contentDescription?.toString()?.removePrefix("Conversation status: ")
    private fun statusNode(): AccessibilityNodeInfo {
        var found: AccessibilityNodeInfo? = null
        await("status strip", 5_000) { found = anyWindow { it.contentDescription?.toString()?.startsWith("Conversation status: ") == true }; found != null }
        return checkNotNull(found)
    }
    private fun editor() = anyWindow { it.isEditable && it.isVisibleToUser }
    /** The sheet's title is its own text; the strip only carries it as a description. */
    private fun closeDetails() {
        repeat(3) {
            if (text("Conversation status") == null) return
            uiAutomation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK)
            val deadline = SystemClock.elapsedRealtime() + 4_000
            while (text("Conversation status") != null && SystemClock.elapsedRealtime() < deadline) SystemClock.sleep(50)
        }
        check(text("Conversation status") == null) { "The status details did not close" }
    }
    /** Clicks the node's clickable owner; a real tap at its centre if the action is refused. */
    private fun tap(node: AccessibilityNodeInfo) {
        var owner = node
        while (!owner.isClickable && owner.parent != null) owner = checkNotNull(owner.parent)
        if (!owner.performAction(AccessibilityNodeInfo.ACTION_CLICK)) {
            val r = bounds(owner)
            uiAutomation.executeShellCommand("input tap ${r.centerX()} ${r.centerY()}").close()
        }
    }
    private val shots = mutableListOf<String>()
    private fun shot(name: String) {
        SystemClock.sleep(400)
        val bitmap = checkNotNull(uiAutomation.takeScreenshot()) { "No screenshot" }
        try { File(targetContext.getExternalFilesDir(null), "status-$pass-$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) } }
        finally { bitmap.recycle() }
        shots += name
    }
    private fun set(model: ZerusViewModel, name: String, value: Any) {
        model.javaClass.declaredMethods.single { it.name == name }.apply { isAccessible = true }.invoke(model, value)
    }

    private fun smoke(): String {
        uiAutomation.serviceInfo = uiAutomation.serviceInfo.apply { flags = flags or AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS }
        val density = targetContext.resources.displayMetrics.density
        val target = Target("fixture-computer", "claude/fixture/status", "fixture-run", "fixture-conversation", "fixture-workspace")
        val raw = JSONObject().put("name", target.session).put("run_id", target.run).put("conversation_id", target.conversation)
            .put("tracked", true).put("runtime_state", "live").put("process_state", "running").put("activity", "idle").put("phase", "idle")
            .put("agent", "claude")
        fun row(i: Int) = JSONObject().put("type", if (i % 3 == 0) "UserPromptSubmit" else "AgentMessage").put("seq", i).put("at", i.toDouble())
            .put("detail", "Synthetic status row $i. " + "Conversation text that stays where it is while the status changes. ".repeat(2))
        raw.put("events", JSONArray((1..30).map(::row)))
        val session = Session(target, "Status fixture", "claude", "idle", "", "", 0, raw)
        // ok | delay:<ms> | io | refuse
        val mode = AtomicReference("ok")
        val inspections = AtomicInteger()
        val mutations = AtomicInteger()
        val capabilities = JSONObject().put("protocol_version", 1).put("operations", JSONArray(listOf("inspect", "voice_profiles")))
        val client = OkHttpClient.Builder().addInterceptor { chain ->
            val request = chain.request()
            check(request.url.host == "relay.example.test")
            fun response(value: JSONObject) = Response.Builder().request(request).protocol(Protocol.HTTP_1_1).code(200).message("OK")
                .body(value.toString().toResponseBody("application/json".toMediaType())).build()
            // The catalog stays online; other reads (the notification event poll) are network failures it already handles.
            if (request.method == "GET") return@addInterceptor when (request.url.encodedPath) {
                "/v1/capabilities" -> response(capabilities)
                "/v1/computers" -> response(JSONObject().put("computers", JSONArray().put(JSONObject().put("id", target.computerId)
                    .put("name", "Fixture computer").put("online", true).put("snapshot", JSONObject()
                        .put("sessions", JSONArray().put(synchronized(raw) { JSONObject(raw.toString()) })).put("mobile_capabilities", capabilities)))))
                else -> throw IOException("Unexpected synthetic GET ${request.url.encodedPath}")
            }
            check(request.method == "POST" && request.url.encodedPath == "/v1/requests")
            val input = JSONObject(okio.Buffer().also { request.body?.writeTo(it) }.readUtf8())
            val operation = input.string("operation")
            var state = "completed"
            val result = when (operation) {
                "inspect" -> {
                    inspections.incrementAndGet()
                    val current = mode.get()
                    if (current.startsWith("delay:")) SystemClock.sleep(current.removePrefix("delay:").toLong())
                    if (current == "io") throw IOException("Fixture relay is unreachable")
                    if (current == "refuse") state = "failed"
                    synchronized(raw) { JSONObject(raw.toString()) }
                }
                "voice_profiles" -> JSONObject().put("schema", 1).put("computers", JSONArray())
                else -> { mutations.incrementAndGet(); error("Unexpected fixture operation $operation") }
            }
            val receipt = JSONObject().put("request_id", input.string("request_id")).put("state", state)
            if (state == "completed") receipt.put("result", result) else receipt.put("error", "Fixture computer refused the read.")
            Response.Builder().request(chain.request()).protocol(Protocol.HTTP_1_1).code(200).message("OK")
                .body(receipt.toString().toResponseBody("application/json".toMediaType())).build()
        }.build()
        val activity = startActivitySync(Intent(targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
        val model = main { activity.javaClass.getDeclaredMethod("getModel").apply { isAccessible = true }.invoke(activity) as ZerusViewModel }
        val report = mutableListOf<String>()
        fun refresh(explicit: Boolean) = main { model.refreshActivity(explicit) }
        fun settled() = main { !model.detailBusy }
        try {
            await("private storage") { main { model.storageReady } }
            main {
                model.javaClass.getDeclaredField("api").apply { isAccessible = true }.set(model, RelayApi(client))
                set(model, "setConnections", listOf(Connection(target.connectionId, "Fixture workspace", "https://relay.example.test", "fixture-token")))
                set(model, "setMachines", listOf(Machine(target.connectionId, target.computerId, "Fixture computer", true)))
                set(model, "setSessions", listOf(session))
                set(model, "setRelayOperations", mapOf(target.connectionId to emptySet<String>()))
                set(model, "setMachineOperations", mapOf(MachineKey(target.connectionId, target.computerId) to emptySet<String>()))
                set(model, "setSelected", session)
                model.refreshActivity()
            }
            await("verified conversation") { main { model.activityVerified } && settled() }
            await("updated status") { status() == "Updated just now" }
            val strip = bounds(statusNode())
            check(strip.height() <= (36 * density + 1)) { "The status strip is taller than one line: $strip" }
            shot("01-updated")

            // The editor keeps its text and place; the shown messages keep theirs.
            val draft = "Draft that stays put"
            checkNotNull(editor()).performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, Bundle().apply {
                putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, draft) })
            await("draft text") { editor()?.text?.toString() == draft }
            val anchorText = "Synthetic status row 30."
            await("a shown message") { text(anchorText) != null }
            val editorBefore = bounds(checkNotNull(editor()))
            val messageBefore = bounds(checkNotNull(text(anchorText)))
            fun unmoved(stage: String) {
                check(editor()?.text?.toString() == draft) { "$stage: the draft changed" }
                check(bounds(checkNotNull(editor())) == editorBefore) { "$stage: the editor moved ${bounds(checkNotNull(editor()))} from $editorBefore" }
                check(bounds(checkNotNull(text(anchorText)) { "$stage: the shown message disappeared" }) == messageBefore) { "$stage: the shown message moved" }
                check(bounds(statusNode()).height() == strip.height()) { "$stage: the status strip changed height" }
            }

            // An explicit refresh answers at once and keeps everything in place while it waits.
            mode.set("delay:4000")
            val explicitAt = SystemClock.elapsedRealtime()
            refresh(true)
            // The explicit refresh sets visible progress in the same main-thread call.
            val stateMs = SystemClock.elapsedRealtime() - explicitAt
            check(main { model.detailBusy && model.detailProgress }) { "An explicit refresh did not show progress at once" }
            // Confirming the strip's text walks every window's accessibility tree, which itself takes time.
            await("refresh feedback in the strip", 3_000) { status() == "Refreshing…" }
            report += "explicit refresh progress state ${stateMs}ms; strip text confirmed after ${SystemClock.elapsedRealtime() - explicitAt}ms incl. accessibility scan"
            shot("02-refreshing")
            unmoved("explicit refresh")
            await("delayed refresh settles") { settled() && status() == "Updated just now" }
            unmoved("after the delayed refresh")

            // An automatic refresh stays quiet within its delay: no spinner flicker, no new claim.
            mode.set("delay:2500")
            refresh(false)
            val quietUntil = SystemClock.elapsedRealtime() + 2_000
            while (SystemClock.elapsedRealtime() < quietUntil) {
                check(status() == "Updated just now") { "An automatic refresh changed the status to ${status()}" }
                SystemClock.sleep(100)
            }
            await("automatic refresh settles") { settled() }
            report += "automatic refresh quiet for 2000ms"

            // A lost connection keeps the messages and says so, once, in the strip.
            mode.set("io")
            refresh(true)
            await("offline status") { settled() && status()?.startsWith("Offline · updated") == true }
            check(find(root()) { it.contentDescription?.toString() == "Dismiss error" } == null) { "The read failure is also shown as a global banner" }
            check(main { !model.activityVerified }) { "A failed read left the frame verified" }
            unmoved("offline")
            shot("03-offline")
            tap(statusNode())
            await("status details") { text("Conversation status") != null && text("No connection") != null }
            shot("04-details-offline")
            closeDetails()

            // A refusal is not reported as offline.
            mode.set("refuse")
            refresh(true)
            await("refused status") { settled() && status()?.startsWith("Couldn't refresh · updated") == true }
            shot("05-refused")

            // Reconnecting clears the warning.
            mode.set("ok")
            refresh(true)
            await("reconnected") { settled() && status() == "Updated just now" }
            unmoved("reconnected")
            shot("06-reconnected")

            // Older messages outside the available history are a detail, not a list banner.
            synchronized(raw) { raw.put("history_truncated", true) }
            refresh(true)
            await("truncated history") { settled() && main { model.conversationHistoryTruncated } }
            check(text("Showing recent activity. Older messages may be outside the available history.") == null) { "The bounded notice is still a list item" }
            tap(statusNode())
            await("history detail") { text("Older messages may be outside the available history") != null }
            shot("07-details-history")
            closeDetails()

            // Shared read outcomes arrive through the status seam (zerus-a4mub.9 owns the authority).
            main { model.noteSharedRead(target, SharedReadStatus.Pending(System.currentTimeMillis())) }
            tap(statusNode())
            await("pending read detail") { text("Waiting for the computer to confirm") != null }
            shot("08-details-read-pending")
            closeDetails()
            main { model.noteSharedRead(target, SharedReadStatus.Failed("No connection", System.currentTimeMillis())) }
            await("failed read status") { status() == "Read not synced · updated just now" }
            shot("09-read-failed")
            main { model.noteSharedRead(target, SharedReadStatus.Confirmed(System.currentTimeMillis())) }
            await("confirmed read status") { status() == "Updated just now" }

            // Background and foreground keep the conversation, the draft and an honest status.
            uiAutomation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_HOME)
            SystemClock.sleep(2_000)
            // The same activity returns to the front; startActivitySync would wait for a new one.
            targetContext.startActivity(Intent(targetContext, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT))
            await("resumed conversation") { text(anchorText) != null && editor()?.text?.toString() == draft }
            await("resumed status") { status()?.let { it.startsWith("Updated") || it == "Refreshing…" } == true }
            await("resumed refresh settles") { settled() && status() == "Updated just now" }
            shot("10-resumed")

            // With the keyboard open the strip stays one line above the editor.
            checkNotNull(editor()).performAction(AccessibilityNodeInfo.ACTION_CLICK)
            await("keyboard") { uiAutomation.windows.any { it.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD } }
            SystemClock.sleep(600)
            val keyboardStrip = bounds(statusNode())
            val keyboardEditor = bounds(checkNotNull(editor()))
            check(keyboardStrip.height() == strip.height() && keyboardStrip.bottom <= keyboardEditor.top) {
                "The status strip is hidden or wraps with the keyboard: strip=$keyboardStrip editor=$keyboardEditor"
            }
            shot("11-keyboard")
            report += "strip ${strip.height() / density}dp, screen ${targetContext.resources.displayMetrics.widthPixels / density}dp wide"
            check(mutations.get() == 0) { "Unexpected mutation" }
            report += "inspections=${inspections.get()}"
            report += "screenshots=${shots.joinToString(",")}"
            return report.joinToString("; ")
        } catch (error: Throwable) {
            uiAutomation.takeScreenshot()?.let { bitmap ->
                File(targetContext.getExternalFilesDir(null), "status-$pass-failure.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
                bitmap.recycle()
            }
            throw IllegalStateException("${error.message}; status=${status()}; report=$report", error)
        } finally {
            main { model.back(); activity.finish() }
        }
    }
}
