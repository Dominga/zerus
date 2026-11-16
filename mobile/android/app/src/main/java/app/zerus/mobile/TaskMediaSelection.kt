package app.zerus.mobile

import app.zerus.mobile.replica.TaskMediaContent
import app.zerus.mobile.replica.TaskStore

/** Immutable selected entry and file, independent of later history row positions. */
internal data class TaskMediaSelection(val task: String, val entry: String?, val token: String) {
    fun read(store: TaskStore): TaskMediaContent {
        val metadata = store.media(task, entry)
        val index = metadata.indexOfFirst { it.token == token }
        check(index >= 0 && metadata.count { it.token == token } == 1) { "Refresh this task to open its media." }
        return store.readMedia(task, entry, index.toUInt(), token)
    }
}
