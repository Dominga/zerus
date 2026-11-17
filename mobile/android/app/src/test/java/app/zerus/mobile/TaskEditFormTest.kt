package app.zerus.mobile

import app.zerus.mobile.replica.SessionRef
import app.zerus.mobile.replica.TaskEditView
import app.zerus.mobile.replica.TaskMediaInfo
import app.zerus.mobile.replica.TaskTarget
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class TaskEditFormTest {
    private fun view() = TaskEditView("project", "task", "Original title", "Original body",
        TaskTarget.Session(SessionRef("machine", "old label", "launch_id:opaque")), "V", emptyList())
    @Test fun savedFormKeepsOriginalIdentityAndOnlyExplicitTargetChanges() {
        val original = view()
        val form = TaskEditForm.initial(original).copy(text = "Draft body", sequence = 7)
        val restored = TaskEditForm.read(form.json(), form.sequence)
        assertEquals(form, restored)
        assertNull(restored.target)
        assertEquals("launch_id:opaque", (restored.originalTarget as TaskTarget.Session).session.sessionId)
        assertTrue(restored.changed(original))
        assertEquals("Original body", original.text)
    }
    @Test fun refreshingTheDisplayedTaskDoesNotReplaceTheCapturedForm() {
        val original = view()
        val form = TaskEditForm.initial(original).copy(comment = "Person's comment")
        val refreshed = original.copy(title = "Remote replacement", target = TaskTarget.AnySession)
        assertEquals("Original title", form.title)
        assertNull(form.target)
        assertEquals("Remote replacement", refreshed.title)
        assertEquals(TaskEditForm.initial(original).originalTarget, form.originalTarget)
    }
    @Test fun malformedSavedFilesAndMediaSizesFailClosed() {
        val original = TaskEditForm.initial(view()).json()
        val malformed = JSONObject(original).put("files", org.json.JSONArray().put("not metadata"))
        assertTrue(runCatching { TaskEditForm.read(malformed.toString(), 0) }.isFailure)
        val file = TaskMediaInfo("file", "text/plain", "", 1u, "hash", "token")
        val media = JSONObject(TaskEditForm.initial(view().copy(attachments = listOf(file))).json())
        media.getJSONArray("kept").getJSONObject(0).put("size", -1)
        assertTrue(runCatching { TaskEditForm.read(media.toString(), 0) }.isFailure)
    }
}
