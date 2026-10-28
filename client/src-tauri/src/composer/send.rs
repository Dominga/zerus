//! The open session's part of the composer, ported from `SessionsWindow`:
//! the block reason (`messageBlockReason`, S1), the cache chip and its
//! details (S2), the session status Qt shows in the composer
//! (`setSessionStatus`), and the inspect when no Activity state arrived.
//!
//! The delivery (`sendMessage`, `deliverMessage`, `finishMessage`): the
//! guarded send of the held draft, the cold cache question before it, and
//! the local message rows (S4) that show what was sent until the agent's
//! own history does. Unknown delivery never authorizes replay:
//! - the durable "sending" save comes before any call, and only its answer
//!   can start the call; a closed hub (the app quits) starts nothing, and a
//!   draft saved as sending reloads as uncertain;
//! - one request id per delivery, made right before its one call: the local
//!   row, the CLI request and the receipt check carry it;
//! - an uncertain outcome (or a call whose thread died) never sends again
//!   by itself: the draft stays, marked uncertain, until Allow retry, and its
//!   row offers no Retry;
//! - every path that could send re-checks the composer: Send (with the text
//!   epoch its text is based on), a question's answer, Retry, and the last
//!   gate before the save (`deliver`).
//!
//! The ticker recomputes the open composer's session part once a second, as
//! Qt's cache chip refreshes, and sleeps while none is open. It wakes the
//! pump only when that part changed: the chip's minute, or a row or status
//! no batch announced. A write mark wakes it at once (`Actions::on_writes`).

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use zerus_client_core::attachments::{self, Attachment, StoredFile};
use zerus_client_core::cache::{self, Chip, ColdAction, ColdPrompt};
use zerus_client_core::local_messages::{self, Inspect, LocalMessage, LocalMessages, Status};
use zerus_client_core::send::{block_reason, BlockFacts};
use zerus_client_core::terminal::{self, TerminalFacts};
use zerus_client_core::{Action, Exit, Outcome, Target, TooLarge, ViewRow};

use super::{Composer, Composers, Hub, SessionRef, EVENTS};
use crate::actions::WriteKind;

/// No Activity state this long after opening: inspect.
const DETAILS_WAIT: f64 = 3.0;
/// A failed inspect is tried again this often while nothing streams (Qt's
/// `m_activityPollMs`, `SessionsWindow.h:565`; its timer, 2013-2016).
const RETRY_WAIT: f64 = 2.5;
/// The ticker's period: Qt's cache chip refreshes once a second.
const TICK: Duration = Duration::from_secs(1);

/// The session's state for the composer view.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SessionView {
    pub(crate) available: bool,
    pub(crate) unavailable_reason: String,
    pub(crate) block_reason: Option<String>,
    pub(crate) attachments_available: bool,
    pub(crate) attachments_reason: String,
    pub(crate) session_status: String,
    pub(crate) session_status_error: bool,
    pub(crate) session_blocks_input: bool,
    pub(crate) cache: Chip,
    pub(crate) cache_warning: String,
    pub(crate) cache_details: String,
}

/// What Qt's session hint reads (`SessionsWindow.cpp:3913-3922, 3955-3958`).
/// A2 fills `question_error` (`pending_questions_error`); Piece E fills
/// `restore_error` (`last_restore_error`) and `occupied` (another session
/// uses this archived session's name).
#[derive(Default)]
pub(crate) struct HintFacts<'a> {
    pub(crate) terminating: bool,
    pub(crate) online: bool,
    pub(crate) archived: bool,
    /// `m_inspectError`, its whole text.
    pub(crate) inspect_error: Option<&'a str>,
    pub(crate) question_error: &'a str,
    pub(crate) restore_error: &'a str,
    pub(crate) occupied: bool,
}

/// The composer's session status (`setSessionStatus(contextVisible ? hint
/// : "", contextError, contextError && !messageProblem.isEmpty())`,
/// `SessionsWindow.cpp:3984-3986`); `blocked`: a block reason stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SessionStatus {
    pub(crate) text: String,
    pub(crate) error: bool,
    pub(crate) blocks_input: bool,
}

pub(crate) fn session_status(facts: &HintFacts<'_>, blocked: bool) -> SessionStatus {
    let restore_failed = facts.archived && !facts.restore_error.is_empty();
    let error = !facts.online
        || facts.inspect_error.is_some()
        || !facts.question_error.is_empty()
        || restore_failed;
    if !error && !(facts.archived && facts.occupied) {
        return SessionStatus::default();
    }
    // The first six links of the hint chain: one of them holds whenever the
    // status shows.
    let text = if facts.terminating {
        "Waiting for the session to stop. Its native conversation history is kept.".to_owned()
    } else if !facts.online {
        "Machine unavailable. Showing the last known state; actions will return when it reconnects."
            .to_owned()
    } else if restore_failed {
        format!("Last restore failed: {}", facts.restore_error)
    } else if let Some(error) = facts.inspect_error {
        error.to_owned()
    } else if !facts.question_error.is_empty() {
        format!(
            "Could not load agent questions: {}. Check Terminal.",
            facts.question_error
        )
    } else {
        "Another active or saved session uses this name. Restore becomes available when that name is free."
            .to_owned()
    };
    SessionStatus {
        text,
        error,
        blocks_input: error && blocked,
    }
}

/// An inspect's answer as Qt reads it (`HgsClient::requestInspection` and
/// `failureDetail`, `HgsClient.cpp:466-479, 931-944`): the JSON object, or
/// the detail of "Could not refresh activity: %1". `run_limited`'s cap
/// stands for Qt's unparseable reply.
pub(crate) fn inspection(hgs: &Path, read: Result<Exit, TooLarge>) -> Result<Value, String> {
    const INVALID: &str = "Invalid activity response";
    let stderr = |text: &str, fallback: String| {
        let text: String = text.trim().chars().take(4000).collect();
        if text.is_empty() {
            fallback
        } else {
            text
        }
    };
    match read {
        Err(_) => Err(INVALID.into()),
        Ok(Exit::Code {
            code: 0, stdout, ..
        }) => match serde_json::from_slice::<Value>(&stdout) {
            Ok(value) if value.is_object() => Ok(value),
            _ => Err(INVALID.into()),
        },
        Ok(Exit::Code {
            code, stderr: text, ..
        }) => Err(stderr(&text, format!("exit code {code}"))),
        Ok(Exit::Abnormal { stderr: text }) => Err(stderr(&text, "hgs ended abnormally".into())),
        Ok(Exit::TimedOut) => Err("timed out".into()),
        Ok(Exit::StartFailed(error)) => Err(format!("cannot start '{}': {error}", hgs.display())),
        Ok(Exit::WriteFailed) => Err("could not send the request".into()),
    }
}

/// The agent command the send rules read: the row says `deepseek` for `dsh`.
pub(crate) fn cmd(row: &ViewRow) -> String {
    if row.agent == "deepseek" {
        "dsh".into()
    } else {
        row.agent.clone()
    }
}

/// Local time as the cache details show it (Qt's `d MMM HH:mm:ss`, English
/// month names); formatted in Rust, never in the webview.
fn local_time(seconds: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(seconds, 0)
        .single()
        .map(|time| time.format("%-d %b %H:%M:%S").to_string())
        .unwrap_or_default()
}

/// The cache expiry of `details` in local time, as `cache::details` names
/// it: the seconds and their text. Callers format it before they take the
/// hub lock, since local time may read the time zone files.
pub(crate) fn expiry(details: &Value) -> Option<(i64, String)> {
    let at = details["session_usage"]["prompt_cache"]["expires_at"]
        .as_f64()
        .filter(|at| *at > 0.0)? as i64;
    Some((at, local_time(at)))
}

/// The row's run, conversation and state an inspect read: a change of any
/// makes the inspected details stale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Basis {
    run_id: Option<String>,
    conversation_id: Option<String>,
    state: String,
}

impl Basis {
    pub(crate) fn of(row: &ViewRow) -> Self {
        Basis {
            run_id: row.run_id.clone(),
            conversation_id: row.conversation_id.clone(),
            state: row.state.clone(),
        }
    }
}

/// The open session's row, online state, details and facts.
pub(crate) struct Context {
    pub(crate) session: SessionRef,
    pub(crate) key: String,
    pub(crate) row: ViewRow,
    pub(crate) online: bool,
    pub(crate) details: Option<Arc<Value>>,
    pub(crate) facts: BlockFacts,
}

impl Context {
    pub(crate) fn reason(&self) -> Option<String> {
        block_reason(&self.facts, self.details.as_deref())
    }

    /// Qt's `recorded`: session data that is not live (`SessionsWindow.cpp:3783`).
    pub(crate) fn recorded(&self) -> bool {
        !self.online
            || matches!(self.row.state.as_str(), "archived" | "paused")
            || self.row.process_state == "exited"
            || self.facts.inspect_error
    }
}

impl Composers {
    pub(crate) fn now(&self) -> f64 {
        (self.0.deps.clock)()
    }

    /// The open session as the catalog and the followed Activity show it now.
    pub(crate) fn context(&self, hub: &Hub) -> Option<Context> {
        let session = hub.open.clone()?;
        let key = hub.open_key.clone()?;
        let (row, online) = (self.0.deps.rows)(&session.computer, &session.key, &session.name)?;
        let followed = hub.followed.computer == session.computer && hub.followed.key == session.key;
        let details = if followed {
            hub.followed.details.clone()
        } else {
            None
        };
        let write = (self.0.deps.facts)(&session.computer, &session.name);
        let facts = BlockFacts {
            start_send_waiting: false,
            compacting: false,
            clearing: false,
            terminating: write == Some(WriteKind::Terminate),
            settings_changing: false,
            online,
            state: row.state.clone(),
            updating: write == Some(WriteKind::Update),
            cmd: cmd(&row),
            inspect_error: followed && hub.followed.inspect_error.is_some(),
        };
        Some(Context {
            session,
            key,
            row,
            online,
            details,
            facts,
        })
    }

    /// One `inspect` of the open session, off the UI thread; its value
    /// stands in for the stream's state until the next state frame. As Qt's
    /// `inspect()` (`SessionsWindow.cpp:3467-3492`), it reads nothing while
    /// the stream keeps the details or the computer is offline, and one
    /// inspect runs at a time. The tick calls it for the fallback and its
    /// retry; Tasks 4b and later call it after their actions, and Task 6's
    /// Refresh, where Qt calls `inspect()`.
    pub(crate) fn refresh_details(&self) {
        let mut hub = self.lock();
        if hub.closed || hub.followed.inspecting.is_some() || hub.followed.streamed {
            return;
        }
        let Some(ctx) = self.context(&hub) else {
            return;
        };
        let followed =
            hub.followed.computer == ctx.session.computer && hub.followed.key == ctx.session.key;
        if !followed || !ctx.online {
            return;
        }
        hub.next_inspect += 1;
        let number = hub.next_inspect;
        hub.followed.inspecting = Some(number);
        hub.followed.inspected = Some(Basis::of(&ctx.row));
        hub.followed.inspected_at = self.now();
        drop(hub);
        let target = Target {
            computer: ctx.session.computer.clone(),
            name: ctx.session.name.clone(),
            run_id: ctx.row.run_id.clone().unwrap_or_default(),
            conversation_id: ctx.row.conversation_id.clone().unwrap_or_default(),
        };
        let archive = ctx.session.archive_id;
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let read = (me.0.deps.inspect)(&target, archive.as_deref()).map(|mut value| {
                // The details keep no events: they join the followed window
                // apart, as Qt's inspection reply joins `m_events`.
                let replace = value["replace_events"].as_bool() == Some(true);
                let events = value
                    .as_object_mut()
                    .and_then(|fields| fields.remove("events"));
                let expiry = expiry(&value);
                (Arc::new(value), expiry, (replace, events))
            });
            let mut hub = me.lock();
            // Another session was opened, or the follow ended, meanwhile:
            // this answer is not the current one.
            if hub.followed.inspecting != Some(number) {
                return;
            }
            hub.followed.inspecting = None;
            // A state frame that arrived meanwhile is newer.
            if !hub.followed.streamed {
                match read {
                    Ok((details, expiry, events)) => {
                        hub.followed.inspect_error = None;
                        hub.followed.details = Some(details);
                        hub.followed.expiry = expiry;
                        let (replace, events) = events;
                        merge_events(&mut hub.followed.events, replace, events);
                    }
                    Err(detail) => {
                        hub.followed.inspect_error =
                            Some(format!("Could not refresh activity: {detail}"))
                    }
                }
            }
            drop(hub);
            me.after_observe();
        }));
    }

    /// Whether the tick inspects. Never read yet: once no Activity state
    /// arrived within 3 s of opening, and at once after the follow ended.
    /// Read: a failure again every 2.5 s, and any read again when the row's
    /// run, conversation or state changed.
    fn wants_details(&self, hub: &Hub, now: f64) -> bool {
        let followed = &hub.followed;
        if hub.open.is_none() || followed.inspecting.is_some() || followed.streamed {
            return false;
        }
        match &followed.inspected {
            None => followed.details.is_some() || now - followed.opened_at >= DETAILS_WAIT,
            Some(basis) => {
                (followed.inspect_error.is_some() && now - followed.inspected_at >= RETRY_WAIT)
                    || self
                        .context(hub)
                        .is_some_and(|ctx| Basis::of(&ctx.row) != *basis)
            }
        }
    }

    /// Once a second while a composer is open: the session part of its view
    /// (the cache chip's minute, and a write, row or status no batch
    /// announced), and the inspect fallback.
    pub fn tick(&self) {
        let now = self.now();
        let mut hub = self.lock();
        let seen = hub
            .open_key
            .as_ref()
            .and_then(|key| hub.composers.get(key))
            .map(|composer| session_view(self, &hub, composer));
        let changed = hub.seen != seen;
        hub.seen = seen;
        let wants = self.wants_details(&hub, now);
        drop(hub);
        if wants {
            self.refresh_details();
        }
        if changed {
            self.changed();
        }
    }

    /// Ticks once a second while a composer is open, until the app quits.
    pub fn start_ticker(&self) {
        self.start_ticker_every(TICK);
    }

    fn start_ticker_every(&self, every: Duration) -> JoinHandle<()> {
        let me = self.clone();
        std::thread::Builder::new()
            .name("zerus-composer".into())
            .spawn(move || {
                while me.wait_open() {
                    std::thread::sleep(every);
                    me.tick();
                }
            })
            .expect("the composer ticker starts")
    }

    /// Sleeps while no composer is open, so a closed composer never wakes
    /// the app; false once the app quits.
    fn wait_open(&self) -> bool {
        let hub = self
            .0
            .opened
            .wait_while(self.lock(), |hub| {
                let waiting = hub.open.is_none() && !hub.closed;
                #[cfg(test)]
                self.0
                    .parked
                    .store(waiting, std::sync::atomic::Ordering::SeqCst);
                waiting
            })
            .unwrap_or_else(PoisonError::into_inner);
        !hub.closed
    }
}

/// The open session's part of the composer view (`renderDetails`,
/// `SessionsWindow.cpp:3783-3791, 3982-3989`).
pub(crate) fn session_view(composers: &Composers, hub: &Hub, composer: &Composer) -> SessionView {
    let Some(ctx) = composers.context(hub).filter(|ctx| ctx.key == composer.key) else {
        return SessionView {
            unavailable_reason: "Select a session.".into(),
            attachments_available: true,
            ..SessionView::default()
        };
    };
    let details = ctx.details.as_deref().unwrap_or(&Value::Null);
    let now = composers.now();
    let reason = ctx.reason();
    let status = session_status(
        &HintFacts {
            terminating: ctx.facts.terminating,
            online: ctx.online,
            archived: ctx.row.state == "archived",
            inspect_error: ctx
                .facts
                .inspect_error
                .then_some(hub.followed.inspect_error.as_deref())
                .flatten(),
            ..HintFacts::default()
        },
        reason.is_some(),
    );
    let attachments_available = ctx.facts.cmd != "opencode"
        && details
            .get("attachments_supported")
            .and_then(Value::as_bool)
            .unwrap_or(true);
    let attachments_reason = details
        .get("attachments_reason")
        .and_then(Value::as_str)
        .unwrap_or(
            "This native adapter does not support file attachments. Your existing files remain saved; remove them to send text.",
        )
        .to_owned();
    let recorded = ctx.recorded();
    // The expiry was formatted when the details came; local time may read
    // files, so it is never formatted under the hub lock.
    let expiry = hub.followed.expiry.as_ref();
    let local = |seconds: i64| match expiry {
        Some((at, text)) if *at == seconds => text.clone(),
        _ => local_time(seconds),
    };
    SessionView {
        available: reason.is_none(),
        unavailable_reason: reason.clone().unwrap_or_default(),
        block_reason: reason,
        attachments_available,
        attachments_reason,
        session_status: status.text,
        session_status_error: status.error,
        session_blocks_input: status.blocks_input,
        cache: cache::chip(details, now),
        cache_warning: cache::warning(details, now),
        cache_details: cache::details(details, recorded, local),
    }
}

/// An inspection's events joined to the followed window as Qt joins them to
/// `m_events` (`SessionsWindow.cpp:3639-3651`): the window replaced when the
/// reply says so, else one event per `seq`; the last 500 stay.
fn merge_events(kept: &mut VecDeque<Value>, replace: bool, events: Option<Value>) {
    let Some(Value::Array(events)) = events else {
        return;
    };
    if replace {
        kept.clear();
    }
    let known: HashSet<i64> = kept
        .iter()
        .filter_map(|event| event["seq"].as_i64())
        .collect();
    let skip = events.len().saturating_sub(EVENTS);
    for event in events.into_iter().skip(skip) {
        if event["seq"]
            .as_i64()
            .is_some_and(|seq| known.contains(&seq))
        {
            continue;
        }
        kept.push_back(event);
    }
    while kept.len() > EVENTS {
        kept.pop_front();
    }
}

/// `deliveryFinished(false, …)` when the session changed before the call.
pub(crate) const CHANGED: &str = "The session changed. Your draft was kept without sending.";
/// A question's draft changed before its answer (`SessionsWindow.cpp:4845`).
const DRAFT_CHANGED: &str = "Draft changed. Send it when you are ready.";
/// A failed row names files that cannot be read back (`read_stored`'s text).
const UNAVAILABLE: &str =
    "The saved attachment is unavailable or changed. Attach it again before sending.";
/// Only the latest questions can be answered.
const PROMPTS: u64 = 8;

/// What `send_message`, `send_answer` and `local_retry` answer.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendReply {
    /// The delivery started; its end arrives with the composer view.
    Sending,
    /// Nothing happened: another send or question runs, the draft is not
    /// the one Send was pressed on, or the answer is stale. A refused Send
    /// hands its text back under a new epoch.
    Busy,
    /// The composer shows `reason` (`deliveryFinished(false, reason)`).
    Blocked { reason: String },
    /// A window notice (`showNotice(text, error)`): `error` is Qt's error
    /// tone, for a block reason or a read failure; a plain notice otherwise.
    Notice { text: String, error: bool },
    /// Ask "Continue with a cold cache?"; answer with `token`.
    Cold { token: u64, prompt: ColdPrompt },
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    Cold { action: ColdAction },
    Cancel,
}

/// A Retry between its checks and the read of its cached files.
pub(crate) struct RetryBegun {
    handle: u64,
    id: String,
    ctx: Context,
    row: LocalMessage,
    files: Vec<StoredFile>,
}

/// A failed row's message, sent again by Retry.
#[derive(Clone, Debug)]
pub(crate) struct Retry {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) attachments: Vec<Attachment>,
}

/// A question waiting for its answer, with the identity it was asked about
/// and the message it asked to send. Qt's question is modal, so it sends
/// what Send held; here the draft must still hold it when the answer comes.
#[derive(Clone, Debug)]
pub(crate) struct Prompt {
    pub(crate) key: String,
    pub(crate) handle: u64,
    pub(crate) session: SessionRef,
    pub(crate) run: String,
    pub(crate) conversation: String,
    pub(crate) text: String,
    pub(crate) attachments: Vec<Attachment>,
    pub(crate) retry: Option<Retry>,
}

/// One delivery: what is sent, to which run, with which consent.
#[derive(Clone, Debug)]
pub(crate) struct Delivery {
    pub(crate) key: String,
    pub(crate) session: SessionRef,
    pub(crate) created: i64,
    /// The row's run and conversation when Send was pressed: Qt's
    /// `observedIdentityMatches` lets the listing catch up with the details.
    pub(crate) row_run: String,
    pub(crate) row_conversation: String,
    pub(crate) run: String,
    pub(crate) conversation: String,
    /// The details the send was checked against (Qt's send evidence); for
    /// a session no longer open they stand for its current ones.
    pub(crate) evidence: Option<Arc<Value>>,
    pub(crate) text: String,
    pub(crate) attachments: Vec<Attachment>,
    pub(crate) retry_id: Option<String>,
    pub(crate) compaction: Option<String>,
    pub(crate) consent: bool,
    /// The composer's draft is not the message (a Retry): keep the draft.
    pub(crate) preserve: bool,
    /// The revision of its "sending" save, set by `deliver`.
    pub(crate) revision: u64,
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Qt's `draftMatches`: the same text and the same files, in order.
fn matches_draft(composer: &Composer, text: &str, attachments: &[Attachment]) -> bool {
    composer.draft.text == text && composer.draft.attachments == attachments
}

impl Context {
    pub(crate) fn run(&self) -> String {
        self.details
            .as_deref()
            .map(|d| text(d, "run_id").to_owned())
            .unwrap_or_default()
    }

    pub(crate) fn conversation(&self) -> String {
        self.details
            .as_deref()
            .map(|d| text(d, "conversation_id").to_owned())
            .unwrap_or_default()
    }
}

impl Composers {
    /// Asks a question about the open session; its answer brings this back.
    /// It holds the message: the retry's, or the draft Send was pressed on.
    pub(crate) fn ask(&self, hub: &mut Hub, ctx: &Context, retry: Option<Retry>) -> u64 {
        hub.next_token += 1;
        let token = hub.next_token;
        // Qt's question is modal: a new one replaces its key's older one,
        // and only the latest few can be answered.
        hub.prompts
            .retain(|old, prompt| *old + PROMPTS > token && prompt.key != ctx.key);
        let composer = hub.composers.get(&ctx.key);
        let (text, attachments) = match (&retry, composer) {
            (Some(retry), _) => (retry.text.clone(), retry.attachments.clone()),
            (None, Some(composer)) => (
                composer.draft.text.clone(),
                composer.draft.attachments.clone(),
            ),
            (None, None) => Default::default(),
        };
        let prompt = Prompt {
            key: ctx.key.clone(),
            handle: composer.map_or(0, |c| c.handle),
            session: ctx.session.clone(),
            run: ctx.run(),
            conversation: ctx.conversation(),
            text,
            attachments,
            retry,
        };
        hub.prompts.insert(token, prompt);
        token
    }

    /// Send pressed (`MessageComposer::send` then `SessionsWindow::sendMessage`).
    /// `text` is the editor's text and `epoch` the text epoch it is based on:
    /// text typed over an older draft (a remove, an Undo, a hand-back) is
    /// never sent. A refusal hands the text back under a new epoch, so an
    /// editor locked for the send unlocks.
    pub fn send_message(
        &self,
        handle: u64,
        rev: u64,
        epoch: u64,
        text: String,
        position: i64,
        anchor: i64,
    ) -> SendReply {
        self.edit(handle, rev, epoch, text.clone(), position, anchor);
        let mut hub = self.lock();
        let reply = self.send_held(&mut hub, handle, epoch, &text);
        if matches!(reply, SendReply::Busy | SendReply::Blocked { .. }) {
            self.hand_back(hub, handle);
        } else {
            drop(hub);
        }
        reply
    }

    fn send_held(&self, hub: &mut Hub, handle: u64, epoch: u64, text: &str) -> SendReply {
        let Some(ctx) = self.context(hub) else {
            return SendReply::Busy;
        };
        let Some(composer) = hub.composers.get(&ctx.key).filter(|c| c.handle == handle) else {
            return SendReply::Busy;
        };
        if hub.closed
            || !composer.loaded
            || composer.busy()
            || composer.uncertain
            || composer.text_epoch != epoch
            || composer.draft.text != text
        {
            return SendReply::Busy;
        }
        if let Some(reason) = ctx.reason() {
            if let Some(composer) = hub.composers.get_mut(&ctx.key) {
                composer.show_error(reason.clone());
            }
            return SendReply::Blocked { reason };
        }
        // Qt's Send is disabled otherwise (`updateControls`,
        // `MessageComposer.cpp:1185`), and `send` returns (814).
        if !self
            .view_of(hub, &ctx.key)
            .is_some_and(|view| view.can_send)
        {
            return SendReply::Busy;
        }
        let (message, attachments) = (
            composer.draft.text.clone(),
            composer.draft.attachments.clone(),
        );
        self.consent_or_deliver(hub, &ctx, message, attachments, None)
    }

    /// The cold cache check, then the delivery (`sendMessage` 4815-4852).
    fn consent_or_deliver(
        &self,
        hub: &mut Hub,
        ctx: &Context,
        text: String,
        attachments: Vec<Attachment>,
        retry: Option<Retry>,
    ) -> SendReply {
        let details = ctx.details.as_deref().unwrap_or(&Value::Null);
        let now = self.now();
        if cache::expired(details, now) {
            let prompt = cache::cold_prompt(
                details,
                self.compacting(hub, &ctx.key),
                ctx.facts.clearing,
                now,
            );
            let token = self.ask(hub, ctx, retry);
            return SendReply::Cold { token, prompt };
        }
        let delivery = self.delivery(hub, ctx, text, attachments, retry, None, false);
        if self.deliver(hub, delivery) {
            SendReply::Sending
        } else {
            SendReply::Busy
        }
    }

    /// Whether a compaction waits to send for `key` (Task 5 fills it in).
    pub(crate) fn compacting(&self, _hub: &Hub, _key: &str) -> bool {
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn delivery(
        &self,
        hub: &Hub,
        ctx: &Context,
        text: String,
        attachments: Vec<Attachment>,
        retry: Option<Retry>,
        compaction: Option<String>,
        consent: bool,
    ) -> Delivery {
        // Qt's `preserveDraft`: a Retry whose message is not the draft.
        let preserve = retry.is_some()
            && hub
                .composers
                .get(&ctx.key)
                .is_some_and(|c| !matches_draft(c, &text, &attachments));
        Delivery {
            key: ctx.key.clone(),
            session: ctx.session.clone(),
            created: ctx.row.created,
            row_run: ctx.row.run_id.clone().unwrap_or_default(),
            row_conversation: ctx.row.conversation_id.clone().unwrap_or_default(),
            run: ctx.run(),
            conversation: ctx.conversation(),
            evidence: ctx.details.clone(),
            text,
            attachments,
            retry_id: retry.map(|r| r.id),
            compaction,
            consent,
            preserve,
            revision: 0,
        }
    }

    /// The answer to a question asked by this hub. It re-checks everything:
    /// the session's identity, that the composer is neither busy nor (for
    /// its own draft) uncertain, and that the draft still holds the message,
    /// so an older question never resends a draft whose delivery is unknown.
    pub fn answer(&self, token: u64, answer: Answer) -> SendReply {
        let mut hub = self.lock();
        let Some(prompt) = hub.prompts.remove(&token) else {
            return SendReply::Busy;
        };
        let Answer::Cold { action } = answer else {
            return SendReply::Busy;
        };
        let Some(ctx) = self.context(&hub) else {
            return SendReply::Busy;
        };
        // Qt returns quietly when the question's session changed meanwhile.
        if ctx.key != prompt.key
            || ctx.session.computer != prompt.session.computer
            || ctx.session.name != prompt.session.name
            || ctx.run() != prompt.run
            || ctx.conversation() != prompt.conversation
        {
            return SendReply::Busy;
        }
        let Some(composer) = hub
            .composers
            .get(&prompt.key)
            .filter(|c| c.handle == prompt.handle)
        else {
            return SendReply::Busy;
        };
        if hub.closed
            || !composer.loaded
            || composer.busy()
            || (prompt.retry.is_none() && composer.uncertain)
        {
            return SendReply::Busy;
        }
        match action {
            ColdAction::Send => {
                if let Some(reason) = ctx.reason() {
                    return SendReply::Notice {
                        text: reason,
                        error: true,
                    };
                }
                if prompt.retry.is_none()
                    && !matches_draft(composer, &prompt.text, &prompt.attachments)
                {
                    return SendReply::Notice {
                        text: DRAFT_CHANGED.into(),
                        error: false,
                    };
                }
                let delivery = self.delivery(
                    &hub,
                    &ctx,
                    prompt.text,
                    prompt.attachments,
                    prompt.retry,
                    None,
                    true,
                );
                if self.deliver(&mut hub, delivery) {
                    SendReply::Sending
                } else {
                    SendReply::Busy
                }
            }
            // Task 5 runs these.
            ColdAction::Compact | ColdAction::Clear => SendReply::Busy,
        }
    }

    /// `prepareSendingAsync`, the last gate: the draft is saved as "sending"
    /// before anything runs, so a crash afterwards reloads it as uncertain,
    /// and only that save's answer can start the call. An uncertain draft is
    /// never sent again here: only a Retry that keeps it passes. A delivery
    /// drops the open questions of its key and clears Undo, so no older
    /// question resends and Undo never brings a sent file back.
    pub(crate) fn deliver(&self, hub: &mut Hub, mut delivery: Delivery) -> bool {
        if hub.closed {
            return false;
        }
        // `deliverMessage`: a Retry's row must still be a definite failure.
        if let Some(id) = &delivery.retry_id {
            let retained = hub
                .local
                .rows(&delivery.key)
                .iter()
                .any(|row| &row.id == id && row.status == Status::Error && !row.uncertain);
            if !retained {
                return false;
            }
        }
        let Some(composer) = hub.composers.get_mut(&delivery.key) else {
            return false;
        };
        if !composer.loaded
            || composer.sending
            || composer.attaching
            || composer.renaming
            || (composer.uncertain && !delivery.preserve)
        {
            return false;
        }
        composer.sending = true;
        composer.waiting = None;
        composer.kept = delivery
            .preserve
            .then(|| (composer.error, composer.notice.clone()));
        composer.error = false;
        composer.notice = "Saving before sending…".into();
        composer.removed = None;
        // The durable save takes the next save revision: an older save's
        // answer, or its retry, is ignored from here on.
        composer.saves += 1;
        composer.saving = true;
        let (key, handle, revision) = (composer.key.clone(), composer.handle, composer.saves);
        composer.delivering = Some(revision);
        delivery.revision = revision;
        self.0.store.save_durable(&key, composer.stored(), {
            let me = self.clone();
            move |saved| me.prepared(delivery, handle, revision, saved)
        });
        hub.prompts.retain(|_, prompt| prompt.key != key);
        self.changed();
        true
    }

    /// The durable save answered (on the store's writer).
    fn prepared(&self, delivery: Delivery, handle: u64, revision: u64, saved: Result<(), String>) {
        let mut hub = self.lock();
        // Only the answer of the newest save of this composer counts (Qt's
        // `m_saveRevision`, `MessageComposer.cpp:741`).
        let Some(composer) = hub.composers.get_mut(&delivery.key).filter(|c| {
            c.handle == handle && c.saves == revision && c.sending && c.delivering == Some(revision)
        }) else {
            return;
        };
        composer.saving = false;
        if let Err(error) = saved {
            composer.sending = false;
            composer.delivering = None;
            composer.kept = None;
            composer.notice.clear();
            composer.save_failed = true;
            composer.storage_error = Some(error);
            // The editor, locked for the send, unlocks.
            composer.text_epoch += 1;
            let key = composer.key.clone();
            drop(hub);
            self.retry_save(key, handle, revision);
            self.changed();
            return;
        }
        composer.save_failed = false;
        composer.storage_error = None;
        // The app quits: the draft is on disk as "sending" and reloads as
        // uncertain; nothing starts now.
        if hub.closed {
            return;
        }
        drop(hub);
        if !self.still_valid(&delivery) {
            self.ended(&delivery, false, CHANGED, false);
            return;
        }
        {
            let mut hub = self.lock();
            if hub.closed {
                return;
            }
            if let Some(composer) = hub.composers.get_mut(&delivery.key) {
                composer.notice = "Sending…".into();
                // Qt clears the editor once the save holds the draft (751).
                composer.text_epoch += 1;
            }
        }
        self.changed();
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || me.send_now(delivery)));
    }

    /// Caches the files for Retry, records the local row, runs the one call.
    fn send_now(&self, delivery: Delivery) {
        let key = delivery.key.clone();
        let cache = self.cache();
        let files = match attachments::prepare(cache.root(), &delivery.attachments) {
            Ok(files) => files,
            Err(error) => return self.ended(&delivery, false, &error, false),
        };
        let me = self.clone();
        cache.prune_later(&self.0.deps.spawn, Arc::new(move || me.retry_files()));
        if !self.still_valid(&delivery) {
            return self.ended(&delivery, false, CHANGED, false);
        }
        // One request id per delivery, made here for its one dispatch: the
        // local row, the CLI request and the receipt check carry it, and no
        // other delivery reuses it.
        let request_id = uuid::Uuid::new_v4().to_string();
        let now = self.now();
        {
            let mut hub = self.lock();
            if hub.closed {
                return;
            }
            // Qt (`deliverMessage`, 4905): the delivery ended meanwhile, or a
            // newer one took its place; either way this one calls nothing.
            if !hub
                .composers
                .get(&key)
                .is_some_and(|c| c.sending && c.delivering == Some(delivery.revision))
            {
                drop(hub);
                return self.ended(&delivery, false, CHANGED, false);
            }
            hub.local.sending(
                &key,
                &request_id,
                delivery.retry_id.as_deref(),
                &delivery.text,
                &files,
                &delivery.run,
                &delivery.conversation,
                now,
            );
        }
        self.changed();
        let target = Target {
            computer: delivery.session.computer.clone(),
            name: delivery.session.name.clone(),
            run_id: delivery.run.clone(),
            conversation_id: delivery.conversation.clone(),
        };
        let action = Action::Send {
            text: delivery.text.clone(),
            attachments: delivery.attachments.clone(),
            agent_id: None,
            expected_compaction_id: delivery.compaction.clone(),
        };
        let dispatch = &self.0.deps.dispatch;
        let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch(target, action, request_id.clone())
        }));
        let outcome = match called {
            Ok(Ok(outcome)) => outcome,
            // A2 answers a busy send lane and every refusal before the call as
            // `Failed`; an `Err` (or a panic) means the call's thread died, so
            // it may have run.
            Ok(Err(error)) => Outcome::Uncertain { detail: error },
            Err(_) => Outcome::Uncertain {
                detail: "The send ended abnormally.".into(),
            },
        };
        let detail = self.lock().local.finished(&key, &request_id, &outcome);
        let ok = matches!(outcome, Outcome::Done { .. });
        let uncertain = matches!(outcome, Outcome::Uncertain { .. });
        self.ended(&delivery, ok, &detail, uncertain);
        // `finishMessage`: the open session is read again (`inspect()`).
        if self.lock().open_key.as_deref() == Some(key.as_str()) {
            self.refresh_details();
        }
    }

    /// Qt's `stillValid` (`SessionsWindow.cpp:4880-4904`).
    pub(crate) fn still_valid(&self, delivery: &Delivery) -> bool {
        let now = self.now();
        let expired = |details: Option<&Value>| details.is_some_and(|d| cache::expired(d, now));
        // Without consent, a cache that has expired since Send was pressed
        // asks first.
        if !delivery.consent && expired(delivery.evidence.as_deref()) {
            return false;
        }
        let hub = self.lock();
        let Some((row, online)) = (self.0.deps.rows)(
            &delivery.session.computer,
            &delivery.session.key,
            &delivery.session.name,
        ) else {
            return false;
        };
        // `observedIdentityMatches`: the row still shows the run it showed,
        // or the listing caught up with the details' run.
        let (row_run, row_conversation) = (
            row.run_id.as_deref().unwrap_or(""),
            row.conversation_id.as_deref().unwrap_or(""),
        );
        let observed = (row_run == delivery.row_run
            && row_conversation == delivery.row_conversation)
            || (row_run == delivery.run && row_conversation == delivery.conversation);
        if row.name != delivery.session.name
            || row.created != delivery.created
            || !observed
            || !online
        {
            return false;
        }
        // A termination, or any session update or rename (Qt's `m_pending`).
        if (self.0.deps.facts)(&delivery.session.computer, &delivery.session.name).is_some() {
            return false;
        }
        if let Some(id) = &delivery.retry_id {
            let retained = hub
                .local
                .rows(&delivery.key)
                .iter()
                .any(|row| &row.id == id && row.status == Status::Error && !row.uncertain);
            if !retained {
                return false;
            }
        }
        let open = hub.open_key.as_deref() == Some(delivery.key.as_str());
        let ctx = if open { self.context(&hub) } else { None };
        if open && ctx.is_none() {
            return false;
        }
        // The open session's current details, else the evidence the send
        // was checked against.
        let evidence = match &ctx {
            Some(ctx) => ctx.details.clone(),
            None => delivery.evidence.clone(),
        };
        if let Some(details) = evidence.as_deref() {
            if text(details, "run_id") != delivery.run
                || text(details, "conversation_id") != delivery.conversation
                || text(details, "runtime_state") != "live"
                || text(details, "process_state") != "running"
                || (!delivery.consent && expired(Some(details)))
            {
                return false;
            }
        } else if open {
            return false;
        }
        !ctx.is_some_and(|ctx| ctx.reason().is_some())
    }

    /// `MessageComposer::deliveryFinished`. A Retry that kept the draft
    /// gives the draft back its marks; otherwise a success clears the draft,
    /// and an uncertain end marks it, so Send waits for Allow retry.
    pub(crate) fn delivery_finished(
        &self,
        key: &str,
        ok: bool,
        detail: &str,
        uncertain: bool,
        preserve: bool,
    ) {
        self.finish(key, None, ok, detail, uncertain, preserve);
    }

    /// The end of `delivery`: it ends the composer's send only while it is
    /// still the delivery under way.
    fn ended(&self, delivery: &Delivery, ok: bool, detail: &str, uncertain: bool) {
        let revision = Some(delivery.revision);
        self.finish(
            &delivery.key,
            revision,
            ok,
            detail,
            uncertain,
            delivery.preserve,
        );
    }

    fn finish(
        &self,
        key: &str,
        revision: Option<u64>,
        ok: bool,
        detail: &str,
        uncertain: bool,
        preserve: bool,
    ) {
        let mut hub = self.lock();
        if let Some(composer) = hub.composers.get_mut(key) {
            // A delivery that already ended, or that a newer one replaced,
            // ends nothing.
            if revision.is_some_and(|revision| composer.delivering != Some(revision)) {
                return;
            }
            composer.delivering = None;
            composer.sending = false;
            composer.waiting = None;
            let kept = composer.kept.take();
            if preserve {
                if let Some((error, notice)) = kept {
                    composer.error = error;
                    composer.notice = notice;
                }
            } else {
                composer.error = !ok;
                composer.uncertain = uncertain;
                composer.notice = if ok {
                    "Submitted to terminal".into()
                } else {
                    detail.to_owned()
                };
                if ok {
                    composer.draft = attachments::Draft::default();
                    composer.position = 0;
                    composer.anchor = 0;
                    composer.attachments_changed();
                }
            }
            composer.text_epoch += 1;
            self.save(composer);
        }
        drop(hub);
        self.reconcile();
        self.changed();
    }

    /// The cached files every failed, definite local row still needs, of
    /// every session S4 keeps rows for. Only those rows are copied under the
    /// hub lock; their files are read after it.
    pub(crate) fn retry_files(&self) -> HashSet<PathBuf> {
        let failed: Vec<LocalMessage> = {
            let hub = self.lock();
            hub.local
                .keys()
                .flat_map(|key| hub.local.rows(key))
                .filter(|row| row.status == Status::Error && !row.uncertain)
                .cloned()
                .collect()
        };
        failed
            .iter()
            .filter_map(LocalMessages::stored_files)
            .flatten()
            .map(|file| file.local_path)
            .collect()
    }

    /// Drops the open session's local rows its history now shows
    /// (`reconcileMessages`); a failed row delivered after all clears a
    /// draft that still holds it. True when a row went.
    pub(crate) fn reconcile(&self) -> bool {
        let (key, removed, clear) = {
            let mut guard = self.lock();
            let Some(key) = guard.open_key.clone() else {
                return false;
            };
            if guard.local.rows(&key).is_empty() {
                return false;
            }
            let hub = &mut *guard;
            let details = hub.followed.details.clone();
            let reconciled = hub.local.reconcile(
                &key,
                details.as_deref().unwrap_or(&Value::Null),
                hub.followed.events.make_contiguous(),
            );
            let clear = hub.composers.get(&key).is_some_and(|c| {
                !c.sending
                    && c.draft.attachments.is_empty()
                    && reconciled.delivered.contains(&c.draft.text)
            });
            (key, reconciled.removed > 0, clear)
        };
        if clear {
            self.delivery_finished(&key, true, "", false, false);
        }
        removed
    }

    /// Retry on a failed row (`messageAction` "retry"). Blocking: it reads
    /// the cached files back between `retry_begin` and `retry_finish`. Only
    /// a definite failure (S4's `retryable`) is ever sent again, as a new
    /// submission with its own request id.
    pub fn retry(&self, handle: u64, id: &str) -> SendReply {
        let begun = match self.retry_begin(handle, id) {
            Ok(begun) => begun,
            Err(reply) => return reply,
        };
        let read = attachments::read_stored(self.cache().root(), &begun.files);
        self.retry_finish(begun, read)
    }

    /// Retry's checks before the read (`messageAction`, 4484-4492): the row
    /// a definite failure of the run the session shows, the composer idle,
    /// no block reason and no other read of this session's files.
    pub(crate) fn retry_begin(&self, handle: u64, id: &str) -> Result<RetryBegun, SendReply> {
        let mut hub = self.lock();
        let Some(ctx) = self.context(&hub) else {
            return Err(SendReply::Busy);
        };
        if hub.closed
            || !hub
                .composers
                .get(&ctx.key)
                .is_some_and(|c| c.handle == handle && !c.busy())
        {
            return Err(SendReply::Busy);
        }
        let Some(row) = hub
            .local
            .retryable(&ctx.key, id, &ctx.run(), &ctx.conversation())
            .cloned()
        else {
            return Err(SendReply::Busy);
        };
        if let Some(reason) = ctx.reason() {
            return Err(SendReply::Notice {
                text: reason,
                error: true,
            });
        }
        let Some(files) = LocalMessages::stored_files(&row) else {
            return Err(SendReply::Notice {
                text: UNAVAILABLE.into(),
                error: true,
            });
        };
        if !hub.retry_reads.insert(ctx.key.clone()) {
            return Err(SendReply::Busy);
        }
        Ok(RetryBegun {
            handle,
            id: id.to_owned(),
            ctx,
            row,
            files,
        })
    }

    /// Retry after the read (`messageAction`, 4497-4521).
    pub(crate) fn retry_finish(
        &self,
        begun: RetryBegun,
        read: Result<Vec<Attachment>, String>,
    ) -> SendReply {
        let RetryBegun {
            handle,
            id,
            ctx,
            row,
            ..
        } = begun;
        let mut hub = self.lock();
        hub.retry_reads.remove(&ctx.key);
        let attachments = match read {
            Ok(attachments) => attachments,
            Err(error) => {
                return SendReply::Notice {
                    text: error,
                    error: true,
                }
            }
        };
        if hub.closed {
            return SendReply::Busy;
        }
        // Qt returns quietly when the session or its run changed during the
        // read, or a write or a send began (4501-4509).
        let Some((now_row, online)) =
            (self.0.deps.rows)(&ctx.session.computer, &ctx.session.key, &ctx.session.name)
        else {
            return SendReply::Busy;
        };
        let open = self.context(&hub).filter(|now| now.key == ctx.key);
        let evidence = match &open {
            Some(now) => now.details.clone(),
            None => ctx.details.clone(),
        };
        let evidence = evidence.as_deref().unwrap_or(&Value::Null);
        let observed = (now_row.run_id == ctx.row.run_id
            && now_row.conversation_id == ctx.row.conversation_id)
            || (now_row.run_id.as_deref() == Some(row.run_id.as_str())
                && now_row.conversation_id.as_deref() == Some(row.conversation_id.as_str()));
        let unchanged = now_row.name == ctx.session.name
            && now_row.created == ctx.row.created
            && observed
            && online
            && (self.0.deps.facts)(&ctx.session.computer, &ctx.session.name).is_none()
            && text(evidence, "run_id") == row.run_id
            && text(evidence, "conversation_id") == row.conversation_id
            && text(evidence, "runtime_state") == "live"
            && text(evidence, "process_state") == "running"
            && hub
                .composers
                .get(&ctx.key)
                .is_some_and(|c| c.handle == handle && !c.busy())
            && hub
                .local
                .retryable(&ctx.key, &id, &row.run_id, &row.conversation_id)
                .is_some_and(|now| *now == row);
        if !unchanged {
            return SendReply::Busy;
        }
        let retry = Retry {
            id,
            text: row.text.clone(),
            attachments: attachments.clone(),
        };
        match open {
            // `sendMessage(key, text, files, id)` (4515): its block reason
            // ends in the composer's error (4811), then the cold question.
            Some(now) => {
                if let Some(reason) = now.reason() {
                    if let Some(composer) = hub.composers.get_mut(&now.key) {
                        composer.show_error(reason.clone());
                    }
                    drop(hub);
                    self.changed();
                    return SendReply::Blocked { reason };
                }
                self.consent_or_deliver(&mut hub, &now, row.text, attachments, Some(retry))
            }
            // Another session opened during the read: a warm retry stays
            // with its own session; a cold one needs the question there
            // (4517-4521).
            None => {
                if cache::expired(evidence, self.now()) {
                    return SendReply::Busy;
                }
                let delivery =
                    self.delivery(&hub, &ctx, row.text, attachments, Some(retry), None, false);
                if self.deliver(&mut hub, delivery) {
                    SendReply::Sending
                } else {
                    SendReply::Busy
                }
            }
        }
    }

    /// Delete on a failed row.
    pub fn delete_local(&self, handle: u64, id: &str) {
        let mut hub = self.lock();
        let Some(key) = hub.open_key.clone() else {
            return;
        };
        if hub.composers.get(&key).is_some_and(|c| c.handle == handle) && hub.local.delete(&key, id)
        {
            drop(hub);
            self.changed();
        }
    }

    /// Inspect on a failed row: Native UI, Terminal, or a refresh with Qt's
    /// notice (`messageAction` "inspect").
    pub fn inspect_local(&self, handle: u64, id: &str) -> Option<Inspect> {
        let hub = self.lock();
        let ctx = self.context(&hub)?;
        hub.composers.get(&ctx.key).filter(|c| c.handle == handle)?;
        hub.local
            .rows(&ctx.key)
            .iter()
            .find(|row| row.id == id && row.status == Status::Error)?;
        let facts = TerminalFacts {
            cmd: ctx.facts.cmd.clone(),
            terminal_supported: ctx.row.terminal_supported,
            terminal_reported: ctx.row.terminal_reported,
            terminal_attach_supported: ctx.row.terminal_attach_supported,
            terminal_attach_reported: ctx.row.terminal_attach_reported,
            ..TerminalFacts::default()
        };
        let details = ctx.details.as_deref().unwrap_or(&Value::Null);
        let routed = local_messages::inspect(
            &ctx.facts.cmd,
            terminal::supported(&facts, details),
            &ctx.row.machine,
        );
        drop(hub);
        if matches!(routed, Inspect::Refresh { .. }) {
            self.refresh_details();
        }
        Some(routed)
    }

    #[cfg(test)]
    pub(crate) fn delivery_for_test(&self, handle: u64, message: &str) -> Delivery {
        let hub = self.lock();
        let ctx = self.context(&hub).unwrap();
        assert!(hub
            .composers
            .get(&ctx.key)
            .is_some_and(|c| c.handle == handle));
        self.delivery(
            &hub,
            &ctx,
            message.to_owned(),
            Vec::new(),
            None,
            None,
            false,
        )
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn send_message(
    state: State<'_, Composers>,
    handle: u64,
    rev: u64,
    epoch: u64,
    text: String,
    position: i64,
    anchor: i64,
) -> Result<SendReply, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        composers.send_message(handle, rev, epoch, text, position, anchor)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn send_answer(state: State<'_, Composers>, token: u64, answer: Answer) -> SendReply {
    state.answer(token, answer)
}

#[tauri::command]
pub async fn local_retry(
    state: State<'_, Composers>,
    handle: u64,
    id: String,
) -> Result<SendReply, String> {
    let composers = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || composers.retry(handle, &id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn local_delete(state: State<'_, Composers>, handle: u64, id: String) {
    state.delete_local(handle, &id);
}

#[tauri::command]
pub fn local_inspect(state: State<'_, Composers>, handle: u64, id: String) -> Option<Inspect> {
    state.inspect_local(handle, &id)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::composer::tests::{dir, drafts, hold, session, until, Scratch, KEY};
    use crate::composer::{lock, ComposerView, Composers, Config, Deps, Job, Spawn};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use zerus_client_core::drafts::DraftStore;
    use zerus_client_core::{ActivityOut, Batch, ComputerId, RowOut};

    const OFFLINE: &str =
        "Machine unavailable. Showing the last known state; actions will return when it reconnects.";

    /// What the fake `dispatch` answers a call with.
    type Answers = Box<dyn Fn(&Action, &str) -> Outcome + Send>;

    /// The session, clock, inspect and calls a test controls.
    pub(crate) struct Fake {
        pub(crate) row: Mutex<ViewRow>,
        pub(crate) now: Mutex<f64>,
        pub(crate) inspected: Mutex<Option<Value>>,
        pub(crate) inspects: AtomicUsize,
        pub(crate) online: Mutex<bool>,
        pub(crate) write: Mutex<Option<WriteKind>>,
        pub(crate) calls: Mutex<Vec<(Target, Action, String)>>,
        pub(crate) outcome: Mutex<Answers>,
    }

    pub(crate) fn row(name: &str) -> ViewRow {
        ViewRow {
            name: name.into(),
            agent: "claude".into(),
            state: "running".into(),
            tracked: true,
            run_id: Some("run-1".into()),
            conversation_id: Some("conv-1".into()),
            ..ViewRow::default()
        }
    }

    pub(crate) fn send_hub_on(store: &Arc<DraftStore>) -> (Composers, Arc<Fake>) {
        send_hub_with(
            store,
            Arc::new(|job: Job| {
                std::thread::spawn(job);
            }),
        )
    }

    /// `send_hub_on` with the test's own `spawn`.
    fn send_hub_with(store: &Arc<DraftStore>, spawn: Spawn) -> (Composers, Arc<Fake>) {
        let fake = Arc::new(Fake {
            row: Mutex::new(row("claude/sample-project/review")),
            now: Mutex::new(1_793_000_100.0),
            inspected: Mutex::new(None),
            inspects: AtomicUsize::new(0),
            online: Mutex::new(true),
            write: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
            outcome: Mutex::new(Box::new(receipt)),
        });
        let (f1, f2, f3, f4) = (fake.clone(), fake.clone(), fake.clone(), fake.clone());
        let f5 = fake.clone();
        let composers = Composers::new(
            Arc::clone(store),
            Config {
                cache: store.root().with_file_name("cache"),
            },
            Deps {
                spawn,
                notify: Arc::new(|| {}),
                clock: Arc::new(move || *lock(&f1.now)),
                rows: Arc::new(move |_, _, _| Some((lock(&f2.row).clone(), *lock(&f2.online)))),
                facts: Arc::new(move |_, _| *lock(&f4.write)),
                inspect: Arc::new(move |_, _| {
                    f3.inspects.fetch_add(1, Ordering::SeqCst);
                    lock(&f3.inspected)
                        .clone()
                        .ok_or_else(|| "timed out".to_owned())
                }),
                dispatch: Arc::new(move |target, action, id| {
                    let out = (lock(&f5.outcome))(&action, &id);
                    lock(&f5.calls).push((target, action, id));
                    Ok(out)
                }),
            },
        );
        (composers, fake)
    }

    pub(crate) fn send_hub(root: &Scratch) -> (Composers, Arc<Fake>) {
        send_hub_on(&drafts(root))
    }

    pub(crate) fn ready() -> Value {
        json!({"tracked": true, "run_id": "run-1", "conversation_id": "conv-1", "runtime_state": "live", "process_state": "running",
               "phase": "idle", "activity": "idle", "compact_context_supported": true, "clear_context_supported": true})
    }

    pub(crate) fn state(details: Value) -> Batch {
        Batch {
            activity: vec![ActivityOut {
                computer: ComputerId::from("studio"),
                key: "run:run-1".into(),
                kind: "state".into(),
                payload: json!({"value": details, "rev": 1}),
            }],
            ..Batch::default()
        }
    }

    pub(crate) fn opened(c: &Composers, details: Value) -> ComposerView {
        c.open(Some(session()));
        c.observe(&state(details));
        until(c, |v| v.loaded)
    }

    #[test]
    fn a_ready_session_takes_messages() {
        let root = dir("ready");
        let (c, _) = send_hub(&root);
        let view = opened(&c, ready());
        assert!(
            view.available && view.block_reason.is_none() && view.unavailable_reason.is_empty()
        );
        let handle = view.handle;
        c.edit(handle, 1, view.text_epoch, "hello".into(), 5, 5);
        assert!(c.current().unwrap().can_send);
    }

    #[test]
    fn a_block_reason_is_shown() {
        let root = dir("blocked");
        let (c, _) = send_hub(&root);
        let mut details = ready();
        details["phase"] = json!("approval");
        let view = opened(&c, details);
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Respond to the agent in the Terminal tab.")
        );
        assert_eq!(
            view.unavailable_reason,
            "Respond to the agent in the Terminal tab."
        );
        assert!(!view.available);
        c.edit(view.handle, 1, view.text_epoch, "hi".into(), 2, 2);
        let view = c.current().unwrap();
        assert!(!view.can_send);
        assert_eq!(view.status, "Respond to the agent in the Terminal tab.");
    }

    #[test]
    fn the_cache_chip_counts_down_in_minutes() {
        let root = dir("chip");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["session_usage"] =
            json!({"prompt_cache": {"status": "warm", "expires_at": 1_793_000_100.0 + 540.0}});
        let view = opened(&c, details);
        assert_eq!(
            (view.cache.label.as_str(), view.cache.short_label.as_str()),
            ("Cache ~9m", "~9m")
        );
        assert!(view
            .cache_details
            .starts_with("Estimated expiry from the TTL reported in native usage: "));
        *lock(&fake.now) += 240.0;
        c.tick();
        assert_eq!(c.current().unwrap().cache.label, "Cache ~5m");
    }

    #[test]
    fn without_a_state_the_session_is_inspected_once() {
        let root = dir("inspect");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        assert_eq!(view.block_reason.as_deref(), Some("Checking the session…"));
        *lock(&fake.now) += 3.5;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        c.tick();
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_fallback_waits_three_seconds_after_opening() {
        let root = dir("inspect-wait");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 2.9;
        c.tick();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 0);
        *lock(&fake.now) += 0.2;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_state_after_a_failed_inspect_clears_its_reason() {
        let root = dir("inspect-failed");
        let (c, fake) = send_hub(&root);
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 3.5;
        c.tick();
        until(&c, |v| {
            v.block_reason.as_deref() == Some("Refresh the session before sending.")
        });
        c.observe(&state(ready()));
        assert!(c.current().unwrap().block_reason.is_none());
    }

    /// Waits until `done` holds, within five seconds.
    fn eventually(done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_chip_changes_when_its_minute_does_not_on_the_clocks() {
        let root = dir("chip-boundary");
        let (c, fake) = send_hub(&root);
        let start = *lock(&fake.now);
        // Ten minutes from a time that is not on a whole minute.
        let mut details = ready();
        details["session_usage"] =
            json!({"prompt_cache": {"status": "warm", "expires_at": start + 545.0}});
        assert_eq!(opened(&c, details).cache.label, "Cache ~10m");
        assert!(c.take_view().is_some());
        c.tick();
        assert!(c.take_view().is_none(), "the same minute wakes nothing");
        *lock(&fake.now) = start + 5.0;
        c.tick();
        assert_eq!(c.take_view().unwrap().cache.label, "Cache ~9m");
        *lock(&fake.now) = start + 544.0;
        c.tick();
        assert_eq!(c.take_view().unwrap().cache.label, "Cache ~1m");
        *lock(&fake.now) = start + 545.0;
        c.tick();
        let view = c.take_view().unwrap();
        assert_eq!(view.cache.label, "Cold cache");
        assert_eq!(
            view.cache_warning,
            "The estimated cache lifetime has ended. Continuing may rebuild the cached context."
        );
    }

    #[test]
    fn a_tick_without_an_open_composer_wakes_nothing() {
        let root = dir("tick-closed");
        let store = drafts(&root);
        let woken = Arc::new(AtomicUsize::new(0));
        let count = woken.clone();
        let now = Arc::new(Mutex::new(1_793_000_100.0));
        let clock = now.clone();
        let c = Composers::new(
            Arc::clone(&store),
            Config {
                cache: root.join("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                }),
                clock: Arc::new(move || *lock(&clock)),
                rows: Arc::new(|_, _, _| Some((row("claude/sample-project/review"), true))),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(|_, _| panic!("nothing is open to inspect")),
                dispatch: Arc::new(|_, _, _| Err("no call in these tests".into())),
            },
        );
        // The stored draft keys' first answer wakes the pump once.
        eventually(|| woken.load(Ordering::SeqCst) > 0);
        let before = woken.load(Ordering::SeqCst);
        for minute in 1..5 {
            *lock(&now) += 60.0 * f64::from(minute);
            c.tick();
        }
        assert_eq!(woken.load(Ordering::SeqCst), before);
    }

    #[test]
    fn the_ticker_sleeps_while_no_composer_is_open() {
        let root = dir("ticker");
        let store = drafts(&root);
        let clocks = Arc::new(AtomicUsize::new(0));
        let counted = clocks.clone();
        let c = Composers::new(
            Arc::clone(&store),
            Config {
                cache: root.join("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(|| {}),
                clock: Arc::new(move || {
                    counted.fetch_add(1, Ordering::SeqCst);
                    1_793_000_100.0
                }),
                rows: Arc::new(|_, _, _| None),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(|_, _| Err("timed out".into())),
                dispatch: Arc::new(|_, _, _| Err("no call in these tests".into())),
            },
        );
        let parked = || c.0.parked.load(Ordering::SeqCst);
        let ticker = c.start_ticker_every(Duration::from_millis(5));
        eventually(parked);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(clocks.load(Ordering::SeqCst), 0, "nothing open: no tick");
        c.open(Some(session()));
        let opened = clocks.load(Ordering::SeqCst);
        eventually(|| clocks.load(Ordering::SeqCst) > opened + 3);
        c.open(None);
        // Parked again: from here on the ticker reads no clock.
        eventually(parked);
        let closed = clocks.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            clocks.load(Ordering::SeqCst),
            closed,
            "closed again: no tick"
        );
        c.flush();
        let (done, ended) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            ticker.join().unwrap();
            let _ = done.send(());
        });
        ended
            .recv_timeout(Duration::from_secs(5))
            .expect("the ticker ends when the app quits");
    }

    #[test]
    fn nothing_is_inspected_while_the_computer_is_offline() {
        let root = dir("inspect-offline");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        *lock(&fake.online) = false;
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 10.0;
        c.tick();
        c.refresh_details();
        assert!(c.lock().followed.inspecting.is_none(), "none started");
        std::thread::sleep(Duration::from_millis(50));
        let view = c.current().unwrap();
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Machine unavailable. Your draft stays here.")
        );
        assert!(view.cache_details.ends_with("Last recorded session data."));
        assert_eq!(view.status, OFFLINE);
        assert!(view.status_error && view.session_blocks_input);
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 0);
        *lock(&fake.online) = true;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_inspect_is_retried_and_a_read_is_repeated_on_a_change() {
        let root = dir("inspect-again");
        let (c, fake) = send_hub(&root);
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        // The n-th inspect has answered.
        let reads = |n| {
            eventually(|| {
                fake.inspects.load(Ordering::SeqCst) == n && c.lock().followed.inspecting.is_none()
            })
        };
        let wait = |seconds: f64| {
            *lock(&fake.now) += seconds;
            c.tick();
        };
        wait(3.5);
        reads(1);
        let view = c.current().unwrap();
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Refresh the session before sending.")
        );
        assert_eq!(view.status, "Could not refresh activity: timed out");
        // Qt polls every 2.5 s while nothing streams.
        wait(1.0);
        wait(1.0);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1, "not before 2.5 s");
        wait(0.5);
        reads(2);
        *lock(&fake.inspected) = Some(ready());
        wait(2.5);
        reads(3);
        until(&c, |v| v.block_reason.is_none() && v.status.is_empty());
        // A read is repeated only when the row's run, conversation or state changed.
        wait(10.0);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            fake.inspects.load(Ordering::SeqCst),
            3,
            "a success is not polled"
        );
        lock(&fake.row).run_id = Some("run-2".into());
        wait(0.1);
        reads(4);
        lock(&fake.row).conversation_id = Some("conv-2".into());
        wait(0.1);
        reads(5);
        lock(&fake.row).state = "paused".into();
        wait(0.1);
        reads(6);
        for _ in 0..3 {
            wait(1.0);
        }
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 6);
    }

    #[test]
    fn the_stream_keeps_the_details_and_an_end_inspects() {
        let root = dir("inspect-streamed");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        opened(&c, ready());
        *lock(&fake.now) += 10.0;
        lock(&fake.row).state = "paused".into();
        c.tick();
        c.refresh_details();
        assert!(c.lock().followed.inspecting.is_none(), "none started");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            fake.inspects.load(Ordering::SeqCst),
            0,
            "Qt reads nothing while streamed"
        );
        lock(&fake.row).state = "running".into();
        c.observe(&Batch {
            activity: vec![ActivityOut {
                computer: ComputerId::from("studio"),
                key: "run:run-1".into(),
                kind: "ended".into(),
                payload: Value::Null,
            }],
            ..Batch::default()
        });
        assert!(
            c.current().unwrap().block_reason.is_none(),
            "the details stay, as in Qt"
        );
        c.tick();
        reads_settle(&c, &fake, 1);
        assert!(c.current().unwrap().block_reason.is_none());
    }

    /// The n-th inspect has answered.
    fn reads_settle(c: &Composers, fake: &Fake, n: usize) {
        eventually(|| {
            fake.inspects.load(Ordering::SeqCst) == n && c.lock().followed.inspecting.is_none()
        });
    }

    fn ended() -> Batch {
        Batch {
            activity: vec![ActivityOut {
                computer: ComputerId::from("studio"),
                key: "run:run-1".into(),
                kind: "ended".into(),
                payload: Value::Null,
            }],
            ..Batch::default()
        }
    }

    #[test]
    fn an_end_after_states_reads_the_session_again() {
        let root = dir("inspect-end");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 3.5;
        c.tick();
        reads_settle(&c, &fake, 1);
        c.observe(&state(ready()));
        c.observe(&state(ready()));
        // The row is unchanged: only the end says to read again.
        c.observe(&ended());
        *lock(&fake.now) += 0.1;
        c.tick();
        reads_settle(&c, &fake, 2);
        let view = c.current().unwrap();
        assert!(view.block_reason.is_none(), "{view:?}");
    }

    /// The composers with an inspect that waits for the test's word before it
    /// answers with `answer(n)` (n counts from 1) .
    fn gated(
        root: &Scratch,
        answer: fn(usize) -> Value,
    ) -> (
        Composers,
        Arc<Mutex<f64>>,
        std::sync::mpsc::Sender<()>,
        Arc<AtomicUsize>,
    ) {
        let now = Arc::new(Mutex::new(1_793_000_100.0));
        let clock = now.clone();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let answered = Arc::new(AtomicUsize::new(0));
        let counted = answered.clone();
        let c = Composers::new(
            drafts(root),
            Config {
                cache: root.join("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(|| {}),
                clock: Arc::new(move || *lock(&clock)),
                rows: Arc::new(|_, _, name| Some((row(name), true))),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(move |_, _| {
                    let _ = lock(&gate).recv_timeout(Duration::from_secs(5));
                    Ok(answer(counted.fetch_add(1, Ordering::SeqCst) + 1))
                }),
                dispatch: Arc::new(|_, _, _| Err("no call in these tests".into())),
            },
        );
        (c, now, release, answered)
    }

    fn approval() -> Value {
        let mut details = ready();
        details["phase"] = json!("approval");
        details
    }

    #[test]
    fn a_late_inspect_never_overwrites_a_newer_state() {
        let root = dir("inspect-newer");
        let (c, now, release, answered) = gated(&root, |_| ready());
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&now) += 3.5;
        c.tick();
        assert!(c.lock().followed.inspecting.is_some());
        c.observe(&state(approval()));
        release.send(()).unwrap();
        eventually(|| {
            answered.load(Ordering::SeqCst) == 1 && c.lock().followed.inspecting.is_none()
        });
        assert_eq!(
            c.current().unwrap().block_reason.as_deref(),
            Some("Respond to the agent in the Terminal tab."),
            "the state frame is newer than the inspect"
        );
    }

    #[test]
    fn an_end_drops_the_inspect_under_way() {
        let root = dir("inspect-ended");
        let (c, now, release, answered) =
            gated(&root, |n| if n == 1 { ready() } else { approval() });
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&now) += 3.5;
        c.tick();
        assert!(c.lock().followed.inspecting.is_some());
        c.observe(&state(approval()));
        c.observe(&ended());
        assert!(c.lock().followed.inspecting.is_none());
        release.send(()).unwrap();
        eventually(|| answered.load(Ordering::SeqCst) == 1);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            c.current().unwrap().block_reason.as_deref(),
            Some("Respond to the agent in the Terminal tab."),
            "the answer started before the state is dropped"
        );
        // The end reads anew, and that answer counts.
        c.tick();
        release.send(()).unwrap();
        until(&c, |v| {
            v.block_reason.as_deref() == Some("Respond to the agent in the Terminal tab.")
                && answered.load(Ordering::SeqCst) == 2
        });
    }

    #[test]
    fn an_inspect_of_a_session_no_longer_open_is_dropped() {
        let root = dir("inspect-late");
        let store = drafts(&root);
        let now = Arc::new(Mutex::new(1_793_000_100.0));
        let clock = now.clone();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let inspects = Arc::new(AtomicUsize::new(0));
        let counted = inspects.clone();
        let c = Composers::new(
            Arc::clone(&store),
            Config {
                cache: root.join("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(|| {}),
                clock: Arc::new(move || *lock(&clock)),
                rows: Arc::new(|_, key, name| {
                    let mut row = row(name);
                    row.run_id = Some(key.trim_start_matches("run:").into());
                    Some((row, true))
                }),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(move |target, _| {
                    // The first inspect waits until the test lets it answer.
                    if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                        let _ = lock(&gate).recv_timeout(Duration::from_secs(5));
                    }
                    let mut details = ready();
                    details["run_id"] = json!(target.run_id);
                    Ok(details)
                }),
                dispatch: Arc::new(|_, _, _| Err("no call in these tests".into())),
            },
        );
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&now) += 3.5;
        c.tick();
        eventually(|| inspects.load(Ordering::SeqCst) == 1);
        let mut other = session();
        other.key = "run:run-2".into();
        other.name = "claude/sample-project/other".into();
        c.open(Some(other));
        until(&c, |v| v.loaded && v.key.ends_with("/other"));
        release.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let view = c.current().unwrap();
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Checking the session…"),
            "the first session's answer is not the second's"
        );
        *lock(&now) += 3.5;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        assert_eq!(
            c.lock().followed.details.as_ref().unwrap()["run_id"],
            "run-2"
        );
        assert_eq!(inspects.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_batch_with_the_open_row_or_its_status_changes_the_view_at_once() {
        let root = dir("row-batch");
        let (c, fake) = send_hub(&root);
        opened(&c, ready());
        assert!(c.take_view().unwrap().available);
        lock(&fake.row).state = "paused".into();
        assert!(c.take_view().is_none(), "nothing announced it yet");
        let mut paused = row("claude/sample-project/review");
        paused.state = "paused".into();
        // Another session's row changes nothing.
        let mut other = paused.clone();
        other.name = "claude/sample-project/other".into();
        let upsert = |key: &str, row: ViewRow| Batch {
            upserts: vec![RowOut {
                computer: ComputerId::from("studio"),
                key: key.into(),
                row,
            }],
            ..Batch::default()
        };
        c.observe(&upsert("run:run-2", other));
        assert!(c.take_view().is_none());
        c.observe(&upsert("run:run-1", paused));
        assert_eq!(
            c.take_view().unwrap().block_reason.as_deref(),
            Some("Resume the session to send a message.")
        );
        lock(&fake.row).state = "running".into();
        *lock(&fake.online) = false;
        c.observe(&Batch {
            computers: vec![zerus_client_core::ComputerOut {
                computer: ComputerId::from("studio"),
                status: "reconnecting".into(),
                attempt: 1,
            }],
            ..Batch::default()
        });
        assert_eq!(
            c.take_view().unwrap().block_reason.as_deref(),
            Some("Machine unavailable. Your draft stays here.")
        );
    }

    #[test]
    fn a_write_blocks_messages_and_the_tick_shows_it() {
        let root = dir("write");
        let (c, fake) = send_hub(&root);
        opened(&c, ready());
        assert!(c.take_view().is_some());
        c.tick();
        assert!(c.take_view().is_none(), "nothing changed");
        *lock(&fake.write) = Some(WriteKind::Update);
        c.tick();
        let view = c.take_view().unwrap();
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Wait for the session update to finish.")
        );
        *lock(&fake.write) = Some(WriteKind::Terminate);
        c.tick();
        assert_eq!(
            c.take_view().unwrap().block_reason.as_deref(),
            Some("The session is terminating. Your draft stays here.")
        );
        *lock(&fake.write) = None;
        c.tick();
        assert!(c.take_view().unwrap().available);
    }

    #[test]
    fn files_wait_for_an_agent_that_takes_them() {
        let root = dir("attachments");
        let file = root.join("notes.txt");
        std::fs::write(&file, "notes").unwrap();
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        c.attach_paths(
            view.handle,
            vec![file],
            zerus_client_core::attachments::Selection::default(),
        );
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        assert!(view.can_send && view.attachments_available);
        assert!(c.can_attach(view.handle));
        lock(&fake.row).agent = "opencode".into();
        let view = c.current().unwrap();
        assert!(!view.attachments_available && !view.can_send);
        assert!(!c.can_attach(view.handle), "Qt's canAttachFiles");
        assert_eq!(view.status, "This native adapter does not support file attachments. Your existing files remain saved; remove them to send text.");
        assert_eq!(view.attachments_reason, view.status);
        lock(&fake.row).agent = "claude".into();
        let mut details = ready();
        details["attachments_supported"] = json!(false);
        details["attachments_reason"] = json!("This agent takes no files here.");
        c.observe(&state(details));
        let view = c.current().unwrap();
        assert_eq!(view.status, "This agent takes no files here.");
        assert!(!view.can_send);
    }

    #[test]
    fn a_deepseek_row_is_dsh_to_the_rules() {
        let root = dir("dsh");
        let (c, fake) = send_hub(&root);
        lock(&fake.row).agent = "deepseek".into();
        let mut details = ready();
        details["auth_required"] = json!(true);
        assert_eq!(
            opened(&c, details).block_reason.as_deref(),
            Some("Sign in to DeepSeek above. Your draft stays here.")
        );
    }

    #[test]
    fn the_expiry_is_local_time_in_qts_form() {
        let text = local_time(1_793_000_640);
        let parts: Vec<&str> = text.split(' ').collect();
        assert_eq!(parts.len(), 3, "{text}");
        assert!(
            (1..=31).contains(&parts[0].parse::<u32>().unwrap()),
            "{text}"
        );
        assert!(
            ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
                .contains(&parts[1]),
            "{text}"
        );
        assert!(
            parts[2].len() == 8 && parts[2].split(':').all(|n| n.len() == 2),
            "{text}"
        );
        assert_eq!(
            expiry(&json!({"session_usage": {"prompt_cache": {"expires_at": 1_793_000_640.5}}})),
            Some((1_793_000_640, text))
        );
        assert_eq!(expiry(&ready()), None);
    }

    #[test]
    fn the_session_status_follows_qts_hint_chain() {
        let status = |facts: HintFacts<'_>, blocked: bool| session_status(&facts, blocked);
        let online = || HintFacts {
            online: true,
            ..HintFacts::default()
        };
        assert_eq!(status(online(), true), SessionStatus::default());
        let offline = status(HintFacts::default(), true);
        assert_eq!(
            (offline.text.as_str(), offline.error, offline.blocks_input),
            (OFFLINE, true, true)
        );
        assert!(
            !status(HintFacts::default(), false).blocks_input,
            "no block reason: no lock"
        );
        let text = |facts: HintFacts<'_>| status(facts, true).text;
        assert_eq!(
            text(HintFacts {
                terminating: true,
                ..HintFacts::default()
            }),
            "Waiting for the session to stop. Its native conversation history is kept."
        );
        assert_eq!(
            text(HintFacts {
                archived: true,
                restore_error: "occupied",
                inspect_error: Some("Could not refresh activity: timed out"),
                ..HintFacts::default()
            }),
            OFFLINE,
            "offline comes before every error"
        );
        assert_eq!(
            text(HintFacts {
                archived: true,
                restore_error: "name taken",
                inspect_error: Some("Could not refresh activity: timed out"),
                question_error: "bad",
                ..online()
            }),
            "Last restore failed: name taken"
        );
        assert_eq!(
            text(HintFacts {
                inspect_error: Some("Could not refresh activity: timed out"),
                question_error: "bad",
                ..online()
            }),
            "Could not refresh activity: timed out"
        );
        assert_eq!(
            text(HintFacts {
                question_error: "bad",
                ..online()
            }),
            "Could not load agent questions: bad. Check Terminal."
        );
        let occupied = status(
            HintFacts {
                archived: true,
                occupied: true,
                ..online()
            },
            true,
        );
        assert_eq!(
            occupied.text,
            "Another active or saved session uses this name. Restore becomes available when that name is free."
        );
        assert!(
            !occupied.error && !occupied.blocks_input,
            "shown, not an error"
        );
        assert_eq!(
            status(
                HintFacts {
                    occupied: true,
                    restore_error: "x",
                    ..online()
                },
                true
            ),
            SessionStatus::default(),
            "only an archived session shows them"
        );
    }

    #[test]
    fn offline_the_status_locks_the_files_and_send() {
        let root = dir("status-offline");
        let files: Vec<std::path::PathBuf> = ["a", "b", "c"]
            .iter()
            .map(|name| {
                let file = root.join(format!("{name}.txt"));
                std::fs::write(&file, *name).unwrap();
                file
            })
            .collect();
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        let at = zerus_client_core::attachments::Selection::default();
        c.attach_paths(view.handle, files[..2].to_vec(), at);
        let view = until(&c, |v| v.attachments.len() == 2 && !v.attaching);
        assert!(view.can_send && !view.session_blocks_input && view.status.is_empty());
        c.remove(view.handle, 1);
        assert_eq!(c.current().unwrap().removed.as_deref(), Some("b.txt"));
        *lock(&fake.online) = false;
        let view = c.current().unwrap();
        assert_eq!(view.status, OFFLINE);
        assert!(view.status_error && view.session_blocks_input && !view.can_send);
        assert_eq!(
            view.unavailable_reason,
            "Machine unavailable. Your draft stays here."
        );
        assert!(!c.can_attach(view.handle));
        // Qt's `canEditAttachments`: no remove, Undo or attach while locked;
        // each refusal hands the text back.
        c.remove(view.handle, 0);
        let after = c.current().unwrap();
        assert_eq!(after.attachments.len(), 1, "no remove while locked");
        assert!(
            after.text_epoch > view.text_epoch,
            "the text is handed back"
        );
        c.undo_remove(view.handle);
        let undone = c.current().unwrap();
        assert_eq!(undone.attachments.len(), 1, "no Undo while locked");
        assert!(undone.text_epoch > after.text_epoch);
        c.attach_paths(view.handle, vec![files[2].clone()], at);
        let view = c.current().unwrap();
        assert!(
            !view.attaching && view.attachments.len() == 1,
            "no attach while locked"
        );
        *lock(&fake.online) = true;
        let view = c.current().unwrap();
        assert!(view.status.is_empty() && !view.session_blocks_input && view.can_send);
        assert!(c.can_attach(view.handle));
        c.undo_remove(view.handle);
        assert_eq!(c.current().unwrap().attachments.len(), 2);
    }

    #[test]
    fn a_failed_read_shows_its_reason_and_locks() {
        let root = dir("status-read");
        let (c, _) = send_hub(&root);
        let view = opened(&c, ready());
        c.edit(view.handle, 1, view.text_epoch, "hi".into(), 2, 2);
        assert!(c.current().unwrap().can_send);
        c.lock().followed.inspect_error = Some("Could not refresh activity: timed out".into());
        let view = c.current().unwrap();
        assert_eq!(view.status, "Could not refresh activity: timed out");
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Refresh the session before sending.")
        );
        assert!(view.status_error && view.session_blocks_input && !view.can_send);
    }

    #[test]
    fn inspection_reads_like_qt() {
        let hgs = Path::new("hgs");
        let code = |code: i32, stdout: &str, stderr: &str| {
            inspection(
                hgs,
                Ok(Exit::Code {
                    code,
                    stdout: stdout.as_bytes().to_vec(),
                    stderr: stderr.into(),
                }),
            )
        };
        assert_eq!(
            code(0, r#"{"run_id": "run-1"}"#, ""),
            Ok(json!({"run_id": "run-1"}))
        );
        let invalid = Err("Invalid activity response".to_owned());
        assert_eq!(code(0, "[1]", ""), invalid);
        assert_eq!(code(0, "not json", ""), invalid);
        assert_eq!(
            code(2, "", "  no such session \n"),
            Err("no such session".into())
        );
        assert_eq!(code(3, "", " "), Err("exit code 3".into()));
        assert_eq!(code(1, "", &"x".repeat(5000)), Err("x".repeat(4000)));
        assert_eq!(inspection(hgs, Ok(Exit::TimedOut)), Err("timed out".into()));
        assert_eq!(
            inspection(hgs, Ok(Exit::StartFailed("not found".into()))),
            Err("cannot start 'hgs': not found".into())
        );
        assert_eq!(
            inspection(
                hgs,
                Ok(Exit::Abnormal {
                    stderr: String::new()
                })
            ),
            Err("hgs ended abnormally".into())
        );
        assert_eq!(
            inspection(
                hgs,
                Err(TooLarge {
                    cap: 8 << 20,
                    stderr: String::new()
                })
            ),
            invalid
        );
    }

    #[test]
    fn a_write_shows_at_once_without_the_tick() {
        use crate::actions::{Actions, Lane};
        let root = dir("write-hook");
        let actions = Actions::new(root.join("marks.json"));
        let facts = actions.clone();
        let c = Composers::new(
            drafts(&root),
            Config {
                cache: root.join("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(|| {}),
                clock: Arc::new(|| 1_793_000_100.0),
                rows: Arc::new(|_, _, name| Some((row(name), true))),
                facts: Arc::new(move |computer, name| {
                    facts.session_write(computer, name, || (None, true))
                }),
                inspect: Arc::new(|_, _| Err("timed out".into())),
                dispatch: Arc::new(|_, _, _| Err("no call in these tests".into())),
            },
        );
        actions.on_writes(c.changed_hook());
        opened(&c, ready());
        assert!(c.take_view().unwrap().available);
        let target = zerus_client_core::Target {
            computer: "studio".into(),
            name: "claude/sample-project/other".into(),
            run_id: "run-2".into(),
            conversation_id: "conv-2".into(),
        };
        let pause = actions
            .begin(
                Lane::key(&target, &zerus_client_core::Action::Pause),
                &zerus_client_core::Action::Pause,
            )
            .unwrap();
        assert_eq!(
            c.take_view().unwrap().block_reason.as_deref(),
            Some("Wait for the session update to finish."),
            "another session's write, shown before any tick"
        );
        drop(pause);
        assert!(c.take_view().unwrap().available);
    }

    pub(crate) fn receipt(action: &Action, id: &str) -> Outcome {
        Outcome::Done {
            receipt: json!({"status": "submitted", "request_id": id, "kind": format!("{action:?}").split_whitespace().next()}),
            cleanup: None,
        }
    }

    pub(crate) fn sent(fake: &Fake) -> Vec<(Target, Action, String)> {
        lock(&fake.calls).clone()
    }

    #[test]
    fn send_runs_the_held_draft_once_and_clears_it() {
        let root = dir("send-once");
        let file = root.join("a.txt");
        std::fs::write(&file, "attached").unwrap();
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        c.attach_paths(
            view.handle,
            vec![file],
            zerus_client_core::attachments::Selection { start: 0, end: 0 },
        );
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        c.edit(
            view.handle,
            4,
            view.text_epoch,
            "hello [File #1] ".into(),
            0,
            0,
        );
        assert!(c.current().unwrap().can_send);
        assert_eq!(
            c.send_message(
                view.handle,
                5,
                view.text_epoch,
                "hello [File #1] ".into(),
                0,
                0
            ),
            SendReply::Sending
        );
        let view = until(&c, |v| !v.sending && v.text.is_empty());
        let calls = sent(&fake);
        assert_eq!(calls.len(), 1);
        let (target, action, id) = &calls[0];
        assert_eq!(
            (
                target.computer.as_str(),
                target.run_id.as_str(),
                target.conversation_id.as_str()
            ),
            ("studio", "run-1", "conv-1")
        );
        let Action::Send {
            text,
            attachments,
            agent_id,
            expected_compaction_id,
        } = action
        else {
            panic!("not a send")
        };
        assert_eq!(text, "hello [File #1] ");
        assert_eq!(&*attachments[0].bytes, b"attached");
        assert!(agent_id.is_none() && expected_compaction_id.is_none());
        assert!(view.attachments.is_empty());
        assert_eq!(view.local.len(), 1);
        assert_eq!(
            &view.local[0].id, id,
            "the row carries the request's own id"
        );
        assert_eq!(
            view.local[0].status,
            zerus_client_core::local_messages::Status::Sent
        );
    }

    #[test]
    fn a_blocked_send_runs_nothing() {
        let root = dir("blocked-send");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["phase"] = json!("approval");
        let view = opened(&c, details);
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "hi".into(), 2, 2),
            SendReply::Blocked {
                reason: "Respond to the agent in the Terminal tab.".into()
            }
        );
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_cold_cache_asks_first_and_sends_only_after_consent() {
        let root = dir("cold");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details);
        assert_eq!(view.cache.label, "Cold cache");
        let SendReply::Cold { token, prompt } =
            c.send_message(view.handle, 1, view.text_epoch, "hi".into(), 2, 2)
        else {
            panic!("no question")
        };
        assert_eq!(prompt.title, "Continue with a cold cache?");
        assert!(sent(&fake).is_empty());
        assert_eq!(c.answer(token, Answer::Cancel), SendReply::Busy);
        assert_eq!(
            c.answer(
                token,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Busy,
            "a cancelled question is gone"
        );
        let SendReply::Cold { token, .. } =
            c.send_message(view.handle, 2, view.text_epoch, "hi".into(), 2, 2)
        else {
            panic!()
        };
        assert_eq!(
            c.answer(
                token,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Sending
        );
        until(&c, |v| !v.sending);
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn an_uncertain_send_is_never_retried() {
        let root = dir("uncertain");
        let store = drafts(&root);
        let (c, fake) = send_hub_on(&store);
        *lock(&fake.outcome) = Box::new(|_, _| {
            Outcome::Uncertain { detail: "Delivery timed out. The message may have reached the agent; inspect Terminal before retrying.".into() }
        });
        let view = opened(&c, ready());
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5),
            SendReply::Sending
        );
        let view = until(&c, |v| !v.sending && v.uncertain);
        assert_eq!(view.text, "maybe", "the draft comes back");
        assert!(!view.can_send);
        let row = &view.local[0];
        assert!(row.uncertain);
        assert_eq!(row.error, "Delivery is not confirmed. Check Terminal before retrying. Delivery timed out. The message may have reached the agent; inspect Terminal before retrying.");
        assert_eq!(c.retry(view.handle, &row.id), SendReply::Busy);
        assert_eq!(
            c.send_message(view.handle, 2, view.text_epoch, "maybe".into(), 5, 5),
            SendReply::Busy,
            "Send waits for Allow retry"
        );
        c.flush();
        store.flush();
        drop(c);
        let (again, _) = send_hub_on(&store);
        let view = opened(&again, ready());
        assert!(
            view.uncertain && !view.can_send,
            "a restart keeps it uncertain"
        );
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn a_stale_question_never_resends_an_uncertain_draft() {
        let root = dir("stale-question");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details);
        let SendReply::Cold { token: first, .. } =
            c.send_message(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5)
        else {
            panic!("no question")
        };
        let SendReply::Cold { token: second, .. } =
            c.send_message(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5)
        else {
            panic!("no question")
        };
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Uncertain {
            detail: "Delivery timed out.".into(),
        });
        assert_eq!(
            c.answer(
                second,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Sending
        );
        until(&c, |v| !v.sending && v.uncertain);
        assert_eq!(
            c.answer(
                first,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Busy,
            "the older question went with the delivery"
        );
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn a_send_whose_save_is_queued_at_exit_never_starts() {
        let root = dir("exit-send");
        let store = drafts(&root);
        let (c, fake) = send_hub_on(&store);
        let view = opened(&c, ready());
        // Hold the writer, so the "sending" save waits in the queue.
        let (release, held) = std::sync::mpsc::channel::<()>();
        store.load("hold", move |_| {
            let _ = held.recv();
        });
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "at exit".into(), 7, 7),
            SendReply::Sending
        );
        c.flush();
        release.send(()).unwrap();
        store.flush();
        assert!(sent(&fake).is_empty(), "nothing starts once the app quits");
        let saved = zerus_client_core::drafts::read_draft(
            &root.join("drafts"),
            crate::composer::tests::KEY,
        );
        assert!(
            saved.draft.uncertain,
            "the draft saved as sending reloads as uncertain"
        );
    }

    #[test]
    fn a_definite_failure_retries_its_own_text_and_keeps_a_newer_draft() {
        let root = dir("retry-row");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed {
            detail: "hgs: refused".into(),
        });
        let view = opened(&c, ready());
        c.send_message(view.handle, 1, view.text_epoch, "first".into(), 5, 5);
        let view = until(&c, |v| {
            !v.sending && v.local.first().is_some_and(|r| !r.error.is_empty())
        });
        let id = view.local[0].id.clone();
        c.edit(
            view.handle,
            9,
            view.text_epoch,
            "a newer draft".into(),
            13,
            13,
        );
        *lock(&fake.outcome) = Box::new(receipt);
        assert_eq!(c.retry(view.handle, &id), SendReply::Sending);
        let view = until(&c, |v| {
            !v.sending && v.local.iter().all(|r| r.error.is_empty())
        });
        let calls = sent(&fake);
        assert_eq!(calls.len(), 2);
        let Action::Send { text, .. } = &calls[1].1 else {
            panic!()
        };
        assert_eq!(text, "first");
        assert_ne!(
            calls[0].2, calls[1].2,
            "a retry is a new submission with its own request id"
        );
        assert_eq!(view.text, "a newer draft");
        assert_eq!(view.local.len(), 1, "the retry replaced the failed row");
    }

    #[test]
    fn the_history_reconciles_sent_rows() {
        let root = dir("reconcile");
        let (c, _) = send_hub(&root);
        let view = opened(&c, ready());
        c.send_message(
            view.handle,
            1,
            view.text_epoch,
            "shown in history".into(),
            0,
            0,
        );
        until(&c, |v| !v.sending && v.local.len() == 1);
        let mut batch = state(ready());
        batch.activity.push(ActivityOut {
            computer: ComputerId::from("studio"), key: "run:run-1".into(), kind: "events".into(),
            payload: json!({"events": [{"seq": 9, "type": "UserPromptSubmit", "detail": "shown in history", "at": 1_793_000_100.0}], "cursor": 9}),
        });
        c.observe(&batch);
        until(&c, |v| v.local.is_empty());
    }

    #[test]
    fn a_session_that_changed_is_not_sent_to() {
        let root = dir("changed");
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        let delivery = c.delivery_for_test(view.handle, "hi");
        assert!(c.still_valid(&delivery));
        lock(&fake.row).created += 1;
        assert!(
            !c.still_valid(&delivery),
            "another session under the same name"
        );
        lock(&fake.row).created -= 1;
        lock(&fake.row).run_id = Some("run-2".into());
        assert!(!c.still_valid(&delivery), "another run");
    }

    // The guards each have a test of their own below, so a mutation of one
    // is caught even where another guard stands behind it.

    /// Jobs the hub spawns, held until the test runs them.
    type Held = Arc<Mutex<Vec<Job>>>;

    fn held_spawn() -> (Spawn, Held) {
        let jobs: Held = Arc::default();
        let queue = Arc::clone(&jobs);
        (Arc::new(move |job: Job| lock(&queue).push(job)), jobs)
    }

    fn run_held(jobs: &Held) {
        let taken: Vec<Job> = std::mem::take(&mut *lock(jobs));
        for job in taken {
            job();
        }
    }

    /// The oldest held job, once one is there (within five seconds).
    fn next_job(jobs: &Held) -> Job {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let job = {
                let mut queue = lock(jobs);
                (!queue.is_empty()).then(|| queue.remove(0))
            };
            if let Some(job) = job {
                return job;
            }
            assert!(std::time::Instant::now() < deadline, "no job came");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn other_session() -> SessionRef {
        SessionRef {
            key: "run:run-2".into(),
            name: "claude/sample-project/other".into(),
            ..session()
        }
    }

    /// A definite failure of `text`, sent from the open composer: its row id.
    fn failed_send(c: &Composers, fake: &Fake, text: &str) -> String {
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed {
            detail: "hgs: refused".into(),
        });
        let view = c.current().unwrap();
        c.send_message(
            view.handle,
            view.rev + 1,
            view.text_epoch,
            text.into(),
            0,
            0,
        );
        let view = until(c, |v| {
            !v.sending && v.local.iter().any(|row| row.status == Status::Error)
        });
        *lock(&fake.outcome) = Box::new(receipt);
        view.local[0].id.clone()
    }

    /// Waits until `done` holds, within five seconds.
    fn settles(done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn retry_pins_follow_the_rows_not_the_sessions_opened() {
        let root = dir("send-retry-pins");
        let file = root.join("a.txt");
        std::fs::write(&file, "attached").unwrap();
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed {
            detail: "hgs: refused".into(),
        });
        let view = opened(&c, ready());
        c.attach_paths(
            view.handle,
            vec![file],
            zerus_client_core::attachments::Selection::default(),
        );
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        c.send_message(view.handle, 1, view.text_epoch, view.text.clone(), 0, 0);
        let view = until(&c, |v| {
            !v.sending && v.local.first().is_some_and(|r| !r.error.is_empty())
        });
        let stored = LocalMessages::stored_files(&view.local[0]).unwrap();
        let pinned = || c.retry_files();
        assert_eq!(pinned(), HashSet::from([stored[0].local_path.clone()]));
        // The hub forgot which sessions it opened (past 512 of them); S4
        // still keeps the row, so Retry still needs its file.
        c.lock().opened.clear();
        assert_eq!(pinned(), HashSet::from([stored[0].local_path.clone()]));
    }

    #[test]
    fn a_warm_retry_stays_with_its_session_when_another_opens() {
        let root = dir("send-retry-away");
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        let id = failed_send(&c, &fake, "first");
        let Ok(begun) = c.retry_begin(view.handle, &id) else {
            panic!("no retry")
        };
        // Another session opens while the cached files are read.
        c.open(Some(other_session()));
        until(&c, |v| v.loaded && v.key.ends_with("/other"));
        let read = attachments::read_stored(c.cache().root(), &begun.files);
        assert_eq!(c.retry_finish(begun, read), SendReply::Sending);
        settles(|| sent(&fake).len() == 2 && !c.lock().composers[KEY].sending);
        let calls = sent(&fake);
        let (target, action, _) = &calls[1];
        assert_eq!(
            (target.name.as_str(), target.run_id.as_str()),
            ("claude/sample-project/review", "run-1"),
            "the retry's own session"
        );
        let Action::Send { text, .. } = action else {
            panic!("not a send")
        };
        assert_eq!(text, "first");
        let view = c.current().unwrap();
        assert!(view.key.ends_with("/other") && !view.sending && view.local.is_empty());
    }

    #[test]
    fn a_cold_retry_never_reaches_a_session_no_longer_open() {
        let root = dir("send-retry-away-cold");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed {
            detail: "hgs: refused".into(),
        });
        let SendReply::Cold { token, .. } =
            c.send_message(view.handle, 1, view.text_epoch, "first".into(), 0, 0)
        else {
            panic!("no question")
        };
        assert_eq!(
            c.answer(
                token,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Sending
        );
        let view = until(&c, |v| !v.sending && !v.local.is_empty());
        let id = view.local[0].id.clone();
        *lock(&fake.outcome) = Box::new(receipt);
        let Ok(begun) = c.retry_begin(view.handle, &id) else {
            panic!("no retry")
        };
        c.open(Some(other_session()));
        until(&c, |v| v.loaded && v.key.ends_with("/other"));
        let read = attachments::read_stored(c.cache().root(), &begun.files);
        assert_eq!(c.retry_finish(begun, read), SendReply::Busy);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(sent(&fake).len(), 1, "only the first send");
        assert!(!c.lock().composers[KEY].sending);
    }

    #[test]
    fn a_write_that_starts_during_the_save_keeps_the_draft() {
        let root = dir("send-write-mid");
        let store = drafts(&root);
        let (c, fake) = send_hub_on(&store);
        let view = opened(&c, ready());
        let release = hold(&store);
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "mid write".into(), 9, 9),
            SendReply::Sending
        );
        // Qt's `m_pending`: any session's update now runs.
        *lock(&fake.write) = Some(WriteKind::Update);
        release.send(()).unwrap();
        let view = until(&c, |v| !v.sending);
        assert_eq!(view.status, CHANGED);
        assert_eq!(view.text, "mid write");
        std::thread::sleep(Duration::from_millis(50));
        assert!(sent(&fake).is_empty());
        // The same with another session open: the write alone stops it,
        // as Qt's `m_pending` does (4889).
        *lock(&fake.write) = None;
        let view = c.current().unwrap();
        let release = hold(&store);
        assert_eq!(
            c.send_message(view.handle, 2, view.text_epoch, "mid write".into(), 9, 9),
            SendReply::Sending
        );
        c.open(Some(other_session()));
        *lock(&fake.write) = Some(WriteKind::Update);
        release.send(()).unwrap();
        settles(|| !c.lock().composers[KEY].sending);
        assert_eq!(c.lock().composers[KEY].notice, CHANGED);
        std::thread::sleep(Duration::from_millis(50));
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn an_older_deliverys_call_never_runs_after_a_newer_one_started() {
        let root = dir("send-older-call");
        let store = drafts(&root);
        let (spawn, jobs) = held_spawn();
        let (c, fake) = send_hub_with(&store, spawn);
        let view = opened(&c, ready());
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "once".into(), 4, 4),
            SendReply::Sending
        );
        let older = next_job(&jobs);
        // A cancel (Task 5) ends the send, and the draft is sent anew.
        c.delivery_finished(KEY, false, "Cancelled.", false, false);
        let view = c.current().unwrap();
        assert!(!view.sending && view.text == "once");
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "once".into(), 4, 4),
            SendReply::Sending
        );
        let newer = next_job(&jobs);
        older();
        assert!(sent(&fake).is_empty(), "the older delivery calls nothing");
        assert!(c.current().unwrap().sending, "nor ends the newer one");
        newer();
        until(&c, |v| !v.sending);
        assert_eq!(sent(&fake).len(), 1);
        assert_eq!(c.current().unwrap().local.len(), 1);
    }

    #[test]
    fn a_block_reason_at_the_answer_is_an_error_notice() {
        let root = dir("send-question-blocked");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details.clone());
        let SendReply::Cold { token, .. } =
            c.send_message(view.handle, 1, view.text_epoch, "asked".into(), 5, 5)
        else {
            panic!("no question")
        };
        details["phase"] = json!("approval");
        c.observe(&state(details));
        assert_eq!(
            c.answer(
                token,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Notice {
                text: "Respond to the agent in the Terminal tab.".into(),
                error: true,
            }
        );
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_send_over_an_older_text_epoch_is_refused_and_handed_back() {
        let root = dir("send-epoch");
        let file = root.join("a.txt");
        std::fs::write(&file, "attached").unwrap();
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        c.attach_paths(
            view.handle,
            vec![file],
            zerus_client_core::attachments::Selection::default(),
        );
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        // The mention is gone from the text, so the remove keeps the text
        // and changes only the files.
        c.edit(view.handle, 1, view.text_epoch, "hi".into(), 2, 2);
        let older = view.text_epoch;
        c.remove(view.handle, 0);
        let removed = c.current().unwrap();
        assert!(
            removed.text_epoch > older && removed.attachments.is_empty() && removed.text == "hi"
        );
        assert_eq!(
            c.send_message(view.handle, 2, older, "hi".into(), 2, 2),
            SendReply::Busy
        );
        assert!(
            c.current().unwrap().text_epoch > removed.text_epoch,
            "the text is handed back"
        );
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            sent(&fake).is_empty(),
            "text typed over an older draft is never sent"
        );
        // The editor's text must be the draft's too.
        let view = c.current().unwrap();
        c.edit(view.handle, 3, view.text_epoch, "hi".into(), 2, 2);
        assert_eq!(
            c.send_message(
                view.handle,
                3,
                view.text_epoch,
                "not what it holds".into(),
                2,
                2
            ),
            SendReply::Busy
        );
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_newer_save_keeps_a_queued_send_from_starting() {
        let root = dir("send-revision");
        let store = drafts(&root);
        let (spawn, jobs) = held_spawn();
        let (c, fake) = send_hub_with(&store, spawn);
        let view = opened(&c, ready());
        let release = hold(&store);
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "queued".into(), 6, 6),
            SendReply::Sending
        );
        // Another write to the key took the next save revision meanwhile:
        // the "sending" save's answer is no longer the newest.
        c.lock().composers.get_mut(KEY).unwrap().saves += 1;
        release.send(()).unwrap();
        store.flush();
        assert!(
            lock(&jobs).is_empty(),
            "an older save's answer starts nothing"
        );
        run_held(&jobs);
        assert!(sent(&fake).is_empty());
    }

    /// A hub over `store` whose calls go to `dispatch`.
    fn dispatching(store: &Arc<DraftStore>, dispatch: crate::composer::Dispatch) -> Composers {
        Composers::new(
            Arc::clone(store),
            Config {
                cache: store.root().with_file_name("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(|| {}),
                clock: Arc::new(|| 1_793_000_100.0),
                rows: Arc::new(|_, _, name| Some((row(name), true))),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(|_, _| Err("timed out".into())),
                dispatch,
            },
        )
    }

    #[test]
    fn a_failed_saves_retry_never_overtakes_a_send() {
        let root = dir("send-save-retry");
        let blocker = root.join("blocked");
        std::fs::write(&blocker, "a file where the store's folder belongs").unwrap();
        let store = Arc::new(DraftStore::open(blocker.join("drafts")));
        let (spawn, jobs) = held_spawn();
        let (c, fake) = send_hub_with(&store, spawn);
        let view = opened(&c, ready());
        c.edit(view.handle, 1, view.text_epoch, "kept".into(), 4, 4);
        let view = until(&c, |v| v.status_error);
        // Qt's 2 s retry of the failed save, held until the send's save waits.
        let retry = next_job(&jobs);
        std::fs::remove_file(&blocker).unwrap();
        let release = hold(&store);
        // Send comes at the same rev, so its edit saves nothing of its own.
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "kept".into(), 4, 4),
            SendReply::Sending
        );
        // The retry comes due while the "sending" save waits: it finds a
        // newer revision and saves nothing.
        retry();
        release.send(()).unwrap();
        store.flush();
        let call = next_job(&jobs);
        call();
        let view = until(&c, |v| !v.sending);
        assert!(view.text.is_empty() && !view.status_error, "{view:?}");
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn a_call_whose_thread_died_is_uncertain() {
        for (name, panics) in [("send-died-err", false), ("send-died-panic", true)] {
            let root = dir(name);
            let store = drafts(&root);
            let calls = Arc::new(AtomicUsize::new(0));
            let counted = Arc::clone(&calls);
            let c = dispatching(
                &store,
                Arc::new(move |_, _, _| {
                    counted.fetch_add(1, Ordering::SeqCst);
                    if panics {
                        panic!("the call's thread ended");
                    }
                    Err("the call's thread ended".into())
                }),
            );
            let view = opened(&c, ready());
            assert_eq!(
                c.send_message(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5),
                SendReply::Sending
            );
            let view = until(&c, |v| !v.sending && v.uncertain);
            assert_eq!(view.text, "maybe", "the draft stays");
            assert!(view.local[0].uncertain && !view.can_send, "{view:?}");
            std::thread::sleep(Duration::from_millis(50));
            assert_eq!(calls.load(Ordering::SeqCst), 1, "called once, never again");
        }
    }

    #[test]
    fn the_last_gate_never_sends_an_uncertain_draft() {
        let root = dir("send-gate");
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        c.edit(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5);
        let delivery = c.delivery_for_test(view.handle, "maybe");
        {
            let mut hub = c.lock();
            hub.composers.get_mut(KEY).unwrap().uncertain = true;
            assert!(
                !c.deliver(&mut hub, delivery),
                "an uncertain draft never reaches its save"
            );
            assert!(!hub.composers[KEY].sending);
        }
        std::thread::sleep(Duration::from_millis(50));
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn an_uncertain_draft_is_never_asked_about() {
        let root = dir("send-uncertain-cold");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details);
        c.edit(view.handle, 1, view.text_epoch, "maybe".into(), 5, 5);
        c.lock().composers.get_mut(KEY).unwrap().uncertain = true;
        let view = c.current().unwrap();
        assert_eq!(
            c.send_message(view.handle, 2, view.text_epoch, "maybe".into(), 5, 5),
            SendReply::Busy
        );
        assert!(c.lock().prompts.is_empty(), "no question to answer later");
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_cache_that_turns_cold_before_the_call_sends_nothing() {
        let root = dir("send-turns-cold");
        let store = drafts(&root);
        let (c, fake) = send_hub_on(&store);
        let start = *lock(&fake.now);
        let mut details = ready();
        details["session_usage"] =
            json!({"prompt_cache": {"status": "warm", "expires_at": start + 30.0}});
        let view = opened(&c, details);
        let release = hold(&store);
        assert_eq!(
            c.send_message(
                view.handle,
                1,
                view.text_epoch,
                "warm when pressed".into(),
                0,
                0
            ),
            SendReply::Sending
        );
        // It expires before the "sending" save lands: no consent was given.
        *lock(&fake.now) = start + 60.0;
        release.send(()).unwrap();
        let view = until(&c, |v| !v.sending);
        assert_eq!(view.status, CHANGED);
        assert_eq!(view.text, "warm when pressed", "the draft is kept");
        std::thread::sleep(Duration::from_millis(50));
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_hub_closed_before_the_call_starts_nothing() {
        // Closed while the "sending" save waits: its answer spawns nothing.
        let root = dir("send-closed-save");
        let store = drafts(&root);
        let (spawn, jobs) = held_spawn();
        let (c, fake) = send_hub_with(&store, spawn);
        let view = opened(&c, ready());
        let release = hold(&store);
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "at exit".into(), 7, 7),
            SendReply::Sending
        );
        c.flush();
        release.send(()).unwrap();
        store.flush();
        assert!(lock(&jobs).is_empty(), "the save's answer starts nothing");
        assert!(sent(&fake).is_empty());
        // Closed after the save, before the call's thread runs: no call.
        let root = dir("send-closed-call");
        let store = drafts(&root);
        let (spawn, jobs) = held_spawn();
        let (c, fake) = send_hub_with(&store, spawn);
        let view = opened(&c, ready());
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "at exit".into(), 7, 7),
            SendReply::Sending
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while lock(&jobs).is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "the save never answered"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        c.flush();
        run_held(&jobs);
        assert!(sent(&fake).is_empty(), "nothing starts once the app quits");
        assert!(
            c.current().unwrap().local.is_empty(),
            "no row for a call never made"
        );
        store.flush();
        let saved = zerus_client_core::drafts::read_draft(&root.join("drafts"), KEY);
        assert!(saved.draft.uncertain && saved.draft.draft.text == "at exit");
    }

    #[test]
    fn a_send_clears_undo() {
        let root = dir("send-undo");
        let files: Vec<std::path::PathBuf> = ["a", "b"]
            .iter()
            .map(|name| {
                let file = root.join(format!("{name}.txt"));
                std::fs::write(&file, *name).unwrap();
                file
            })
            .collect();
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed {
            detail: "hgs: refused".into(),
        });
        let view = opened(&c, ready());
        c.attach_paths(
            view.handle,
            files,
            zerus_client_core::attachments::Selection::default(),
        );
        let view = until(&c, |v| v.attachments.len() == 2 && !v.attaching);
        c.remove(view.handle, 1);
        let view = c.current().unwrap();
        assert_eq!(view.removed.as_deref(), Some("b.txt"));
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, view.text.clone(), 0, 0),
            SendReply::Sending
        );
        let view = until(&c, |v| !v.sending && v.status == "hgs: refused");
        assert_eq!(
            view.removed, None,
            "Undo never brings back a file of a sent message"
        );
        c.undo_remove(view.handle);
        assert_eq!(c.current().unwrap().attachments.len(), 1);
    }

    #[test]
    fn a_failed_sending_save_sends_nothing_and_unlocks() {
        let root = dir("send-save-failed");
        let blocker = root.join("blocked");
        std::fs::write(&blocker, "a file where the store's folder belongs").unwrap();
        let store = Arc::new(DraftStore::open(blocker.join("drafts")));
        let (c, fake) = send_hub_on(&store);
        let view = opened(&c, ready());
        let before = view.text_epoch;
        assert_eq!(
            c.send_message(view.handle, 1, view.text_epoch, "unsaved".into(), 7, 7),
            SendReply::Sending
        );
        let view = until(&c, |v| !v.sending);
        assert!(
            view.status.starts_with("Draft not saved") && view.status_error,
            "{view:?}"
        );
        assert_eq!(view.text, "unsaved");
        assert!(view.text_epoch > before, "the editor unlocks");
        std::thread::sleep(Duration::from_millis(50));
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn a_new_question_replaces_its_composers_older_one() {
        let root = dir("send-question-again");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details);
        let SendReply::Cold { token: first, .. } =
            c.send_message(view.handle, 1, view.text_epoch, "asked".into(), 5, 5)
        else {
            panic!("no question")
        };
        let SendReply::Cold { token: second, .. } =
            c.send_message(view.handle, 1, view.text_epoch, "asked".into(), 5, 5)
        else {
            panic!("no question")
        };
        assert_eq!(c.lock().prompts.len(), 1, "one open question per composer");
        assert_eq!(
            c.answer(
                first,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Busy
        );
        assert_eq!(
            c.answer(
                second,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Sending
        );
        until(&c, |v| !v.sending);
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn a_question_whose_draft_changed_sends_nothing() {
        let root = dir("send-question-changed");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["cache_hint"] = json!({"status": "cold", "tokens": 120000});
        let view = opened(&c, details);
        let SendReply::Cold { token, .. } =
            c.send_message(view.handle, 1, view.text_epoch, "asked".into(), 5, 5)
        else {
            panic!("no question")
        };
        c.edit(
            view.handle,
            2,
            view.text_epoch,
            "asked, then changed".into(),
            19,
            19,
        );
        assert_eq!(
            c.answer(
                token,
                Answer::Cold {
                    action: ColdAction::Send
                }
            ),
            SendReply::Notice {
                text: "Draft changed. Send it when you are ready.".into(),
                error: false,
            }
        );
        std::thread::sleep(Duration::from_millis(50));
        assert!(sent(&fake).is_empty());
    }

    #[test]
    fn inspected_events_reconcile_rows_too() {
        let root = dir("send-inspected");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        // Seen by the inspect that the send's end starts (3.5 s on).
        details["events"] = json!([{"seq": 3, "type": "UserPromptSubmit", "detail": "seen by inspect", "at": 1_793_000_103.5}]);
        *lock(&fake.inspected) = Some(details);
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 3.5;
        c.tick();
        let view = until(&c, |v| v.block_reason.is_none());
        assert!(
            c.lock()
                .followed
                .details
                .as_ref()
                .unwrap()
                .get("events")
                .is_none(),
            "the details keep no events"
        );
        c.send_message(
            view.handle,
            1,
            view.text_epoch,
            "seen by inspect".into(),
            0,
            0,
        );
        until(&c, |v| !v.sending && v.local.is_empty());
        assert_eq!(sent(&fake).len(), 1);
    }

    #[test]
    fn the_followed_events_are_bounded() {
        let root = dir("send-events");
        let (c, _) = send_hub(&root);
        opened(&c, ready());
        let events = |kind: &str, seqs: std::ops::Range<i64>| Batch {
            activity: vec![ActivityOut {
                computer: ComputerId::from("studio"),
                key: "run:run-1".into(),
                kind: kind.into(),
                payload: json!({"events": seqs.map(|seq| json!({"seq": seq})).collect::<Vec<_>>()}),
            }],
            ..Batch::default()
        };
        c.observe(&events("events", 0..300));
        c.observe(&events("events", 300..600));
        let seqs = || {
            c.lock()
                .followed
                .events
                .iter()
                .map(|e| e["seq"].as_i64().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(seqs(), (100..600).collect::<Vec<_>>(), "Qt keeps 500");
        c.observe(&events("replace", 7..9));
        assert_eq!(seqs(), vec![7, 8]);
    }

    #[test]
    fn a_session_that_leaves_the_catalog_drops_its_rows() {
        let root = dir("send-forget");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed {
            detail: "hgs: refused".into(),
        });
        let view = opened(&c, ready());
        c.send_message(view.handle, 1, view.text_epoch, "failed".into(), 6, 6);
        until(&c, |v| !v.sending && v.local.len() == 1);
        let removed = |reason: &str| Batch {
            removes: vec![zerus_client_core::KeyOut {
                computer: ComputerId::from("studio"),
                key: "run:run-1".into(),
                reason: Some(reason.into()),
            }],
            ..Batch::default()
        };
        c.observe(&removed("replaced"));
        assert_eq!(
            c.current().unwrap().local.len(),
            1,
            "a replaced key is the same session"
        );
        c.observe(&removed("archived"));
        assert!(c.current().unwrap().local.is_empty());
    }

    #[test]
    fn a_definite_failure_offers_retry_inspect_and_delete() {
        let root = dir("send-row-actions");
        let (c, fake) = send_hub(&root);
        *lock(&fake.outcome) = Box::new(|_, _| Outcome::Failed {
            detail: "hgs: refused".into(),
        });
        let mut details = ready();
        details["terminal_supported"] = json!(true);
        let view = opened(&c, details);
        c.send_message(view.handle, 1, view.text_epoch, "failed".into(), 6, 6);
        let view = until(&c, |v| !v.sending && v.local.len() == 1);
        let id = view.local[0].id.clone();
        assert!(!view.local[0].uncertain, "a definite failure");
        assert_eq!(c.inspect_local(view.handle, &id), Some(Inspect::Terminal));
        assert_eq!(c.inspect_local(view.handle, "not-a-row"), None);
        // Retry: a new submission whose row replaces this one.
        assert_eq!(c.retry(view.handle, &id), SendReply::Sending);
        let view = until(&c, |v| {
            !v.sending && v.local.len() == 1 && v.local[0].id != id
        });
        let again = view.local[0].id.clone();
        assert_eq!(sent(&fake).len(), 2);
        c.delete_local(view.handle + 1, &again);
        assert_eq!(
            c.current().unwrap().local.len(),
            1,
            "another composer's handle deletes nothing"
        );
        c.delete_local(view.handle, &again);
        assert!(c.current().unwrap().local.is_empty());
        assert_eq!(sent(&fake).len(), 2);
    }
}
