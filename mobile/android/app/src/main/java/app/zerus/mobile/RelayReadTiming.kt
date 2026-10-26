package app.zerus.mobile

import android.util.Log
import kotlinx.coroutines.asContextElement
import kotlinx.coroutines.withContext
import java.util.concurrent.atomic.AtomicInteger

/** Opt-in diagnostic preview only: fixed stage names and monotonic durations, never request data. */
internal object RelayReadTiming {
    enum class Stage {
        MembershipHttp, SubmitHttp, ReceiptHttp, CatalogHttp, EventsHttp, OtherHttp,
        MembershipSync, MembershipLock, Seal, Reserve, OpenResult, PollDelay, Read,
        OpenConversation, VisibleChecking, VisibleChecked, VisibleHistory,
    }
    enum class Operation { Inspect, History, Projection, Catalog, Other }
    private data class Trace(val sequence: Int, val operation: Operation)
    private val current = ThreadLocal<Trace?>()
    private val next = AtomicInteger(1)

    suspend fun <T> read(operation: Operation, block: suspend () -> T): T {
        if (!BuildConfig.READ_TIMING_ENABLED) return block()
        return withContext(current.asContextElement(Trace(next.getAndIncrement(), operation))) {
            measure(Stage.Read) { block() }
        }
    }

    fun httpStage(path: String, post: Boolean): Stage = when {
        path.startsWith("/v2/membership?") -> Stage.MembershipHttp
        path == "/v2/requests" && post -> Stage.SubmitHttp
        path.startsWith("/v2/requests/") -> Stage.ReceiptHttp
        path.startsWith("/v2/computers?") -> Stage.CatalogHttp
        path.startsWith("/v2/events?") -> Stage.EventsHttp
        else -> Stage.OtherHttp
    }

    fun begin(): Long = if (BuildConfig.READ_TIMING_ENABLED) System.nanoTime() else 0

    fun point(stage: Stage) = finish(stage, begin())

    fun finish(stage: Stage, start: Long) {
        if (BuildConfig.READ_TIMING_ENABLED) {
            val end = System.nanoTime()
            val trace = current.get()
            // Optional observation must never replace a read's result or error.
            runCatching {
                Log.d("ZerusReadTiming", "trace=${trace?.sequence ?: 0} operation=${trace?.operation ?: Operation.Other} stage=${stage.name} start_ms=${start / 1_000_000} duration_ms=${(end - start) / 1_000_000}")
            }
        }
    }

    inline fun <T> measure(stage: Stage, block: () -> T): T {
        val start = begin()
        try { return block() } finally { finish(stage, start) }
    }
}
