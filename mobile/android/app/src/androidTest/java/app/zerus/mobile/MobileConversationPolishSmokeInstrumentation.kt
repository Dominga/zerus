package app.zerus.mobile

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Rect
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** Fresh owned emulator only: synthetic reads/send, mounted-row geometry and production account marks. */
class MobileConversationPolishSmokeInstrumentation : Instrumentation() {
    private val transportFailure = AtomicReference<String?>(null)
    override fun onCreate(arguments: Bundle?) { super.onCreate(arguments); start() }
    override fun onStart() {
        val result = Bundle()
        try {
            check(BuildConfig.DEBUG && Build.HARDWARE in setOf("ranchu", "goldfish"))
            smoke()
            result.putString("stream", "Conversation polish passed: one fresh synthetic send, unchanged mounted card identity/pixels and empty composer across held metadata refreshes, no permanent status banner, plain Latest navigation, and D/U account marks at font scales1.0 and1.8. No real relay or native agent input.\n")
            finish(Activity.RESULT_OK, result)
        } catch (error: Throwable) {
            screenshot("conversation-polish-failed.png")
            result.putString("stream", "Conversation polish failed: transport=${transportFailure.get()}\n${error.stackTraceToString()}\n")
            finish(Activity.RESULT_CANCELED, result)
        }
    }
    private fun <T> main(action: () -> T): T {
        val result = AtomicReference<Result<T>>()
        runOnMainSync { result.set(runCatching(action)) }
        return result.get().getOrThrow()
    }
    private fun await(label: String, timeout: Long = 15_000, condition: () -> Boolean) {
        val end = SystemClock.elapsedRealtime() + timeout
        while (!condition()) {
            check(SystemClock.elapsedRealtime() < end) { "Timed out: $label; transport=${transportFailure.get()}" }
            SystemClock.sleep(30)
        }
    }
    private fun find(root: AccessibilityNodeInfo?, predicate: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? {
        if (root == null) return null
        root.refresh()
        if (root.isVisibleToUser && predicate(root)) return root
        for (index in 0 until root.childCount) find(root.getChild(index), predicate)?.let { return it }
        return null
    }
    private fun text(value: String) = find(uiAutomation.rootInActiveWindow) { it.text?.toString()?.contains(value) == true }
    private fun bounds(node: AccessibilityNodeInfo) = Rect().also(node::getBoundsInScreen)
    private fun screenshot(name: String) {
        checkNotNull(uiAutomation.takeScreenshot()).let { bitmap ->
            File(targetContext.getExternalFilesDir(null), name).outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }
    private fun set(model: ZerusViewModel, name: String, value: Any) {
        model.javaClass.declaredMethods.single { it.name == name }.apply { isAccessible = true }.invoke(model, value)
    }
    private fun noPermanentBanner() {
        check(text("Latest activity checked") == null) { "A settled status banner is still mounted" }
        check(find(uiAutomation.rootInActiveWindow) { it.text?.toString()?.startsWith("Updated ") == true } == null)
        check(text("Showing available messages.") == null)
    }
    private fun emptyComposer(model: ZerusViewModel, target: Target): Boolean = main { model.draft(target).text.isBlank() } &&
        find(uiAutomation.rootInActiveWindow) { it.isEditable && (it.text.isNullOrBlank() ||
            it.text?.toString() == "Message" || it.isShowingHintText && it.text == it.hintText) } != null

    private fun smoke() {
        val store = PrivateStore(targetContext)
        check(store.connections().isEmpty() && store.messageState() == MessageState()) { "Use fresh isolated emulator app data" }
        val target = Target("fixture-computer", "codex/fixture/polish", "fixture-run", "fixture-conversation", "fixture-workspace")
        val ownText = "Synthetic freshly sent message.\nKeep this card mounted through metadata checks."
        val events = JSONArray((1..8).map { index ->
            JSONObject().put("type", "AgentMessage").put("seq", index).put("at", index.toDouble())
                .put("detail", "Synthetic response $index.\n" + "Retained conversation row.\n".repeat(6))
        })
        val raw = AtomicReference(JSONObject().put("name", target.session).put("run_id", target.run)
            .put("conversation_id", target.conversation).put("tracked", true).put("runtime_state", "live")
            .put("process_state", "running").put("agent", "codex").put("activity", "idle").put("phase", "idle")
            .put("events", events))
        val gate = AtomicReference(CountDownLatch(0))
        val inspections = AtomicInteger()
        val sends = AtomicInteger()
        val unexpected = AtomicInteger()
        val sendId = AtomicReference("")
        val capabilities = JSONObject().put("protocol_version", 1)
            .put("operations", JSONArray(listOf("inspect", "send", "voice_profiles")))
        val client = OkHttpClient.Builder().addInterceptor { chain ->
            val request = chain.request()
            fun response(value: JSONObject) = Response.Builder().request(request).protocol(Protocol.HTTP_1_1)
                .code(200).message("OK").body(value.toString().toResponseBody("application/json".toMediaType())).build()
            try {
                check(request.url.host == "relay.example.test")
                if (request.method == "GET") return@addInterceptor when (request.url.encodedPath) {
                    "/v1/capabilities" -> response(capabilities)
                    "/v1/computers" -> {
                        val snapshot = JSONObject().put("sessions", JSONArray().put(raw.get())).put("mobile_capabilities", capabilities)
                        val computer = JSONObject().put("id", target.computerId).put("name", "Fixture computer")
                            .put("online", true).put("snapshot", snapshot)
                        response(JSONObject().put("computers", JSONArray().put(computer)))
                    }
                    else -> throw IOException("Unexpected synthetic GET ${request.url.encodedPath}")
                }
                check(request.method == "POST" && request.url.encodedPath == "/v1/requests")
                val input = JSONObject(okio.Buffer().also { request.body?.writeTo(it) }.readUtf8())
                val value = when (input.string("operation")) {
                    "inspect" -> {
                        inspections.incrementAndGet()
                        check(gate.get().await(12, TimeUnit.SECONDS)) { "Synthetic inspection gate did not release" }
                        JSONObject(raw.get().toString())
                    }
                    "send" -> {
                        check(sends.incrementAndGet() == 1) { "The fresh synthetic message was reposted" }
                        val payload = input.getJSONObject("payload")
                        check(payload.string("text") == ownText && payload.string("expected_run_id") == target.run &&
                            payload.string("expected_conversation_id") == target.conversation)
                        sendId.set(input.string("request_id"))
                        val echo = JSONObject().put("type", "UserPromptSubmit").put("source", "hgs_delivery")
                            .put("message_id", sendId.get()).put("at", System.currentTimeMillis() / 1000.0).put("detail", ownText)
                        val next = JSONObject(raw.get().toString())
                        next.getJSONArray("events").put(echo)
                        raw.set(next)
                        JSONObject().put("request_id", sendId.get()).put("name", target.session).put("run_id", target.run)
                            .put("conversation_id", target.conversation).put("status", "submitted").put("submitted_text", ownText)
                    }
                    "voice_profiles" -> JSONObject().put("schema", 1).put("computers", JSONArray())
                    else -> { unexpected.incrementAndGet(); throw IOException("Unexpected synthetic operation ${input.string("operation")}") }
                }
                response(JSONObject().put("request_id", input.string("request_id")).put("state", "completed").put("result", value))
            } catch (error: Exception) {
                transportFailure.set("${request.method} ${request.url.encodedPath}: ${error.message}")
                throw IOException(transportFailure.get(), error)
            }
        }.build()
        val activity = startActivitySync(Intent(targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
        val model = main { activity.javaClass.getDeclaredMethod("getModel").apply { isAccessible = true }.invoke(activity) as ZerusViewModel }
        val measured = JSONArray()
        try {
            await("private storage/read tracking") { main { model.storageReady && model.javaClass
                .getDeclaredMethod("getReadTrackingReady").apply { isAccessible = true }.invoke(model) == true } }
            main {
                model.javaClass.getDeclaredField("api").apply { isAccessible = true }.set(model, RelayApi(client))
                set(model, "setConnections", listOf(Connection(target.connectionId, "Fixture workspace", "https://relay.example.test", "fixture-token")))
                set(model, "setMachines", listOf(Machine(target.connectionId, target.computerId, "Fixture computer", true)))
                val session = Session(target, "Conversation polish fixture", "codex", "idle", "", "", 0, raw.get())
                set(model, "setSessions", listOf(session))
                set(model, "setRelayOperations", mapOf(target.connectionId to setOf("inspect", "send")))
                set(model, "setMachineOperations", mapOf(MachineKey(target.connectionId, target.computerId) to setOf("inspect", "send")))
                model.open(session)
            }
            await("verified opening") { main { model.activityVerified && !model.detailBusy } && text("Synthetic response 8.") != null }
            SystemClock.sleep(500)
            noPermanentBanner()

            fun refresh(label: String, anchor: String, explicit: Boolean, heldMs: Long) {
                uiAutomation.waitForIdle(100, 2_000)
                val node = checkNotNull(text(anchor)) { "No visible anchor $anchor" }
                val before = bounds(node)
                val held = CountDownLatch(1)
                val count = inspections.get()
                raw.set(JSONObject(raw.get().toString()).put("activity", if (count % 2 == 0) "busy" else "idle")
                    .put("phase", if (count % 2 == 0) "working" else "idle").put("last_event_at", 1000.0 + count))
                gate.set(held)
                main { model.refreshActivity(explicit = explicit) }
                await("$label held read") { inspections.get() > count && main { model.detailBusy } }
                SystemClock.sleep(heldMs)
                val during = checkNotNull(text(anchor))
                check(during == node && bounds(during) == before) { "$label remounted or moved anchor during inspection: $before -> ${bounds(during)}" }
                if (anchor == ownText) check(emptyComposer(model, target)) { "A held read restored the sent editor" }
                noPermanentBanner()
                screenshot("conversation-polish-$label-held.png")
                held.countDown()
                await("$label settled") { main { !model.detailBusy && model.activityVerified } }
                val after = checkNotNull(text(anchor))
                check(after == node && bounds(after) == before) { "$label remounted or moved anchor after inspection" }
                noPermanentBanner()
                measured.put(JSONObject().put("phase", label).put("bounds", before.toShortString()).put("mounted_row_retained", true))
            }
            refresh("before-send", "Synthetic response 8.", explicit = true, heldMs = 450)
            main { model.edit(target, ownText); model.send(target) }
            await("one send and empty next editor") { sends.get() == 1 && text(ownText) != null &&
                main { !model.sendingBlocked(target) } && emptyComposer(model, target) }
            // Install the exact native echo before measuring metadata-only refreshes.
            main { model.refreshActivity(explicit = true) }
            await("native echo settled") { main { !model.detailBusy && model.conversationEvents.count { it.receiptRequestId == sendId.get() } == 1 } }
            SystemClock.sleep(500)
            refresh("after-send", ownText, explicit = false, heldMs = 5_500)
            check(emptyComposer(model, target)) { "Metadata refresh restored the sent payload into the editor" }
            refresh("after-send-repeat", ownText, explicit = true, heldMs = 450)
            check(emptyComposer(model, target) && sends.get() == 1 && unexpected.get() == 0)
            var list = checkNotNull(text(ownText))
            while (list.collectionInfo == null) list = list.parent ?: error("No conversation list around sent row")
            check(list.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SCROLL_UP.id))
            await("floating latest navigation") { find(uiAutomation.rootInActiveWindow) {
                it.contentDescription?.toString() == "Jump to latest messages" } != null }
            val latest = checkNotNull(find(uiAutomation.rootInActiveWindow) { it.contentDescription?.toString() == "Jump to latest messages" })
            check(find(latest) { it.text?.toString()?.contains("unread", ignoreCase = true) == true } == null)
            check(latest.stateDescription?.toString()?.contains("unread", ignoreCase = true) != true)
            check(text("Latest") != null)
            screenshot("conversation-polish-latest.png")
            File(targetContext.getExternalFilesDir(null), "conversation-polish-results.json").writeText(
                JSONObject().put("synthetic_send_posts", sends.get()).put("inspections", inspections.get())
                    .put("geometry", measured).put("empty_composer", true).toString(2))
            main { model.back(); model.refresh(explicit = true) }
            await("settled main catalog") {
                main { !model.catalogBusy && !model.catalogRefresh.running && model.catalogRefresh.workspaces.isNotEmpty() }
            }
            noPermanentBanner()
            screenshot("conversation-polish-main-settled.png")
            accountMarks(activity, 1f, false)
            accountMarks(activity, 1.8f, true)
        } finally {
            gate.get().countDown()
            main { activity.finish() }
            client.dispatcher.cancelAll()
            client.connectionPool.evictAll()
        }
    }
    private fun accountMarks(activity: MainActivity, scale: Float, narrow: Boolean) {
        main {
            activity.setContent {
                MaterialTheme(colorScheme = darkColorScheme()) {
                    val density = LocalDensity.current
                    CompositionLocalProvider(LocalDensity provides Density(density.density, scale)) {
                        Surface {
                            Column(Modifier.fillMaxSize().systemBarsPadding().padding(16.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                                Text("Production account marks / font scale $scale")
                                Column(if (narrow) Modifier.width(280.dp) else Modifier, verticalArrangement = Arrangement.spacedBy(16.dp)) {
                                    AccountMarkChip(AccountMark("Default synthetic account", "D", 0xFF4DABF7L, 0xFF061014L, "codex"))
                                    AccountMarkChip(AccountMark("User synthetic account", "U", 0xFFFFC857L, 0xFF061014L, "codex"))
                                }
                            }
                        }
                    }
                }
            }
        }
        await("account marks at $scale") { text("Production account marks / font scale $scale") != null && listOf("Default", "User").all { name ->
            find(uiAutomation.rootInActiveWindow) { it.contentDescription?.toString() == "Account: $name synthetic account" }
                ?.let { !bounds(it).isEmpty } == true } }
        uiAutomation.waitForIdle(200, 2_000)
        // Accessibility can publish the new tree before its pixels are committed.
        // Wait for this layout's draw before capturing a different font scale.
        val committed = CountDownLatch(1)
        main {
            activity.window.decorView.viewTreeObserver.registerFrameCommitCallback { committed.countDown() }
            activity.window.decorView.invalidate()
        }
        check(committed.await(5, TimeUnit.SECONDS)) { "Account mark frame was not committed at $scale" }
        screenshot(if (narrow) "conversation-polish-account-marks-large-narrow.png" else "conversation-polish-account-marks-default.png")
    }
}
