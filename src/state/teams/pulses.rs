//! Task 133 (owner, 2026-11-07): member pulses. At each member's turn end
//! Zerus writes a 2-3 line pulse into the team's working tier (docs/lore.md
//! 11.2, 11.5: a plain team keeps it in teams.sqlite3): the member's own
//! `hgs team note` when it gave one, else a short summary of the turn's tool
//! events. No chat text is read or stored: only tool names, the files they
//! touched, commands and error lines, each cut to one line. A summary with
//! nothing concrete is not written. The lead's digest and the Team tab show
//! the latest pulse per member.
use super::model::{Task, Team};
use serde::Serialize;
use serde_json::{json, Value};

/// At most this many characters a pulse field holds (11.4).
pub(crate) const FIELD: usize = 500;
/// A pulse by the same member and task within this many seconds replaces it.
pub(crate) const REPLACE_WITHIN: f64 = 300.0;
/// At most this many pulses a team keeps (11.4).
pub(crate) const PER_TEAM: i64 = 2_000;
/// A closed task's pulses stay this long after the close (11.3, pulse_days).
pub(crate) const KEEP_SECONDS: f64 = 14.0 * 86_400.0;
/// At most this many rows one prune deletes.
pub(crate) const PRUNE_ROWS: i64 = 500;
/// At most this many files an automatic pulse names.
const FILES: usize = 5;
/// One line of a command or an error, at most this long.
const LINE: usize = 160;

/// One pulse: what a member did, does next and is blocked on.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub(crate) struct Pulse {
    pub id: String,
    pub team: String,
    pub member: String,
    /// The team task, 0 for none.
    pub task: u32,
    pub done: String,
    pub next: String,
    pub blocked: String,
    pub at: f64,
    /// `note` (the member's own) or `auto` (Zerus's summary of the turn).
    pub source: String,
}

impl Pulse {
    pub(crate) fn json(&self) -> Value {
        json!({"id":self.id,"member":self.member,"task":(self.task > 0).then_some(self.task),"done":self.done,"next":self.next,
            "blocked":self.blocked,"at":self.at,"source":self.source})
    }
}

/// One line, at most `limit` characters.
pub(crate) fn one_line(text: &str, limit: usize) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let flat = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > limit {
        flat.chars().take(limit.saturating_sub(1)).collect::<String>() + "\u{2026}"
    } else {
        flat
    }
}

/// Checks a note's fields (11.4): at most 500 characters each, one at least.
pub(crate) fn check_note(done: &str, next: &str, blocked: &str) -> anyhow::Result<()> {
    for (name, value) in [("done", done), ("next", next), ("blocked", blocked)] {
        let count = value.chars().count();
        anyhow::ensure!(count <= FIELD, "The pulse's \"{name}\" has {count} characters; a pulse field holds at most {FIELD}");
        anyhow::ensure!(!value.chars().any(|ch| ch.is_control() && ch != '\n'), "The pulse's \"{name}\" holds a control character");
    }
    anyhow::ensure!(
        !(done.trim().is_empty() && next.trim().is_empty() && blocked.trim().is_empty()),
        "A pulse says at least one of \"done\", \"next\" or \"blocked\""
    );
    Ok(())
}

/// Tools whose detail is the file they changed.
fn edits(tool: &str) -> bool {
    matches!(tool, "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" | "edit" | "write" | "write_file" | "edit_file" | "str_replace")
}

fn text<'a>(event: &'a Value, key: &str) -> &'a str {
    event.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn at(event: &Value) -> f64 {
    match event.get("at") {
        Some(Value::String(text)) => text.parse().unwrap_or(0.0),
        Some(value) => value.as_f64().unwrap_or(0.0),
        None => 0.0,
    }
}

/// A path as the pulse shows it: relative to the member's folder when it is
/// inside it, forward slashes.
fn shown_path(path: &str, folder: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let folder = folder.trim().replace('\\', "/");
    let folder = folder.trim_end_matches('/');
    if !folder.is_empty() && path.to_lowercase().starts_with(&format!("{}/", folder.to_lowercase())) {
        return path[folder.len() + 1..].to_owned();
    }
    path
}

/// The automatic pulse of one turn (task 133): the task, the files the turn
/// changed and its last tool or error line, from the session's tool events
/// after `since`. The task is named by its id and title, so a pulse can be
/// built beside the tick (task 185) without the team record. None when the turn
/// did nothing concrete: no generic "worked on the task" pulse is ever written.
/// Assistant and user text are never read.
pub(crate) fn auto(task: u32, title: &str, events: &[Value], since: f64, folder: &str) -> Option<(String, String, String)> {
    let turn: Vec<&Value> = events.iter().filter(|event| at(event) > since).collect();
    let mut files: Vec<String> = Vec::new();
    let mut last_tool: Option<String> = None;
    let mut error: Option<String> = None;
    for event in &turn {
        let kind = text(event, "type");
        let tool = text(event, "tool");
        let detail = text(event, "detail");
        match kind {
            "PostToolUse" | "tool_result" | "tool" => {
                if edits(tool) && !detail.trim().is_empty() {
                    let path = shown_path(&one_line(detail, 300), folder);
                    if !files.contains(&path) {
                        files.push(path);
                    }
                }
                if !tool.is_empty() {
                    last_tool = Some(if detail.trim().is_empty() { tool.to_owned() } else { format!("{tool}: {}", one_line(detail, LINE)) });
                    error = None;
                }
            }
            "PostToolUseFailure" | "tool_error" | "error" => {
                let line = one_line(detail, LINE);
                if !line.is_empty() {
                    error = Some(if tool.is_empty() { line } else { format!("{tool} failed: {line}") });
                }
            }
            _ => {}
        }
    }
    if files.is_empty() && last_tool.is_none() && error.is_none() {
        return None;
    }
    let title = one_line(title, 80);
    let changed = if files.is_empty() {
        String::new()
    } else {
        let more = files.len().saturating_sub(FILES);
        let shown = files.iter().take(FILES).cloned().collect::<Vec<_>>().join(", ");
        if more > 0 { format!("; changed {shown} and {more} more") } else { format!("; changed {shown}") }
    };
    let done = one_line(&format!("#{task} {title}{changed}"), FIELD);
    let next = last_tool.map(|line| one_line(&format!("last: {line}"), FIELD)).unwrap_or_default();
    let blocked = error.map(|line| one_line(&line, FIELD)).unwrap_or_default();
    Some((done, next, blocked))
}

/// How long ago, in words the digest reads.
pub(crate) fn ago(now: f64, at: f64) -> String {
    let seconds = (now - at).max(0.0) as u64;
    match seconds {
        0..=59 => "just now".into(),
        60..=3_599 => format!("{} min ago", seconds / 60),
        3_600..=86_399 => format!("{} h ago", seconds / 3_600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

/// The digest's block: the latest pulse per member, newest first.
pub(crate) fn digest(team: &Team, pulses: &[Pulse], now: f64) -> String {
    if pulses.is_empty() {
        return String::new();
    }
    let mut lines = vec!["What each member is doing (latest pulse):".to_owned()];
    for pulse in pulses.iter().take(20) {
        let task = if pulse.task > 0 { format!(" #{}", pulse.task) } else { String::new() };
        let mut line = format!("- {}{task}, {}", team.name_of(&pulse.member), ago(now, pulse.at));
        for (name, value) in [("done", &pulse.done), ("next", &pulse.next), ("blocked", &pulse.blocked)] {
            if !value.is_empty() {
                line.push_str(&format!("; {name}: {}", one_line(value, 200)));
            }
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// When a task closed, for its pulses' expiry: its newest decision, or None
/// while it is open.
pub(crate) fn closed_at(task: &Task) -> Option<f64> {
    if !task.status.finished() {
        return None;
    }
    task.attempts.iter().filter_map(|attempt| attempt.decided_at).fold(None, |best: Option<f64>, at| Some(best.map_or(at, |best| best.max(at))))
}

#[cfg(test)]
#[path = "tests/pulses.rs"]
mod tests;
