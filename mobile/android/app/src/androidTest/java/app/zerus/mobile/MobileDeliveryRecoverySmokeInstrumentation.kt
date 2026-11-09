package app.zerus.mobile

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Rect
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.accessibility.AccessibilityNodeInfo
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** Synthetic HTTP only. Run recovery, force-stop this package, then restart for an actual cold reload. */
class MobileDeliveryRecoverySmokeInstrumentation : Instrumentation() {
    private var scenario = "recovery"
    private val observations = mutableListOf<String>()
    private val transportFailure = AtomicReference<String?>(null)
    private val target = Target("fixture-computer", "codex/fixture/delivery-recovery", "fixture-run", "fixture-conversation", "fixture-workspace")
    private val originalId = "00000000-0000-4000-8000-000000000701"
    private val originalText = "Original message with an unknown delivery."
    private val nextText = "A distinct next message kept in the editor."

    override fun onCreate(arguments: Bundle?) {
        scenario = arguments?.getString("scenario") ?: "recovery"
        super.onCreate(arguments)
        start()
    }

    override fun onStart() {
        val result = Bundle()
        try {
            check(BuildConfig.DEBUG && Build.HARDWARE in setOf("ranchu", "goldfish"))
            check(scenario in setOf("recovery", "restart", "timeout"))
            smoke()
            result.putString("recovery_scenario", scenario)
            result.putString("recovery_checks", observations.joinToString(","))
            result.putString("stream", "Mobile delivery recovery passed ($scenario): ${observations.joinToString("; ")}. Synthetic receipts only.\n")
            finish(Activity.RESULT_OK, result)
        } catch (error: Throwable) {
            screenshot("failed")
            result.putString("stream", "Mobile delivery recovery failed ($scenario): ${observations.joinToString("; ")}\ntransport=${transportFailure.get()}\n${error.stackTraceToString()}\n")
            finish(Activity.RESULT_CANCELED, result)
        }
    }

    private fun <T> main(action: () -> T): T {
        val result = AtomicReference<Result<T>>()
        runOnMainSync { result.set(runCatching(action)) }
        return result.get().getOrThrow()
    }

    private fun await(label: String, timeout: Long = 10_000, condition: () -> Boolean) {
        val end = SystemClock.elapsedRealtime() + timeout
        while (!condition()) {
            transportFailure.get()?.let { error(it) }
            check(SystemClock.elapsedRealtime() < end) { "Timed out: $label" }
            SystemClock.sleep(25)
        }
        transportFailure.get()?.let { error(it) }
    }

    private fun find(root: AccessibilityNodeInfo?, predicate: (AccessibilityNodeInfo) -> Boolean): AccessibilityNodeInfo? {
        if (root == null || !root.refresh()) return null
        if (root.isVisibleToUser && predicate(root)) return root
        for (index in 0 until root.childCount) find(root.getChild(index), predicate)?.let { return it }
        return null
    }

    private fun visible(label: String) = find(uiAutomation.rootInActiveWindow) {
        it.text?.toString() == label || it.contentDescription?.toString() == label
    }

    private fun action(label: String): AccessibilityNodeInfo {
        var found: AccessibilityNodeInfo? = null
        await(label) { found = visible(label); found != null }
        var current = checkNotNull(found)
        while (!current.isClickable && current.parent != null) current = checkNotNull(current.parent)
        check(current.refresh() && current.isVisibleToUser && current.isEnabled && current.isClickable) { "Inactive action: $label" }
        return current
    }

    /** One physical DOWN/UP pair on a freshly checked action; no retry of a mutation. */
    private fun tap(label: String) {
        uiAutomation.waitForIdle(100, 2_000)
        var bounds = Rect()
        var owner: AccessibilityNodeInfo? = null
        var previous: Rect? = null
        await("stable physical target $label") {
            owner = action(label)
            bounds = Rect().also { checkNotNull(owner).getBoundsInScreen(it) }
            val stable = previous == bounds
            previous = Rect(bounds)
            stable
        }
        val node = checkNotNull(owner)
        val screen = targetContext.resources.displayMetrics
        check(!bounds.isEmpty && bounds.left >= 0 && bounds.top >= 0 && bounds.right <= screen.widthPixels && bounds.bottom <= screen.heightPixels)
        check(node.windowId == checkNotNull(uiAutomation.rootInActiveWindow).windowId)
        val down = SystemClock.uptimeMillis()
        for (type in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), type, bounds.exactCenterX(), bounds.exactCenterY(), 0)
            event.source = InputDevice.SOURCE_TOUCHSCREEN
            try { check(uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
            if (type == MotionEvent.ACTION_DOWN) SystemClock.sleep(80)
        }
    }

    private fun edit(text: String) {
        val editor = checkNotNull(find(uiAutomation.rootInActiveWindow) { it.isEditable })
        check(editor.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
        }))
    }

    private fun reveal(label: String) {
        repeat(12) {
            if (visible(label) != null) return
            val scroll = find(uiAutomation.rootInActiveWindow) { it.isScrollable }
            check(scroll != null && scroll.performAction(AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD)) { "Could not reveal $label" }
            uiAutomation.waitForIdle(100, 1_000)
        }
        check(visible(label) != null) { "Could not reveal $label" }
    }

    private fun screenshot(name: String) {
        uiAutomation.takeScreenshot()?.let { bitmap ->
            targetContext.getExternalFilesDir(null)?.resolve("mobile-delivery-recovery-$scenario-$name.png")?.outputStream()?.use {
                check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it))
            }
            bitmap.recycle()
        }
    }

    private fun set(model: ZerusViewModel, name: String, value: Any) {
        model.javaClass.declaredMethods.single { it.name == name }.apply { isAccessible = true }.invoke(model, value)
    }

    private inner class Network : Interceptor {
        val calls = mutableListOf<Pair<String, String>>()
        val original = AtomicReference(originalId)
        val receiptReads = AtomicInteger()
        val holdRecoveryReceipt = CountDownLatch(if (scenario == "recovery") 1 else 0)
        val holdTimeoutReceipt = AtomicBoolean(false)
        val confirmOriginal = AtomicBoolean(false)
        val release = AtomicBoolean(false)
        val timedOutSendCancelled = AtomicBoolean(false)
        val timedOutReceiptCancelled = AtomicBoolean(false)
        private val now = System.currentTimeMillis() / 1000.0
        val raw = JSONObject().put("name", target.session).put("run_id", target.run).put("conversation_id", target.conversation)
            .put("tracked", true).put("runtime_state", "live").put("process_state", "running")
            .put("activity", "idle").put("phase", "idle").put("agent", "codex")
            .put("events", JSONArray().put(JSONObject().put("type", "AgentMessage").put("seq", 1).put("at", now - 20).put("detail", "A retained fixture reply.")))
            .put("pending_questions", JSONArray().put(JSONObject().put("question_id", "fixture-optional-question").put("question_hash", "fixture-question-hash")
                .put("run_id", target.run).put("conversation_id", target.conversation).put("source", "codex_async").put("optional", true)
                .put("can_answer", false).put("can_skip", true).put("created_at", now - 172800)
                .put("answer_unavailable_reason", "Synthetic optional request is unavailable for input.")
                .put("questions", JSONArray().put(JSONObject().put("id", "q_0").put("question", "Independent optional fixture request")
                    .put("allow_other", true).put("options", JSONArray().put(JSONObject().put("id", "opt_0").put("label", "Fixture option")))))))

        fun posted() = synchronized(calls) { calls.toList() }
        fun acknowledgement(id: String, text: String) = JSONObject().put("request_id", id).put("state", "completed").put("result",
            JSONObject().put("request_id", id).put("name", target.session).put("run_id", target.run).put("conversation_id", target.conversation)
                .put("status", "submitted").put("submitted_text", text).put("submitted_at", System.currentTimeMillis() / 1000.0))

        private fun hold(chain: Interceptor.Chain, cancelled: AtomicBoolean): Nothing {
            val end = SystemClock.elapsedRealtime() + 70_000
            while (!release.get() && !chain.call().isCanceled() && SystemClock.elapsedRealtime() < end) SystemClock.sleep(20)
            if (chain.call().isCanceled()) cancelled.set(true)
            throw IOException("Synthetic held HTTP call ended without an acknowledgement")
        }

        override fun intercept(chain: Interceptor.Chain): Response {
            val request = chain.request()
            fun response(value: JSONObject, code: Int = 200) = Response.Builder().request(request).protocol(Protocol.HTTP_1_1)
                .code(code).message(if (code == 200) "OK" else "Synthetic unavailable")
                .body(value.toString().toResponseBody("application/json".toMediaType())).build()
            try {
                check(request.url.host == "relay.example.test")
                if (request.method == "GET") {
                    val capabilities = JSONObject().put("protocol_version", 1).put("operations", JSONArray(listOf("inspect", "send", "voice_profiles")))
                    return when (request.url.encodedPath) {
                        "/v1/capabilities" -> response(capabilities)
                        "/v1/computers" -> response(JSONObject().put("computers", JSONArray().put(JSONObject()
                            .put("id", target.computerId).put("name", "Fixture computer").put("online", true)
                            .put("snapshot", JSONObject().put("sessions", JSONArray().put(raw)).put("mobile_capabilities", capabilities)))))
                        "/v1/requests/${original.get()}" -> {
                            receiptReads.incrementAndGet()
                            check(holdRecoveryReceipt.await(20, TimeUnit.SECONDS)) { "Synthetic receipt release not reached" }
                            if (holdTimeoutReceipt.get()) hold(chain, timedOutReceiptCancelled)
                            if (confirmOriginal.get()) response(acknowledgement(original.get(), originalText))
                            else response(JSONObject().put("error", "Synthetic original receipt is not available"), 404)
                        }
                        else -> throw IOException("Unexpected fixture GET ${request.url.encodedPath}")
                    }
                }
                check(request.method == "POST" && request.url.encodedPath == "/v1/requests")
                val input = JSONObject(okio.Buffer().also { request.body?.writeTo(it) }.readUtf8())
                check(input.string("session") == target.session)
                val id = input.getString("request_id")
                return when (input.getString("operation")) {
                    "inspect" -> response(JSONObject().put("request_id", id).put("state", "completed").put("result", synchronized(raw) { JSONObject(raw.toString()) }))
                    "voice_profiles" -> response(JSONObject().put("request_id", id).put("state", "completed").put("result", JSONObject().put("schema", 1).put("computers", JSONArray())))
                    "send" -> {
                        val payload = input.getJSONObject("payload")
                        check(payload.string("request_id") == id && payload.string("expected_run_id") == target.run && payload.string("expected_conversation_id") == target.conversation)
                        val text = payload.getString("text")
                        val number = synchronized(calls) { calls += id to text; calls.size }
                        if (scenario == "timeout" && number == 1) {
                            check(text == originalText)
                            original.set(id)
                            hold(chain, timedOutSendCancelled)
                        }
                        check(id != original.get() && text == nextText) { "An old payload or request was reposted" }
                        response(acknowledgement(id, text))
                    }
                    else -> throw IOException("Unexpected fixture operation ${input.string("operation")}")
                }
            } catch (error: Exception) {
                if (!release.get() && !chain.call().isCanceled()) transportFailure.compareAndSet(null,
                    "Synthetic ${request.method} ${request.url.encodedPath}: ${error.message}")
                throw IOException("Synthetic ${request.method} ${request.url.encodedPath}: ${error.message}", error)
            }
        }
    }

    private fun smoke() {
        val store = PrivateStore(targetContext)
        check(store.connections().isEmpty()) { "Use isolated app data without a connected workspace" }
        if (scenario != "restart") {
            check(store.messageState() == MessageState()) { "Use fresh isolated app data for recovery/timeout" }
            val initial = if (scenario == "timeout") emptyList() else listOf(OutgoingMessage(originalId, target, originalText, emptyList(),
                System.currentTimeMillis() / 1000.0 - 10, status = "uncertain", error = "Synthetic original delivery remains unknown."))
            store.saveMessageState(MessageState(drafts = listOf(Draft(target, if (scenario == "timeout") originalText else nextText)), outgoing = initial))
        } else {
            val disk = PrivateStore(targetContext).messageState()
            check(disk.outgoing.single().let { it.requestId == originalId && it.status == "uncertain" && it.deliveryDismissed && it.text == originalText })
            check(disk.drafts.single().let { it.target == target && it.text == nextText && it.detachedId.isBlank() })
        }
        val network = Network()
        val client = OkHttpClient.Builder().retryOnConnectionFailure(false).addInterceptor(network).build()
        val activity = startActivitySync(Intent(targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
        val model = main { activity.javaClass.getDeclaredMethod("getModel").apply { isAccessible = true }.invoke(activity) as ZerusViewModel }
        try {
            await("private storage") { main { model.storageReady } }
            main {
                model.javaClass.getDeclaredField("api").apply { isAccessible = true }.set(model, RelayApi(client))
                set(model, "setConnections", listOf(Connection(target.connectionId, "Fixture workspace", "https://relay.example.test", "fixture-token")))
                set(model, "setMachines", listOf(Machine(target.connectionId, target.computerId, "Fixture computer", true)))
                set(model, "setRelayOperations", mapOf(target.connectionId to setOf("inspect", "send")))
                set(model, "setMachineOperations", mapOf(MachineKey(target.connectionId, target.computerId) to setOf("inspect", "send")))
                model.open(Session(target, "Delivery recovery fixture", "codex", "idle", "", "", 0, network.raw))
            }
            await("verified fixture") { main { model.activityVerified && !model.detailBusy } }
            when (scenario) {
                "recovery" -> recovery(model, network)
                "restart" -> restart(model, network)
                "timeout" -> timeout(model, network)
            }
            check(network.posted().none { it.first == originalId })
            screenshot("passed")
        } finally {
            network.release.set(true)
            network.holdRecoveryReceipt.countDown()
            client.dispatcher.cancelAll()
            main { activity.finish() }
            client.connectionPool.evictAll()
            client.dispatcher.executorService.shutdown()
        }
    }

    private fun dismiss(model: ZerusViewModel, id: String) {
        tap("Review previous message")
        await("exact delivery review") { visible("Review delivery") != null }
        check(visible("Question unavailable") == null && visible("Prepare again") == null)
        check(visible("This only removes the warning from this phone. Delivery may still be confirmed later.") != null)
        tap("Dismiss warning")
        await("durable local dismissal") { main { model.outgoing.single { it.requestId == id }.deliveryDismissed && !model.sendingBlocked(target) } && visible("Previous message not confirmed") == null }
        val disk = PrivateStore(targetContext).messageState()
        check(disk.outgoing.single { it.requestId == id }.let { it.status == "uncertain" && it.deliveryDismissed })
        check(disk.drafts.single { it.target == target && it.questionId.isBlank() && it.detachedId.isBlank() }.text == nextText)
    }

    private fun recovery(model: ZerusViewModel, network: Network) {
        await("blocked original and receipt lookup") { network.receiptReads.get() > 0 && main {
            model.sendingBlocked(target) && "outgoing:$originalId" in model.receiptFlights && model.draft(target).text == nextText
        } }
        val captured = main { model.outgoing.single() }
        val draft = main { model.draft(target) }
        tap("Questions (1)")
        await("independent expanded question") { visible("Independent optional fixture request") != null && visible("Collapse") != null }
        sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        await("expanded question closed") { visible("Collapse") == null }
        check(main { model.draft(target) == draft })
        dismiss(model, originalId)
        check(main { "outgoing:$originalId" in model.receiptFlights }) { "Dismissal waited for or cancelled the read-only receipt lookup" }
        val disk = PrivateStore(targetContext).messageState()
        check(disk.outgoing.single() == captured.copy(deliveryDismissed = true) && disk.drafts.single() == draft)
        network.holdRecoveryReceipt.countDown()
        await("original lookup settled without releasing the retained identity") { main { "outgoing:$originalId" !in model.receiptFlights } }
        check(main { model.outgoing.single() == captured.copy(deliveryDismissed = true) && model.draft(target) == draft })
        check(network.posted().isEmpty())
        observations += "physical_dismiss_while_checking=true"
        observations += "independent_question_and_new_composition_preserved=true"
        observations += "disk_dismissed_original_unknown=true"
        observations += "original_posts=0; cold_restart_requires_scenario_restart"
    }

    private fun restart(model: ZerusViewModel, network: Network) {
        check(main { model.outgoing.single().deliveryDismissed && !model.sendingBlocked(target) && model.draft(target).text == nextText })
        check(visible("Previous message not confirmed") == null && visible("Question unavailable") == null)
        screenshot("cold-reloaded")
        tap("Send message")
        await("one distinct next message accepted") { network.posted().size == 1 && main { model.outgoing.any { it.text == nextText && it.status == "submitted" } } }
        val sent = network.posted().single()
        check(sent.first != originalId && sent.second == nextText)
        check(main { model.outgoing.single { it.requestId == originalId }.let { it.status == "uncertain" && it.deliveryDismissed && it.text == originalText } })
        network.confirmOriginal.set(true)
        reveal("Check delivery")
        tap("Check delivery")
        await("late exact receipt still reconciles") { main { model.outgoing.single { it.requestId == originalId }.let { it.status == "submitted" && it.deliveryDismissed } } }
        synchronized(network.raw) {
            network.raw.getJSONArray("events").put(JSONObject().put("type", "UserPromptSubmit").put("seq", 2)
                .put("source", "hgs_delivery").put("message_id", originalId)
                .put("run_id", target.run).put("conversation_id", target.conversation)
                .put("at", System.currentTimeMillis() / 1000.0).put("detail", originalText).put("request_id", originalId))
            check(NativeParser.events(network.raw).any { it.source == "hgs_delivery" && it.requestId == originalId })
        }
        main { model.refreshActivity(explicit = true) }
        await("late native identity still records the original") { main { model.outgoing.single { it.requestId == originalId }.let { it.status == "recorded" && it.deliveryDismissed } } }
        check(network.posted() == listOf(sent))
        observations += "cold_reload_preserved_next_composition=true"
        observations += "next_distinct_posts=1; original_posts=0"
        observations += "dismissed_original_late_receipt_and_native_confirmation=true"
    }

    private fun timeout(model: ZerusViewModel, network: Network) {
        val began = SystemClock.elapsedRealtime()
        tap("Send message")
        await("held original POST") { network.posted().size == 1 && main { model.outgoing.single().status == "sending" } }
        val id = network.original.get()
        edit(nextText)
        await("next editor remains usable during held POST") { main { model.draft(target).text == nextText } }
        await("whole send campaign timeout", 50_000) { main { model.outgoing.single { it.requestId == id }.status == "uncertain" } }
        val sendElapsed = SystemClock.elapsedRealtime() - began
        check(sendElapsed in 40_000..50_000) { "Unexpected original send campaign duration $sendElapsed" }
        await("HTTP send cancellation observed") { network.timedOutSendCancelled.get() }
        await("automatic receipt probe settles") { network.receiptReads.get() > 0 && main { "outgoing:$id" !in model.receiptFlights } }
        network.holdTimeoutReceipt.set(true)
        val before = network.receiptReads.get()
        val checked = SystemClock.elapsedRealtime()
        tap("Check previous message delivery")
        await("owned held receipt lookup") { network.receiptReads.get() > before && main { "outgoing:$id" in model.receiptFlights } }
        dismiss(model, id)
        tap("Send message")
        await("distinct next send while original check remains pending") { network.posted().size == 2 && main {
            "outgoing:$id" in model.receiptFlights && model.outgoing.any { it.text == nextText && it.status == "submitted" }
        } }
        await("owned receipt timeout", 50_000) { main { "outgoing:$id" !in model.receiptFlights && model.error.startsWith("Receipt check timed out.") } }
        val receiptElapsed = SystemClock.elapsedRealtime() - checked
        check(receiptElapsed in 40_000..50_000) { "Unexpected receipt campaign duration $receiptElapsed" }
        await("HTTP receipt cancellation observed") { network.timedOutReceiptCancelled.get() }
        val calls = network.posted()
        check(calls.size == 2 && calls.map { it.first }.distinct().size == 2 && calls[0] == (id to originalText) && calls[1].second == nextText)
        check(main { model.outgoing.single { it.requestId == id }.let { it.status == "uncertain" && it.deliveryDismissed && it.text == originalText } })
        observations += "send_campaign_timeout_ms=$sendElapsed"
        observations += "receipt_campaign_timeout_ms=$receiptElapsed"
        observations += "physical_dismiss_and_distinct_next_send_while_checking=true"
        observations += "original_posts=1; next_distinct_posts=1; reposts=0"
    }
}
