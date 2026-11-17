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
import org.json.JSONObject

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun TaskContentSheet(model: ZerusViewModel) {
    val state = model.taskContentState ?: return
    var ticketJson by rememberSaveable { mutableStateOf<String?>(null) }
    var discard by remember(state.token) { mutableStateOf(false) }
    var targets by remember(state.token) { mutableStateOf(false) }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        val raw = ticketJson; ticketJson = null
        if (raw != null) runCatching {
            val value = JSONObject(raw)
            TaskContentPicker(value.getString("token"), TaskContentDraft.parse(value.getJSONObject("draft")))
        }.getOrNull()?.let { model.taskContentPicked(it, uris) }
    }
    ModalBottomSheet(onDismissRequest = model::closeTaskContent, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)) {
        Column(Modifier.fillMaxWidth().fillMaxHeight(.92f).verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(if (state.draft?.isNew == true) R.string.task_content_new else R.string.task_content_open), style = MaterialTheme.typography.titleLarge)
            Text(state.context.projectName + " / " + state.context.computer.name, style = MaterialTheme.typography.bodySmall)
            if (state.loading) LinearProgressIndicator(Modifier.fillMaxWidth())
            val live = !state.loading && !state.busy && !state.complete && !state.held
            val draft = state.draft
            if (draft != null) {
                val form = draft.form
                OutlinedTextField(form.text, { value -> model.changeTaskContent(state.token) { it.copy(text = value) } },
                    Modifier.fillMaxWidth(), enabled = live, label = { Text(stringResource(R.string.task_edit_requirement)) }, minLines = 3, maxLines = 12)
                form.kept.forEach { file ->
                    Row(Modifier.fillMaxWidth()) {
                        TaskMediaLink(model, state.context.key, draft.scope.task, file, Modifier.weight(1f))
                        TextButton(onClick = { model.changeTaskContent(state.token) { it.copy(kept = it.kept.filterNot { kept -> kept.token == file.token }) } }, enabled = live) {
                            Text(stringResource(R.string.task_media_remove))
                        }
                    }
                }
                form.files.forEach { file ->
                    Row(Modifier.fillMaxWidth()) {
                        TextButton(onClick = { model.previewTaskDraftMedia(file) }, modifier = Modifier.weight(1f)) { Text(file.name) }
                        TextButton(onClick = { model.removeTaskContentFile(state.token, file.id) }, enabled = live) {
                            Text(stringResource(R.string.task_media_remove))
                        }
                    }
                }
                OutlinedButton(onClick = { model.taskContentPicker(state.token) { ticket ->
                    ticketJson = JSONObject().put("token", ticket.token).put("draft", ticket.draft.json()).toString()
                    runCatching { picker.launch(arrayOf("*/*")) }.onFailure { model.taskContentPicked(ticket, emptyList()) }
                } }, enabled = live) { Text(stringResource(R.string.task_media_attach)) }
                Box {
                    OutlinedButton(onClick = { targets = true }, enabled = live) { Text(stringResource(R.string.task_edit_target) + ": " + model.taskContentTargetLabel(state)) }
                    DropdownMenu(targets, { targets = false }) {
                        DropdownMenuItem(text = { Text(stringResource(R.string.task_edit_keep_target)) }, onClick = { model.changeTaskContent(state.token) { it.copy(target = null) }; targets = false })
                        DropdownMenuItem(text = { Text(stringResource(R.string.task_edit_any)) }, onClick = { model.changeTaskContent(state.token) { it.copy(target = app.zerus.mobile.replica.TaskTarget.AnySession) }; targets = false })
                        model.taskEditorSessions(state.context.key).forEach { session ->
                            DropdownMenuItem(text = { Text(session.title.ifBlank { session.target.session }) }, onClick = { model.chooseTaskContentSession(state.token, session); targets = false })
                        }
                    }
                }
                if (state.complete) Text(stringResource(R.string.task_edit_saved))
                else Button(onClick = { model.saveTaskContent(state.token) }, enabled = live && form.text.isNotBlank()) { Text(stringResource(R.string.task_edit_save)) }
                if (live) TextButton(onClick = { discard = true }) { Text(stringResource(R.string.task_edit_discard)) }
            }
            if (state.error.isNotEmpty()) Text(state.error, color = MaterialTheme.colorScheme.error)
            TextButton(onClick = model::closeTaskContent, enabled = !state.busy) { Text(stringResource(R.string.task_edit_close)) }
        }
    }
    if (discard) AlertDialog(onDismissRequest = { discard = false }, text = { Text(stringResource(R.string.task_edit_discard_question)) },
        confirmButton = { TextButton(onClick = { discard = false; model.discardTaskContent(state.token) }) { Text(stringResource(R.string.task_edit_discard)) } },
        dismissButton = { TextButton(onClick = { discard = false }) { Text(stringResource(R.string.task_edit_cancel)) } })
}

/** One selected immutable entry, at most eight metadata rows; no history-sized media cache. */
@Composable internal fun TaskCommentFiles(model: ZerusViewModel, key: ProjectKey, task: String, entry: String) {
    var files by remember(key, task, entry) { mutableStateOf<List<app.zerus.mobile.replica.TaskMediaInfo>?>(null) }
    var error by remember(key, task, entry) { mutableStateOf("") }
    LaunchedEffect(key, task, entry) {
        try { files = model.taskCommentFiles(key, task, entry) }
        catch (cancel: kotlinx.coroutines.CancellationException) { throw cancel }
        catch (failure: Exception) { error = failure.message.orEmpty() }
    }
    if (files == null && error.isEmpty()) LinearProgressIndicator(Modifier.fillMaxWidth())
    files?.forEach { file -> TaskMediaLink(model, key, task, file, entry = entry) }
    if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error)
}
