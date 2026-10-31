//! U62, #760 (re-added #722; owner, 2026-10-30): the merge queue. With the
//! project's `[merge_queue] enabled`, the lead's (or the owner's) accept of a
//! report does not decide it at once: Zerus merges the commit its author
//! reported into the lead's branch, runs the task's affected tests on the
//! merged commit through the test queue, and then keeps the merge and accepts
//! the task, or rolls the merge back and sends the task back to its author
//! with the failure. A conflict goes back the same way with its files. One
//! merge is under test at a time, so a rollback never drops another's work,
//! and a phase integration only checks what came after the last tested merge.

use super::model::{AttemptState, Decider, MergeEntry, MergeLog, Task, TaskStatus, Team};
use super::store::Event;
use super::test_queue::{Request, TestStore};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// How long one git step of the queue may take.
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// The project turned the queue on (the owner may override it in the team).
pub(crate) fn enabled(team: &Team) -> bool {
    if team.folder.is_empty() {
        return false;
    }
    crate::test_discipline::config::load(Path::new(&team.folder), team.settings.tests.as_ref())
        .is_ok_and(|loaded| loaded.active() && loaded.config.merge_queue.enabled)
}

/// The queue entry an accept of task `id` makes, or None when the accept
/// decides at once: the queue is off, the task is a check, a phase build or a
/// sub-team's, or its author worked in the lead's own folder (nothing to merge).
pub(crate) fn entry(team: &Team, id: u32, by: &str, by_owner: bool, reason: &str) -> Option<MergeEntry> {
    let task = team.task(id)?;
    if task.is_check() || task.phase_build || task.subteam.is_some() || !enabled(team) {
        return None;
    }
    let attempt = task.attempts.iter().rev().find(|attempt| attempt.state == AttemptState::Reported)?;
    if attempt.folder_head.is_empty() {
        return None;
    }
    let member = team.member(&attempt.member)?;
    let folder = super::dispatcher::member_folder_path(team, member).ok()?;
    if folder.is_empty() || super::dispatcher::same_path(&folder, &team.folder) {
        return None;
    }
    Some(MergeEntry {
        state: "queued".into(),
        author: attempt.member.clone(),
        commit: attempt.folder_head.clone(),
        branch: attempt.folder_branch.clone(),
        folder,
        at: super::super::now(),
        by: by.to_owned(),
        by_owner,
        reason: reason.to_owned(),
        ..MergeEntry::default()
    })
}

/// What merging an entry into the lead's folder did.
#[derive(Debug, PartialEq)]
pub(crate) enum Merged {
    /// The lead branch moved from `prev` to `merged` (equal: it had it already).
    Done { prev: String, merged: String },
    /// The merge stopped on these files and was aborted.
    Conflict(Vec<String>),
    /// The lead's folder cannot take it now (uncommitted changes); it waits.
    Busy(String),
    Failed(String),
}

fn git(folder: &str, args: &[&str]) -> super::templates::Outcome {
    let hooks = format!("core.hooksPath={}", if cfg!(windows) { "NUL" } else { "/dev/null" });
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(folder)
        .args(["-c", hooks.as_str(), "-c", "core.fsmonitor=false", "-c", "commit.gpgsign=false", "-c", "gpg.program="])
        .arg("--no-optional-locks")
        .args(args);
    super::templates::run_tree(&mut command, GIT_TIMEOUT, None)
}

/// A git read: its stdout alone (warnings on stderr say nothing), or None.
fn git_text(folder: &str, args: &[&str]) -> Option<String> {
    let hooks = format!("core.hooksPath={}", if cfg!(windows) { "NUL" } else { "/dev/null" });
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["-c", hooks.as_str(), "-c", "core.fsmonitor=false", "--no-optional-locks"])
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Merges the reported commit into the lead's folder, as the lead's own
/// merges do: hooks and the repository's code-running config off, a clone's
/// branch fetched first; a conflict is aborted and its files named.
pub(crate) fn merge(lead: &str, entry: &MergeEntry, message: &str) -> Merged {
    let listing = git_text(lead, &["config", "--list", "--show-scope", "--name-only"]).unwrap_or_default();
    if let Some(reason) = super::dispatcher::refused_repository_config(&listing) {
        return Merged::Failed(format!("This repository {reason}; Zerus does not merge in it"));
    }
    match git_text(lead, &["status", "--porcelain", "--untracked-files=no"]) {
        Some(status) if !status.is_empty() => return Merged::Busy("the lead's folder has uncommitted changes".into()),
        None => return Merged::Failed("the lead's folder was not read".into()),
        _ => {}
    }
    let Some(prev) = git_text(lead, &["rev-parse", "HEAD"]) else { return Merged::Failed("the lead branch has no commit".into()) };
    if !super::dispatcher::same_repository(lead, &entry.folder, Instant::now() + GIT_TIMEOUT) {
        let source = if entry.branch.is_empty() { "HEAD" } else { entry.branch.as_str() };
        let fetched = git(lead, &["fetch", "--no-tags", "--quiet", &entry.folder, source]);
        if fetched.exit != Some(0) {
            return Merged::Failed(format!("The author's branch was not fetched from {}:\n{}", entry.folder, fetched.tail));
        }
    }
    if git(lead, &["cat-file", "-e", &format!("{}^{{commit}}", entry.commit)]).exit != Some(0) {
        return Merged::Failed(format!("The reported commit {} is not in the repository", entry.commit));
    }
    if git(lead, &["merge-base", "--is-ancestor", &entry.commit, "HEAD"]).exit == Some(0) {
        return Merged::Done { merged: prev.clone(), prev };
    }
    let outcome = git(lead, &["merge", "--no-edit", "-m", message, &entry.commit]);
    if outcome.exit == Some(0) {
        let merged = git_text(lead, &["rev-parse", "HEAD"]).unwrap_or_default();
        return Merged::Done { prev, merged };
    }
    let files: Vec<String> = git_text(lead, &["diff", "--name-only", "--diff-filter=U"])
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .filter(|line| !line.is_empty())
        .collect();
    let _ = git(lead, &["merge", "--abort"]);
    if files.is_empty() {
        Merged::Failed(format!("The merge failed:\n{}", outcome.tail))
    } else {
        Merged::Conflict(files)
    }
}

/// The task's affected tests on the merged range, as test queue requests: the
/// project's map over `prev..merged`, each run's command in its folder.
pub(crate) fn affected_requests(team: &Team, id: u32, prev: &str, merged: &str, now: f64) -> anyhow::Result<Vec<Request>> {
    use crate::test_discipline::{affected, config, durations, map};
    let lead = Path::new(&team.folder);
    let loaded = config::load(lead, team.settings.tests.as_ref())?;
    let config = &loaded.config;
    let changed = git_text(&team.folder, &["diff", "--name-only", "--no-renames", prev, merged]).unwrap_or_default();
    let files: Vec<String> = changed.lines().map(str::to_owned).filter(|line| !line.is_empty()).collect();
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let committed = std::fs::read_to_string(lead.join(&config.map.file)).ok().and_then(|text| serde_json::from_str::<map::TestMap>(&text).ok());
    let touched: std::collections::BTreeSet<String> = config
        .providers
        .iter()
        .filter(|provider| committed.is_none() || files.iter().any(|file| map::in_area(provider, file) || provider.paths.iter().any(|rule| crate::test_discipline::glob::any(&rule.files, file))))
        .map(|provider| provider.name.clone())
        .collect();
    let tracked = map::project_files(lead)?;
    let test_map = map::generate_only(lead, config, &tracked, committed.as_ref(), Some(&touched))?;
    let durations = durations::Durations::read(&lead.join(&config.map.durations)).unwrap_or_default();
    let selection = affected::select(lead, config, &test_map, &durations, &files, &Default::default(), false);
    let mut requests = Vec::new();
    for run in &selection.runs {
        let Some(provider) = config.providers.iter().find(|provider| provider.name == run.provider) else { continue };
        let command = if run.cwd == "." {
            run.command.clone()
        } else if cfg!(windows) {
            format!("cd /d \"{}\" && {}", run.cwd, run.command)
        } else {
            format!("cd '{}' && {}", run.cwd, run.command)
        };
        let budget = if provider.budget_minutes > 0.0 {
            provider.budget_minutes
        } else if config.budgets.suite_minutes > 0.0 {
            config.budgets.suite_minutes
        } else {
            60.0
        };
        requests.push(Request {
            id: uuid::Uuid::new_v4().to_string(),
            team: team.id.clone(),
            suite: run.provider.clone(),
            task: Some(id),
            targets: run.targets.clone(),
            commit: merged.to_owned(),
            // Never reused: the merge queue measures the merged commit.
            input_hash: String::new(),
            asked_by: "merge queue".into(),
            forced: true,
            at: now,
            seconds: run.seconds.unwrap_or(0.0),
            budget_minutes: budget,
            resources: provider.resources.clone(),
            command,
            platforms: provider.platform_classes(),
        });
    }
    Ok(requests)
}

/// One step of the queue for a team: the merge under test decides when all its
/// results are in; else the oldest queued entry merges. True when it changed
/// something.
pub(crate) fn step(team: &mut Team, database: &TestStore, now: f64, events: &mut Vec<Event>) -> anyhow::Result<bool> {
    if let Some(id) = team.tasks.iter().find(|task| task.merge.as_ref().is_some_and(|entry| entry.state == "testing")).map(|task| task.id) {
        let entry = team.task(id).and_then(|task| task.merge.clone()).unwrap_or_default();
        let mut results = Vec::new();
        for request in &entry.requests {
            match database.result_of(request)? {
                Some(record) => results.push(record),
                None => return Ok(false),
            }
        }
        let failed: Vec<String> = results
            .iter()
            .filter(|record| !matches!(record.status.as_str(), "passed" | "flaky" | "reused"))
            .map(|record| format!("{} {} (record {})", record.suite, record.status, record.id))
            .collect();
        if failed.is_empty() {
            keep(team, id, &entry, results.iter().map(|record| record.suite.clone()).collect(), now, events)?;
        } else {
            let excerpt = results
                .iter()
                .find(|record| !matches!(record.status.as_str(), "passed" | "flaky" | "reused"))
                .map(|record| record.excerpt.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();
            let rolled = roll_back(&team.folder, &entry);
            return_to_author(
                team,
                id,
                &format!(
                    "The merge queue merged this into the lead branch, and its affected tests failed on the merged commit: {}. {rolled}\n{excerpt}",
                    failed.join("; ")
                ),
                events,
            );
        }
        return Ok(true);
    }
    let Some(id) = team
        .tasks
        .iter()
        .filter_map(|task| task.merge.as_ref().filter(|entry| entry.state == "queued").map(|entry| (entry.at, task.id)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
    else {
        return Ok(false);
    };
    let entry = team.task(id).and_then(|task| task.merge.clone()).unwrap_or_default();
    let title = team.task(id).map(|task| task.title.clone()).unwrap_or_default();
    let short: String = entry.commit.chars().take(8).collect();
    let message = format!("Merge accepted work from {} ({short}) (#{id}): {title}", team.name_of(&entry.author));
    match merge(&team.folder, &entry, &message) {
        Merged::Busy(note) => {
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                if found.note == note {
                    return Ok(false);
                }
                found.note = note;
            }
            Ok(true)
        }
        Merged::Conflict(files) => {
            return_to_author(
                team,
                id,
                &format!(
                    "The merge into the lead branch conflicts in: {}. Merge the lead branch into your folder (the git-merge-lead template), resolve these files, commit and report again.",
                    files.join(", ")
                ),
                events,
            );
            events.push(Event::new("merge_conflict", json!({"task":id,"files":files})));
            Ok(true)
        }
        Merged::Failed(text) => {
            return_to_author(team, id, &format!("The merge queue could not merge it: {text}"), events);
            Ok(true)
        }
        Merged::Done { prev, merged } => {
            let requests = if prev == merged { Vec::new() } else { affected_requests(team, id, &prev, &merged, now).unwrap_or_default() };
            for request in &requests {
                database.enqueue(request)?;
            }
            events.push(Event::new("merged", json!({"task":id,"prev":prev,"merged":merged,"tests":requests.len()})));
            if requests.is_empty() {
                let entry = MergeEntry { prev, merged, ..entry };
                keep(team, id, &entry, Vec::new(), now, events)?;
            } else if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                found.state = "testing".into();
                found.prev = prev;
                found.merged = merged;
                found.note.clear();
                found.requests = requests.iter().map(|request| request.id.clone()).collect();
            }
            Ok(true)
        }
    }
}

/// The merge stays: the range is tested, and the task is accepted as the
/// accept that queued it asked.
fn keep(team: &mut Team, id: u32, entry: &MergeEntry, suites: Vec<String>, now: f64, events: &mut Vec<Event>) -> anyhow::Result<()> {
    if !entry.merged.is_empty() {
        team.merge_tested = entry.merged.clone();
        team.merge_log.push(MergeLog { task: id, prev: entry.prev.clone(), merged: entry.merged.clone(), at: now, suites });
        let extra = team.merge_log.len().saturating_sub(50);
        team.merge_log.drain(0..extra);
    }
    let actor = if entry.by_owner { super::ops::Actor::Human } else { super::ops::Actor::Member(team.lead.clone()) };
    let input = json!({"task":id,"reason":entry.reason,"merged":true});
    super::ops::apply(team, "accept", &input, &actor, &Default::default(), events)?;
    Ok(())
}

/// Resets the lead branch to before the merge, when it is still the merge.
fn roll_back(lead: &str, entry: &MergeEntry) -> String {
    match git_text(lead, &["rev-parse", "HEAD"]) {
        Some(head) if head == entry.merged && !entry.prev.is_empty() => {
            if git(lead, &["reset", "--hard", "--quiet", &entry.prev]).exit == Some(0) {
                "The merge was rolled back.".into()
            } else {
                format!("Rolling the merge back failed; revert {} by hand.", entry.merged)
            }
        }
        _ => format!("The lead branch moved since; revert {} by hand.", entry.merged),
    }
}

/// The task goes back to its author with why: its report is rejected by Zerus
/// and it waits to be handed out to the same member again.
pub(crate) fn return_to_author(team: &mut Team, id: u32, reason: &str, events: &mut Vec<Event>) {
    let Some(task) = team.task_mut(id) else { return };
    let author = task.merge.take().map(|entry| entry.author).unwrap_or_else(|| task.assignee.clone());
    if let Some(attempt) = task.attempts.iter_mut().rev().find(|attempt| attempt.state == AttemptState::Reported) {
        attempt.state = AttemptState::Rejected;
    }
    task.status = TaskStatus::Pending;
    task.assignee = author;
    task.pinned = true;
    task.decided_by = Some(Decider::Zerus);
    task.decision_reason = reason.chars().take(4000).collect();
    events.push(Event::new("merge_returned", json!({"task":id,"reason":task.decision_reason})));
}

/// What a phase integration still has to check: the lead-branch range after
/// the queue's last tested merge (None when the queue tested nothing).
pub(crate) fn untested_since(team: &Team) -> Option<String> {
    (!team.merge_tested.is_empty()).then(|| team.merge_tested.clone())
}

/// A queued or tested entry, for status.
pub(crate) fn view(task: &Task) -> Option<Value> {
    task.merge.as_ref().map(|entry| json!({"state":entry.state,"author":entry.author,"commit":entry.commit,"merged":entry.merged,"tests":entry.requests.len(),"note":entry.note}))
}

#[cfg(test)]
#[path = "merge_queue_tests.rs"]
mod tests;
