package app.zerus.mobile

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.zerus.mobile.replica.TaskStage
import org.json.JSONObject

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun TaskEditorSheet(model: ZerusViewModel) {
    val state = model.taskEditorState ?: return
    var ticketJson by rememberSaveable { mutableStateOf<String?>(null) }
    var discard by remember(state.token) { mutableStateOf(false) }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        val raw = ticketJson
        ticketJson = null
        if (raw != null) runCatching {
            val value = JSONObject(raw)
            TaskEditorPicker(value.getString("token"), value.getString("draft"),
                TaskEditDraftScope.parse(value.getJSONObject("scope")), value.getBoolean("comment"))
        }.getOrNull()?.let { model.taskEditorPicked(it, uris) }
    }
    fun attach(comment: Boolean) = model.taskEditorPicker(state.token, comment) { ticket ->
        ticketJson = JSONObject().put("token", ticket.token).put("draft", ticket.draftId).put("scope", ticket.scope.json())
            .put("comment", ticket.comment).toString()
        runCatching { picker.launch(arrayOf("*/*")) }.onFailure { model.taskEditorPicked(ticket, emptyList()) }
    }
    val live = state.session != null && state.saved == null && !state.complete
    val content = live && state.task.stage in listOf(TaskStage.TODO, TaskStage.BACKLOG)
    ModalBottomSheet(onDismissRequest = model::closeTaskEditor, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().fillMaxHeight(.94f).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).padding(bottom = 28.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(state.task.id, style = MaterialTheme.typography.titleLarge)
            Text(state.context.computer.name, style = MaterialTheme.typography.bodySmall)
            when {
                state.loading -> { LinearProgressIndicator(Modifier.fillMaxWidth()); Text(stringResource(R.string.task_edit_prepare)) }
                state.complete || state.saved?.state == TaskEditDraftState.Completed -> Text(stringResource(R.string.task_edit_saved))
                state.saved != null -> {
                    Text(stringResource(R.string.task_edit_saved_draft), style = MaterialTheme.typography.titleMedium)
                    Text(stringResource(if (state.saved.state in setOf(TaskEditDraftState.Pending, TaskEditDraftState.Unknown)) R.string.task_edit_unknown else R.string.task_edit_draft_kept))
                }
            }
            state.form?.let { form ->
                if (live) {
                    if (content) {
                        OutlinedTextField(form.title, { value -> model.changeTaskEditor(state.token) { it.copy(title = value) } },
                            Modifier.fillMaxWidth(), enabled = !state.busy, label = { Text(stringResource(R.string.task_edit_title)) })
                        OutlinedTextField(form.text, { value -> model.changeTaskEditor(state.token) { it.copy(text = value) } },
                            Modifier.fillMaxWidth(), enabled = !state.busy, label = { Text(stringResource(R.string.task_edit_requirement)) }, minLines = 3)
                    } else { Text(form.title); Text(form.text) }
                    if (content) {
                        form.kept.forEach { file ->
                            Row(Modifier.fillMaxWidth()) {
                                TaskMediaLink(model, state.context.key, state.task.uuid, file, Modifier.weight(1f))
                                TextButton(onClick = { model.changeTaskEditor(state.token) { it.copy(kept = it.kept.filterNot { kept -> kept.token == file.token }) } }, enabled = !state.busy) {
                                    Text(stringResource(R.string.task_media_remove))
                                }
                            }
                        }
                        form.files.forEach { file ->
                            Row(Modifier.fillMaxWidth()) {
                                TextButton(onClick = { model.previewTaskDraftMedia(file) }, modifier = Modifier.weight(1f)) { Text(file.name) }
                                TextButton(onClick = { model.changeTaskEditor(state.token) { it.copy(files = it.files.filterNot { picked -> picked.id == file.id }) } }, enabled = !state.busy) {
                                    Text(stringResource(R.string.task_media_remove))
                                }
                            }
                        }
                        OutlinedButton(onClick = { attach(false) }, enabled = !state.busy) { Text(stringResource(R.string.task_media_attach)) }
                        TaskEditorTarget(model, state)
                    }
                    OutlinedTextField(form.comment, { value -> model.changeTaskEditor(state.token) { it.copy(comment = value) } },
                        Modifier.fillMaxWidth(), enabled = !state.busy, label = { Text(stringResource(R.string.task_edit_comment)) }, minLines = 2)
                    form.commentFiles.forEach { file ->
                        Row(Modifier.fillMaxWidth()) {
                            TextButton(onClick = { model.previewTaskDraftMedia(file) }, modifier = Modifier.weight(1f)) { Text(file.name) }
                            TextButton(onClick = { model.changeTaskEditor(state.token) { it.copy(commentFiles = it.commentFiles.filterNot { picked -> picked.id == file.id }) } }, enabled = !state.busy) {
                                Text(stringResource(R.string.task_media_remove))
                            }
                        }
                    }
                    OutlinedButton(onClick = { attach(true) }, enabled = !state.busy) { Text(stringResource(R.string.task_media_comment_attach)) }
                    Button(onClick = { model.saveTaskEditor(state.token) }, enabled = !state.busy && form.changed(state.session!!.view)) {
                        Text(stringResource(if (state.busy) R.string.task_edit_saving else R.string.task_edit_save))
                    }
                } else {
                    Text(form.title, style = MaterialTheme.typography.titleMedium); Text(form.text)
                    if (form.comment.isNotEmpty()) Text(form.comment)
                    form.kept.forEach { file -> TaskMediaLink(model, state.context.key, state.task.uuid, file) }
                    form.commentFiles.forEach { file -> TextButton(onClick = { model.previewTaskDraftMedia(file) }) { Text(file.name) } }
                    form.files.forEach { file -> TextButton(onClick = { model.previewTaskDraftMedia(file) }) { Text(file.name) } }
                }
            }
            if (state.error.isNotBlank()) Text(state.error, color = MaterialTheme.colorScheme.error)
            if (state.saved?.state in setOf(TaskEditDraftState.Editing, TaskEditDraftState.Refused, TaskEditDraftState.Frozen)) {
                TextButton(onClick = { discard = true }) { Text(stringResource(R.string.task_edit_discard)) }
            }
            TextButton(onClick = model::closeTaskEditor, enabled = !state.busy) { Text(stringResource(R.string.task_edit_close)) }
        }
    }
    if (discard) AlertDialog(onDismissRequest = { discard = false }, text = { Text(stringResource(R.string.task_edit_discard_question)) },
        confirmButton = { TextButton(onClick = { discard = false; model.discardTaskEditor(state.token) }) { Text(stringResource(R.string.task_edit_discard)) } },
        dismissButton = { TextButton(onClick = { discard = false }) { Text(stringResource(R.string.task_edit_cancel)) } })
}

@Composable private fun TaskEditorTarget(model: ZerusViewModel, state: TaskEditorState) {
    var expanded by remember(state.token) { mutableStateOf(false) }
    Box {
        OutlinedButton(onClick = { expanded = true }, enabled = !state.busy) { Text(stringResource(R.string.task_edit_target)) }
        DropdownMenu(expanded, { expanded = false }) {
            DropdownMenuItem(text = { Text(stringResource(R.string.task_edit_keep_target)) }, onClick = { model.changeTaskEditor(state.token) { it.copy(target = null) }; expanded = false })
            DropdownMenuItem(text = { Text(stringResource(R.string.task_edit_any)) }, onClick = { model.chooseTaskEditorAny(state.token); expanded = false })
            model.taskEditorSessions(state.context.key).forEach { session ->
                DropdownMenuItem(text = { Text(session.title.ifBlank { session.target.session }) }, onClick = {
                    model.chooseTaskEditorSession(state.token, session); expanded = false
                })
            }
        }
    }
    if (state.task.stage == TaskStage.TODO) OutlinedButton(onClick = { model.moveTaskEditorTop(state.token) }, enabled = !state.busy) { Text(stringResource(R.string.task_edit_top)) }
}

@Composable internal fun TaskMediaLink(model: ZerusViewModel, key: ProjectKey, task: String,
    file: app.zerus.mobile.replica.TaskMediaInfo, modifier: Modifier = Modifier) {
    val state = model.taskMediaState(key, task, file)
    Column(modifier) {
        TextButton(onClick = { model.previewTaskMedia(key, task, file) }, enabled = state?.loading != true) {
            Text(file.name)
            if (state?.loading == true) { Spacer(Modifier.width(8.dp)); CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp) }
        }
        if (!state?.error.isNullOrEmpty()) Text(state!!.error, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
    }
}
