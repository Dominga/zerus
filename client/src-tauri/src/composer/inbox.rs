//! Remove one OpenCode inbox message that was not delivered
//! (zerus-5wmh.13.6; Qt `SessionsWindow::removeQueuedMessage`, 4433-4467).
//!
//! The webview names the message and the run and conversation its
//! confirmation was opened for. The native action is built here only, after
//! the open session's current state, under the hub lock, still reports that
//! exact message as removable ([`inbox_removable`]): the same session and
//! composer, the same run and conversation, the action still allowed, and
//! the message still not delivered. Otherwise nothing is sent and the
//! webview says the message changed. One removal runs at a time across all
//! sessions (Qt's `m_removeQueuedRequest`). Its outcome is the CLI's own
//! receipt (`actions::classify_remove_queued`) and is never retried.
use serde::Serialize;
use serde_json::Value;
use tauri::State;
use zerus_client_core::actions::allowed_with;
use zerus_client_core::input_queue::{inbox_removable, InboxFacts};
use zerus_client_core::{Action, Outcome, Target};

use super::send::outcome_of;
use super::{Composers, Hub};

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The removal that runs: its session's draft key and its message.
#[derive(Default)]
pub(crate) struct Inbox {
    pub(crate) removing: Option<(String, String)>,
}

/// What one Remove came to, for the webview's notice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RemoveReply {
    /// OpenCode confirmed the removal.
    Removed,
    /// The open session is no longer the confirmation's, or its message is
    /// no longer removable there: nothing was sent.
    Changed,
    /// Another removal runs: nothing was sent.
    Busy,
    /// Definitely not removed.
    Failed { detail: String },
    /// The removal may have happened: never retried.
    Uncertain { detail: String },
}

impl Composers {
    /// Qt's confirmed `removeQueuedMessage`: the check again, on the open
    /// session's current state, then the one native call. Blocking.
    pub(crate) fn remove_queued(
        &self,
        handle: u64,
        run_id: &str,
        conversation_id: &str,
        message_id: &str,
    ) -> RemoveReply {
        let (target, key) = {
            let mut hub = self.lock();
            if hub.closed {
                return RemoveReply::Changed;
            }
            if hub.inbox.removing.is_some() {
                return RemoveReply::Busy;
            }
            let Some(ctx) = self.context(&hub) else {
                return RemoveReply::Changed;
            };
            let open = hub
                .composers
                .get(&ctx.key)
                .is_some_and(|composer| composer.handle == handle);
            let Some(details) = ctx.details.clone() else {
                return RemoveReply::Changed;
            };
            let inspect_error = hub.followed.inspect_error.is_some();
            let events = hub.followed.events.make_contiguous();
            let facts = InboxFacts {
                cmd: &ctx.row.agent,
                online: ctx.online,
                inspect_error,
                busy: false,
                allowed: allowed_with(&ctx.row, &details, "remove_queued"),
                details: &details,
                events,
            };
            let current = open
                && ctx.session.subagent.is_none()
                && text(&details, "run_id") == run_id
                && text(&details, "conversation_id") == conversation_id
                && inbox_removable(&facts, message_id);
            if !current {
                return RemoveReply::Changed;
            }
            hub.inbox.removing = Some((ctx.key.clone(), message_id.to_owned()));
            let target = Target {
                computer: ctx.session.computer.clone(),
                name: ctx.session.name.clone(),
                run_id: run_id.to_owned(),
                conversation_id: conversation_id.to_owned(),
            };
            (target, ctx.key)
        };
        self.changed();
        let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self.0.deps.dispatch)(
                target,
                Action::RemoveQueued {
                    message_id: message_id.to_owned(),
                },
                uuid::Uuid::new_v4().to_string(),
            )
        }));
        let reply = match called {
            // The session's Lane::Inbox was taken: no call ran.
            Ok(Err(error)) if error == crate::actions::BUSY => RemoveReply::Busy,
            other => match outcome_of(other, "") {
                Outcome::Done { .. } => RemoveReply::Removed,
                Outcome::Failed { detail } => RemoveReply::Failed { detail },
                Outcome::Uncertain { detail } => RemoveReply::Uncertain { detail },
            },
        };
        let current = {
            let mut hub = self.lock();
            hub.inbox.removing = None;
            hub.open_key.as_deref() == Some(key.as_str())
        };
        self.changed();
        if current && reply != RemoveReply::Busy {
            self.refresh_details();
        }
        reply
    }

    /// The view's removal state: the message of this session being removed,
    /// and whether any removal runs.
    pub(crate) fn inbox_part(hub: &Hub, key: &str) -> (Option<String>, bool) {
        let removing = hub
            .inbox
            .removing
            .as_ref()
            .filter(|(owner, _)| owner == key)
            .map(|(_, message)| message.clone());
        (removing, hub.inbox.removing.is_some())
    }
}

/// Remove for one not-delivered OpenCode message, after the person
/// confirmed it in the webview. Off the main thread: the call is blocking.
#[tauri::command(rename_all = "snake_case")]
pub async fn inbox_remove(
    state: State<'_, Composers>,
    handle: u64,
    run_id: String,
    conversation_id: String,
    message_id: String,
) -> Result<RemoveReply, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        composers.remove_queued(handle, &run_id, &conversation_id, &message_id)
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
