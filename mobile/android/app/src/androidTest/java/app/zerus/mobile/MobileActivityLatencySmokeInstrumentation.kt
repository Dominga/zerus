package app.zerus.mobile

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.graphics.Bitmap
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo
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
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** A serial computer queue with an unfinished large index; no real relay, agents or mutations. */
class MobileActivityLatencySmokeInstrumentation : Instrumentation() {
    private var scenario = "all"
    private val observations = mutableListOf<String>()
    private val transportFailure = AtomicReference<String?>(null)
    override fun onCreate(arguments: Bundle?) {
        scenario = arguments?.getString("scenario") ?: "all"
        super.onCreate(arguments)
        start()
    }
    override fun onStart() {
        val result = Bundle()
        try {
            check(BuildConfig.DEBUG && Build.HARDWARE in setOf("ranchu", "goldfish"))
            smoke()
            result.putString("stream", "Mobile Activity latency passed: ${observations.joinToString("; ")}. Zero mutations.\n")
            finish(Activity.RESULT_OK, result)
        } catch (error: Throwable) {
            screenshot("mobile-activity-latency-failed.png")
            result.putString("stream", "Mobile Activity latency failed: ${observations.joinToString("; ")}\ntransport=${transportFailure.get()}\n${error.stackTraceToString()}\n")
            finish(Activity.RESULT_CANCELED, result)
        }
    }
    private fun <T> main(action: () -> T): T {
        val value = AtomicReference<Result<T>>()
        runOnMainSync { value.set(runCatching(action)) }
        return value.get().getOrThrow()
    }
    private fun await(label: String, timeout: Long = 8_000, condition: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + timeout
        while (!condition()) {
            check(SystemClock.elapsedRealtime() < deadline) { "Timed out: $label; transport=${transportFailure.get()}" }
            SystemClock.sleep(20)
        }
    }
    private fun find(root: AccessibilityNodeInfo?, value: String): AccessibilityNodeInfo? {
        if (root == null) return null
        root.refresh()
        if (root.isVisibleToUser && root.text?.toString()?.contains(value) == true) return root
        for (index in 0 until root.childCount) find(root.getChild(index), value)?.let { return it }
        return null
    }
    private fun visible(value: String) = find(uiAutomation.rootInActiveWindow, value) != null
    private fun description(root: AccessibilityNodeInfo?, value: String): AccessibilityNodeInfo? {
        if(root==null) return null
        root.refresh()
        if(root.isVisibleToUser && root.contentDescription?.toString()==value) return root
        for(index in 0 until root.childCount) description(root.getChild(index),value)?.let { return it }
        return null
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
    private fun smoke() {
        val store = PrivateStore(targetContext)
        check(store.connections().isEmpty() && store.messageState() == MessageState()) { "Use fresh isolated emulator app data" }
        val target = Target("fixture-computer", "codex/fixture/long-activity", "fixture-run", "fixture-conversation", "fixture-workspace")
        val latest = "Recent reply at the actual conversation tail."
        val earlier = "Earlier reply in the retained reading window."
        fun nativeRow(seq: Int, body: String) = JSONObject().put("type", "AgentMessage").put("seq", seq).put("at", seq.toDouble()).put("detail", body)
        fun canonicalRow(seq: Int, body: String) = nativeRow(seq, body).put("history_id", "fixture-$seq")
            .put("history_cursor", "fixture-cursor-$seq").put("incoming_seq", JSONObject.NULL)
        val raw = JSONObject().put("name", target.session).put("run_id", target.run).put("conversation_id", target.conversation)
            .put("tracked", true).put("runtime_state", "live").put("process_state", "running")
            .put("activity", "idle").put("phase", "idle").put("agent", "codex")
            .put("events", JSONArray(listOf(nativeRow(900, latest))))
        val queue = Any()
        val headReads = AtomicInteger()
        val inspectionReads = AtomicInteger()
        val olderReads = AtomicInteger()
        val mutations = AtomicInteger()
        val emptyInspection = AtomicBoolean(false)
        val heldInspection = AtomicReference<CountDownLatch?>(null)
        val client = OkHttpClient.Builder().addInterceptor { chain ->
            val request = chain.request()
            fun response(value:JSONObject)=Response.Builder().request(request).protocol(Protocol.HTTP_1_1).code(200).message("OK")
                .body(value.toString().toResponseBody("application/json".toMediaType())).build()
            try {
            check(request.url.host == "relay.example.test") { "Unexpected synthetic host" }
            if(request.method=="GET") {
                val capabilities=JSONObject().put("protocol_version",1).put("operations",JSONArray(listOf("history","inspect","voice_profiles")))
                val value=when(request.url.encodedPath) {
                    "/v1/computers" -> {
                        val snapshot=JSONObject().put("sessions",JSONArray(listOf(raw))).put("mobile_capabilities",capabilities)
                        val computer=JSONObject().put("id",target.computerId).put("name","Fixture computer").put("online",true).put("snapshot",snapshot)
                        JSONObject().put("computers",JSONArray(listOf(computer)))
                    }
                    "/v1/capabilities" -> capabilities
                    else -> throw IOException("Unexpected synthetic GET route ${request.url.encodedPath}")
                }
                return@addInterceptor response(value)
            }
            check(request.method=="POST" && request.url.encodedPath=="/v1/requests") {
                "Unexpected synthetic route ${request.method} ${request.url.encodedPath}"
            }
            val input = JSONObject(okio.Buffer().also { request.body?.writeTo(it) }.readUtf8())
            val output = synchronized(queue) {
                when (input.string("operation")) {
                    "inspect" -> {
                        inspectionReads.incrementAndGet()
                        check(heldInspection.get()?.await(10,TimeUnit.SECONDS)!=false)
                        JSONObject(raw.toString()).also { if(emptyInspection.get()) it.put("events",JSONArray()) }
                    }
                    "history" -> {
                        val payload = input.getJSONObject("payload")
                        check(payload.string("expected_run_id") == target.run && payload.string("expected_conversation_id") == target.conversation)
                        val around = payload.has("around")
                        if (around) olderReads.incrementAndGet() else {
                            headReads.incrementAndGet()
                            // Current connectors spend two seconds advancing a huge old index before inspection can run.
                            SystemClock.sleep(2_000)
                        }
                        JSONObject().put("request_id", input.string("request_id")).put("name", target.session)
                            .put("run_id", target.run).put("conversation_id", target.conversation).put("history_epoch", "fixture-epoch")
                            .put("events", JSONArray(listOf(canonicalRow(100, earlier))))
                            .put("has_more_before", true).put("has_more_after", around)
                            .put("next_before", "fixture-cursor-100").put("next_after", if (around) "fixture-cursor-100" else JSONObject.NULL)
                            .put("head", JSONObject().put("complete", false).put("incoming_seq", JSONObject.NULL).put("total_incoming", JSONObject.NULL))
                            .put("indexing", true).put("truncated", true)
                    }
                    "voice_profiles" -> JSONObject().put("schema", 1).put("computers", JSONArray())
                    else -> { mutations.incrementAndGet(); error("Unexpected fixture operation ${input.string("operation")}") }
                }
            }
            response(JSONObject().put("request_id",input.string("request_id")).put("state","completed").put("result",output))
            } catch(error:Exception) {
                val failure="Synthetic ${request.method} ${request.url.encodedPath}: ${error.message}"
                transportFailure.compareAndSet(null,failure)
                throw IOException(failure,error)
            }
        }.build()
        val activity = startActivitySync(Intent(targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
        val model = main { activity.javaClass.getDeclaredMethod("getModel").apply { isAccessible = true }.invoke(activity) as ZerusViewModel }
        try {
            await("private storage and read tracking") { main { model.storageReady && model.javaClass.getDeclaredMethod("getReadTrackingReady").apply { isAccessible = true }.invoke(model) as Boolean } }
            main {
                model.javaClass.getDeclaredField("api").apply { isAccessible = true }.set(model, RelayApi(client))
                set(model, "setConnections", listOf(Connection(target.connectionId, "Fixture workspace", "https://relay.example.test", "fixture-token")))
                set(model, "setMachines", listOf(Machine(target.connectionId, target.computerId, "Fixture computer", true)))
                set(model, "setRelayOperations", mapOf(target.connectionId to setOf("history")))
                set(model, "setMachineOperations", mapOf(MachineKey(target.connectionId, target.computerId) to setOf("history")))
                if(scenario=="all") {
                    @Suppress("UNCHECKED_CAST")
                    val writer=model.javaClass.getDeclaredField("readWriter").apply { isAccessible=true }.get(model) as OrderedStatePersistence<ConversationReadState>
                    val stale=Event("fixture-old-read", "AgentMessage", "Old previously read reply", "", nativeType="AgentMessage",historyEpoch="fixture-epoch",incomingSeq=899)
                    val state=writer.edit { it.head(target,"fixture-epoch",899,899,true).visible(target,listOf(stale)) }
                    set(model,"setReadState",state)
                    set(model,"setCurrentHistoryHeads",setOf(ConversationReadPolicies.key(target)))
                }
            }
            uiAutomation.waitForIdle(200, 3_000)
            val session = Session(target, "Long conversation fixture", "codex", "idle", "", "", 0, raw)
            val opened = SystemClock.elapsedRealtime()
            main { model.open(session) }
            await("recent inspected reply is visible") { visible(latest) && main { model.activityVerified && model.readPositionReady && !model.detailBusy } }
            val openingMs = SystemClock.elapsedRealtime() - opened
            observations += "cold_recent_ms=$openingMs, head_reads=${headReads.get()}"
            check(main { model.hasOlder }) { "A bounded inspection without a truncation flag hid explicit earlier history" }
            check(main { model.unreadCount(target)==null }) { "A stale complete head claimed exact unread totals for a newer inspected reply" }
            if (scenario != "latest") {
                check(headReads.get() == 0) { "Opening recent Activity started full canonical history indexing before the user requested an older page" }
                check(openingMs < 2_000) { "Recent Activity took ${openingMs}ms despite an immediately available inspection" }
            }
            if (scenario != "opening") {
                await("opening reads settled") { main { !model.olderLoading && !model.detailBusy } }
                // This is the same request path used for a restored saved viewport, after its opaque cursor is verified.
                main {
                    model.javaClass.getDeclaredMethod("loadHistory", Target::class.java, String::class.java, String::class.java, Boolean::class.javaPrimitiveType)
                        .apply { isAccessible = true }.invoke(model, target, "around", "fixture-cursor-100", false)
                }
                await("older reading window") { main { !model.olderLoading && model.hasNewer && model.conversationEvents.any { it.text == earlier } } }
                val headsBefore = headReads.get()
                val jumped = SystemClock.elapsedRealtime()
                val job = main { model.jumpToLatest(target) }
                check(job != null) { "Latest navigation was not accepted" }
                await("latest navigation settled") { job!!.isCompleted && main { !model.olderLoading } }
                observations += "latest_ms=${SystemClock.elapsedRealtime() - jumped}, head_reads_added=${headReads.get() - headsBefore}"
                check(main { model.olderError.isBlank() && !model.hasNewer && model.conversationEvents.any { it.text == latest } }) {
                    "Latest navigation remained blocked by historical indexing: " + main { model.olderError }
                }
                check(headReads.get() == headsBefore) { "Latest navigation restarted complete historical indexing" }
                check(SystemClock.elapsedRealtime() - jumped < 2_000) { "Latest navigation waited for historical backfill" }
                await("recent reply rendered after latest") { visible(latest) }
                check(main { !model.historyComplete && model.unreadCount(target) == null }) { "Recent inspection invented complete unread evidence" }
                check(main { model.conversationViewport(target)?.following==true && model.conversationViewport(target)?.cursor.orEmpty().isBlank() }) {
                    "Latest navigation retained an older canonical reading position"
                }
                val headsBeforeReopen=headReads.get()
                main { model.back();model.open(session) }
                await("following tail reopens from recent inspection") { visible(latest) && main { model.activityVerified && !model.detailBusy && !model.olderLoading } }
                check(headReads.get()==headsBeforeReopen) { "Reopening the followed tail restarted canonical history indexing" }
                observations += "following_reopen_head_reads=${headReads.get()-headsBeforeReopen}"
                if(scenario=="all") {
                    // Force the disk display-cache path, whose metadata deliberately
                    // omits event streams. A following marker must retain its rows.
                    val inspectionRelease=CountDownLatch(1)
                    heldInspection.set(inspectionRelease)
                    main {
                        model.back()
                        (model.javaClass.getDeclaredField("frames").apply { isAccessible=true }.get(model) as MutableMap<*,*>).clear()
                        model.open(session)
                    }
                    await("following cached rows remain readable before fresh inspection") {
                        visible(latest) && main { model.historyCached && !model.activityVerified }
                    }
                    inspectionRelease.countDown();heldInspection.set(null)
                    await("cached following view freshly verified") { main { model.activityVerified && !model.detailBusy } }
                    check(headReads.get()==headsBeforeReopen) { "Cached following reopen requested canonical history" }
                    observations += "following_disk_cache_visible_before_network=true"
                    // A user may explicitly request old messages. An unfinished
                    // index keeps the recent view and offers one deliberate retry.
                    val beforeEarlier=headReads.get()
                    main { model.loadOlder(target) }
                    await("requested older history remains incomplete") { main { !model.olderLoading && model.olderError.isNotBlank() } }
                    check(headReads.get()==beforeEarlier+1)
                    check(main { model.historyIndexing && model.historyEpoch.isBlank() && model.hasOlder && !model.hasNewer && model.conversationEvents.any { it.text==latest } }) {
                        "Incomplete older history replaced or hid the actual recent view"
                    }
                    check(visible(latest))
                    main { model.retryHistory(target) }
                    await("one explicit retry completes its bounded read") { headReads.get()==beforeEarlier+2 && main { !model.olderLoading } }
                    SystemClock.sleep(500)
                    check(headReads.get()==beforeEarlier+2) { "Incomplete older reads started an uncontrolled automatic backfill loop" }
                    observations += "explicit_earlier_attempts=${headReads.get()-beforeEarlier}"

                    main {
                        model.javaClass.getDeclaredMethod("loadHistory",Target::class.java,String::class.java,String::class.java,Boolean::class.javaPrimitiveType)
                            .apply { isAccessible=true }.invoke(model,target,"around","fixture-cursor-100",false)
                    }
                    await("older viewport before an empty recent inspection") { main { !model.olderLoading && model.hasNewer && model.conversationEvents.any { it.text==earlier } } }
                    emptyInspection.set(true)
                    val emptyJump=main { model.jumpToLatest(target) }
                    check(emptyJump!=null)
                    await("empty recent inspection rejected") { emptyJump!!.isCompleted && main { !model.olderLoading } }
                    check(main { model.hasNewer && model.olderError.isNotBlank() && model.conversationEvents.any { it.text==earlier } }) {
                        "An empty bounded inspection erased the older readable window"
                    }
                    emptyInspection.set(false)
                    observations += "empty_recent_retains_older_view=true"

                    val headsBeforeRetry=headReads.get()
                    val inspectionsBeforeRetry=inspectionReads.get()
                    await("Retry latest messages action") { visible("Retry latest messages") }
                    var retry=checkNotNull(find(uiAutomation.rootInActiveWindow,"Retry latest messages"))
                    while(!retry.isClickable) retry=checkNotNull(retry.parent)
                    check(retry.isEnabled && retry.performAction(AccessibilityNodeInfo.ACTION_CLICK))
                    await("latest retry uses a fresh inspection") { inspectionReads.get()>inspectionsBeforeRetry && main { !model.olderLoading && !model.hasNewer && model.olderError.isBlank() && model.conversationEvents.any { it.text==latest } } }
                    check(headReads.get()==headsBeforeRetry) { "Retry latest messages retried the old canonical selector" }
                    observations += "latest_retry_inspects_without_canonical_read=true"

                    main {
                        model.javaClass.getDeclaredMethod("loadHistory",Target::class.java,String::class.java,String::class.java,Boolean::class.javaPrimitiveType)
                            .apply { isAccessible=true }.invoke(model,target,"around","fixture-cursor-100",false)
                    }
                    await("older viewport before pending-navigation UI check") { main { !model.olderLoading && model.hasNewer && model.conversationEvents.any { it.text==earlier } } }

                    // Latest remains a usable UI action while the previous
                    // older-history request still occupies the computer queue.
                    val pendingEarlier=headReads.get()
                    main { model.loadOlder(target) }
                    await("older navigation is pending") { main { model.olderLoading && model.hasNewer } && headReads.get()>pendingEarlier }
                    await("visible Latest action during older navigation") { description(uiAutomation.rootInActiveWindow,"Jump to latest messages")!=null }
                    var button=checkNotNull(description(uiAutomation.rootInActiveWindow,"Jump to latest messages"))
                    while(!button.isClickable) button=checkNotNull(button.parent)
                    check(button.isEnabled) { "Latest UI action was disabled by the older history request" }
                    check(button.performAction(AccessibilityNodeInfo.ACTION_CLICK))
                    await("Latest UI supersedes the pending older navigation") { main { !model.olderLoading && !model.hasNewer && model.olderError.isBlank() && model.conversationEvents.any { it.text==latest } } }
                    observations += "latest_ui_supersedes_pending_older=true"

                    main {
                        model.javaClass.getDeclaredMethod("loadHistory",Target::class.java,String::class.java,String::class.java,Boolean::class.javaPrimitiveType)
                            .apply { isAccessible=true }.invoke(model,target,"around","fixture-cursor-100",false)
                    }
                    await("older view before delayed cache race") { main { !model.olderLoading && model.hasNewer && model.conversationEvents.any { it.text==earlier } } }

                    val pageCache=model.javaClass.getDeclaredField("pageCache").apply { isAccessible=true }.get(model) as ConversationPageCache
                    val files=pageCache.javaClass.getDeclaredField("files").apply { isAccessible=true }.get(pageCache)
                    val diskLock=files.javaClass.getDeclaredField("diskLock").apply { isAccessible=true }.get(files)
                    val heldCache=CountDownLatch(1)
                    val cacheRelease=CountDownLatch(1)
                    val cacheHolder=Thread {
                        synchronized(diskLock) { heldCache.countDown();check(cacheRelease.await(10,TimeUnit.SECONDS)) }
                    }.apply { name="fixture-held-history-cache";start() }
                    try {
                        check(heldCache.await(2,TimeUnit.SECONDS))
                        main {
                            val old=model.conversationEvents.single { it.text==earlier }
                            model.saveConversationViewport(target,old.id,0,following=false)
                            // Start a retained around-page read at a null window,
                            // then let Latest leave the map null too (the ABA case).
                            (model.javaClass.getDeclaredField("historyWindows").apply { isAccessible=true }.get(model) as MutableMap<*,*>).clear()
                            val navigation=model.javaClass.getDeclaredField("navigationId").apply { isAccessible=true }.getLong(model)
                            model.javaClass.getDeclaredMethod("restoreSavedWindow",Target::class.java,Long::class.javaPrimitiveType)
                                .apply { isAccessible=true }.invoke(model,target,navigation)
                        }
                        val afterCache=main { model.jumpToLatest(target) }
                        check(afterCache!=null)
                        await("Latest settles while cached older restore is blocked") { afterCache!!.isCompleted && main { !model.olderLoading && !model.hasNewer && model.conversationEvents.any { it.text==latest } } }
                    } finally { cacheRelease.countDown();cacheHolder.join(2_000) }
                    uiAutomation.waitForIdle(300,3_000)
                    SystemClock.sleep(250)
                    check(main { !model.hasNewer && model.historyEpoch.isBlank() && model.conversationEvents.any { it.text==latest } && model.conversationEvents.none { it.text==earlier } }) {
                        "Delayed encrypted cached history replaced the accepted Latest view"
                    }
                    observations += "late_cached_older_restore_cannot_replace_latest=true"
                }
            }
            screenshot("mobile-activity-latency-passed.png")
            check(inspectionReads.get() > 0 && mutations.get() == 0)
        } finally {
            heldInspection.get()?.countDown()
            main { activity.finish() }
            client.connectionPool.evictAll()
            client.dispatcher.executorService.shutdown()
        }
    }
}
