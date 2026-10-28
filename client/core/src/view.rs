//! Compact view rows projected from `hgs ls --json --local` session rows.
//!
//! The presentation follows the Qt client (`tray/src/SessionPresentation.h`,
//! `SessionsWindowRows.cpp`) for everything that does not depend on the clock:
//! a countdown, an elapsed time or a scheduled run's time of day are left to the
//! view, which gets the timestamps it needs. Unknown fields are ignored and
//! missing ones default, so a newer or older `hgs` never fails a row.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The longest `summary`, in characters; longer text ends with an ellipsis.
pub const SUMMARY_CHARS: usize = 240;

/// One session card.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewRow {
    /// The session's full name, its identity in `hgs` commands.
    pub name: String,
    /// The card title: the tag, else the name after its project, else the project.
    pub title: String,
    /// `claude`, `codex`, `kimi`, `deepseek`, `opencode`, `sh` or what `hgs` reports.
    pub agent: String,
    /// The presentation status: `attention`, `error`, `working`, `background`,
    /// `waiting`, `scheduled`, `ready`, `paused`, `starting` or `ended`. A computer
    /// that is not live shows its rows as offline; the view applies that.
    pub status: String,
    /// The status caption the Qt client shows, without clock-dependent parts.
    pub caption: String,
    /// The raw phase (`idle`, `working`, `tool`, `approval`, `input`, `error`,
    /// `interrupted`, `compacting`, `starting`, ...), None when unknown.
    pub phase: Option<String>,
    /// Why the session needs attention: `approval` or `question`; None when it
    /// does not, or for other reasons (an error that needs action, a subagent).
    pub attention: Option<String>,
    /// A reply the person has not read (shared read marks of this computer).
    pub unread: bool,
    pub muted: bool,
    /// The computer as the client names it: the peer's alias, or this
    /// computer's host name. Filled by the catalog state.
    pub machine: String,
    pub project: String,
    /// The folder people know the checkout by (a linked worktree names its
    /// main checkout).
    pub folder: String,
    /// The branch, `detached HEAD`, or None outside Git.
    pub branch: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The current action (line 3 of the card), empty when the status says it all.
    pub summary: String,
    /// The last hook event, seconds since the Unix epoch.
    pub updated_at: Option<f64>,
    /// When the current turn, compaction or background job started, for the
    /// Working clock.
    pub turn_started: Option<f64>,
    pub run_id: Option<String>,
    pub conversation_id: Option<String>,
    /// The raw `state` (`running`, `paused`, `stopped`, `archived`, ...).
    pub state: String,
    pub tracked: bool,
    pub resumable: bool,
    pub activity: String,
    pub process_state: String,
    pub archive_id: Option<String>,
    /// The latest reply and the attention it raised, for marking them read.
    pub reply_id: Option<String>,
    pub attention_signature: Option<String>,
    /// The row has the key `attention_signature`: its computer shares read marks.
    pub shared_read: bool,
    /// The read mark of this conversation, None when it belongs to another one.
    pub read_reply_id: Option<String>,
    pub read_attention_signature: Option<String>,
    /// The operations the computer reports as allowed, None when it reports none.
    pub allowed_actions: Option<Vec<String>>,
    pub action_reasons: BTreeMap<String, String>,
}

fn text<'a>(row: &'a Value, field: &str) -> &'a str {
    row.get(field).and_then(Value::as_str).unwrap_or("")
}

fn object<'a>(row: &'a Value, field: &str) -> Option<&'a Map<String, Value>> {
    row.get(field).and_then(Value::as_object)
}

fn number(row: &Value, field: &str) -> Option<f64> {
    row.get(field).and_then(Value::as_f64)
}

fn count(row: &Value, field: &str, inner: &str) -> i64 {
    object(row, field)
        .and_then(|value| value.get(inner))
        .and_then(Value::as_i64)
        .unwrap_or(-1)
}

fn some(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

/// `name.section('/', from)`: the segments from `from` on, joined by `/`.
fn section(name: &str, from: usize) -> &str {
    let mut start = 0;
    for _ in 0..from {
        match name[start..].find('/') {
            Some(at) => start += at + 1,
            None => return "",
        }
    }
    &name[start..]
}

fn segment(name: &str, index: usize) -> &str {
    name.split('/').nth(index).unwrap_or("")
}

fn path_parts(path: &str) -> Vec<&str> {
    path.split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect()
}

/// The fields of one row the presentation reads.
struct Session<'a> {
    row: &'a Value,
    name: &'a str,
    state: &'a str,
    phase: &'a str,
    activity: &'a str,
    process_state: &'a str,
    conversation_state: &'a str,
    run_id: &'a str,
    conversation_id: &'a str,
    recovery: &'a str,
    tracked: bool,
    background: i64,
    scheduled: i64,
    subagents_active: i64,
}

impl<'a> Session<'a> {
    fn new(row: &'a Value) -> Self {
        let recovery = object(row, "recovery")
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str)
            .unwrap_or("");
        Self {
            row,
            name: text(row, "name"),
            state: row
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("running"),
            phase: text(row, "phase"),
            activity: text(row, "activity"),
            process_state: text(row, "process_state"),
            conversation_state: text(row, "conversation_state"),
            run_id: text(row, "run_id"),
            conversation_id: text(row, "conversation_id"),
            recovery,
            tracked: row.get("tracked").and_then(Value::as_bool).unwrap_or(false),
            background: count(row, "process_work", "count"),
            scheduled: count(row, "schedules", "count"),
            subagents_active: row
                .get("subagent_active_count")
                .and_then(Value::as_i64)
                .or_else(|| row.get("subagent_count").and_then(Value::as_i64))
                .unwrap_or(0),
        }
    }

    fn running(&self) -> bool {
        self.state == "running" && self.process_state != "exited"
    }

    fn error_kind(&self) -> &str {
        object(self.row, "provider_error")
            .and_then(|value| value.get("error_kind"))
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    fn has_provider_error(&self) -> bool {
        object(self.row, "provider_error").is_some_and(|value| !value.is_empty())
    }

    fn needs_action(&self) -> bool {
        let recovering = matches!(self.recovery, "waiting" | "dispatching" | "retrying");
        let kind = self.error_kind();
        self.running()
            && (self.phase == "approval"
                || self.phase == "input"
                || (self.phase == "error"
                    && (!recovering || kind == "quota" || kind == "provider_policy"))
                || matches!(self.recovery, "uncertain" | "blocked" | "exhausted"))
    }

    fn coordination_wait(&self) -> Option<&'a Map<String, Value>> {
        let wait = object(self.row, "coordination_wait")?;
        let count = wait.get("count").and_then(Value::as_f64).unwrap_or(0.0);
        let valid = wait.get("schema").and_then(Value::as_i64) == Some(1)
            && wait.get("status").and_then(Value::as_str) == Some("ok")
            && !self.run_id.is_empty()
            && !self.conversation_id.is_empty()
            && wait.get("run_id").and_then(Value::as_str) == Some(self.run_id)
            && wait.get("conversation_id").and_then(Value::as_str) == Some(self.conversation_id)
            && count > 0.0
            && count <= 1024.0
            && count.fract() == 0.0;
        valid.then_some(wait)
    }

    fn idle(&self) -> bool {
        self.activity == "idle" && self.phase == "idle"
    }

    fn background_idle(&self) -> bool {
        self.idle() && self.background > 0
    }

    fn scheduled_idle(&self) -> bool {
        self.idle() && self.scheduled > 0
    }

    fn waiting_idle(&self) -> bool {
        self.running()
            && self.idle()
            && self.background <= 0
            && self.subagents_active <= 0
            && self.coordination_wait().is_some()
    }

    fn current_activity(&self) -> bool {
        self.running()
            && self.activity != "unknown"
            && !(self.conversation_state == "ended" && self.activity != "idle")
    }

    fn reply_in_progress(&self) -> bool {
        self.current_activity()
            && (self.activity == "busy"
                || self.background > 0
                || self.subagents_active > 0
                || (self.idle() && self.coordination_wait().is_some()))
    }

    /// The read mark of this conversation, when the computer shares read state.
    fn read_mark(&self, field: &str) -> &'a str {
        let Some(read) = object(self.row, "read") else {
            return "";
        };
        if self.conversation_id.is_empty()
            || read.get("conversation_id").and_then(Value::as_str) != Some(self.conversation_id)
        {
            return "";
        }
        read.get(field).and_then(Value::as_str).unwrap_or("")
    }

    /// A computer whose `hgs` reports `attention_signature` shares read marks;
    /// an older one keeps them per device, which this client does not have yet.
    fn shared_read(&self) -> bool {
        self.row.get("attention_signature").is_some()
    }

    fn attention_acknowledged(&self) -> bool {
        let signature = text(self.row, "attention_signature");
        self.shared_read()
            && !signature.is_empty()
            && signature == self.read_mark("attention_signature")
    }

    fn unread_reply(&self) -> bool {
        let reply = text(self.row, "reply_id");
        self.shared_read()
            && self.state != "archived"
            && !self.conversation_id.is_empty()
            && !reply.is_empty()
            && self.read_mark("reply_id") != reply
    }

    fn child_needs_input(&self) -> bool {
        if text(self.row, "subagent_source") == "hook_profiles" {
            return false;
        }
        let asks = |child: &Value| {
            let state = child
                .get("display_state")
                .and_then(Value::as_str)
                .filter(|state| !state.is_empty())
                .or_else(|| child.get("state").and_then(Value::as_str))
                .unwrap_or("");
            matches!(state, "approval" | "input" | "attention")
        };
        let children = object(self.row, "subagents").is_some_and(|map| map.values().any(asks));
        let previews = self
            .row
            .get("subagent_previews")
            .and_then(Value::as_array)
            .is_some_and(|list| list.iter().any(asks));
        children || previews
    }

    fn provider_failure(&self) -> &'static str {
        match self.error_kind() {
            "session_limit" => "Session limit reached",
            "quota" => "Usage limit reached",
            "rate_limit" => "Rate limit reached",
            "capacity" => "Model at capacity",
            "authentication" => "Sign-in failed",
            "provider_policy" => "Request blocked by provider",
            "context_limit" => "Context limit reached",
            "model_unavailable" => "Model unavailable",
            _ => "Provider error",
        }
    }

    fn waiting_caption(&self) -> &'static str {
        let awaiting_result = self
            .coordination_wait()
            .and_then(|wait| wait.get("items"))
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("state"))
            .and_then(Value::as_str)
            == Some("awaiting_result");
        if awaiting_result {
            "Waiting for result"
        } else {
            "Waiting for acceptance"
        }
    }

    /// The Qt caption (`SessionPresentation::status`) and the status key.
    fn status(&self) -> (&'static str, String) {
        let caption = |key: &'static str, text: &str| (key, text.to_owned());
        if self.state == "archived" {
            return caption("ended", "Archived");
        }
        if self.state == "paused" {
            return caption("paused", "Paused");
        }
        if self.state == "stopped" {
            return caption("paused", "Stopped");
        }
        if self.process_state == "exited" {
            return caption("ended", "Agent exited");
        }
        if !self.tracked && self.activity.is_empty() {
            return caption("ready", "Not tracked");
        }
        let acknowledged = self.attention_acknowledged();
        let attention = |text: &str| {
            if acknowledged {
                ("ready", text.to_owned())
            } else {
                ("attention", text.to_owned())
            }
        };
        if self.phase == "approval" {
            return attention("Needs approval");
        }
        if self.phase == "input" {
            let sign_in = text(self.row, "cmd") == "dsh"
                && text(self.row, "activity_summary") == "Sign in required";
            return attention(if sign_in { "Sign in" } else { "Needs input" });
        }
        let kind = self.error_kind();
        if self.phase == "error" && (kind == "quota" || kind == "provider_policy") {
            return caption("error", self.provider_failure());
        }
        match self.recovery {
            "waiting" => return caption("error", "Waiting to retry"),
            "dispatching" => return caption("error", "Retrying"),
            "uncertain" | "blocked" | "exhausted" => return attention("Needs attention"),
            _ => {}
        }
        if self.phase == "error" {
            let text = if self.has_provider_error() {
                self.provider_failure()
            } else {
                "Error"
            };
            return caption("error", text);
        }
        if self.phase == "interrupted" {
            return caption("error", "Interrupted");
        }
        if !acknowledged && self.current_activity() && self.child_needs_input() {
            return caption("attention", "Needs input");
        }
        match self.phase {
            "compacting" => return caption("working", "Compacting"),
            "starting" => return caption("starting", "Starting"),
            "tool" => return caption("working", "Using a tool"),
            _ => {}
        }
        if self.activity == "busy" {
            return caption("working", "Working");
        }
        if self.background_idle() {
            return caption("background", "Background");
        }
        if self.waiting_idle() {
            return caption("waiting", self.waiting_caption());
        }
        if self.scheduled_idle() {
            return caption("scheduled", "Scheduled");
        }
        if self.activity == "idle" {
            return caption("ready", "Ready");
        }
        if self.activity == "ended" || self.activity == "unknown" {
            return caption("ready", "Status unknown");
        }
        caption("starting", "Connecting")
    }

    /// Line 3 of the card (`SessionPresentation::currentAction`), empty where
    /// Qt repeats the status alone.
    fn summary(&self) -> String {
        let detail = |main: &str, extra: &str| {
            if extra.is_empty() {
                main.to_owned()
            } else if main.is_empty() {
                extra.to_owned()
            } else {
                format!("{main}: {extra}")
            }
        };
        let activity_detail = text(self.row, "activity_detail");
        let tool_detail = text(self.row, "tool_detail");
        let prompt = text(self.row, "prompt");
        if self.needs_action() {
            return [activity_detail, tool_detail, prompt]
                .into_iter()
                .find(|value| !value.is_empty())
                .unwrap_or("")
                .to_owned();
        }
        if self.unread_reply() && !self.reply_in_progress() {
            return String::new();
        }
        if !self.current_activity() {
            return String::new();
        }
        if self.background_idle() {
            let jobs = if self.background == 1 {
                "1 running command".to_owned()
            } else {
                format!("{} running commands", self.background)
            };
            let command = object(self.row, "process_work")
                .and_then(|work| work.get("command"))
                .and_then(Value::as_str)
                .unwrap_or("");
            return detail(&jobs, command);
        }
        if self.waiting_idle() {
            return self.waiting_detail();
        }
        if self.scheduled_idle() {
            return "Waiting for its next scheduled run".to_owned();
        }
        let summary = text(self.row, "activity_summary");
        if !summary.is_empty() {
            return detail(summary, activity_detail);
        }
        let tool = text(self.row, "current_tool");
        if !tool.is_empty() {
            return detail(tool, tool_detail);
        }
        prompt.to_owned()
    }

    fn waiting_detail(&self) -> String {
        let Some(wait) = self.coordination_wait() else {
            return String::new();
        };
        let item = wait
            .get("items")
            .and_then(Value::as_array)
            .and_then(|items| items.first());
        let field = |outer: &str, inner: &str| {
            item.and_then(|item| item.get(outer))
                .and_then(|value| value.get(inner))
                .and_then(Value::as_str)
                .unwrap_or("")
        };
        let mut result = self.waiting_caption().to_owned();
        let producer = section(field("producer", "name"), 2);
        if !producer.is_empty() {
            result = format!("{result} from {producer}");
        }
        let host = field("producer", "host");
        if !host.is_empty() {
            result = format!("{result} on {host}");
        }
        let label = item
            .and_then(|item| item.get("label"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !label.is_empty() {
            result = format!("{result}: {label}");
        }
        let count = wait.get("count").and_then(Value::as_i64).unwrap_or(0);
        if count > 1 {
            result = format!("{result} (+{})", count - 1);
        }
        result
    }

    fn folder(&self) -> String {
        if self.row.get("git_worktree").and_then(Value::as_bool) == Some(true) {
            let mut parts = path_parts(text(self.row, "git_common_dir"));
            if parts.len() > 1 && parts.last().is_some_and(|last| last.starts_with('.')) {
                parts.pop();
            }
            let mut name = parts.last().copied().unwrap_or("");
            if name.len() > 4 && name.ends_with(".git") {
                name = &name[..name.len() - 4];
            }
            if !name.is_empty() {
                return name.to_owned();
            }
        }
        let path = [
            text(self.row, "git_root"),
            text(self.row, "cwd"),
            text(self.row, "cwd_canonical"),
        ]
        .into_iter()
        .find(|path| !path.is_empty())
        .unwrap_or("");
        path_parts(path).last().copied().unwrap_or("").to_owned()
    }

    fn branch(&self) -> Option<String> {
        let branch = text(self.row, "git_branch");
        if !branch.is_empty() {
            return Some(branch.to_owned());
        }
        (self.row.get("git_detached").and_then(Value::as_bool) == Some(true))
            .then(|| "detached HEAD".to_owned())
    }

    fn project(&self) -> String {
        let project = text(self.row, "project");
        if project.is_empty() {
            segment(self.name, 1).to_owned()
        } else {
            project.to_owned()
        }
    }

    fn title(&self) -> String {
        let tag = text(self.row, "tag");
        let tag = if tag.is_empty() {
            section(self.name, 2)
        } else {
            tag
        };
        if !tag.is_empty() {
            return tag.to_owned();
        }
        let project = text(self.row, "project");
        if project.is_empty() {
            self.name.to_owned()
        } else {
            project.to_owned()
        }
    }

    fn muted(&self) -> bool {
        object(self.row, "notification_preferences").is_some_and(|preferences| {
            !self.conversation_id.is_empty()
                && preferences.get("conversation_id").and_then(Value::as_str)
                    == Some(self.conversation_id)
                && preferences.get("muted").and_then(Value::as_bool) == Some(true)
        })
    }

    fn turn_started(&self) -> Option<f64> {
        let started = if self.background_idle() {
            object(self.row, "process_work")
                .and_then(|work| work.get("started_at"))
                .and_then(Value::as_f64)
        } else if self.phase == "compacting" {
            number(self.row, "compaction_started")
        } else {
            number(self.row, "turn_started")
        };
        started.filter(|at| *at > 0.0)
    }
}

/// Collapses whitespace like `QString::simplified` and bounds the length.
fn simplified(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(SUMMARY_CHARS * 4));
    let mut chars = 0;
    for word in text.split_whitespace() {
        if chars > 0 {
            out.push(' ');
            chars += 1;
        }
        for character in word.chars() {
            if chars >= SUMMARY_CHARS {
                out.push('\u{2026}');
                return out;
            }
            out.push(character);
            chars += 1;
        }
    }
    out
}

/// The view row of one catalog row.
pub fn row(value: &Value) -> ViewRow {
    let session = Session::new(value);
    let (status, caption) = session.status();
    let attention = (status == "attention")
        .then(|| match session.phase {
            "approval" => Some("approval".to_owned()),
            "input" => Some("question".to_owned()),
            _ => None,
        })
        .flatten();
    let agent = match text(value, "agent") {
        "" => text(value, "cmd"),
        agent => agent,
    };
    ViewRow {
        name: session.name.to_owned(),
        title: session.title(),
        agent: if agent == "dsh" { "deepseek" } else { agent }.to_owned(),
        status: status.to_owned(),
        caption,
        phase: some(session.phase),
        attention,
        unread: session.unread_reply() && !session.reply_in_progress(),
        muted: session.muted(),
        machine: String::new(),
        project: session.project(),
        folder: session.folder(),
        branch: session.branch(),
        model: some(text(value, "model")),
        effort: some(text(value, "effort")),
        summary: simplified(&session.summary()),
        updated_at: number(value, "last_event_at").filter(|at| *at > 0.0),
        turn_started: session.turn_started(),
        run_id: some(session.run_id),
        conversation_id: some(session.conversation_id),
        state: text(value, "state").to_owned(),
        tracked: session.tracked,
        resumable: value
            .get("resumable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        activity: session.activity.to_owned(),
        process_state: session.process_state.to_owned(),
        archive_id: some(text(value, "archive_id")),
        reply_id: some(text(value, "reply_id")),
        attention_signature: some(text(value, "attention_signature")),
        shared_read: session.shared_read(),
        read_reply_id: some(session.read_mark("reply_id")),
        read_attention_signature: some(session.read_mark("attention_signature")),
        allowed_actions: value
            .get("allowed_actions")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            }),
        action_reasons: object(value, "action_reasons")
            .map(|map| {
                map.iter()
                    .filter_map(|(key, reason)| Some((key.clone(), reason.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_busy_tool_row_maps_the_card_fields() {
        let view = row(&json!({
            "name": "claude/sample-project/review", "agent": "claude", "project": "sample-project",
            "tag": "review", "run_id": "run-1", "conversation_id": "conv-1", "tracked": true,
            "phase": "tool", "activity": "busy", "process_state": "running",
            "current_tool": "Bash", "tool_detail": "cargo test", "activity_summary": "Running tests",
            "activity_detail": "cargo   test\n--workspace", "git_root": "/home/example/src/sample-project",
            "git_branch": "main", "model": "example-model-large", "effort": "high",
            "last_event_at": 1787840100.5, "turn_started": 1787840000.0, "later_field": [1, 2]
        }));
        assert_eq!(view.name, "claude/sample-project/review");
        assert_eq!(view.title, "review");
        assert_eq!(view.agent, "claude");
        assert_eq!(view.status, "working");
        assert_eq!(view.caption, "Using a tool");
        assert_eq!(view.phase.as_deref(), Some("tool"));
        assert_eq!(view.attention, None);
        assert_eq!(view.project, "sample-project");
        assert_eq!(view.folder, "sample-project");
        assert_eq!(view.branch.as_deref(), Some("main"));
        assert_eq!(view.model.as_deref(), Some("example-model-large"));
        assert_eq!(view.effort.as_deref(), Some("high"));
        assert_eq!(view.summary, "Running tests: cargo test --workspace");
        assert_eq!(view.updated_at, Some(1787840100.5));
        assert_eq!(view.turn_started, Some(1787840000.0));
        assert_eq!(view.run_id.as_deref(), Some("run-1"));
        assert_eq!(view.conversation_id.as_deref(), Some("conv-1"));
        assert!(!view.unread);
        assert_eq!(view.machine, "");
    }

    #[test]
    fn missing_fields_default_and_wrong_types_are_ignored() {
        let view =
            row(&json!({"name": "sh/sample-project", "phase": 7, "model": null, "read": "x"}));
        // Like Qt's sessionLabel: no tag and no project field leave the name.
        assert_eq!(view.title, "sh/sample-project");
        assert_eq!(view.project, "sample-project");
        assert_eq!(view.status, "ready");
        assert_eq!(view.caption, "Not tracked");
        assert_eq!(view.phase, None);
        assert_eq!(view.model, None);
        assert_eq!(view.run_id, None);
        assert_eq!(view.summary, "");
        let empty = row(&json!({}));
        assert_eq!(
            empty,
            ViewRow {
                status: "ready".into(),
                caption: "Not tracked".into(),
                ..ViewRow::default()
            }
        );
        let not_object = row(&json!([1, 2]));
        assert_eq!(not_object.status, "ready");
    }

    #[test]
    fn attention_follows_the_phase_and_the_shared_read_mark() {
        let approval = row(
            &json!({"name": "kimi/docs-site/outline", "run_id": "r", "conversation_id": "c",
            "tracked": true, "phase": "approval", "activity": "busy", "activity_detail": "rm -rf scratch",
            "attention_signature": "sig-1", "read": {"conversation_id": "c", "attention_signature": "sig-0"}}),
        );
        assert_eq!(approval.status, "attention");
        assert_eq!(approval.attention.as_deref(), Some("approval"));
        assert_eq!(approval.caption, "Needs approval");
        assert_eq!(approval.summary, "rm -rf scratch");
        let question = row(
            &json!({"name": "claude/docs-site/draft", "tracked": true, "phase": "input",
            "activity": "busy", "process_state": "running"}),
        );
        assert_eq!(question.attention.as_deref(), Some("question"));
        let acknowledged = row(
            &json!({"name": "claude/docs-site/draft", "run_id": "r", "conversation_id": "c",
            "tracked": true, "phase": "input", "activity": "busy",
            "attention_signature": "sig-1", "read": {"conversation_id": "c", "attention_signature": "sig-1"}}),
        );
        assert_eq!(acknowledged.status, "ready");
        assert_eq!(acknowledged.attention, None);
        let child = row(
            &json!({"name": "codex/p/t", "tracked": true, "phase": "idle", "activity": "idle",
            "subagents": {"a": {"display_state": "approval"}}}),
        );
        assert_eq!(child.status, "attention");
        assert_eq!(child.attention, None);
    }

    #[test]
    fn unread_needs_a_reply_newer_than_the_read_mark_of_this_conversation() {
        let base = json!({"name": "codex/sample-project/tests", "run_id": "r", "conversation_id": "c",
            "tracked": true, "phase": "idle", "activity": "idle", "reply_id": "reply-7",
            "attention_signature": null, "read": {"conversation_id": "c", "reply_id": "reply-6"}});
        assert!(row(&base).unread);
        let mut read = base.clone();
        read["read"]["reply_id"] = json!("reply-7");
        assert!(!row(&read).unread);
        let mut other = base.clone();
        other["read"]["conversation_id"] = json!("older");
        other["read"]["reply_id"] = json!("reply-7");
        assert!(
            row(&other).unread,
            "a mark of another conversation does not count"
        );
        let mut busy = base.clone();
        busy["activity"] = json!("busy");
        assert!(
            !row(&busy).unread,
            "a reply in progress is not shown as unread yet"
        );
        let mut legacy = base;
        legacy
            .as_object_mut()
            .unwrap()
            .remove("attention_signature");
        assert!(
            !row(&legacy).unread,
            "per-device read state is not known here"
        );
    }

    #[test]
    fn statuses_follow_the_qt_precedence() {
        let status = |value: Value| {
            let view = row(&value);
            (view.status, view.caption)
        };
        let s = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(status(json!({"state": "archived"})), s("ended", "Archived"));
        assert_eq!(status(json!({"state": "paused"})), s("paused", "Paused"));
        assert_eq!(status(json!({"state": "stopped"})), s("paused", "Stopped"));
        assert_eq!(
            status(json!({"tracked": true, "process_state": "exited"})),
            s("ended", "Agent exited")
        );
        assert_eq!(
            status(json!({"tracked": true, "phase": "error", "activity": "idle"})),
            s("error", "Error")
        );
        assert_eq!(
            status(
                json!({"tracked": true, "phase": "error", "provider_error": {"error_kind": "quota"}})
            ),
            s("error", "Usage limit reached")
        );
        assert_eq!(
            status(json!({"tracked": true, "recovery": {"state": "waiting"}})),
            s("error", "Waiting to retry")
        );
        assert_eq!(
            status(json!({"tracked": true, "recovery": {"state": "blocked"}})),
            s("attention", "Needs attention")
        );
        assert_eq!(
            status(json!({"tracked": true, "phase": "interrupted"})),
            s("error", "Interrupted")
        );
        assert_eq!(
            status(json!({"tracked": true, "phase": "compacting", "activity": "busy"})),
            s("working", "Compacting")
        );
        assert_eq!(
            status(json!({"tracked": true, "phase": "starting", "activity": "busy"})),
            s("starting", "Starting")
        );
        assert_eq!(
            status(json!({"tracked": true, "phase": "working", "activity": "busy"})),
            s("working", "Working")
        );
        assert_eq!(
            status(
                json!({"tracked": true, "phase": "idle", "activity": "idle", "process_work": {"count": 2}})
            ),
            s("background", "Background")
        );
        assert_eq!(
            status(
                json!({"tracked": true, "phase": "idle", "activity": "idle", "schedules": {"count": 1}})
            ),
            s("scheduled", "Scheduled")
        );
        assert_eq!(
            status(
                json!({"tracked": true, "run_id": "r", "conversation_id": "c", "phase": "idle", "activity": "idle",
                "coordination_wait": {"schema": 1, "status": "ok", "run_id": "r", "conversation_id": "c", "count": 1,
                    "items": [{"state": "awaiting_result"}]}})
            ),
            s("waiting", "Waiting for result")
        );
        assert_eq!(
            status(json!({"tracked": true, "phase": "idle", "activity": "idle"})),
            s("ready", "Ready")
        );
        assert_eq!(
            status(json!({"tracked": true, "activity": "unknown"})),
            s("ready", "Status unknown")
        );
        assert_eq!(
            status(json!({"tracked": true, "activity": "launching"})),
            s("starting", "Connecting")
        );
    }

    #[test]
    fn background_summary_counts_commands() {
        let view = row(
            &json!({"tracked": true, "phase": "idle", "activity": "idle",
            "process_work": {"count": 2, "command": "npm run dev", "started_at": 1787840000.0}}),
        );
        assert_eq!(view.summary, "2 running commands: npm run dev");
        assert_eq!(view.turn_started, Some(1787840000.0));
    }

    #[test]
    fn worktrees_name_their_main_checkout_and_detached_heads_say_so() {
        let worktree = row(
            &json!({"git_worktree": true, "git_common_dir": "/home/example/src/sample-project/.git",
            "git_root": "/home/example/src/sample-project-tests"}),
        );
        assert_eq!(worktree.folder, "sample-project");
        let bare =
            row(&json!({"git_worktree": true, "git_common_dir": "/home/example/src/sample.git"}));
        assert_eq!(bare.folder, "sample");
        let detached = row(&json!({"cwd": "C:\\Users\\example\\docs-site", "git_detached": true}));
        assert_eq!(detached.folder, "docs-site");
        assert_eq!(detached.branch.as_deref(), Some("detached HEAD"));
    }

    #[test]
    fn deepseek_rows_name_their_agent_and_muting_needs_this_conversation() {
        let view = row(&json!({"cmd": "dsh", "conversation_id": "c",
            "notification_preferences": {"conversation_id": "c", "muted": true}}));
        assert_eq!(view.agent, "deepseek");
        assert!(view.muted);
        let other = row(&json!({"cmd": "codex", "conversation_id": "c",
            "notification_preferences": {"conversation_id": "d", "muted": true}}));
        assert!(!other.muted);
    }

    #[test]
    fn summaries_are_bounded_and_simplified() {
        let long = "word ".repeat(200);
        let view = row(
            &json!({"tracked": true, "phase": "working", "activity": "busy", "activity_summary": long}),
        );
        assert_eq!(view.summary.chars().count(), SUMMARY_CHARS + 1);
        assert!(view.summary.ends_with('\u{2026}'));
        assert!(!view.summary.contains("  "));
    }

    #[test]
    fn rows_serialize_with_snake_case_fields() {
        let value =
            serde_json::to_value(row(&json!({"name": "a/b/c", "last_event_at": 2.5}))).unwrap();
        for field in [
            "name",
            "title",
            "agent",
            "status",
            "caption",
            "phase",
            "attention",
            "unread",
            "muted",
            "machine",
            "project",
            "folder",
            "branch",
            "model",
            "effort",
            "summary",
            "updated_at",
            "turn_started",
            "run_id",
            "conversation_id",
        ] {
            assert!(value.get(field).is_some(), "{field}");
        }
        assert_eq!(value["updated_at"], 2.5);
    }

    #[test]
    fn action_fields_follow_the_catalog_row() {
        let view = row(
            &json!({"name": "claude/p/t", "conversation_id": "conv-1", "state": "paused",
            "resumable": true, "archive_id": "a1", "allowed_actions": ["rename"],
            "action_reasons": {"pause": "already paused", "x": 3},
            "attention_signature": "sig", "reply_id": "rep-1",
            "read": {"conversation_id": "conv-1", "reply_id": "rep-0"}}),
        );
        assert_eq!(view.allowed_actions, Some(vec!["rename".to_owned()]));
        assert!(view.shared_read);
        assert_eq!(view.read_reply_id.as_deref(), Some("rep-0"));
        assert_eq!(view.read_attention_signature, None);
        assert_eq!(view.state, "paused");
        assert!(view.resumable);
        assert_eq!(view.archive_id.as_deref(), Some("a1"));
        assert_eq!(view.reply_id.as_deref(), Some("rep-1"));
        assert_eq!(view.action_reasons.len(), 1);
        let other = row(
            &json!({"conversation_id": "conv-1", "attention_signature": "sig",
            "read": {"conversation_id": "conv-2", "reply_id": "rep-0"}}),
        );
        assert_eq!(other.read_reply_id, None);
        assert_eq!(row(&json!({"name": "a/b/c"})).allowed_actions, None);
        assert!(!row(&json!({"name": "a/b/c"})).shared_read);
    }
}
