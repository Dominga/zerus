//! U62, #760 (re-added #722; owner, 2026-10-30): the merge queue. With the
//! project's `[merge_queue] enabled`, the lead's (or the owner's) accept of a
//! report does not decide it at once: Zerus merges the commit its author
//! reported into the lead's branch, runs the task's affected tests on the
//! merged commit through the test queue, and then keeps the merge and accepts
//! the task, or rolls the merge back and sends the task back to its author
//! with the failure. A conflict goes back the same way with its files. One
//! merge is under test at a time, so a rollback never drops another's work,
//! and a phase integration only checks what came after the last tested merge.

use super::model::{AttemptState, Decider, MergeEntry, MergeLog, OpenNotRun, Task, TaskStatus, Team};
use super::store::Event;
use super::test_queue::{Request, TestStore};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// How long one git step of the queue may take.
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// A bounded per-suite tested-point map; the merge log is bounded too.
const MERGE_SUITES: usize = 64;

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

/// The requests a merge queues, and the suites of another platform it records
/// as not run here because no test runner of their class exists.
#[derive(Debug, Default)]
pub(crate) struct Affected {
    pub requests: Vec<Request>,
    pub not_run: Vec<String>,
}

/// The task's affected tests on the merged range, as test queue requests: the
/// project's map over `prev..merged`, each run's command in its folder. A suite
/// whose declared platforms exclude this computer is queued only when a
/// configured test runner of its class exists; otherwise it is named as not run
/// here and the merge decides on the suites that did run.
pub(crate) fn affected_requests(
    team: &Team,
    id: u32,
    prev: &str,
    merged: &str,
    run_at: &str,
    now: f64,
    host: &[String],
    runners: &[super::test_runners::Runner],
) -> anyhow::Result<Affected> {
    use crate::test_discipline::{affected, config, durations, map};
    let lead = Path::new(&team.folder);
    let loaded = config::load(lead, team.settings.tests.as_ref())?;
    let config = &loaded.config;
    let changed = git_text(&team.folder, &["diff", "--name-only", "--no-renames", prev, merged]).unwrap_or_default();
    let files: Vec<String> = changed.lines().map(str::to_owned).filter(|line| !line.is_empty()).collect();
    if files.is_empty() {
        return Ok(Affected::default());
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
    let mut not_run = Vec::new();
    for run in &selection.runs {
        let Some(provider) = config.providers.iter().find(|provider| provider.name == run.provider) else { continue };
        let platforms = provider.platform_classes();
        if !super::test_runners::runs_here(&platforms, host)
            && !runners.iter().any(|runner| platforms.contains(&runner.platform))
        {
            not_run.push(format!("not run here: {} runs on {}; no runner of that class", run.provider, platforms.join(" or ")));
            continue;
        }
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
            commit: run_at.to_owned(),
            // Never reused: the merge queue measures the merged commit.
            input_hash: String::new(),
            asked_by: "merge queue".into(),
            forced: true,
            at: now,
            seconds: run.seconds.unwrap_or(0.0),
            budget_minutes: budget,
            resources: provider.resources.clone(),
            command,
            platforms,
        });
    }
    Ok(Affected { requests, not_run })
}

/// One step of the queue for a team: the merge under test decides when all its
/// results are in; else the oldest queued entry merges. `host` is this
/// computer's test classes and `runners` the owner's configured test runners;
/// a suite of another class without a runner is recorded as not run here. True
/// when it changed something.
pub(crate) fn step(
    team: &mut Team,
    database: &TestStore,
    now: f64,
    events: &mut Vec<Event>,
    host: &[String],
    runners: &[super::test_runners::Runner],
) -> anyhow::Result<bool> {
    step_with(team, database, now, events, || Ok((host.to_vec(), runners.to_vec())))
}

/// The step whose host classes and configured runners are read only once a
/// merge has completed: a merge that only waits, a conflict or a busy lead
/// folder never reads them.
pub(crate) fn step_with<F>(
    team: &mut Team,
    database: &TestStore,
    now: f64,
    events: &mut Vec<Event>,
    affected_hosts: F,
) -> anyhow::Result<bool>
where
    F: FnOnce() -> anyhow::Result<(Vec<String>, Vec<super::test_runners::Runner>)>,
{
    for task in &mut team.tasks {
        if let Some(entry) = task.merge.as_mut() {
            if entry.state == "waiting" || entry.state == "reverting" {
                entry.state = "queued".into();
                if entry.note.is_empty() {
                    entry.note = "A saved waiting merge was requeued for the current rule".into();
                }
                entry.retry = false;
                entry.requests.clear();
                entry.not_run.clear();
            }
        }
    }
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
            let rolled = roll_back(&team.folder, &entry).note();
            let mut not_run = entry.not_run.clone();
            for message in pending_not_run(team) {
                push_not_run(&mut not_run, &message);
            }
            let note = not_run_note(&not_run);
            let note = if note.is_empty() { String::new() } else { format!("\n{note}") };
            return_to_author(
                team,
                id,
                &format!(
                    "The merge queue merged this into the lead branch, and its affected tests failed on the merged commit: {}. {rolled}\n{excerpt}{note}",
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
    let mut entry = team.task(id).and_then(|task| task.merge.clone()).unwrap_or_default();
    let (head, map) = retry_stamps(team);
    if !entry.merged.is_empty() {
        let reachable = ancestry(&team.folder, &entry.merged);
        if reachable != Ancestry::No {
            return resolve_reachable(team, database, id, entry, &head, reachable, now, events, affected_hosts);
        }
        // A hand reset or revert removed the merge: the commit can be merged again.
        if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
            found.note.clear();
            found.merged.clear();
            found.prev.clear();
            found.retry = false;
        }
        entry = team.task(id).and_then(|task| task.merge.clone()).unwrap_or_default();
    }
    if !entry.note.is_empty() && !entry.retry && !entry.wait_head.is_empty() && entry.wait_head == head && entry.wait_map == map {
        return Ok(false);
    }
    if entry.retry {
        if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
            found.retry = false;
        }
    }
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
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                found.prev = prev.clone();
                found.merged = merged.clone();
            }
            let affected = if prev == merged {
                Affected::default()
            } else {
                let (host, runners) = match affected_hosts() {
                    Ok(pair) => pair,
                    Err(error) => {
                        let reason = format!("The test runners could not be read: {error}");
                        return requeue_after_error(team, id, &prev, &merged, &reason, false, events);
                    }
                };
                match affected_requests(team, id, &prev, &merged, &merged, now, &host, &runners) {
                    Ok(affected) => affected,
                    Err(error) => {
                        let reason = format!("The affected tests could not be selected: {error}");
                        return requeue_after_error(team, id, &prev, &merged, &reason, true, events);
                    }
                }
            };
            for request in &affected.requests {
                database.enqueue(request)?;
            }
            events.push(Event::new("merged", json!({"task":id,"prev":prev,"merged":merged,"tests":affected.requests.len(),"not_run":affected.not_run.clone()})));
            if !affected.not_run.is_empty() {
                events.push(Event::new("merge_not_run", json!({"task":id,"suites":affected.not_run.clone()})));
            }
            if affected.requests.is_empty() {
                let entry = MergeEntry { prev, merged, not_run: affected.not_run, ..entry };
                keep(team, id, &entry, Vec::new(), now, events)?;
            } else if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                found.state = "testing".into();
                found.note.clear();
                found.retry = false;
                found.wait_head.clear();
                found.wait_map.clear();
                found.selection_failed = 0;
                found.prev = prev;
                found.merged = merged;
                found.requests = affected.requests.iter().map(|request| request.id.clone()).collect();
                found.not_run = affected.not_run;
            }
            Ok(true)
        }
    }
}

/// Whether `commit` is reachable from the lead branch's HEAD; only exit 1 is a
/// definite no. A failed git call is unknown and never means "gone".
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ancestry {
    Yes,
    No,
    Unknown,
}

fn ancestry(folder: &str, commit: &str) -> Ancestry {
    if commit.is_empty() {
        return Ancestry::Unknown;
    }
    match git(folder, &["merge-base", "--is-ancestor", commit, "HEAD"]).exit {
        Some(0) => Ancestry::Yes,
        Some(1) => Ancestry::No,
        _ => Ancestry::Unknown,
    }
}

/// A stable fingerprint of the project's tests map and the team's override.
fn tests_stamp(team: &Team) -> String {
    let text = std::fs::read_to_string(Path::new(&team.folder).join(".zerus/tests.toml")).unwrap_or_default();
    let settings = team.settings.tests.as_ref().map(|value| value.to_string()).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(text.as_bytes());
    digest.update(settings.as_bytes());
    format!("{:x}", digest.finalize())
}

/// The HEAD and tests-map fingerprint a queued retry remembers.
fn retry_stamps(team: &Team) -> (String, String) {
    (git_text(&team.folder, &["rev-parse", "HEAD"]).unwrap_or_default(), tests_stamp(team))
}

/// Store a queued entry's reason once; a changed HEAD or map refreshes the
/// stamps without repeating the event.
fn set_queue_note(team: &mut Team, id: u32, reason: &str, kind: &'static str, events: &mut Vec<Event>) -> bool {
    let (head, map) = retry_stamps(team);
    let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) else { return false };
    if found.state == "queued" && found.note == reason && found.wait_head == head && found.wait_map == map {
        return false;
    }
    let announce = found.state != "queued" || found.note != reason;
    found.state = "queued".into();
    found.note = reason.to_owned();
    found.wait_head = head;
    found.wait_map = map;
    found.retry = false;
    found.requests.clear();
    found.not_run.clear();
    if announce {
        events.push(Event::new(kind, json!({"task":id,"reason":reason})));
    }
    true
}

/// An untested merge still reachable from HEAD: never merge it again. Try the
/// rollback while HEAD is still that merge; once HEAD moved, run the recorded
/// prev..merged tests on HEAD. A successful run keeps the merge normally; a
/// selection error returns the task and records the open range.
fn resolve_reachable<F>(
    team: &mut Team,
    database: &TestStore,
    id: u32,
    entry: MergeEntry,
    head: &str,
    reachable: Ancestry,
    now: f64,
    events: &mut Vec<Event>,
    affected_hosts: F,
) -> anyhow::Result<bool>
where
    F: FnOnce() -> anyhow::Result<(Vec<String>, Vec<super::test_runners::Runner>)>,
{
    if head == entry.merged {
        return match roll_back(&team.folder, &entry) {
            Rollback::Done => {
                let head = git_text(&team.folder, &["rev-parse", "HEAD"]).unwrap_or_default();
                let map = tests_stamp(team);
                if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                    found.state = "queued".into();
                    found.note = "The merge was rolled back after its tests could not be selected; retry after a change or the lead's action.".into();
                    found.wait_head = head.clone();
                    found.wait_map = map;
                    found.retry = false;
                    found.requests.clear();
                    found.not_run.clear();
                    found.prev = head;
                    found.merged.clear();
                }
                events.push(Event::new("merge_requeued", json!({"task":id,"reason":"rolled back"})));
                Ok(true)
            }
            Rollback::Failed(note) => {
                let reason = format!("The merge could not be rolled back: {note}");
                Ok(set_queue_note(team, id, &reason, "merge_rollback_blocked", events))
            }
        };
    }
    if reachable == Ancestry::Unknown {
        let reason = format!("git could not tell whether the merge {} is in the lead branch; the merge waits: {head}", entry.merged);
        return Ok(set_queue_note(team, id, &reason, "merge_rollback_blocked", events));
    }
    let (host, runners) = match affected_hosts() {
        Ok(pair) => pair,
        Err(error) => {
            let reason = format!("The test runners could not be read: {error}; the merge waits on HEAD {head}");
            return Ok(set_queue_note(team, id, &reason, "merge_rollback_blocked", events));
        }
    };
    let affected = match affected_requests(team, id, &entry.prev, &entry.merged, head, now, &host, &runners) {
        Ok(affected) => affected,
        Err(error) => {
            let reason = format!("The affected tests could not be selected: {error}");
            return_to_author(team, id, &reason, events);
            events.push(Event::new("merge_affected_failed", json!({"task":id,"reason":reason})));
            return Ok(true);
        }
    };
    for request in &affected.requests {
        database.enqueue(request)?;
    }
    events.push(Event::new(
        "merged",
        json!({"task":id,"prev":entry.prev,"merged":entry.merged,"tests":affected.requests.len(),"not_run":affected.not_run.clone()}),
    ));
    if !affected.not_run.is_empty() {
        events.push(Event::new("merge_not_run", json!({"task":id,"suites":affected.not_run.clone()})));
    }
    if affected.requests.is_empty() {
        keep(
            team,
            id,
            &MergeEntry { prev: entry.prev, merged: entry.merged, not_run: affected.not_run, ..entry },
            Vec::new(),
            now,
            events,
        )?;
    } else if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
        found.state = "testing".into();
        found.note.clear();
        found.retry = false;
        found.wait_head.clear();
        found.wait_map.clear();
        found.selection_failed = 0;
        found.requests = affected.requests.iter().map(|request| request.id.clone()).collect();
        found.not_run = affected.not_run;
    }
    Ok(true)
}

/// A merge that could not select its tests never stays in the lead branch:
/// roll it back at once and put the entry back to queued. A second selection
/// error returns the task; a rollback blocked by a moved branch keeps the entry
/// queued with the reason and the lead inbox stops the queue until it acts.
fn requeue_after_error(
    team: &mut Team,
    id: u32,
    prev: &str,
    merged: &str,
    reason: &str,
    selection: bool,
    events: &mut Vec<Event>,
) -> anyhow::Result<bool> {
    let head = git_text(&team.folder, &["rev-parse", "HEAD"]).unwrap_or_default();
    let rolled = if head == merged {
        roll_back(&team.folder, &MergeEntry { prev: prev.to_owned(), merged: merged.to_owned(), ..Default::default() })
    } else {
        match ancestry(&team.folder, merged) {
            Ancestry::No => Rollback::Done,
            Ancestry::Yes => Rollback::Failed(format!("The lead branch moved since; revert {merged} by hand.")),
            Ancestry::Unknown => {
                let note = format!("{reason}; git could not tell whether {merged} is in the lead branch; the merge waits");
                return Ok(set_queue_note(team, id, &note, "merge_rollback_blocked", events));
            }
        }
    };
    match rolled {
        Rollback::Done => {
            let failed = team.task(id).and_then(|task| task.merge.as_ref()).map(|entry| entry.selection_failed).unwrap_or(0)
                + u32::from(selection);
            if selection && failed >= 2 {
                let reason = format!("{reason}. The merge was rolled back; the tests could not be selected twice.");
                return_to_author(team, id, &reason, events);
                events.push(Event::new("merge_affected_failed", json!({"task":id,"reason":reason})));
                return Ok(true);
            }
            let head = git_text(&team.folder, &["rev-parse", "HEAD"]).unwrap_or_default();
            let map = tests_stamp(team);
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                found.state = "queued".into();
                found.note = format!("{reason}; the merge was rolled back and will be retried");
                found.wait_head = head.clone();
                found.wait_map = map;
                found.retry = false;
                found.selection_failed = failed;
                found.requests.clear();
                found.not_run.clear();
                found.prev = head;
                found.merged.clear();
            }
            events.push(Event::new("merge_requeued", json!({"task":id,"reason":reason})));
            Ok(true)
        }
        Rollback::Failed(note) => {
            let reason = format!("{reason}; {note}");
            Ok(set_queue_note(team, id, &reason, "merge_rollback_failed", events))
        }
    }
}

/// The merge stays: the suites that ran are tested, and the task is accepted as

/// Drop the oldest tested suite by its commit's place in the bounded log, not
/// by its name.
fn trim_tested_suites(team: &mut Team) {
    while team.merge_tested_suites.len() > MERGE_SUITES {
        let oldest = team
            .merge_tested_suites
            .iter()
            .min_by_key(|(suite, commit)| {
                team.merge_log
                    .iter()
                    .position(|entry| entry.merged == **commit && entry.suites.iter().any(|ran| ran == *suite))
                    .unwrap_or(0)
            })
            .map(|(suite, _)| suite.clone());
        let Some(oldest) = oldest else { break };
        team.merge_tested_suites.remove(&oldest);
    }
}

/// The same bound for the open not-run map, without losing a range: past the
/// bound the tested point stays at the oldest open range and the feed says so.
fn guard_not_run(team: &mut Team, events: &mut Vec<Event>) {
    if team.merge_not_run.len() <= MERGE_SUITES {
        return;
    }
    let oldest = team
        .merge_not_run
        .iter()
        .min_by_key(|(_, open)| team.merge_log.iter().position(|entry| entry.merged == open.since || entry.prev == open.since).unwrap_or(0))
        .map(|(_, open)| open.since.clone());
    if let Some(oldest) = oldest {
        if !oldest.is_empty() {
            team.merge_tested = oldest.clone();
        }
        if team.merge_not_run.len() == MERGE_SUITES + 1 {
            events.push(Event::new(
                "merge_not_run_overflow",
                json!({"open":team.merge_not_run.len(),"tested":oldest}),
            ));
        }
    }
}

/// The merge stays: the suites that ran are tested, and the task is accepted as
/// the accept that queued it asked. A suite not run here keeps only its own
/// range open and is named in the accept reason.
fn keep(team: &mut Team, id: u32, entry: &MergeEntry, suites: Vec<String>, now: f64, events: &mut Vec<Event>) -> anyhow::Result<()> {
    // Pending not-run suites from earlier merges stay pending unless this
    // merge ran them; the current merge's own not-run suites join them.
    let mut remaining = Vec::new();
    for message in &entry.not_run {
        push_not_run(&mut remaining, message);
    }
    for message in pending_not_run(team) {
        if suites.iter().any(|ran| ran.as_str() == not_run_suite(&message)) {
            continue;
        }
        push_not_run(&mut remaining, &message);
    }
    if !entry.merged.is_empty() {
        // A suite that ran advances the tested point even when another suite
        // could not run here; a merge with nothing selected advances too. Only
        // a merge whose selected suites were all not run keeps it back.
        if !suites.is_empty() || entry.not_run.is_empty() {
            team.merge_tested = entry.merged.clone();
            for suite in &suites {
                if !suite.is_empty() {
                    team.merge_tested_suites.insert(suite.clone(), entry.merged.clone());
                }
            }
            trim_tested_suites(team);
        }
        // Keep the not-run suites in their own small map: the bounded log may
        // age their entry out, but their range stays open until they run.
        let previous = team.merge_not_run.clone();
        let mut open = std::collections::BTreeMap::new();
        for message in &remaining {
            let suite = not_run_suite(message);
            if suite.is_empty() {
                continue;
            }
            let since = previous.get(suite).map(|open| open.since.clone()).filter(|since| !since.is_empty()).unwrap_or_else(|| {
                if entry.not_run.iter().any(|message| not_run_suite(message) == suite) {
                    entry.prev.clone()
                } else {
                    suite_since(team, suite)
                }
            });
            open.insert(suite.to_owned(), OpenNotRun { since, message: message.clone() });
        }
        team.merge_not_run = open;
        guard_not_run(team, events);
        team.merge_log.push(MergeLog { task: id, prev: entry.prev.clone(), merged: entry.merged.clone(), at: now, suites, not_run: remaining.clone() });
        let extra = team.merge_log.len().saturating_sub(50);
        team.merge_log.drain(0..extra);
    }
    let mut reason = entry.reason.clone();
    let note = not_run_note(&remaining);
    if !note.is_empty() {
        if !reason.is_empty() {
            reason.push(' ');
        }
        reason.push_str(&note);
    }
    let actor = if entry.by_owner { super::ops::Actor::Human } else { super::ops::Actor::Member(team.lead.clone()) };
    let input = json!({"task":id,"reason":reason,"merged":true});
    super::ops::apply(team, "accept", &input, &actor, &Default::default(), events)?;
    Ok(())
}

/// Whether the lead branch was reset to before the merge.
#[derive(Debug, PartialEq)]
enum Rollback {
    Done,
    Failed(String),
}

impl Rollback {
    fn note(&self) -> String {
        match self {
            Self::Done => "The merge was rolled back.".into(),
            Self::Failed(note) => note.clone(),
        }
    }
}

/// Resets the lead branch to before the merge, when it is still the merge.
fn roll_back(lead: &str, entry: &MergeEntry) -> Rollback {
    match git_text(lead, &["rev-parse", "HEAD"]) {
        Some(head) if head == entry.merged && !entry.prev.is_empty() => {
            if git(lead, &["reset", "--hard", "--quiet", &entry.prev]).exit == Some(0) {
                Rollback::Done
            } else {
                Rollback::Failed(format!("Rolling the merge back failed; revert {} by hand.", entry.merged))
            }
        }
        _ => Rollback::Failed(format!("The lead branch moved since; revert {} by hand.", entry.merged)),
    }
}

/// Whether an entry still names a merge reachable from HEAD (or git cannot say).
pub(crate) fn merge_still_reachable(team: &Team, id: u32) -> bool {
    let Some(entry) = team.task(id).and_then(|task| task.merge.as_ref()) else { return false };
    !entry.merged.is_empty() && ancestry(&team.folder, &entry.merged) != Ancestry::No
}

/// Keep a returned task's untested merge range open for the phase build.
fn record_untested_range(team: &mut Team, id: u32, entry: &MergeEntry, events: &mut Vec<Event>) {
    if entry.merged.is_empty() || entry.prev.is_empty() || ancestry(&team.folder, &entry.merged) == Ancestry::No {
        return;
    }
    let suite = format!("merge-{id}");
    let message = format!(
        "not run here: {suite} runs on all; the merge of {} was not tested on {}",
        entry.commit, entry.merged
    );
    team.merge_not_run.insert(suite, OpenNotRun { since: entry.prev.clone(), message });
    guard_not_run(team, events);
}

/// The task goes back to its author with why: its report is rejected by Zerus
/// and it waits to be handed out to the same member again.
pub(crate) fn return_to_author(team: &mut Team, id: u32, reason: &str, events: &mut Vec<Event>) {
    if let Some(entry) = team.task(id).and_then(|task| task.merge.clone()) {
        record_untested_range(team, id, &entry, events);
    }
    let Some(task) = team.task_mut(id) else { return };
    let author = task.merge.take().map(|entry| entry.author).unwrap_or_else(|| task.assignee.clone());
    if let Some(attempt) = task.attempts.iter_mut().rev().find(|attempt| attempt.state == AttemptState::Reported) {
        attempt.state = AttemptState::Rejected;
        attempt.interim = None;
    }
    task.status = TaskStatus::Pending;
    task.assignee = author;
    task.pinned = true;
    task.decided_by = Some(Decider::Zerus);
    task.decision_reason = reason.chars().take(4000).collect();
    events.push(Event::new("merge_returned", json!({"task":id,"reason":task.decision_reason})));
}

/// A not-run message names its suite: "not run here: SUITE runs on ...".
fn not_run_suite(message: &str) -> &str {
    message.strip_prefix("not run here: ").and_then(|rest| rest.split(" runs on ").next()).unwrap_or_default()
}

fn push_not_run(list: &mut Vec<String>, message: &str) {
    let suite = not_run_suite(message);
    if !list.iter().any(|existing| not_run_suite(existing) == suite) {
        list.push(message.to_owned());
    }
}

/// A sentence naming the not-run suites for the accept reason and the failure.
fn not_run_note(messages: &[String]) -> String {
    if messages.is_empty() {
        return String::new();
    }
    let listed: Vec<&str> = messages.iter().map(|message| message.strip_prefix("not run here: ").unwrap_or(message.as_str())).collect();
    format!("Not run here: {}.", listed.join("; "))
}

/// The not-run suites since each one's own last run: a suite that ran in a
/// later merge is resolved even when another suite was not run there.
fn pending_not_run(team: &Team) -> Vec<String> {
    let mut found = Vec::new();
    // The persisted map outlives the bounded log.
    for open in team.merge_not_run.values() {
        push_not_run(&mut found, &open.message);
    }
    for (at, entry) in team.merge_log.iter().enumerate() {
        for message in &entry.not_run {
            let suite = not_run_suite(message);
            if suite.is_empty() || team.merge_not_run.contains_key(suite) {
                continue;
            }
            let ran_later = team.merge_log[at + 1..].iter().any(|later| later.suites.iter().any(|ran| ran == suite));
            if !ran_later {
                push_not_run(&mut found, message);
            }
        }
    }
    found
}

/// The commit a suite's open range starts at: its persisted open range, its
/// newest run, else the branch it first could not run on.
fn suite_since(team: &Team, suite: &str) -> String {
    if let Some(open) = team.merge_not_run.get(suite).filter(|open| !open.since.is_empty()) {
        return open.since.clone();
    }
    let mut last = team.merge_tested_suites.get(suite).filter(|at| !at.is_empty()).cloned();
    let mut first_not_run = None;
    for entry in &team.merge_log {
        if entry.suites.iter().any(|ran| ran == suite) && !entry.merged.is_empty() {
            last = Some(entry.merged.clone());
        }
        if first_not_run.is_none() && entry.not_run.iter().any(|message| not_run_suite(message) == suite) {
            first_not_run = Some(entry.prev.clone());
        }
    }
    last.or(first_not_run).unwrap_or_else(|| team.merge_tested.clone())
}

/// What a phase integration still has to check: the newest tested merge and
/// the suites not run since their own last run (None when everything selected
/// was tested).
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Untested {
    pub since: String,
    pub not_run: Vec<String>,
    /// The suite and the commit its own open range starts at.
    pub per_suite: Vec<(String, String)>,
}

impl Untested {
    /// The suites named by the not-run messages.
    pub(crate) fn suites(&self) -> Vec<String> {
        let mut found = Vec::new();
        for message in &self.not_run {
            let suite = not_run_suite(message);
            if !suite.is_empty() && !found.iter().any(|existing: &String| existing.as_str() == suite) {
                found.push(suite.to_owned());
            }
        }
        found
    }
}

pub(crate) fn untested_since(team: &Team) -> Option<Untested> {
    let since = team.merge_tested.clone();
    let not_run = pending_not_run(team);
    if since.is_empty() && not_run.is_empty() {
        return None;
    }
    let per_suite = not_run
        .iter()
        .filter_map(|message| {
            let suite = not_run_suite(message);
            (!suite.is_empty()).then(|| (suite.to_owned(), suite_since(team, suite)))
        })
        .collect();
    Some(Untested { since, not_run, per_suite })
}

/// A queued or tested entry, for status.
pub(crate) fn view(task: &Task) -> Option<Value> {
    let entry = task.merge.as_ref()?;
    let mut value = json!({"state":entry.state,"author":entry.author,"commit":entry.commit,"merged":entry.merged,"tests":entry.requests.len(),"note":entry.note});
    if !entry.not_run.is_empty() {
        value["not_run"] = json!(entry.not_run.clone());
    }
    Some(value)
}

#[cfg(test)]
#[path = "merge_queue_tests.rs"]
mod tests;
