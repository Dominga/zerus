//! Checked per-session preference receipts, projected for display only.
//! Neither reconciliation step sends a mutation or authorizes notification delivery.
use crate::{actions::Target, view, ViewRow};
use serde_json::Value;

/// Apply a checked receipt to its exact currently listed live identity.
pub fn apply(row: &mut ViewRow, target: &Target, preferences: &Value) -> bool {
    let Some(confirmed) = preferences.as_object() else {
        return false;
    };
    let run = row.run_id.as_deref().unwrap_or("");
    if row.name != target.name
        || row.state == "archived"
        || row.conversation_id.as_deref().unwrap_or("") != target.conversation_id
        || !row.shared_notifications
        || run.is_empty()
        || run != target.run_id
        || !view::preferences_valid(confirmed, &target.conversation_id)
    {
        return false;
    }
    let incoming = view::preferences_revision(confirmed.get("revision")).unwrap_or(-1);
    let current = row.notification_revision.unwrap_or(-1);
    let applied = view::Preferences {
        muted: confirmed.get("muted") == Some(&Value::Bool(true)),
        suppressed: confirmed.get("suppressed").cloned().unwrap_or(Value::Null),
    };
    let same = row
        .notify
        .preferences
        .as_ref()
        .is_some_and(|kept| kept.same(&applied));
    if incoming < current || (incoming == current && !same) {
        return false;
    }
    row.muted = applied.muted;
    row.notification_revision = Some(incoming);
    row.notify.preferences = Some(applied);
    row.notification_error = None;
    true
}

/// Native keepNotificationPreferences: an older/missing revision keeps the
/// previous value; equal-revision disagreement is a conflict; newer data wins.
pub fn keep(previous: &ViewRow, row: &mut ViewRow) {
    let conversation = row.conversation_id.as_deref().unwrap_or("");
    if conversation.is_empty()
        || previous.conversation_id.as_deref() != Some(conversation)
        || previous.notify.cmd != row.notify.cmd
        || previous.notify.preferences.is_none()
    {
        return;
    }
    if row.shared_notifications {
        let old = previous.notification_revision.unwrap_or(-1);
        let incoming = row.notification_revision.unwrap_or(-1);
        let same = match (&row.notify.preferences, &previous.notify.preferences) {
            (Some(new), Some(old)) => new.same(old),
            (new, old) => new == old,
        };
        if incoming > old || (incoming == old && same) {
            return;
        }
        if incoming == old {
            row.notification_error = Some(crate::state::NOTIFICATION_CONFLICT.to_owned());
        }
    }
    row.notify
        .preferences
        .clone_from(&previous.notify.preferences);
    row.muted = previous.muted;
    row.notification_revision = previous.notification_revision;
}
