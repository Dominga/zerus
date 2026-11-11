package app.zerus.mobile

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.WarningAmber
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay

private val StatusGood = Color(0xFF67E8CB)
private val StatusWarning = Color(0xFFFFCB7D)
private val StatusQuiet = Color(0xFF9AACB2)
private val StatusBusy = Color(0xFFB9C7CC)

internal fun ConversationSyncStatus.Tone.color() = when (this) {
    ConversationSyncStatus.Tone.Good -> StatusGood
    ConversationSyncStatus.Tone.Warning -> StatusWarning
    ConversationSyncStatus.Tone.Busy -> StatusBusy
    ConversationSyncStatus.Tone.Quiet -> StatusQuiet
}

/** Wall-clock time for status ages; refreshed twice a minute while shown, never per frame. */
@Composable internal fun rememberStatusNow(): Long {
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) { while (true) { delay(30_000); now = System.currentTimeMillis() } }
    return now
}

/**
 * One fixed-height row above the conversation (zerus-a4mub.10): the compact status on the
 * left (details on tap), and [trailing] (unread and questions) on the right. Its height
 * never changes with the state, so the messages and the editor keep their positions.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun ConversationStatusStrip(facts: ConversationSyncStatus.Facts, refreshing: Boolean,
    onRefresh: () -> Unit, trailing: @Composable RowScope.() -> Unit) {
    // Only this row recomposes as the age ticks; [facts].now is replaced by the shown clock.
    val now = rememberStatusNow()
    val status = remember(facts, now) { ConversationSyncStatus.present(facts.copy(now = now)) }
    var details by remember { mutableStateOf(false) }
    val primary = status.primary
    val color = primary.tone.color()
    Row(Modifier.fillMaxWidth().height(36.dp).padding(start = 8.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        // The status takes the room the compact trailing items leave.
        Row(Modifier.weight(1f).heightIn(min = 32.dp).clickable(role = Role.Button, onClickLabel = "Conversation status details") { details = true }
            .semantics { contentDescription = "Conversation status: ${primary.text}" }.padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Box(Modifier.size(14.dp), contentAlignment = Alignment.Center) {
                if (primary.progress) CircularProgressIndicator(Modifier.size(12.dp), color = color, strokeWidth = 1.5.dp)
                else Icon(when (primary.tone) {
                    ConversationSyncStatus.Tone.Good -> Icons.Default.CheckCircle
                    ConversationSyncStatus.Tone.Warning -> Icons.Default.WarningAmber
                    ConversationSyncStatus.Tone.Busy -> Icons.Default.Schedule
                    ConversationSyncStatus.Tone.Quiet -> Icons.Default.Schedule
                }, null, Modifier.size(14.dp), tint = color)
            }
            Text(primary.text, color = color, style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        trailing()
    }
    if (details) {
        ObscureConversation()
        // Fully expanded: every fact and the explicit refresh are visible at once.
        ModalBottomSheet(onDismissRequest = { details = false }, sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
            containerColor = MaterialTheme.colorScheme.background) {
            ConversationStatusDetails(status, refreshing, onRefresh = { onRefresh() })
        }
    }
}

/** The details on demand: one labelled fact per row, then an explicit refresh. */
@Composable internal fun ConversationStatusDetails(status: ConversationSyncStatus.Presentation, refreshing: Boolean, onRefresh: () -> Unit) {
    Column(Modifier.fillMaxWidth().heightIn(max = 560.dp).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(Icons.Default.Visibility, null, Modifier.size(18.dp), tint = status.primary.tone.color())
            Text("Conversation status", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        }
        Text(status.primary.text, color = status.primary.tone.color(), style = MaterialTheme.typography.bodyMedium)
        HorizontalDivider(color = StatusQuiet.copy(alpha = .3f))
        status.details.forEach { detail ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(detail.label, Modifier.width(112.dp), color = StatusQuiet, style = MaterialTheme.typography.labelMedium)
                Text(detail.value, Modifier.weight(1f), color = if (detail.tone == ConversationSyncStatus.Tone.Quiet) MaterialTheme.colorScheme.onBackground
                    else detail.tone.color(), style = MaterialTheme.typography.bodySmall)
            }
        }
        Text("This status only describes what the phone shows. Answering, sending and marking read keep their own checks.",
            color = StatusQuiet, style = MaterialTheme.typography.labelSmall)
        Row(Modifier.fillMaxWidth().padding(bottom = 16.dp), horizontalArrangement = Arrangement.End) {
            TextButton(onClick = onRefresh, enabled = !refreshing) { Text(if (refreshing) "Refreshing…" else "Refresh now") }
        }
    }
}
