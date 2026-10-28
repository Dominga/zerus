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
    /// The preferences are valid and mute this conversation.
    pub muted: bool,
    /// The row's `notification_preferences` pass the Qt validation
    /// (`SessionNotifications::valid`).
    pub shared_notifications: bool,
    /// The revision of valid preferences.
    pub notification_revision: Option<i64>,
    /// `created`, truncated to whole seconds.
    pub created: i64,
    /// The compact JSON the attention marks compare (`attentionFingerprint`).
    pub attention_fingerprint: String,
    /// The session needs the person's action (approval, input, error, recovery).
    pub needs_action: bool,
    /// `status` and `caption` with the attention acknowledged.
    pub acknowledged_status: String,
    pub acknowledged_caption: String,
    /// A finished reply of a live conversation; the marks decide whether it is unread.
    pub unread_candidate: bool,
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
    /// When an archived row was archived (epoch seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<f64>,
    /// When the current turn, compaction or background job started, for the
    /// Working clock.
    pub turn_started: Option<f64>,
    pub run_id: Option<String>,
    pub conversation_id: Option<String>,
    /// The raw `state` (`running`, `paused`, `stopped`, `archived`, ...);
    /// `running` when the row lists none (a live session).
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
    /// The raw working directory (`folder` is the name people know it by).
    pub cwd: String,
    /// The row's `terminal_supported`, else true unless the agent is
    /// `dsh` or `opencode` (`HgsClient.cpp:217`).
    pub terminal_supported: bool,
    /// The row has a boolean `terminal_supported`.
    pub terminal_reported: bool,
    pub terminal_attach_supported: bool,
    /// The row has a boolean `terminal_attach_supported`.
    pub terminal_attach_reported: bool,
    /// The row's `bulk_lifecycle_supported`, else true unless the agent is
    /// `opencode` (`HgsClient.cpp:218`).
    #[serde(default)]
    pub bulk_lifecycle_supported: bool,
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
        self.status_with(self.attention_acknowledged())
    }

    /// `status` with the attention acknowledged or not.
    fn status_with(&self, acknowledged: bool) -> (&'static str, String) {
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

    /// The preferences if they pass `SessionNotifications::valid`.
    fn notifications(&self) -> Option<&'a Map<String, Value>> {
        object(self.row, "notification_preferences")
            .filter(|preferences| notifications::valid(preferences, self.conversation_id))
    }

    /// `attentionFingerprint` of the Qt client.
    fn attention_fingerprint(&self) -> String {
        let mut children = Map::new();
        if text(self.row, "subagent_source") != "hook_profiles" {
            for (id, child) in object(self.row, "subagents").into_iter().flatten() {
                let phase = child
                    .get("display_state")
                    .and_then(Value::as_str)
                    .or_else(|| child.get("state").and_then(Value::as_str))
                    .unwrap_or("");
                if !matches!(phase, "approval" | "input" | "attention") {
                    continue;
                }
                let request = child
                    .get("attention_id")
                    .and_then(Value::as_str)
                    .or_else(|| child.get("question_id").and_then(Value::as_str))
                    .unwrap_or("");
                let at = if request.is_empty() {
                    child.get("last_event_at").cloned().unwrap_or(Value::Null)
                } else {
                    Value::Null
                };
                children.insert(id.clone(), serde_json::json!([phase, request, at]));
            }
        }
        let needs = self.needs_action();
        let attention_id = text(self.row, "attention_id");
        let at = if needs && attention_id.is_empty() {
            serde_json::json!(number(self.row, "last_event_at").unwrap_or(0.0))
        } else {
            Value::Null
        };
        Value::Array(vec![
            self.run_id.into(),
            if needs { self.phase } else { "" }.into(),
            if needs { attention_id } else { "" }.into(),
            at,
            Value::Object(children),
        ])
        .to_string()
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

/// The Qt `SessionNotifications` validation of the host's preferences.
/// `SessionNotifications::valid`: complete, bounded shared notification
/// preferences of that conversation.
pub fn preferences_valid(value: &Map<String, Value>, conversation: &str) -> bool {
    notifications::valid(value, conversation)
}

/// `SessionNotifications::revision`: a whole, non-negative number.
pub fn preferences_revision(value: Option<&Value>) -> Option<i64> {
    notifications::revision(value)
}

mod notifications {
    use serde_json::{Map, Value};

    const BUDGET: usize = 384 * 1024;

    fn keys(object: &Map<String, Value>, expected: &[&str]) -> bool {
        object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
    }

    fn text(value: Option<&Value>, maximum: usize, nonempty: bool, line_feed: bool) -> bool {
        let Some(string) = value.and_then(Value::as_str) else {
            return false;
        };
        !((nonempty && string.is_empty())
            || string.len() > maximum
            || string
                .chars()
                .any(|c| (c < ' ' && !(line_feed && c == '\n')) || c == '\u{7f}'))
    }

    fn identity(value: Option<&Value>, maximum: usize) -> bool {
        text(value, maximum, true, false)
    }

    pub fn revision(value: Option<&Value>) -> Option<i64> {
        let number = value?.as_f64()?;
        (number >= 0.0 && number.fract() == 0.0 && number < 9.2e18).then_some(number as i64)
    }

    fn started(value: &Value) -> bool {
        value.is_null() || value.as_f64().is_some_and(|at| at.is_finite() && at > 0.0)
    }

    fn attention(value: &Value, main: bool) -> bool {
        let Some(value) = value.as_object() else {
            return false;
        };
        let expected: &[&str] = if main {
            &["run_id", "phase", "request_id", "started_at"]
        } else {
            &["phase", "request_id", "started_at"]
        };
        if !keys(value, expected) || (main && !identity(value.get("run_id"), 128)) {
            return false;
        }
        if !text(value.get("phase"), 256, false, false)
            || !text(value.get("request_id"), 65536, false, true)
            || !started(&value["started_at"])
        {
            return false;
        }
        if value["request_id"] == "" && value["started_at"].is_null() {
            return false;
        }
        main || matches!(
            value["phase"].as_str(),
            Some("approval" | "input" | "attention")
        )
    }

    fn sha(value: &str) -> bool {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    /// `valid(preferences, conversation_id)`.
    pub fn valid(value: &Map<String, Value>, conversation: &str) -> bool {
        // The host applies this ASCII-escaped budget before persisting unmute.
        let json = serde_json::to_string(value).unwrap_or_default();
        let mut bytes = 0;
        for unit in json.encode_utf16() {
            bytes += if unit < 128 { 1 } else { 6 };
            if bytes > BUDGET {
                return false;
            }
        }
        if !keys(
            value,
            &["conversation_id", "muted", "revision", "suppressed"],
        ) || !identity(value.get("conversation_id"), 256)
            || value["conversation_id"] != conversation
            || !value["muted"].is_boolean()
            || revision(value.get("revision")).is_none()
        {
            return false;
        }
        let Some(baseline) = value["suppressed"].as_object() else {
            return false;
        };
        if !keys(
            baseline,
            &[
                "reply_id",
                "attention_signature",
                "optional_question_ids",
                "main_attention",
                "child_attention",
            ],
        ) {
            return false;
        }
        if !baseline["reply_id"].is_null() && !identity(baseline.get("reply_id"), 256) {
            return false;
        }
        let signature = &baseline["attention_signature"];
        if !signature.is_null() && !signature.as_str().is_some_and(sha) {
            return false;
        }
        let (Some(optional), Some(children)) = (
            baseline["optional_question_ids"].as_array(),
            baseline["child_attention"].as_object(),
        ) else {
            return false;
        };
        if optional.len() > 64 {
            return false;
        }
        let mut previous: Option<&str> = None;
        for id in optional {
            if !identity(Some(id), 256) {
                return false;
            }
            let id = id.as_str().unwrap_or("");
            if previous.is_some_and(|last| id.as_bytes() <= last.as_bytes()) {
                return false;
            }
            previous = Some(id);
        }
        let main = &baseline["main_attention"];
        if !main.is_null() && !attention(main, true) {
            return false;
        }
        children.len() <= 256
            && children.iter().all(|(id, child)| {
                identity(Some(&Value::String(id.clone())), 256) && attention(child, false)
            })
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
    let (acknowledged_status, acknowledged_caption) = session.status_with(true);
    let notifications = session.notifications();
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
    let cmd = match text(value, "cmd") {
        "" => agent,
        cmd => cmd,
    };
    let flag = |field: &str| value.get(field).and_then(Value::as_bool);
    ViewRow {
        name: session.name.to_owned(),
        title: session.title(),
        agent: if agent == "dsh" { "deepseek" } else { agent }.to_owned(),
        status: status.to_owned(),
        caption,
        phase: some(session.phase),
        attention,
        unread: session.unread_reply() && !session.reply_in_progress(),
        muted: notifications.is_some_and(|preferences| {
            preferences.get("muted").and_then(Value::as_bool) == Some(true)
        }),
        shared_notifications: notifications.is_some(),
        notification_revision: notifications
            .and_then(|preferences| notifications::revision(preferences.get("revision"))),
        created: number(value, "created").unwrap_or(0.0) as i64,
        attention_fingerprint: session.attention_fingerprint(),
        needs_action: session.needs_action(),
        acknowledged_status: acknowledged_status.to_owned(),
        acknowledged_caption,
        unread_candidate: session.state != "archived"
            && !session.conversation_id.is_empty()
            && !text(value, "reply_id").is_empty()
            && !session.reply_in_progress(),
        machine: String::new(),
        project: session.project(),
        folder: session.folder(),
        branch: session.branch(),
        model: some(text(value, "model")),
        effort: some(text(value, "effort")),
        summary: simplified(&session.summary()),
        updated_at: number(value, "last_event_at").filter(|at| *at > 0.0),
        archived_at: number(value, "archived_at"),
        turn_started: session.turn_started(),
        run_id: some(session.run_id),
        conversation_id: some(session.conversation_id),
        // A live row lists no state; Qt reads it as running (`HgsClient.cpp`).
        state: session.state.to_owned(),
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
        cwd: text(value, "cwd").to_owned(),
        terminal_supported: flag("terminal_supported").unwrap_or(cmd != "dsh" && cmd != "opencode"),
        terminal_reported: flag("terminal_supported").is_some(),
        terminal_attach_supported: flag("terminal_attach_supported").unwrap_or(false),
        terminal_attach_reported: flag("terminal_attach_supported").is_some(),
        bulk_lifecycle_supported: flag("bulk_lifecycle_supported").unwrap_or(cmd != "opencode"),
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
                acknowledged_status: "ready".into(),
                acknowledged_caption: "Not tracked".into(),
                attention_fingerprint: "[\"\",\"\",\"\",null,{}]".into(),
                // Qt's default for a row without a state (`HgsClient.cpp`).
                state: "running".into(),
                terminal_supported: true,
                bulk_lifecycle_supported: true,
                ..ViewRow::default()
            }
        );
        assert_eq!(row(&json!({"state": "paused"})).state, "paused");
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
            "notification_preferences": preferences()}));
        assert_eq!(view.agent, "deepseek");
        assert!(view.muted);
        let other = row(&json!({"cmd": "codex", "conversation_id": "d",
            "notification_preferences": preferences()}));
        assert!(!other.muted);
        let partial = row(&json!({"cmd": "dsh", "conversation_id": "c",
            "notification_preferences": {"conversation_id": "c", "muted": true}}));
        assert!(!partial.muted, "preferences must be complete");
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

    /// The rows the frontend's action tests build their menus from
    /// (`tests/fixtures/action-rows.json`) are what this projection gives.
    #[test]
    fn the_shared_action_rows_are_the_projection() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/action-rows.json")).unwrap();
        let cases = fixture["rows"].as_array().unwrap();
        assert!(cases.len() >= 4);
        for case in cases {
            assert_eq!(
                serde_json::to_value(row(&case["hgs"])).unwrap(),
                case["view"],
                "{}",
                case["case"]
            );
        }
    }

    #[test]
    fn bulk_lifecycle_defaults_to_true_except_for_opencode() {
        assert!(row(&json!({"cmd": "claude"})).bulk_lifecycle_supported);
        assert!(!row(&json!({"cmd": "opencode"})).bulk_lifecycle_supported);
        assert!(!row(&json!({"agent": "opencode"})).bulk_lifecycle_supported);
        assert!(
            row(&json!({"cmd": "opencode", "bulk_lifecycle_supported": true}))
                .bulk_lifecycle_supported
        );
        assert!(
            !row(&json!({"cmd": "claude", "bulk_lifecycle_supported": false}))
                .bulk_lifecycle_supported
        );
        assert!(
            row(&json!({"cmd": "claude", "bulk_lifecycle_supported": "no"}))
                .bulk_lifecycle_supported
        );
    }

    #[test]
    fn terminal_and_cwd_fields_follow_the_qt_client() {
        let plain = row(&json!({"cmd": "claude", "cwd": "/home/example/sample/project"}));
        assert_eq!(plain.cwd, "/home/example/sample/project");
        assert_eq!(plain.folder, "project");
        assert!(plain.terminal_supported && !plain.terminal_reported);
        assert!(!plain.terminal_attach_supported && !plain.terminal_attach_reported);
        for cmd in ["dsh", "opencode"] {
            assert!(!row(&json!({ "cmd": cmd })).terminal_supported, "{cmd}");
        }
        assert!(!row(&json!({"agent": "opencode"})).terminal_supported);
        let reported = row(&json!({"cmd": "dsh", "terminal_supported": true,
            "terminal_attach_supported": false}));
        assert!(reported.terminal_supported && reported.terminal_reported);
        assert!(!reported.terminal_attach_supported && reported.terminal_attach_reported);
        let attach = row(&json!({"terminal_attach_supported": true, "terminal_supported": "yes"}));
        assert!(attach.terminal_attach_supported && attach.terminal_attach_reported);
        assert!(!attach.terminal_reported);
    }

    fn fingerprint_row() -> Value {
        json!({"name": "claude/sample-project/review", "run_id": "run-1", "conversation_id": "c",
            "tracked": true, "phase": "approval", "activity": "busy", "attention_id": "att-1",
            "last_event_at": 5.0, "activity_detail": "rm -rf scratch"})
    }

    #[test]
    fn the_attention_fingerprint_follows_the_request_and_not_the_summary() {
        let base = row(&fingerprint_row()).attention_fingerprint;
        assert!(!base.is_empty());
        let changed = |edit: &dyn Fn(&mut Value)| {
            let mut value = fingerprint_row();
            edit(&mut value);
            row(&value).attention_fingerprint
        };
        assert_ne!(base, changed(&|v| v["phase"] = json!("input")));
        assert_ne!(base, changed(&|v| v["attention_id"] = json!("att-2")));
        assert_ne!(base, changed(&|v| v["run_id"] = json!("run-2")));
        assert_ne!(
            base,
            changed(
                &|v| v["subagents"] = json!({"a": {"display_state": "input", "question_id": "q1"}})
            )
        );
        assert_eq!(
            base,
            changed(&|v| v["activity_detail"] = json!("other text"))
        );
        assert_eq!(base, changed(&|v| v["last_event_at"] = json!(9.0)));
        let mut without_id = fingerprint_row();
        without_id["attention_id"] = json!("");
        let first = row(&without_id).attention_fingerprint;
        without_id["last_event_at"] = json!(9.0);
        assert_ne!(first, row(&without_id).attention_fingerprint);
        let child = |request: &str, source: &str| {
            let mut value = fingerprint_row();
            value["subagent_source"] = json!(source);
            value["subagents"] = json!({"a": {"state": "attention", "attention_id": request}});
            row(&value).attention_fingerprint
        };
        assert_ne!(child("r1", ""), child("r2", ""));
        assert_eq!(child("r1", "hook_profiles"), base);
        let created = row(&json!({"created": 1793000000.9}));
        assert_eq!(created.created, 1793000000);
        assert!(row(&fingerprint_row()).needs_action);
    }

    #[test]
    fn acknowledged_status_covers_approval_input_recovery_and_child_input() {
        let ack = |extra: Value| {
            let mut value = json!({"name": "a/b/c", "tracked": true, "activity": "busy"});
            for (key, item) in extra.as_object().unwrap() {
                value[key] = item.clone();
            }
            row(&value)
        };
        let approval = ack(json!({"phase": "approval"}));
        assert_eq!(
            (approval.status.as_str(), approval.caption.as_str()),
            ("attention", "Needs approval")
        );
        assert_eq!(approval.acknowledged_status, "ready");
        assert_eq!(approval.acknowledged_caption, "Needs approval");
        let input = ack(json!({"phase": "input"}));
        assert_eq!(input.acknowledged_status, "ready");
        assert_eq!(input.acknowledged_caption, "Needs input");
        let recovery = ack(json!({"phase": "idle", "recovery": {"state": "blocked"}}));
        assert_eq!(recovery.status, "attention");
        assert_eq!(recovery.acknowledged_status, "ready");
        assert_eq!(recovery.acknowledged_caption, "Needs attention");
        let child = ack(json!({"phase": "idle", "subagents": {"a": {"display_state": "input"}}}));
        assert_eq!(child.status, "attention");
        assert_eq!(child.acknowledged_status, "working");
        assert_eq!(child.acknowledged_caption, "Working");
        let plain = ack(json!({"phase": "tool"}));
        assert_eq!(plain.acknowledged_status, plain.status);
        assert_eq!(plain.acknowledged_caption, plain.caption);
    }

    #[test]
    fn unread_candidate_needs_a_live_conversation_reply_that_is_finished() {
        let base = json!({"name": "a/b/c", "conversation_id": "c", "reply_id": "r1",
            "tracked": true, "phase": "idle", "activity": "idle"});
        assert!(row(&base).unread_candidate);
        let with = |key: &str, value: Value| {
            let mut changed = base.clone();
            changed[key] = value;
            row(&changed).unread_candidate
        };
        assert!(!with("state", json!("archived")));
        assert!(!with("conversation_id", json!("")));
        assert!(!with("reply_id", json!("")));
        assert!(!with("activity", json!("busy")));
        assert!(row(&base).summary.is_empty());
        let mut unread = base.clone();
        unread["activity_detail"] = json!("Reply text");
        unread["attention_signature"] = json!("");
        assert!(row(&unread).unread);
    }

    fn preferences() -> Value {
        json!({"conversation_id": "c", "muted": true, "revision": 3, "suppressed": {
            "reply_id": "r1", "attention_signature": "a".repeat(64),
            "optional_question_ids": ["q1", "q2"],
            "main_attention": {"run_id": "run-1", "phase": "approval", "request_id": "x", "started_at": null},
            "child_attention": {"kid": {"phase": "input", "request_id": "", "started_at": 12.5}}}})
    }

    fn with_preferences(preferences: Value) -> ViewRow {
        row(
            &json!({"name": "a/b/c", "conversation_id": "c", "notification_preferences": preferences}),
        )
    }

    #[test]
    fn shared_notifications_port_the_qt_validation() {
        let valid = with_preferences(preferences());
        assert!(valid.shared_notifications && valid.muted);
        assert_eq!(valid.notification_revision, Some(3));
        let mut unmuted = preferences();
        unmuted["muted"] = json!(false);
        let unmuted = with_preferences(unmuted);
        assert!(unmuted.shared_notifications && !unmuted.muted);
        let mut cases: Vec<(&str, Value)> = Vec::new();
        let mut missing = preferences();
        missing.as_object_mut().unwrap().remove("revision");
        cases.push(("missing key", missing));
        let mut extra = preferences();
        extra["more"] = json!(1);
        cases.push(("extra key", extra));
        let mut other = preferences();
        other["conversation_id"] = json!("d");
        cases.push(("other conversation", other));
        let mut negative = preferences();
        negative["revision"] = json!(-1);
        cases.push(("negative revision", negative));
        let mut signature = preferences();
        signature["suppressed"]["attention_signature"] = json!("ABC");
        cases.push(("bad signature", signature));
        let mut unsorted = preferences();
        unsorted["suppressed"]["optional_question_ids"] = json!(["q2", "q1"]);
        cases.push(("unsorted ids", unsorted));
        let mut oversized = preferences();
        let big = json!("\u{e9}".repeat(32_000));
        oversized["suppressed"]["main_attention"]["request_id"] = big.clone();
        oversized["suppressed"]["child_attention"]["kid"]["request_id"] = big.clone();
        oversized["suppressed"]["child_attention"]["kid2"] =
            json!({"phase": "input", "request_id": big, "started_at": null});
        cases.push(("oversized", oversized));
        for (name, case) in cases {
            let view = with_preferences(case);
            assert!(!view.shared_notifications && !view.muted, "{name}");
            assert_eq!(view.notification_revision, None, "{name}");
        }
    }
}
