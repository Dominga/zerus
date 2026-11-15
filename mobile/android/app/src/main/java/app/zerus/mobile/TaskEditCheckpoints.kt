package app.zerus.mobile

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

internal data class TaskEditCheckpoint(val sequence: Long, val changes: String)

/** One physical writer and one replaceable pending snapshot. An offer never cancels I/O. */
internal class TaskEditCheckpoints(private val scope: CoroutineScope, private val debounceMs: Long = 150,
    private val write: suspend (TaskEditCheckpoint) -> Unit,
    private val failed: (TaskEditCheckpoint, Exception) -> Unit = { _, _ -> }) {
    private val lock = Any()
    private var worker: Job? = null
    private var latest: TaskEditCheckpoint? = null
    private var pending: TaskEditCheckpoint? = null
    private var failure: Exception? = null
    private var closed = false

    fun offer(value: TaskEditCheckpoint): Boolean = synchronized(lock) {
        val accepted = latest
        if (closed || !scope.isActive || value.sequence < 0 || accepted != null &&
            (value.sequence < accepted.sequence || value.sequence == accepted.sequence && value.changes != accepted.changes)) return@synchronized false
        latest = value; pending = value
        startLocked()
        true
    }
    private fun startLocked() {
        if (worker != null || closed || pending == null) return
        val job = scope.launch(start = CoroutineStart.LAZY) { pump() }
        worker = job
        job.invokeOnCompletion {
            synchronized(lock) {
                if (worker === job) {
                    worker = null
                    if (!scope.isActive) { closed = true; pending = null }
                    else startLocked()
                }
            }
        }
        job.start()
    }
    private suspend fun pump() {
        while (true) {
            if (synchronized(lock) { closed || pending == null }) return
            delay(debounceMs)
            val value = synchronized(lock) { if (closed) null else pending.also { pending = null } } ?: return
            try { write(value); synchronized(lock) { failure = null } }
            catch (error: CancellationException) { throw error }
            catch (error: Exception) { synchronized(lock) { failure = error }; failed(value, error) }
        }
    }
    /** Wait for the owned writer, rather than starting a competing flush write. */
    suspend fun flush(value: TaskEditCheckpoint) {
        check(offer(value)) { "The original task checkpoint is closed or superseded." }
        while (true) {
            val current = synchronized(lock) { worker }
            if (current != null) current.join()
            else {
                synchronized(lock) {
                    check(!closed) { "The original task checkpoint is closed." }
                    failure?.let { throw it }
                }
                return
            }
        }
    }
    /** Only discard pending work; an already entered write physically settles before its worker exits. */
    fun close() { synchronized(lock) { closed = true; pending = null } }
}
