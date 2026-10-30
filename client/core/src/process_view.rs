//! The Processes tab's rules, ported from `tray/src/ProcessView.cpp`: which
//! group a process belongs to, its labels, the summary, when its output is
//! read again, whether the selection can be stopped, and the sequential stop
//! queue. The inventory itself comes from the `processes` topic; the output
//! read is [`crate::reads::Read::ProcessOutput`] and a stop is
//! [`crate::actions::Action::StopProcess`].
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

fn text<'a>(record: &'a Value, key: &str) -> &'a str {
    record.get(key).and_then(Value::as_str).unwrap_or("")
}

fn status(record: &Value) -> &str {
    text(record, "status")
}

/// Starting, awaiting approval, running, stopping or unconfirmed.
pub fn active(record: &Value) -> bool {
    matches!(
        status(record),
        "starting" | "approval" | "running" | "stopping" | "unknown"
    )
}

/// Active, confirmed, not already stopping, and stoppable by its adapter.
pub fn stoppable(record: &Value) -> bool {
    active(record)
        && !matches!(status(record), "unknown" | "stopping")
        && record["capabilities"]["stop"].as_bool() == Some(true)
}

/// The state column's text.
pub fn status_label(record: &Value) -> String {
    match status(record) {
        "unknown" => "Unconfirmed".into(),
        "approval" => "Awaiting approval".into(),
        "timed_out" => "Timed out".into(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

/// The state column's tooltip.
pub fn status_tooltip(record: &Value) -> String {
    if status(record) == "unknown" {
        format!(
            "Last recorded state: {}. This process has no current live confirmation.",
            text(record, "last_status")
        )
    } else {
        text(record, "detail").to_owned()
    }
}

/// What must change before a selected process's output is read again.
pub fn output_revision(record: &Value) -> Value {
    json!({"status": record.get("status"), "output_at": record.get("output_at"),
           "ended_at": record.get("ended_at")})
}

/// A process of a session that is not live is unconfirmed: its last status
/// is kept as `last_status`.
pub fn normalize(record: &mut Value, live: bool) {
    if !live && active(record) {
        let last = record["status"].clone();
        record["last_status"] = last;
        record["status"] = json!("unknown");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// "Live shells / processes" (or "Last observed processes").
    Live,
    /// "Recorded commands / unconfirmed".
    Recorded,
    /// "Finished", collapsed at first.
    Finished,
}

/// The group of a normalized process.
pub fn group(record: &Value, live: bool) -> Group {
    let observed_shell = live
        && record["background"].as_bool() == Some(true)
        && record["observed"].as_bool() == Some(true)
        && status(record) == "running";
    if text(record, "source") == "process_tree" || observed_shell {
        Group::Live
    } else if active(record) {
        Group::Recorded
    } else {
        Group::Finished
    }
}

pub fn group_title(group: Group, live: bool) -> &'static str {
    match group {
        Group::Live if live => "Live shells / processes",
        Group::Live => "Last observed processes",
        Group::Recorded => "Recorded commands / unconfirmed",
        Group::Finished => "Finished",
    }
}

/// The summary line over normalized processes.
pub fn summary(records: &[Value], live: bool) -> String {
    if records.is_empty() {
        return "No shell commands recorded yet".into();
    }
    let (mut live_processes, mut unconfirmed, mut running) = (0, 0, 0);
    for record in records {
        if text(record, "source") == "process_tree" && live {
            live_processes += 1;
        } else if status(record) == "unknown" {
            unconfirmed += 1;
        } else if active(record) {
            running += 1;
        }
    }
    let prefix = if live_processes > 0 {
        format!("{live_processes} live    ")
    } else {
        String::new()
    };
    format!(
        "{prefix}{running} active    {unconfirmed} unconfirmed    {} recorded",
        records.len() - live_processes
    )
}

/// The inventory's notes, and the summary's tooltip.
pub fn notes(processes: &Value) -> (String, String) {
    let notes: Vec<&str> = processes["notes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let text = notes.join("\n");
    let tooltip = if processes["complete"].as_bool() == Some(true) {
        text.clone()
    } else {
        format!("{text}\nOnly observed commands are shown; older history may be unavailable.")
    };
    (text, tooltip)
}

/// The selected process's detail lines; `machine` is empty for this one.
pub fn details(record: &Value, machine: &str) -> String {
    let command = text(record, "command");
    let shown: String = command.chars().take(300).collect();
    let mut lines = vec![
        if command.chars().count() > 300 {
            format!("{shown}…")
        } else {
            shown
        },
        format!(
            "{}  {}  ({})",
            if machine.is_empty() {
                "This machine"
            } else {
                machine
            },
            text(record, "cwd"),
            status_label(record)
        ),
    ];
    for key in ["description", "progress"] {
        let value = text(record, key);
        if !value.is_empty() {
            lines.push(value.chars().take(200).collect());
        }
    }
    if let Some(code) = record.get("exit_code").filter(|code| !code.is_null()) {
        lines.push(format!(
            "Exit code: {}",
            code.as_f64().unwrap_or(0.0) as i64
        ));
    }
    lines.join("\n")
}

/// The duration column at `now` (seconds since the epoch): `m:ss`, marked
/// for an unconfirmed process, or `—` without a start.
pub fn duration(record: &Value, now: f64) -> String {
    let started = record["started_at"].as_f64().unwrap_or(0.0);
    if started <= 0.0 {
        return "—".into();
    }
    let unknown = status(record) == "unknown";
    let fallback = if unknown {
        record["updated_at"].as_f64().unwrap_or(0.0)
    } else {
        now.floor()
    };
    let end = record["ended_at"].as_f64().unwrap_or(fallback);
    let seconds = ((end - started) as i64).max(0);
    format!(
        "{}:{:02}{}",
        seconds / 60,
        seconds % 60,
        if unknown { " (recorded)" } else { "" }
    )
}

/// The state column's colour role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Danger,
    Muted,
    Warning,
    Success,
}

pub fn tone(record: &Value) -> Tone {
    match status(record) {
        "failed" | "timed_out" => Tone::Danger,
        "unknown" => Tone::Muted,
        _ if active(record) => Tone::Warning,
        _ => Tone::Success,
    }
}

/// Process settings, clamped as Qt's `ProcessSettings`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub enabled: bool,
    pub active_seconds: f64,
    pub background_seconds: f64,
    pub unknown_seconds: f64,
    pub loading_seconds: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: false,
            active_seconds: 2.5,
            background_seconds: 30.0,
            unknown_seconds: 10.0,
            loading_seconds: 0.5,
        }
    }
}

impl Settings {
    /// The values within Qt's bounds, in milliseconds: active 1-60 s,
    /// background 5-600 s, unconfirmed 1-600 s, loading indicator 0-5 s.
    pub fn active_ms(&self) -> u64 {
        (self.active_seconds.clamp(1.0, 60.0) * 1000.0).round() as u64
    }

    pub fn background_ms(&self) -> u64 {
        (self.background_seconds.clamp(5.0, 600.0) * 1000.0).round() as u64
    }

    pub fn unknown_ms(&self) -> u64 {
        (self.unknown_seconds.clamp(1.0, 600.0) * 1000.0).round() as u64
    }

    pub fn loading_ms(&self) -> u64 {
        (self.loading_seconds.clamp(0.0, 5.0) * 1000.0).round() as u64
    }

    /// How often the selected process's output is read again.
    pub fn output_ms(&self, record: &Value) -> u64 {
        if status(record) == "unknown" {
            self.unknown_ms().max(self.active_ms())
        } else {
            self.active_ms()
        }
    }
}

/// Whether the selected process's output is read now (`requestOutput`).
/// `forced` for a new selection or a shown tab; `dirty` after its output
/// revision changed; `age_ms` since the last read, if any.
pub fn output_due(
    record: &Value,
    settings: &Settings,
    forced: bool,
    dirty: bool,
    age_ms: Option<u64>,
) -> bool {
    if forced {
        return true;
    }
    if !dirty && !active(record) {
        return false;
    }
    age_ms.is_none_or(|age| age >= settings.output_ms(record))
}

/// A process whose output only the recorded command or Terminal shows.
pub fn output_unavailable(record: &Value) -> Option<&'static str> {
    (record["capabilities"]["output"] == json!(false))
        .then_some("Live OS process. Output remains in the recorded command or Terminal.")
}

/// What an output reply shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    /// The output to show, `None` to keep what is shown.
    pub text: Option<String>,
    pub notice: String,
    /// The session changed: refresh the inventory.
    pub refresh: bool,
}

/// Checks an output reply against the selected process and session.
pub fn output(reply: &Value, process: &str, run: &str, conversation: &str) -> Output {
    if text(reply, "id") != process
        || text(reply, "run_id") != run
        || text(reply, "conversation_id") != conversation
    {
        return Output {
            text: None,
            notice: "Session changed. Refreshing processes…".into(),
            refresh: true,
        };
    }
    let output = text(reply, "output").to_owned();
    let notice = if reply["truncated"].as_bool() == Some(true) {
        "Showing the retained output tail."
    } else if output.is_empty() {
        "No output recorded yet."
    } else {
        ""
    };
    Output {
        text: Some(output),
        notice: notice.into(),
        refresh: false,
    }
}

/// The session the Processes tab shows, for the stop controls.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StopContext {
    pub enabled: bool,
    pub online: bool,
    pub live: bool,
    /// Stop requests are being sent.
    pub sending: bool,
    /// Processes with a stop already requested.
    pub pending: HashSet<String>,
}

/// The "Stop selected" button.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopControl {
    pub label: String,
    pub enabled: bool,
    pub tooltip: String,
}

/// The stop button for `selected` (process ID and normalized record).
pub fn stop_control(selected: &[(&str, &Value)], context: &StopContext) -> StopControl {
    let label = if selected.is_empty() {
        "Stop selected".to_owned()
    } else {
        format!("Stop selected ({})", selected.len())
    };
    let reason = if !context.enabled {
        Some("Enable Processes in Settings to use process control.")
    } else if !context.online || !context.live {
        Some("Process control requires a live session on an online machine.")
    } else if context.sending {
        Some("Sending stop requests…")
    } else if selected.is_empty() {
        Some("Select processes with Ctrl/Cmd or Shift.")
    } else {
        selected.iter().find_map(|(id, record)| {
            if context.pending.contains(*id) || status(record) == "stopping" {
                Some("A stop request is already pending for a selected process.")
            } else if !active(record) {
                Some("Select active processes to stop.")
            } else if !stoppable(record) {
                Some("Targeted stop is unavailable for this recorded command. Select its running process in Live shells / processes, or a supported native job.")
            } else {
                None
            }
        })
    };
    let tooltip = match reason {
        Some(reason) => reason.to_owned(),
        None if selected
            .iter()
            .any(|(_, record)| text(record, "source") == "process_tree") =>
        {
            "Stop selected processes and their children: TERM, then KILL after 2 seconds if needed."
                .into()
        }
        None => "Request native cancellation of exactly the selected processes.".into(),
    };
    StopControl {
        label,
        enabled: reason.is_none(),
        tooltip,
    }
}

/// How one stop request ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stopped {
    Requested,
    AlreadyFinished,
    Failed(String),
}

/// Checks a stop receipt: the same process, run and conversation, with
/// `requested` or `already-finished`.
pub fn stop_result(
    reply: Result<&Value, &str>,
    process: &str,
    run: &str,
    conversation: &str,
) -> Stopped {
    let reply = match reply {
        Err(error) if !error.is_empty() => return Stopped::Failed(error.to_owned()),
        Err(_) => return Stopped::Failed("Stop was not confirmed".into()),
        Ok(reply) => reply,
    };
    let receipt = text(reply, "id") == process
        && text(reply, "run_id") == run
        && text(reply, "conversation_id") == conversation;
    match status(reply) {
        "requested" if receipt => Stopped::Requested,
        "already-finished" if receipt => Stopped::AlreadyFinished,
        _ => Stopped::Failed("Stop was not confirmed".into()),
    }
}

/// What the stop queue does next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Send a stop for this process, then report its result.
    Stop(String),
    /// Nothing left: show this message.
    Done(String),
}

/// Stops sent one at a time, never retried (`stopSelected`, `stopNext`).
/// A change of session identity cancels the requests not yet sent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StopQueue {
    key: String,
    queue: Vec<String>,
    current: Option<String>,
    sent: usize,
    skipped: usize,
    errors: Vec<String>,
    /// Processes with a stop requested and not yet seen ending.
    pub pending: HashSet<String>,
}

fn short(id: &str) -> String {
    id.chars().take(12).collect()
}

impl StopQueue {
    /// Queues `ids` for the session identified by `key` (host, name,
    /// archive, run, conversation and host generation).
    pub fn start(&mut self, key: &str, ids: Vec<String>) {
        self.key = key.to_owned();
        self.queue = ids;
        self.current = None;
        self.sent = 0;
        self.skipped = 0;
        self.errors.clear();
    }

    pub fn sending(&self) -> bool {
        self.current.is_some() || !self.queue.is_empty()
    }

    /// The next step for the session now shown as `key`, its normalized
    /// processes by ID, and whether stops can be sent.
    pub fn next(
        &mut self,
        key: &str,
        find: impl Fn(&str) -> Option<Value>,
        available: bool,
    ) -> Option<Next> {
        if self.current.is_some() || key != self.key {
            return None;
        }
        while !self.queue.is_empty() {
            let id = self.queue.remove(0);
            if !available {
                self.errors.push(
                    "Machine or session became unavailable; remaining requests were cancelled."
                        .into(),
                );
                self.queue.clear();
                break;
            }
            match find(&id) {
                Some(record) if active(&record) => {
                    if !stoppable(&record) {
                        self.errors.push(format!(
                            "{}: process control is no longer available",
                            short(&id)
                        ));
                        continue;
                    }
                    self.current = Some(id.clone());
                    self.pending.insert(id.clone());
                    return Some(Next::Stop(id));
                }
                _ => self.skipped += 1,
            }
        }
        let mut message = format!("Stop requested for {} process(es).", self.sent);
        if self.sent > 0 {
            message.push_str(" Waiting for completion.");
        }
        if self.skipped > 0 {
            message.push_str(&format!(
                " {} already finished or left the list.",
                self.skipped
            ));
        }
        if !self.errors.is_empty() {
            message.push_str(&format!(
                " {} not confirmed. No automatic retries.\n{}",
                self.errors.len(),
                self.errors.join("\n")
            ));
        }
        Some(Next::Done(message))
    }

    /// Records the result of the stop in flight.
    pub fn finished(&mut self, result: Stopped) {
        let Some(id) = self.current.take() else {
            return;
        };
        match result {
            Stopped::Requested => self.sent += 1,
            Stopped::AlreadyFinished => {
                self.pending.remove(&id);
                self.skipped += 1;
            }
            Stopped::Failed(error) => {
                self.pending.remove(&id);
                self.errors.push(format!("{}: {error}", short(&id)));
            }
        }
    }

    /// A new session identity drops every stop not yet sent.
    pub fn reset(&mut self) {
        *self = StopQueue::default();
    }

    /// Drops pending marks of processes that ended or left the list.
    pub fn settle(&mut self, find: impl Fn(&str) -> Option<Value>) {
        self.pending
            .retain(|id| find(id).is_some_and(|record| active(&record)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(status: &str) -> Value {
        json!({"id": "p1", "status": status, "command": "cargo test", "cwd": "/home/example/project",
               "capabilities": {"stop": true, "output": true}})
    }

    #[test]
    fn states_labels_and_groups_follow_qt() {
        assert!(active(&process("approval")) && active(&process("unknown")));
        assert!(!active(&process("finished")));
        assert!(stoppable(&process("running")));
        assert!(!stoppable(&process("unknown")) && !stoppable(&process("stopping")));
        let mut no_stop = process("running");
        no_stop["capabilities"]["stop"] = json!(false);
        assert!(!stoppable(&no_stop));
        assert_eq!(status_label(&process("unknown")), "Unconfirmed");
        assert_eq!(status_label(&process("approval")), "Awaiting approval");
        assert_eq!(status_label(&process("timed_out")), "Timed out");
        assert_eq!(status_label(&process("running")), "Running");
        let mut record = process("running");
        normalize(&mut record, false);
        assert_eq!(
            (status(&record), text(&record, "last_status")),
            ("unknown", "running")
        );
        assert_eq!(
            status_tooltip(&record),
            "Last recorded state: running. This process has no current live confirmation."
        );
        assert_eq!(group(&record, false), Group::Recorded);
        assert_eq!(group(&process("finished"), true), Group::Finished);
        let mut tree = process("running");
        tree["source"] = json!("process_tree");
        assert_eq!(group(&tree, false), Group::Live);
        let mut shell = process("running");
        shell["background"] = json!(true);
        shell["observed"] = json!(true);
        assert_eq!(group(&shell, true), Group::Live);
        assert_eq!(group(&shell, false), Group::Recorded);
        assert_eq!(group_title(Group::Live, false), "Last observed processes");
        assert_eq!(
            summary(
                &[
                    tree.clone(),
                    process("running"),
                    record.clone(),
                    process("finished")
                ],
                true
            ),
            "1 live    1 active    1 unconfirmed    3 recorded"
        );
        assert_eq!(summary(&[], true), "No shell commands recorded yet");
        assert_eq!(tone(&process("timed_out")), Tone::Danger);
        assert_eq!(tone(&record), Tone::Muted);
        assert_eq!(tone(&process("running")), Tone::Warning);
        assert_eq!(tone(&process("finished")), Tone::Success);
    }

    #[test]
    fn details_durations_and_notes() {
        let mut record = process("failed");
        record["exit_code"] = json!(2);
        record["description"] = json!("Run the tests");
        assert_eq!(
            details(&record, ""),
            "cargo test\nThis machine  /home/example/project  (Failed)\nRun the tests\nExit code: 2"
        );
        record["command"] = json!("x".repeat(301));
        assert!(details(&record, "studio").starts_with(&format!("{}…\nstudio", "x".repeat(300))));
        let mut timed = process("running");
        timed["started_at"] = json!(1000.0);
        assert_eq!(duration(&timed, 1125.7), "2:05");
        timed["status"] = json!("unknown");
        timed["updated_at"] = json!(1061.0);
        assert_eq!(duration(&timed, 5000.0), "1:01 (recorded)");
        assert_eq!(duration(&process("running"), 5000.0), "—");
        let (shown, tooltip) = notes(&json!({"notes": ["Claude hooks"], "complete": false}));
        assert_eq!(shown, "Claude hooks");
        assert_eq!(
            tooltip,
            "Claude hooks\nOnly observed commands are shown; older history may be unavailable."
        );
    }

    #[test]
    fn output_is_read_at_qt_s_cadence_and_checked() {
        let settings = Settings::default();
        assert_eq!(settings.output_ms(&process("running")), 2500);
        assert_eq!(settings.output_ms(&process("unknown")), 10000);
        assert_eq!(
            Settings {
                active_seconds: 0.1,
                loading_seconds: 9.0,
                ..settings
            }
            .active_ms(),
            1000
        );
        assert_eq!(
            Settings {
                loading_seconds: 9.0,
                ..settings
            }
            .loading_ms(),
            5000
        );
        assert!(output_due(
            &process("finished"),
            &settings,
            true,
            false,
            Some(0)
        ));
        assert!(!output_due(
            &process("finished"),
            &settings,
            false,
            false,
            None
        ));
        assert!(output_due(
            &process("finished"),
            &settings,
            false,
            true,
            None
        ));
        assert!(!output_due(
            &process("running"),
            &settings,
            false,
            false,
            Some(2499)
        ));
        assert!(output_due(
            &process("running"),
            &settings,
            false,
            false,
            Some(2500)
        ));
        let mut tree = process("running");
        tree["capabilities"]["output"] = json!(false);
        assert!(
            output_unavailable(&tree).is_some()
                && output_unavailable(&process("running")).is_none()
        );
        let reply = json!({"id": "p1", "run_id": "r", "conversation_id": "c", "output": "", "truncated": false});
        assert_eq!(
            output(&reply, "p1", "r", "c"),
            Output {
                text: Some(String::new()),
                notice: "No output recorded yet.".into(),
                refresh: false
            }
        );
        let tail = json!({"id": "p1", "run_id": "r", "conversation_id": "c", "output": "ok", "truncated": true});
        assert_eq!(
            output(&tail, "p1", "r", "c").notice,
            "Showing the retained output tail."
        );
        assert!(output(&reply, "p1", "r2", "c").refresh);
    }

    #[test]
    fn stop_controls_explain_why_not() {
        let mut context = StopContext {
            enabled: true,
            online: true,
            live: true,
            ..StopContext::default()
        };
        let running = process("running");
        let control = stop_control(&[("p1", &running)], &context);
        assert_eq!(
            (
                control.label.as_str(),
                control.enabled,
                control.tooltip.as_str()
            ),
            (
                "Stop selected (1)",
                true,
                "Request native cancellation of exactly the selected processes."
            )
        );
        let mut tree = process("running");
        tree["source"] = json!("process_tree");
        assert!(stop_control(&[("p1", &running), ("p2", &tree)], &context)
            .tooltip
            .starts_with("Stop selected processes and their children"));
        assert_eq!(
            stop_control(&[], &context).tooltip,
            "Select processes with Ctrl/Cmd or Shift."
        );
        assert_eq!(
            stop_control(&[("p1", &process("finished"))], &context).tooltip,
            "Select active processes to stop."
        );
        context.pending.insert("p1".into());
        assert_eq!(
            stop_control(&[("p1", &running)], &context).tooltip,
            "A stop request is already pending for a selected process."
        );
        context.live = false;
        assert!(!stop_control(&[("p1", &running)], &context).enabled);
        context.enabled = false;
        assert_eq!(
            stop_control(&[], &context).tooltip,
            "Enable Processes in Settings to use process control."
        );
    }

    #[test]
    fn stops_go_one_at_a_time_without_retries() {
        let records = |id: &str| -> Option<Value> {
            match id {
                "process-one-long-id" | "p2" => Some(process("running")),
                "p3" => Some(process("finished")),
                "p4" => Some(process("unknown")),
                _ => None,
            }
        };
        let mut queue = StopQueue::default();
        queue.start(
            "key",
            ["process-one-long-id", "p2", "p3", "p4", "gone"]
                .map(String::from)
                .to_vec(),
        );
        assert!(queue.sending());
        assert_eq!(
            queue.next("key", records, true),
            Some(Next::Stop("process-one-long-id".into()))
        );
        assert_eq!(queue.next("key", records, true), None, "one in flight");
        queue.finished(stop_result(
            Err("timed out"),
            "process-one-long-id",
            "r",
            "c",
        ));
        assert_eq!(
            queue.next("key", records, true),
            Some(Next::Stop("p2".into()))
        );
        let receipt =
            json!({"id": "p2", "run_id": "r", "conversation_id": "c", "status": "requested"});
        queue.finished(stop_result(Ok(&receipt), "p2", "r", "c"));
        assert!(queue.pending.contains("p2"));
        let Some(Next::Done(message)) = queue.next("key", records, true) else {
            panic!("done")
        };
        assert_eq!(
            message,
            "Stop requested for 1 process(es). Waiting for completion. 2 already finished or left the list. 2 not confirmed. No automatic retries.\nprocess-one-: timed out\np4: process control is no longer available"
        );
        // Another session drops what was not sent.
        queue.start("key", vec!["p2".into()]);
        assert_eq!(queue.next("other", records, true), None);
        queue.start("key", vec!["p2".into(), "p2".into()]);
        let Some(Next::Done(message)) = queue.next("key", records, false) else {
            panic!("cancelled")
        };
        assert!(message.ends_with(
            "Machine or session became unavailable; remaining requests were cancelled."
        ));
        assert_eq!(
            stop_result(
                Ok(
                    &json!({"id": "p9", "run_id": "r", "conversation_id": "c", "status": "requested"})
                ),
                "p2",
                "r",
                "c"
            ),
            Stopped::Failed("Stop was not confirmed".into())
        );
        queue.pending.insert("p3".into());
        queue.settle(records);
        assert!(!queue.pending.contains("p3"));
    }
}
