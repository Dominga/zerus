//! Owner 2026-10-31: every task carries a time estimate learned from this
//! computer's own accepted tasks. The history lives in one file under the
//! Zerus state folder, shared by every team and session here, and is kept per
//! role: tasks of the same role, the same agent and the same effort first, then
//! the role, then every role. A task the lead estimated keeps the lead's
//! number; a task without one shows "no estimate yet" until this role has
//! solved its first task.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

use super::model::{Task, TaskStatus, Team};
use crate::state::storage;

/// The per-computer history; the file name under the Zerus state folder.
pub(crate) const FILE: &str = "team-estimates.json";
/// Newest entries kept: a bounded file, oldest dropped first.
pub(crate) const MAX_ENTRIES: usize = 5000;
/// No estimate is longer than one day: a cancelled or forgotten task's time is
/// not a plan, and the owner asked for minutes, never a unit like a week.
pub(crate) const MAX_MINUTES: f64 = 1440.0;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct History {
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// Teams whose accepted tasks were imported once, so the estimates start
    /// from the work already accepted here.
    #[serde(default)]
    pub imported: Vec<String>,
}

/// One accepted task's working time, with what the next estimate compares
/// against: its role, agent, model, effort, the title's kind words and the
/// number of attempts it took.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Entry {
    pub role: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub kind: Vec<String>,
    pub minutes: f64,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub task: u32,
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub at: f64,
}

/// One task's estimate, or none until its role has a solved task.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Estimate {
    pub minutes: Option<f64>,
    /// `history` (same role, agent and effort), `role`, `lead` or `none`.
    pub source: &'static str,
}

impl Estimate {
    /// What the status and the hand-out say.
    pub(crate) fn text(self) -> String {
        match self.minutes {
            Some(minutes) => format!("{} min", minutes.round() as u64),
            None => "no estimate yet".into(),
        }
    }
}

/// The file under the Zerus state folder; None when the folder is unknown.
pub(crate) fn path() -> Option<PathBuf> {
    crate::config::Config::load()
        .ok()
        .map(|config| config.state.join(FILE))
}

/// Reads the history; a missing or malformed file is an empty one, never an
/// error of the team command.
pub(crate) fn load() -> History {
    path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(history: &History) -> Result<()> {
    let Some(path) = path() else { return Ok(()) };
    let text = serde_json::to_string_pretty(history).context("The estimate history is not JSON")?;
    storage::atomic_private(&path, &text).map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(())
}

fn trim(history: &mut History) {
    let excess = history.entries.len().saturating_sub(MAX_ENTRIES);
    if excess > 0 {
        history.entries.drain(..excess);
    }
}

/// The task's role: its assignee's role, or the role it names for the hand-out.
pub(crate) fn role_of(team: &Team, task: &Task) -> String {
    if task.assignee.is_empty() {
        return task.role.clone();
    }
    team.role_of(&task.assignee)
}

/// The assignee's agent, model and effort at the time the entry is read.
fn traits_of(team: &Team, task: &Task) -> (String, String, String) {
    let Some(member) = team.member(&task.assignee) else {
        return (String::new(), String::new(), String::new());
    };
    (member.agent.clone(), member.model.clone(), member.effort.clone())
}

/// The title's kind words: the first meaningful words, lowercased, which name
/// the kind of task (a save endpoint, a tests pass) without keeping the whole
/// title.
pub(crate) fn kind_words(title: &str) -> Vec<String> {
    const STOP: &[&str] = &["a", "an", "the", "and", "or", "for", "to", "of", "in", "on", "with", "without", "its", "this", "that"];
    title
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .map(str::to_lowercase)
        .filter(|word| word.len() >= 3 && !STOP.contains(&word.as_str()))
        .take(4)
        .collect()
}

/// The task's working time: the sum of its attempts from hand-out to report,
/// without the spans a question waited for the lead or the owner.
pub(crate) fn working_minutes(task: &Task) -> f64 {
    let mut seconds = 0.0;
    for attempt in &task.attempts {
        let Some(report) = &attempt.report else { continue };
        let start = attempt.delivered_at.or(attempt.started_at).unwrap_or(attempt.sent_at);
        if report.at <= start {
            continue;
        }
        let mut span = report.at - start;
        for question in &attempt.questions {
            if let Some(answered) = question.answered_at {
                let from = question.at.max(start);
                let to = answered.min(report.at).max(from);
                span -= (to - from).max(0.0);
            }
        }
        seconds += span.max(0.0);
    }
    (seconds / 60.0).max(1.0)
}

/// One accepted task's entry.
fn entry_for(team: &Team, task: &Task, at: f64) -> Entry {
    let (agent, model, effort) = traits_of(team, task);
    Entry {
        role: role_of(team, task),
        agent,
        model,
        effort,
        kind: kind_words(&task.title),
        minutes: working_minutes(task),
        attempts: u32::try_from(task.attempts.len()).unwrap_or(u32::MAX),
        task: task.id,
        team: team.id.clone(),
        at,
    }
}

/// Records every accepted task of `team` once: the import of this team's
/// history, and the record of each task accepted since. A task already in the
/// history is left alone.
pub(crate) fn record_accepted(team: &Team, now: f64) -> Result<()> {
    let mut history = load();
    if record_into(&mut history, team, now) {
        trim(&mut history);
        save(&history)?;
    }
    Ok(())
}

/// The in-memory half of `record_accepted`; true when anything changed.
fn record_into(history: &mut History, team: &Team, now: f64) -> bool {
    let mut changed = false;
    for task in &team.tasks {
        if task.status != TaskStatus::Accepted {
            continue;
        }
        if history
            .entries
            .iter()
            .any(|entry| entry.team == team.id && entry.task == task.id)
        {
            continue;
        }
        history.entries.push(entry_for(team, task, now));
        changed = true;
    }
    if !history.imported.iter().any(|id| id == &team.id) {
        history.imported.push(team.id.clone());
        changed = true;
    }
    changed
}

/// The median of the entries that match, rounded to a minute and kept within a
/// day; None when none matches.
fn median<'a>(entries: impl Iterator<Item = &'a Entry>) -> Option<f64> {
    let mut values: Vec<f64> = entries.map(|entry| entry.minutes).collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let middle = values.len() / 2;
    let median = if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    };
    Some(median.clamp(1.0, MAX_MINUTES).round())
}

/// The estimate for `task` from the history, or the lead's own.
pub(crate) fn estimate_from(history: &History, team: &Team, task: &Task) -> Estimate {
    if let Some(minutes) = task.estimate_minutes {
        return Estimate { minutes: Some(minutes), source: "lead" };
    }
    let role = role_of(team, task);
    let (agent, _model, effort) = traits_of(team, task);
    let same_role: Vec<&Entry> = history
        .entries
        .iter()
        .filter(|entry| !role.is_empty() && entry.role.eq_ignore_ascii_case(&role))
        .collect();
    // The same kind of task by the same role on the same agent and effort.
    if let Some(minutes) = median(
        same_role
            .iter()
            .copied()
            .filter(|entry| entry.agent.eq_ignore_ascii_case(&agent) && entry.effort.eq_ignore_ascii_case(&effort)),
    ) {
        return Estimate { minutes: Some(minutes), source: "history" };
    }
    // Then the role's own tasks.
    if let Some(minutes) = median(same_role.into_iter()) {
        return Estimate { minutes: Some(minutes), source: "role" };
    }
    // Then every solved task here.
    if let Some(minutes) = median(history.entries.iter()) {
        return Estimate { minutes: Some(minutes), source: "history" };
    }
    Estimate { minutes: None, source: "none" }
}

/// The minutes a running task has spent
/// The minutes a running task has spent against its estimate so far.
pub(crate) fn spent_minutes(task: &Task, now: f64) -> Option<f64> {
    if task.status.finished() {
        return None;
    }
    let open = task.attempts.iter().rev().find(|attempt| attempt.report.is_none())?;
    let start = open.delivered_at.or(open.started_at).unwrap_or(open.sent_at);
    (now > start).then(|| ((now - start) / 60.0).max(0.0))
}

/// What the hand-out says about the task's time: the estimate, or that it
/// fills in after this role's first solved task.
pub(crate) fn prompt_line(team: &Team, task: &Task) -> String {
    prompt_line_from(&load(), team, task)
}

/// `prompt_line` against a history the caller already read.
pub(crate) fn prompt_line_from(history: &History, team: &Team, task: &Task) -> String {
    let estimate = estimate_from(history, team, task);
    match estimate.minutes {
        Some(_) => {
            let source = match estimate.source {
                "history" => "this role's solved tasks",
                "role" => "this role's solved tasks",
                "lead" => "the lead's plan",
                _ => "experience",
            };
            format!("\nTime estimate: {} (from {}).\n", estimate.text(), source)
        }
        None => "\nTime estimate: no estimate yet; it fills in after this role solves its first task.\n".into(),
    }
}

/// Adds each task's estimate and, while one runs, the time spent against it to
/// the status-looking task objects of `value`. The history is refreshed first,
/// so a task accepted just now counts for the next estimate.
pub(crate) fn fill_status(value: &mut Value, team: &Team, now: f64) {
    let _ = record_accepted(team, now);
    let history = load();
    let Some(tasks) = value["tasks"].as_array_mut() else { return };
    for entry in tasks {
        let Some(id) = entry["id"].as_u64() else { continue };
        let Some(task) = team.task(u32::try_from(id).unwrap_or(0)) else { continue };
        let estimate = estimate_from(&history, team, task);
        entry["estimate_minutes"] = estimate.minutes.map_or(Value::Null, |minutes| json!(minutes));
        entry["estimate_source"] = json!(estimate.source);
        entry["estimate_text"] = json!(estimate.text());
        if let Some(spent) = spent_minutes(task, now) {
            entry["estimate_spent_minutes"] = json!(spent.round());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::teams::model::{Attempt, Member, Report, TeamStatus};

    fn member(id: &str, role: &str, agent: &str, effort: &str) -> Member {
        Member { id: id.into(), role: role.into(), agent: agent.into(), effort: effort.into(), ..Default::default() }
    }

    fn team() -> Team {
        Team {
            id: "t".into(),
            status: TeamStatus::Running,
            members: vec![member("backend", "Backend", "codex", "high"), member("qa", "Tester", "kimi", "low")],
            ..Default::default()
        }
    }

    fn accepted(id: u32, assignee: &str, sent: f64, reported: f64) -> Task {
        Task {
            id,
            title: format!("Save endpoint #{id}"),
            assignee: assignee.into(),
            status: TaskStatus::Accepted,
            attempts: vec![Attempt {
                n: 1,
                member: assignee.into(),
                sent_at: sent,
                delivered_at: Some(sent),
                report: Some(Report { text: "done".into(), at: reported, ..Default::default() }),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn the_working_time_sums_attempts_without_the_questions_wait() {
        let mut task = accepted(1, "backend", 100.0, 1000.0);
        task.attempts[0].questions = vec![crate::state::teams::model::Question {
            id: "q".into(),
            at: 200.0,
            answered_at: Some(500.0),
            ..Default::default()
        }];
        // 900 s of work, 300 s of it waited for the lead: 10 minutes.
        assert_eq!(working_minutes(&task), 10.0);
        // A second attempt adds its own span.
        task.attempts.push(Attempt {
            n: 2,
            member: "backend".into(),
            sent_at: 2000.0,
            delivered_at: Some(2000.0),
            report: Some(Report { text: "done".into(), at: 2600.0, ..Default::default() }),
            ..Default::default()
        });
        assert_eq!(working_minutes(&task), 20.0);
    }

    #[test]
    fn the_estimate_is_the_median_of_the_closest_solved_tasks() {
        let history = History {
            entries: vec![
                Entry { role: "Backend".into(), agent: "codex".into(), effort: "high".into(), minutes: 30.0, ..Default::default() },
                Entry { role: "Backend".into(), agent: "codex".into(), effort: "high".into(), minutes: 60.0, ..Default::default() },
                Entry { role: "Backend".into(), agent: "kimi".into(), effort: "low".into(), minutes: 200.0, ..Default::default() },
                Entry { role: "Tester".into(), agent: "kimi".into(), effort: "low".into(), minutes: 10.0, ..Default::default() },
            ],
            ..Default::default()
        };
        let team = team();
        let mut task = accepted(1, "backend", 0.0, 60.0);
        assert_eq!(estimate_from(&history, &team, &task), Estimate { minutes: Some(45.0), source: "history" });
        // Another agent on the same role: the role's median (45), not the
        // other agent's 200 alone.
        task.assignee = "qa".into();
        assert_eq!(estimate_from(&history, &team, &task), Estimate { minutes: Some(10.0), source: "history" });
        // An unestimated task of a role with no entry takes every role's median.
        task.assignee = "ghost".into();
        task.role = "Designer".into();
        assert_eq!(estimate_from(&history, &team, &task), Estimate { minutes: Some(45.0), source: "history" });
        // No history at all: no estimate yet, never a made-up unit.
        assert_eq!(estimate_from(&History::default(), &team, &task), Estimate { minutes: None, source: "none" });
        assert_eq!(Estimate { minutes: None, source: "none" }.text(), "no estimate yet");
        // The lead's own number wins.
        task.estimate_minutes = Some(12.0);
        assert_eq!(estimate_from(&history, &team, &task), Estimate { minutes: Some(12.0), source: "lead" });
    }

    #[test]
    fn an_accepted_task_is_recorded_once_and_the_file_stays_bounded() {
        let mut history = History::default();
        let team = team();
        let mut one = team.clone();
        one.tasks = vec![accepted(1, "backend", 0.0, 60.0), accepted(2, "qa", 0.0, 120.0)];
        assert!(record_into(&mut history, &one, 1000.0));
        assert_eq!(history.entries.len(), 2);
        assert!(history.imported.contains(&"t".to_owned()));
        // A second import writes nothing and keeps one entry per accepted task.
        assert!(!record_into(&mut history, &one, 2000.0));
        assert_eq!(history.entries.len(), 2);
        let entry = history.entries.iter().find(|entry| entry.task == 1).unwrap();
        assert_eq!((entry.role.as_str(), entry.agent.as_str(), entry.attempts), ("Backend", "codex", 1));
        assert_eq!(entry.kind, vec!["save".to_owned(), "endpoint".to_owned()]);
        for id in 3..5005 {
            history.entries.push(Entry { role: "Backend".into(), minutes: 30.0, task: id, ..Default::default() });
        }
        trim(&mut history);
        assert_eq!(history.entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn a_new_estimate_fills_in_after_the_first_solved_task() {
        let team = team();
        let mut task = accepted(1, "backend", 0.0, 60.0);
        let mut history = History::default();
        assert_eq!(estimate_from(&history, &team, &task).minutes, None);
        record_into(&mut history, &team, team.tasks.first().map_or(0.0, |_| 0.0) + 1000.0);
        // The task itself is now solved (accepted): the next read has a number.
        let mut fresh = team.clone();
        fresh.tasks = vec![accepted(2, "backend", 0.0, 600.0)];
        record_into(&mut history, &fresh, 2000.0);
        task.estimate_minutes = None;
        assert_eq!(estimate_from(&history, &team, &task).minutes, Some(10.0));
    }

    #[test]
    fn the_hand_out_says_the_estimate_or_that_it_fills_in() {
        let team = team();
        let task = accepted(1, "backend", 0.0, 60.0);
        assert!(prompt_line_from(&History::default(), &team, &task).contains("no estimate yet"));
        let history = History {
            entries: vec![Entry { role: "Backend".into(), agent: "codex".into(), effort: "high".into(), minutes: 45.0, ..Default::default() }],
            ..Default::default()
        };
        let line = prompt_line_from(&history, &team, &task);
        assert!(line.contains("45 min") && line.contains("role"), "{line}");
    }

    #[test]
    fn the_spent_time_counts_from_the_hand_out() {
        let mut task = accepted(1, "backend", 100.0, 160.0);
        assert_eq!(spent_minutes(&task, 400.0), None, "a reported attempt is not running");
        task.attempts[0].report = None;
        task.status = TaskStatus::InProgress;
        assert_eq!(spent_minutes(&task, 400.0), Some(5.0));
    }
}
