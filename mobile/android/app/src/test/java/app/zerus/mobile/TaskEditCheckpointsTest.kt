package app.zerus.mobile

import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.util.Collections
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import javax.crypto.KeyGenerator

class TaskEditCheckpointsTest {
    @get:Rule val folders = TemporaryFolder()
    @Test fun blockedPrivateSyncRetainsOneActiveAndOnlyTheLatestPendingSnapshot() = runBlocking {
        val key = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
        val original = TaskEditDraftScope("connection", "swarm", "project", UUID.randomUUID().toString())
        val qualification = UUID.randomUUID().toString(); val edit = UUID.randomUUID().toString()
        val blocked = AtomicBoolean(false); val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val files = TaskEditDraftFiles(folders.newFolder(), { key }, syncDirectory = {
            if (blocked.get()) { entered.countDown(); check(release.await(10, TimeUnit.SECONDS)) { "Private checkpoint sync gate was not released" } }
        })
        val record = files.create(original, UUID.randomUUID().toString(), "host", qualification, edit, "original")
        files.reserve(original, record.draftId, qualification, "qualification")
        files.settle(original, record.draftId, qualification, TaskEditDraftOutcome.Acknowledged)
        val calls = Collections.synchronizedList(mutableListOf<Long>()); val done = AtomicInteger()
        val owner = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val writer = TaskEditCheckpoints(owner, debounceMs = 0, write = { value ->
            calls += value.sequence
            try { withContext(Dispatchers.IO) { files.edit(original, record.draftId, value.sequence, value.changes) } }
            finally { done.incrementAndGet() }
        })
        try {
            blocked.set(true); writer.offer(TaskEditCheckpoint(1, "first"))
            assertTrue("The private filesystem sync did not begin", withContext(Dispatchers.IO) { entered.await(10, TimeUnit.SECONDS) })
            for (sequence in 2L..8L) { writer.offer(TaskEditCheckpoint(sequence, "change $sequence")); yield(); delay(1) }
            val flushing = launch { writer.flush(TaskEditCheckpoint(8, "change 8")) }
            yield(); flushing.cancelAndJoin()
            assertEquals("Cancelled debounce tasks submitted more private writes while sync was blocked", listOf(1L), calls.toList())
        } finally {
            blocked.set(false); release.countDown()
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
            while (done.get() < calls.size && System.nanoTime() < deadline) delay(1)
            check(done.get() == calls.size) { "Private checkpoint callbacks did not physically settle: ${done.get()}/${calls.size}" }
            // Flush may have been cancelled as a waiter; its latest pending snapshot still belongs to this writer.
            writer.flush(TaskEditCheckpoint(8, "change 8")); writer.close(); owner.cancel()
        }
        assertEquals("change 8", files.read(original)!!.changes)
    }
}
