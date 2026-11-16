package app.zerus.mobile

import app.zerus.mobile.replica.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.util.Base64

class TaskMediaSelectionTest {
    @get:Rule val folders = TemporaryFolder()
    @Test fun selectedCommentStaysImmutableAfterRowsArriveOnEveryReadableSchema() {
        val data = JSONObject(javaClass.classLoader!!.getResource("task-comment-media.json")!!.readText()).getJSONArray("cases")
        for (index in 0 until data.length()) TaskStore(folders.newFolder().path, "project", "Fixture").use { store ->
            val row = data.getJSONObject(index); val task = row.getString("task")
            fun import(name: String) { assertTrue(store.importUpdates(listOf(DocumentUpdate(task, Base64.getDecoder().decode(row.getString(name))))).refused.isEmpty()) }
            import("before")
            val original = store.get(task)
            assertEquals(if (row.getBoolean("sealed")) 8L else 3L, original.schema)
            assertFalse(original.writable)
            val entry = original.log.single { it.text == "Original comment" }
            assertEquals(1u, entry.attachmentCount)
            val metadata = store.media(task, entry.entryId).single()
            val selected = TaskMediaSelection(task, entry.entryId, metadata.token)
            assertArrayEquals("original-comment".toByteArray(), selected.read(store).data)
            import("after")
            assertTrue(store.get(task).log.any { it.text == "Arrived while viewing" })
            assertArrayEquals("original-comment".toByteArray(), selected.read(store).data)
            val later = store.get(task).log.single { it.text == "Arrived while viewing" }
            assertTrue(runCatching { selected.copy(entry = later.entryId).read(store) }.isFailure)
            assertTrue(runCatching { selected.copy(entry = null).read(store) }.isFailure)
            assertTrue(runCatching { selected.copy(task = java.util.UUID.randomUUID().toString()).read(store) }.isFailure)
            assertTrue(runCatching { store.comment(task, "No ordinary writer widening") }.isFailure)
        }
    }
}
