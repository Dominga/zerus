//! Task 135 (owner, 2026-11-07; docs/lore.md section 11.12): the handoff brief.
//!
//! A member's new session (a swap of agent, model or account, which ends the old
//! session; a session replaced; a session whose context was compacted or
//! cleared) starts with nothing the team knew. Zerus builds a brief from the
//! team's own record - the goal, what this seat holds, what is done, what is
//! open, what was decided, the last pulses - and gives it as the session's
//! first message. It is built from the team's data only (never from the old
//! session), bounded, and kept with the team: the latest brief of a task
//! survives a restart of the home computer (rule (f) of the working-tier
//! addendum), the older versions of a task go when it closes.
//!
//! A task may carry control questions, each with the strings an answer holds. A
//! fresh session given only the brief answers a question when the brief holds
//! one of its strings; `Built::misses` names the questions the brief cannot
//! answer, and the dispatcher journals them (`handoff_gap`) instead of sending a
//! brief nobody could use unnoticed.
use super::model::{Handoff, HandoffNeed, Task, TaskStatus, Team, TeamStatus};
use serde_json::Value;
use std::collections::BTreeSet;

/// The most UTF-16 units of a brief that goes out with a hand-out.
pub(crate) const BRIEF_UNITS: usize = 6000;
/// The most units of the recall a compacted session gets at its start: it
/// shares the start context (`iron_rules::MAX_START_CONTEXT`) with the rules.
pub(crate) const RECALL_UNITS: usize = 3000;
/// Versions of one task's brief kept, and briefs kept in all.
const VERSIONS_PER_TASK: usize = 20;
const KEPT: usize = 200;
/// Pulse lines in a brief.
const PULSES: usize = 8;

#[derive(Clone, Copy)]
pub(crate) enum Why<'a> {
    /// A swap: the new session of a seat that was handed to another agent,
    /// model or account.
    Swap(&'a HandoffNeed),
    /// Task 209: the owner or the lead relaunched the seat in a fresh
    /// conversation (not only after a failed start).
    Relaunch(&'a HandoffNeed),
    /// A compaction or a clear: the same seat, its context gone.
    Compaction,
}

/// One brief, with the task it is for and the control questions it misses.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Built {
    pub text: String,
    /// The task it is about (0 when the member holds none).
    pub task: u32,
    pub misses: Vec<String>,
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `text` cut to at most `max` UTF-16 units, ending in an ellipsis when cut.
pub(crate) fn clip(text: &str, max: usize) -> String {
    if units(text) <= max {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let n = c.len_utf16();
        if used + n > max.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += n;
    }
    out.push('\u{2026}');
    out
}

/// One line: whitespace folded, then cut.
fn one(text: &str, max: usize) -> String {
    clip(&text.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

/// The task a seat holds: the one its open attempt is on, else the lowest task
/// assigned to it that is not yet decided.
pub(crate) fn seat_task<'a>(team: &'a Team, member: &str) -> Option<&'a Task> {
    team.tasks
        .iter()
        .find(|task| task.open_attempt().is_some_and(|attempt| attempt.member == member))
        .or_else(|| {
            team.tasks.iter().find(|task| {
                task.assignee == member && matches!(task.status, TaskStatus::Pending | TaskStatus::Assigned | TaskStatus::InProgress)
            })
        })
}

fn status_word(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Draft => "draft",
        TaskStatus::Pending => "waiting",
        TaskStatus::Assigned => "assigned",
        TaskStatus::InProgress => "in progress",
        TaskStatus::Submitted => "reported, awaiting a decision",
        TaskStatus::InReview => "in review",
        TaskStatus::InTest => "in test",
        TaskStatus::Accepted => "accepted",
        TaskStatus::Rejected => "rejected",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn your_work(team: &Team, task: &Task) -> String {
    let mut text = format!("Your work: #{} {} ({})\n", task.id, one(&task.title, 160), status_word(task.status));
    if !task.description.trim().is_empty() {
        text += &format!("  {}\n", one(&task.description, 600));
    }
    if !task.criteria.is_empty() {
        text += "  Done when:\n";
        for criterion in task.criteria.iter().take(6) {
            text += &format!("  - {}\n", one(criterion, 200));
        }
    }
    if !task.output.is_empty() {
        let files: Vec<String> = task.output.iter().take(8).map(|file| one(file, 120)).collect();
        text += &format!("  Files: {}\n", files.join(", "));
    }
    if !task.deps.is_empty() {
        let deps: Vec<String> = task
            .deps
            .iter()
            .take(8)
            .map(|dep| {
                let state = team.task(dep.task).map(|found| status_word(found.status)).unwrap_or("unknown");
                format!("#{} ({state})", dep.task)
            })
            .collect();
        text += &format!("  Needs: {}\n", deps.join(", "));
    }
    if let Some(continuation) = &task.continuation {
        text += &format!("  The previous session's attempt {} left:\n", continuation.n);
        if !continuation.report.trim().is_empty() {
            text += &format!("  - partial report: {}\n", one(&continuation.report, 800));
        }
        for question in continuation.questions.iter().take(4) {
            text += &format!("  - open question: {}\n", one(question, 200));
        }
    }
    text
}

fn done(team: &Team) -> String {
    let mut accepted: Vec<&Task> = team.tasks.iter().filter(|task| task.status == TaskStatus::Accepted).collect();
    if accepted.is_empty() {
        return String::new();
    }
    accepted.sort_by_key(|task| std::cmp::Reverse(task.attempts.iter().filter_map(|attempt| attempt.decided_at).fold(0.0f64, f64::max) as i64));
    let mut text = format!("Done ({} accepted, newest first):\n", accepted.len());
    for task in accepted.iter().take(8) {
        let outcome = if task.decision_reason.trim().is_empty() { String::new() } else { format!(": {}", one(&task.decision_reason, 140)) };
        text += &format!("- #{} {}{}\n", task.id, one(&task.title, 100), outcome);
    }
    text
}

fn open(team: &Team, own: Option<u32>) -> String {
    let rest: Vec<&Task> = team
        .tasks
        .iter()
        .filter(|task| Some(task.id) != own && !matches!(task.status, TaskStatus::Accepted | TaskStatus::Cancelled | TaskStatus::Draft))
        .collect();
    if rest.is_empty() {
        return String::new();
    }
    let mut text = format!("Open ({} others):\n", rest.len());
    for task in rest.iter().take(10) {
        let holder = if task.assignee.is_empty() { task.role.clone() } else { team.role_of(&task.assignee) };
        text += &format!("- #{} {} ({}{})\n", task.id, one(&task.title, 100), status_word(task.status), if holder.is_empty() { String::new() } else { format!("; {holder}") });
    }
    text
}

fn decisions(team: &Team) -> String {
    let mut lines = Vec::new();
    if let Some(word) = &team.owner_word {
        lines.push(format!("- the owner's word on the plan: {}", one(&word.text, 200)));
    }
    for proposal in team.proposals.iter().rev().filter(|proposal| proposal["state"] != "open").take(5) {
        let reason = proposal["decision_reason"].as_str().or_else(|| proposal["reason"].as_str()).unwrap_or_default();
        lines.push(format!(
            "- {} {}: {}",
            proposal["kind"].as_str().unwrap_or("proposal"),
            proposal["state"].as_str().unwrap_or("decided"),
            one(reason, 160)
        ));
    }
    for task in team.tasks.iter().filter(|task| matches!(task.status, TaskStatus::Rejected | TaskStatus::Cancelled) && !task.decision_reason.trim().is_empty()).take(4) {
        lines.push(format!("- #{} {}: {}", task.id, status_word(task.status), one(&task.decision_reason, 160)));
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("Decided:\n{}\n", lines.join("\n"))
}

fn findings(team: &Team) -> String {
    let open: Vec<_> = team.findings.iter().filter(|finding| finding.state == "open").collect();
    if open.is_empty() {
        return String::new();
    }
    let mut text = format!("Open findings ({}):\n", open.len());
    for finding in open.iter().rev().take(5) {
        text += &format!("- {}: {} ({})\n", finding.id, one(&finding.title, 120), finding.severity);
    }
    text
}

/// The strings a control question's answer holds, found in `text`, case aside.
fn answers(text: &str, keys: &[String]) -> bool {
    let lower = text.to_lowercase();
    keys.iter().any(|key| lower.contains(&key.to_lowercase()))
}

/// The brief for `member`'s new session, at most `budget` UTF-16 units, and the
/// control questions of the task it holds that it cannot answer.
///
/// `pulses` are the last lines of what the seat did (see `pulse_lines`); the
/// builder reads nothing but the team and these.
pub(crate) fn build(team: &Team, member: &str, why: Why<'_>, pulses: &[String], budget: usize) -> Built {
    let role = team.role_of(member);
    let task = seat_task(team, member);
    let mut head = format!("[Zerus team \u{b7} from: Zerus \u{b7} handoff brief]\nYou are {role} in the team {} ({}), in a new session.\n", team.name, team.id);
    match why {
        Why::Swap(need) => {
            head += "Why you are reading this: this seat was swapped";
            if !need.from.is_empty() {
                head += &format!(" away from {}", one(&need.from, 120));
            }
            if !need.changes.is_empty() {
                head += &format!(" ({})", need.changes.iter().map(|change| one(change, 80)).collect::<Vec<_>>().join("; "));
            }
            head += ".\n";
        }
        Why::Relaunch(need) => {
            head += "Why you are reading this: this seat was relaunched in a fresh conversation";
            if !need.from.is_empty() {
                head += &format!(", its old session {}", one(&need.from, 120));
            }
            head += ".\n";
        }
        Why::Compaction => head += "Why you are reading this: your context was compacted or cleared.\n",
    }
    head += "It is built from the team's own record, not from the old session. Use it before you act, and do not ask the owner what it answers.\n";
    let goal = if team.goal.trim().is_empty() { String::new() } else { format!("Goal: {}\n", one(&team.goal, 700)) };
    let pulse = if pulses.is_empty() {
        String::new()
    } else {
        format!("Last pulses:\n{}\n", pulses.iter().take(PULSES).map(|line| format!("- {}", one(line, 160))).collect::<Vec<_>>().join("\n"))
    };
    let start = match why {
        Why::Swap(_) | Why::Relaunch(_) => "Next: your task follows this brief; do it as it says and report as it says.\n".to_owned(),
        Why::Compaction => format!(
            "Next: {{\"team\":\"{}\"}} | hgs team current --json shows your task and attempt; {}.\n",
            team.id,
            if member == team.lead { "hgs team inbox --json lists what waits for you" } else { "report as your hand-out says" }
        ),
    };
    let sections = [
        goal,
        task.map(|task| your_work(team, task)).unwrap_or_default(),
        done(team),
        open(team, task.map(|task| task.id)),
        decisions(team),
        findings(team),
        pulse,
        start,
    ];
    let mut text = clip(&head, budget);
    for section in sections.iter().filter(|section| !section.is_empty()) {
        let room = budget.saturating_sub(units(&text) + 1);
        if units(section) <= room {
            text += section;
        } else if room > 120 {
            // The first section that does not fit ends the brief, cut, so the
            // order of importance is the order of the text.
            text += &clip(section.trim_end(), room);
            text.push('\n');
            break;
        } else {
            break;
        }
    }
    let misses = task
        .map(|task| task.control.iter().filter(|control| !answers(&text, &control.keys)).map(|control| control.q.clone()).collect())
        .unwrap_or_default();
    // Task 184: the brief is working-tier text (docs/lore.md 11.9), kept in the
    // team record and sent as a message: a token or a password URL a member
    // pasted into a title, a description or a pulse is stored redacted.
    let text = super::secrets::redact_working(&text);
    Built { text, task: task.map(|task| task.id).unwrap_or(0), misses }
}

/// The last lines of what a seat did, from the team's journal items
/// (`Store::events`, newest first): handed out, delivered, reported, decided,
/// questions asked and answered, swaps. A line is `<minutes> ago <kind> #task`.
pub(crate) fn pulse_lines(team: &Team, member: &str, items: &[Value], now: f64) -> Vec<String> {
    let role = team.role_of(member);
    let mine: BTreeSet<u64> = team
        .tasks
        .iter()
        .filter(|task| task.assignee == member || task.attempts.iter().any(|attempt| attempt.member == member))
        .map(|task| u64::from(task.id))
        .collect();
    let mut lines = Vec::new();
    for item in items {
        let detail = &item["detail"];
        let named = ["member", "to", "by"].iter().any(|key| detail[*key].as_str() == Some(role.as_str()));
        let task = detail["task"].as_u64();
        let kind = item["kind"].as_str().unwrap_or_default();
        // Task 278: the lead's brief names what every member waits on.
        let job = matches!(kind, "job_started" | "job_ended");
        if !named && !task.is_some_and(|task| mine.contains(&task)) && !(job && member == team.lead) {
            continue;
        }
        if kind.is_empty() {
            continue;
        }
        let minutes = ((now - item["at"].as_f64().unwrap_or(now)) / 60.0).max(0.0) as u64;
        let ago = if minutes < 120 { format!("{minutes} min ago") } else if minutes < 2880 { format!("{} h ago", minutes / 60) } else { format!("{} d ago", minutes / 1440) };
        if job {
            let who = detail["member"].as_str().unwrap_or_default();
            let what = detail["what"].as_str().unwrap_or_default();
            let since = super::look::clock(detail["since"].as_f64().unwrap_or(0.0));
            lines.push(if kind == "job_started" {
                format!("{ago} {who} started waiting for {what}{}", if since.is_empty() { String::new() } else { format!(" (since {since})") })
            } else {
                format!("{ago} {who}: {what} ended")
            });
            if lines.len() == PULSES {
                break;
            }
            continue;
        }
        lines.push(match task {
            Some(task) => format!("{ago} {} #{task}", kind.replace('_', " ")),
            None => format!("{ago} {}", kind.replace('_', " ")),
        });
        if lines.len() == PULSES {
            break;
        }
    }
    lines
}

/// The hand-out `text` with the brief after its first line. Task 134's doorbell
/// takes its header (`from`, `task`, `attempt`) from the first line of a
/// hand-out's text, so the brief goes under that line, never before it.
pub(crate) fn within(text: &str, brief: &str) -> String {
    match text.split_once('\n') {
        Some((first, rest)) => format!("{first}\n{brief}\n{rest}"),
        None => format!("{text}\n{brief}"),
    }
}

/// Keeps `built` with the team as the next version of its task's brief.
pub(crate) fn record(team: &mut Team, built: &Built, member: &str, why: &str, at: f64) {
    let version = team.handoffs.iter().filter(|handoff| handoff.task == built.task).map(|handoff| handoff.version).max().unwrap_or(0) + 1;
    let role = team.role_of(member);
    team.handoffs.push(Handoff { task: built.task, version, member: role, why: why.to_owned(), at, text: built.text.clone(), misses: built.misses.clone() });
    prune(team);
}

/// The brief of a task as the dispatcher last gave it, or the given version.
pub(crate) fn find(team: &Team, task: u32, version: Option<u32>) -> Option<&Handoff> {
    let of_task = team.handoffs.iter().filter(|handoff| handoff.task == task);
    match version {
        Some(version) => of_task.into_iter().find(|handoff| handoff.version == version),
        None => of_task.max_by_key(|handoff| handoff.version),
    }
}

/// The bounds of 11.4 for the briefs a team keeps: the older versions of a task
/// that has closed (and of every task once the team is no longer running) are
/// dropped, at most 20 versions of one task stay, at most 200 briefs in all; the
/// latest brief of a task is never dropped by a bound.
pub(crate) fn prune(team: &mut Team) {
    let ended = !matches!(team.status, TeamStatus::Draft | TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused);
    let closed: BTreeSet<u32> = team
        .tasks
        .iter()
        .filter(|task| ended || matches!(task.status, TaskStatus::Accepted | TaskStatus::Cancelled))
        .map(|task| task.id)
        .collect();
    let latest: std::collections::BTreeMap<u32, u32> = team.handoffs.iter().fold(Default::default(), |mut latest, handoff| {
        let at = latest.entry(handoff.task).or_insert(0);
        *at = (*at).max(handoff.version);
        latest
    });
    let is_latest = |handoff: &Handoff| latest.get(&handoff.task) == Some(&handoff.version);
    team.handoffs.retain(|handoff| is_latest(handoff) || !closed.contains(&handoff.task));
    // At most 20 versions of a task: the oldest go.
    let mut per_task: std::collections::BTreeMap<u32, usize> = Default::default();
    for handoff in &team.handoffs {
        *per_task.entry(handoff.task).or_default() += 1;
    }
    let mut over: std::collections::BTreeMap<u32, usize> = per_task.into_iter().filter(|(_, count)| *count > VERSIONS_PER_TASK).map(|(task, count)| (task, count - VERSIONS_PER_TASK)).collect();
    team.handoffs.retain(|handoff| {
        if is_latest(handoff) {
            return true;
        }
        match over.get_mut(&handoff.task) {
            Some(extra) if *extra > 0 => {
                *extra -= 1;
                false
            }
            _ => true,
        }
    });
    // At most 200 in all: the oldest that are not a task's latest go.
    let mut extra = team.handoffs.len().saturating_sub(KEPT);
    team.handoffs.retain(|handoff| {
        if extra > 0 && !is_latest(handoff) {
            extra -= 1;
            false
        } else {
            true
        }
    });
}
