package app.zerus.mobile

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

internal data class TaskEditCheckpoint(val sequence: Long, val changes: String)

/** The original debounce strategy, isolated so blocked-write behavior is executable. */
internal class TaskEditCheckpoints(private val scope: CoroutineScope, private val debounceMs: Long = 150,
    private val write: suspend (TaskEditCheckpoint) -> Unit,
    private val failed: (TaskEditCheckpoint, Exception) -> Unit = { _, _ -> }) {
    private var worker: Job? = null
    private var failure: Exception? = null
    fun offer(value: TaskEditCheckpoint) {
        worker?.cancel()
        worker = scope.launch(start = CoroutineStart.UNDISPATCHED) {
            try { delay(debounceMs); write(value); failure = null }
            catch (error: Exception) { failure = error; failed(value, error) }
        }
    }
    suspend fun flush(value: TaskEditCheckpoint) { offer(value); worker?.join(); failure?.let { throw it } }
    fun close() { worker?.cancel(); worker = null }
}
