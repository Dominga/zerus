package app.zerus.mobile

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

data class SavedExecutionRow(val label: String, val value: String?)

/** A small presentation value, never a task action or native observation. */
data class SavedExecutionSectionState(val available: Boolean, val hasCurrent: Boolean = false,
    val phase: String? = null, val notices: List<String> = emptyList(), val rows: List<SavedExecutionRow> = emptyList())

@Composable internal fun SavedExecutionSection(state: SavedExecutionSectionState) {
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(stringResource(R.string.saved_execution), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
        state.notices.forEach { Text(savedExecutionLabel(it), style = MaterialTheme.typography.bodySmall) }
        when {
            !state.available -> Text(stringResource(R.string.saved_execution_unavailable), style = MaterialTheme.typography.bodySmall)
            !state.hasCurrent -> Text(stringResource(R.string.saved_execution_no_current), style = MaterialTheme.typography.bodySmall)
            else -> state.phase?.let { Text(savedExecutionLabel(it), style = MaterialTheme.typography.bodyMedium) }
        }
        if (state.available) state.rows.forEach { row ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(savedExecutionLabel(row.label), Modifier.weight(1f), style = MaterialTheme.typography.labelMedium)
                SelectionContainer(Modifier.weight(2f)) {
                    Text(row.value ?: stringResource(R.string.saved_execution_not_recorded), style = MaterialTheme.typography.bodySmall)
                }
            }
        }
        Text(stringResource(R.string.saved_execution_footer), style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable private fun savedExecutionLabel(key: String): String = stringResource(when (key) {
    "Saved execution" -> R.string.saved_execution
    "Execution records conflict." -> R.string.saved_execution_conflict
    "Some attempt delivery is unknown." -> R.string.saved_execution_unknown
    "Some execution history is unexplained." -> R.string.saved_execution_unexplained
    "Legacy execution record" -> R.string.saved_execution_legacy
    "This saved execution could not be refreshed." -> R.string.saved_execution_stale
    "Attempt" -> R.string.saved_execution_attempt
    "Generation" -> R.string.saved_execution_generation
    "Requirement revision" -> R.string.saved_execution_requirement
    "Current requirement revision" -> R.string.saved_execution_current_requirement
    "Request" -> R.string.saved_execution_request
    "Recorded executor" -> R.string.saved_execution_executor
    "Recorded at" -> R.string.saved_execution_at
    "Begin recorded at" -> R.string.saved_execution_began
    "Progress recorded at" -> R.string.saved_execution_progress
    "Executor identity unresolved" -> R.string.saved_execution_unresolved
    "Submitted — begin unconfirmed" -> R.string.saved_execution_submitted
    "Begin recorded" -> R.string.saved_execution_running
    "Pause recorded" -> R.string.saved_execution_paused
    "Ready for review recorded" -> R.string.saved_execution_ready
    "Cancellation recorded" -> R.string.saved_execution_cancelled
    "Not started" -> R.string.saved_execution_not_started
    "Delivery unknown" -> R.string.saved_execution_delivery_unknown
    "Begin unconfirmed" -> R.string.saved_execution_unconfirmed
    else -> R.string.saved_execution_unavailable
})
