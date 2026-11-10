//! Task 128 (owner, 2026-11-07; docs/lore.md 6): Teams write and read their
//! memory. With a RustCodeLore workspace (memory mode `team` or `project`)
//! Zerus records the phase plans and each task's outcome, the owner's
//! decisions, the faults of fixed review findings and the test readings, as
//! records that pass CodeLore's validation, written `agent:zerus` with the
//! trailer `Zerus-Team`. A record whose required fields Zerus cannot fill is
//! never written half-filled: it waits as a lead inbox item.
//!
//! hgs runs the `lore` binary (docs/lore.md 2.1: linking the crate would turn
//! on serde_json's preserve_order for all of hgs and bring candle in). The
//! writer is a background thread of the team worker, never the dispatcher
//! tick: every pass computes each team's records from its state, diffs them
//! against the workspace's JSONL and writes at most `PASS_RECORDS` changes in
//! one batch per kind of write. A refused record is remembered with its
//! content and never retried until it changes.
use super::model::{AttemptState, Decider, LorePending, PhaseState, Task, TaskStatus, Team, TeamStatus};
use super::store::{Event, Store};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// At most this many records one pass writes for a team.
pub(crate) const PASS_RECORDS: usize = 50;
/// How often the background writer looks at the teams.
pub(crate) const PASS_EVERY: Duration = Duration::from_secs(30);
/// At most this many bytes of a workspace's JSONL are read.
const READ_BYTES: u64 = 4 * 1024 * 1024;
/// At most this many lead inbox items for records waiting on a field.
const PENDING_MAX: usize = 20;
pub(crate) const BY: &str = "agent:zerus";
/// A finished team's last passes (the plan's close) run this long after it.
const FINISHED_WINDOW: f64 = 7.0 * 86_400.0;

/// The `lore` binary: `HGS_LORE`, else beside this hgs, else on PATH.
pub(crate) fn lore_bin() -> Option<PathBuf> {
    let name = if cfg!(windows) { "lore.exe" } else { "lore" };
    if let Some(path) = std::env::var_os("HGS_LORE").filter(|value| !value.is_empty()).map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    if let Some(beside) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(name))) {
        if beside.is_file() {
            return Some(beside);
        }
    }
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(name)).find(|path| path.is_file()))
}

/// The team's workspace, and whether its mode writes there.
pub(crate) fn workspace(team: &Team) -> Option<(PathBuf, bool)> {
    let memory = team.settings.memory.as_ref()?;
    let writes = matches!(memory.mode.as_str(), "team" | "project" | "lore");
    let reads = writes || memory.mode == "read-only";
    (reads && !memory.workspace.is_empty()).then(|| (PathBuf::from(&memory.workspace), writes))
}

fn team8(team: &Team) -> String {
    team.id.chars().filter(|ch| ch.is_ascii_alphanumeric()).take(8).collect::<String>().to_lowercase()
}

/// The id prefix of every record Zerus writes for this team.
pub(crate) fn prefix(team: &Team) -> String {
    format!("t{}-", team8(team))
}

/// The team's own knowledge area (docs/lore.md 4.4; an area id has no dot).
pub(crate) fn area(team: &Team) -> String {
    format!("team-{}", team8(team))
}

pub(crate) fn plan_id(team: &Team, phase: u32) -> String {
    format!("{}phase-{phase}", prefix(team))
}

fn cut(text: &str, limit: usize) -> String {
    let flat = text.trim();
    if flat.chars().count() <= limit {
        return flat.to_owned();
    }
    flat.chars().take(limit.saturating_sub(1)).collect::<String>() + "\u{2026}"
}

fn first_line(text: &str, limit: usize) -> String {
    cut(text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default(), limit)
}

fn decider(task: &Task) -> &'static str {
    match task.decided_by {
        Some(Decider::Human) => "You",
        Some(Decider::Zerus) => "Zerus",
        _ => "Lead",
    }
}

/// The commits of a task's attempts, newest last, each a full sha.
fn commits(task: &Task) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for attempt in &task.attempts {
        for commit in &attempt.commits {
            if !commit.sha.is_empty() && commit.sha.len() <= 64 && !found.contains(&commit.sha) {
                found.push(commit.sha.clone());
            }
        }
    }
    // Task #19 (owner, 2026-11-10): a member that commits with git itself
    // leaves no commit Zerus made; the landing's commit (a hand landing's
    // `hgs team landed`, or the queue's) and the accepted report's commit
    // are the task's commits then.
    if found.is_empty() && task.status == TaskStatus::Accepted {
        let landed = task.landed.as_ref().map(|landed| landed.commit.clone());
        let reported = task
            .attempts
            .iter()
            .rev()
            .find(|attempt| attempt.state == AttemptState::Reported && !attempt.folder_head.is_empty())
            .map(|attempt| attempt.folder_head.clone());
        for sha in [reported, landed].into_iter().flatten() {
            if sha.len() <= 64 && !found.contains(&sha) {
                found.push(sha);
            }
        }
    }
    let skip = found.len().saturating_sub(20);
    found.split_off(skip)
}

/// A task's latest passing test run on a commit, as `template args`: the
/// check a fault names.
fn passing_run(task: &Task) -> Option<String> {
    task.attempts.iter().rev().filter(|attempt| attempt.commits.iter().any(|commit| !commit.sha.is_empty())).find_map(|attempt| {
        let run = attempt.runs.iter().rev().find(|run| run.exit == Some(0) && !run.timed_out && test_template(&run.template))?;
        let args = if run.args.is_null() || run.args == json!({}) { String::new() } else { format!(" {}", run.args) };
        Some(cut(&format!("{}{args}", run.template), 300))
    })
}

/// A passing test run after `since` on a commit, as `template args`.
fn passing_run_after(task: &Task, since: f64) -> Option<String> {
    task.attempts.iter().rev().filter(|attempt| attempt.commits.iter().any(|commit| !commit.sha.is_empty())).find_map(|attempt| {
        let run = attempt.runs.iter().rev().find(|run| run.at > since && run.exit == Some(0) && !run.timed_out && test_template(&run.template))?;
        let args = if run.args.is_null() || run.args == json!({}) { String::new() } else { format!(" {}", run.args) };
        Some(cut(&format!("{}{args}", run.template), 300))
    })
}

/// A report made after `since` by an attempt that was not sent back.
fn report_after(task: &Task, since: f64) -> Option<String> {
    let attempt = task.attempts.iter().rev().find(|attempt| attempt.state != AttemptState::Rejected && attempt.report.as_ref().is_some_and(|report| report.at > since))?;
    let text = attempt.report.as_ref()?.text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// The accepted attempt's report, the text a fault's fix quotes.
fn report_text(task: &Task) -> Option<String> {
    let attempt = task.attempts.iter().rev().find(|attempt| attempt.state != AttemptState::Rejected && attempt.report.is_some())?;
    let text = attempt.report.as_ref()?.text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn test_template(template: &str) -> bool {
    matches!(template, "cargo-test" | "qt-test" | "py-test" | "ctest" | "vstest" | "ue-test" | "android-test")
}

/// The team's phases with their titles, in order: phase 1, the approved
/// initial plan, has no `Phase` record (`Team.phases`); `all` keeps drafts
/// and sent-back phases too (the plain digest), else only approved ones.
pub(crate) fn plan_phases(team: &Team, all: bool) -> Vec<(u32, String)> {
    let mut phases: Vec<(u32, String)> = Vec::new();
    let started = !matches!(team.status, TeamStatus::Draft | TeamStatus::Planning);
    if !team.phases.iter().any(|phase| phase.n == 1) && (started || all) && team.tasks.iter().any(|task| task.phase <= 1) {
        phases.push((1, String::new()));
    }
    for phase in &team.phases {
        let shown = match phase.state {
            PhaseState::Discarded => false,
            PhaseState::Draft | PhaseState::Rejected => all,
            _ => started || all,
        };
        if shown {
            phases.push((phase.n, phase.title.clone()));
        }
    }
    phases.sort_by_key(|(n, _)| *n);
    phases
}

/// The tasks of a phase; a task stored before phases (0) is phase 1's.
fn in_phase(task: &Task, n: u32) -> bool {
    task.phase == n || (n == 1 && task.phase == 0)
}

/// What Zerus would write for the team now (docs/lore.md 6.1), and the
/// records that wait on a field only a person can give.
pub(crate) fn desired(team: &Team) -> (Vec<Value>, Vec<LorePending>) {
    let mut records: Vec<Value> = Vec::new();
    let mut pending: Vec<LorePending> = Vec::new();
    let finished = matches!(team.status, TeamStatus::Done | TeamStatus::Stopped);
    // A plan per approved phase, an item per task of it.
    for (n, phase_title) in plan_phases(team, false) {
        let plan = plan_id(team, n);
        let tasks: Vec<&Task> = team.tasks.iter().filter(|task| in_phase(task, n) && task.status != TaskStatus::Draft).collect();
        if tasks.is_empty() {
            continue;
        }
        let mut undecided = false;
        let mut items = Vec::new();
        for task in &tasks {
            let mut item = Map::new();
            item.insert("kind".into(), json!("item"));
            item.insert("id".into(), json!(format!("{plan}#{}", task.id)));
            item.insert("plan".into(), json!(plan));
            item.insert("title".into(), json!(cut(&task.title, 120)));
            if !task.description.trim().is_empty() {
                item.insert("description".into(), json!(cut(&task.description, 2000)));
            }
            let criteria: Vec<Value> = task.criteria.iter().take(20).map(|criterion| json!(cut(criterion, 300))).collect();
            if !criteria.is_empty() {
                item.insert("criteria".into(), Value::Array(criteria));
            }
            let deps: Vec<Value> = task
                .deps
                .iter()
                .filter(|dep| tasks.iter().any(|other| other.id == dep.task))
                .map(|dep| json!({"item": format!("{plan}#{}", dep.task), "kind": if dep.kind == super::model::DependencyKind::Hard { "hard" } else { "soft" }}))
                .collect();
            if !deps.is_empty() {
                item.insert("deps".into(), Value::Array(deps));
            }
            let status = match task.status {
                TaskStatus::Accepted => {
                    let commits = commits(task);
                    if commits.is_empty() {
                        // An accepted task without a commit cannot be `done`.
                        pending.push(LorePending {
                            id: format!("{plan}#{}", task.id),
                            kind: "item".into(),
                            missing: vec!["commits".into()],
                            text: format!("#{} {} is accepted with no commit Zerus made: name its commit", task.id, cut(&task.title, 80)),
                        });
                        "open"
                    } else {
                        item.insert("commits".into(), json!(commits));
                        item.insert("outcome".into(), json!(cut(if task.decision_reason.is_empty() { "accepted" } else { &task.decision_reason }, 1000)));
                        "done"
                    }
                }
                TaskStatus::Cancelled | TaskStatus::Rejected => {
                    let word = if task.status == TaskStatus::Rejected { "rejected" } else { "cancelled" };
                    item.insert("reason".into(), json!(cut(if task.decision_reason.is_empty() { word } else { &task.decision_reason }, 600)));
                    item.insert("decided_by".into(), json!(decider(task)));
                    "dropped"
                }
                _ if task.hold.is_some() => {
                    let reason = task.hold.as_ref().map(|hold| hold.reason.clone()).filter(|reason| !reason.is_empty()).unwrap_or_else(|| "held".into());
                    item.insert("reason".into(), json!(cut(&reason, 600)));
                    "deferred"
                }
                _ => "open",
            };
            if matches!(status, "open" | "deferred") {
                undecided = true;
            }
            item.insert("status".into(), json!(status));
            items.push(Value::Object(item));
        }
        let mut record = Map::new();
        record.insert("kind".into(), json!("plan"));
        record.insert("id".into(), json!(plan));
        let title = if phase_title.trim().is_empty() { format!("{}: phase {n}", team.name) } else { format!("{}: {}", team.name, phase_title.trim()) };
        record.insert("title".into(), json!(cut(&title, 120)));
        record.insert("goal".into(), json!(cut(if team.goal.trim().is_empty() { &team.name } else { &team.goal }, 800)));
        if finished && !undecided {
            record.insert("status".into(), json!("closed"));
            let outcome = format!("{} of {} tasks done; the team {}", items.iter().filter(|item| item["status"] == "done").count(), items.len(),
                if team.status == TeamStatus::Done { "finished" } else { "stopped" });
            record.insert("outcome".into(), json!(cut(&outcome, 2000)));
        } else {
            record.insert("status".into(), json!("live"));
        }
        records.push(Value::Object(record));
        records.extend(items);
    }
    let area = area(team);
    // Task 156: the team's decision records, superseded ones marked.
    for decision in &team.decisions {
        let mut reason = decision.why.clone();
        if !decision.files.is_empty() || !decision.tasks.is_empty() || !decision.spec.is_empty() || !decision.forbids.is_empty() {
            let tasks: Vec<String> = decision.tasks.iter().map(|id| format!("#{id}")).collect();
            reason = format!("{reason} [spec: {}; files: {}; tasks: {}; forbids: {}]", decision.spec, decision.files.join(", "), tasks.join(", "), decision.forbids.join(", "));
        }
        let mut record = json!({"kind": "decision", "id": format!("{}{}", prefix(team), decision.id), "area": area,
            "title": first_line(&format!("{}: {}", decision.question, decision.choice), 120),
            "question": cut(&decision.question, 300), "choice": cut(&decision.choice, 600),
            "reason": cut(if reason.trim().is_empty() { "no reason given" } else { &reason }, 800),
            "decided_by": cut(&decision.by, 120), "status": "active"});
        if !decision.superseded_by.is_empty() {
            record["status"] = json!("superseded");
            record["superseded_by"] = json!(format!("{}{}", prefix(team), decision.superseded_by));
        }
        records.push(record);
    }
    // The owner's decisions on cards (a card the team's records already hold
    // is written once, as its decision).
    for proposal in &team.proposals {
        let state = proposal["state"].as_str().unwrap_or_default();
        if !matches!(state, "accepted" | "rejected") {
            continue;
        }
        if team.decisions.iter().any(|decision| proposal["id"] == decision.card.as_str()) {
            continue;
        }
        let id = proposal["id"].as_str().unwrap_or_default();
        if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            continue;
        }
        let kind = proposal["kind"].as_str().unwrap_or("proposal");
        let summary = proposal["change"]["summary"].as_str().filter(|text| !text.is_empty()).or(proposal["reason"].as_str()).unwrap_or_default();
        let question = first_line(&format!("{kind}: {summary}"), 300);
        let choice = if state == "accepted" { first_line(&format!("allowed: {summary}"), 600) } else { "denied".to_owned() };
        let reason = proposal["note"].as_str().filter(|note| !note.trim().is_empty()).unwrap_or("no reason given");
        records.push(json!({"kind": "decision", "id": format!("{}{}", prefix(team), id.to_lowercase()), "area": area,
            "title": first_line(&format!("{} {kind}: {summary}", if state == "accepted" { "Allowed" } else { "Denied" }), 120),
            "question": question, "choice": choice, "reason": cut(reason, 800), "decided_by": "owner", "status": "active"}));
    }
    // The faults of fixed review findings, once every field is known.
    for finding in &team.findings {
        if finding.state != "fixed" {
            continue;
        }
        let id = format!("{}finding-{}", prefix(team), finding.id.to_lowercase());
        // The task it names (reopened to fix it) or the follow-ups that name
        // that task as their origin: only their work after the finding counts.
        let source = finding.task.unwrap_or(finding.from_task);
        let fixing: Vec<&Task> = team.tasks.iter().filter(|task| task.id == source || task.origin == Some(source)).collect();
        let check = fixing.iter().find_map(|task| passing_run_after(task, finding.at));
        let fix = if finding.note.trim().is_empty() {
            fixing.iter().filter(|task| task.status == TaskStatus::Accepted).find_map(|task| report_after(task, finding.at)).unwrap_or_default()
        } else {
            finding.note.clone()
        };
        let mut missing = Vec::new();
        if finding.scenario.trim().is_empty() {
            missing.push("symptom".to_owned());
        }
        if finding.title.trim().is_empty() {
            missing.push("cause".to_owned());
        }
        if fix.trim().is_empty() {
            missing.push("fix".to_owned());
        }
        if check.is_none() {
            missing.push("check".to_owned());
        }
        if !missing.is_empty() {
            pending.push(LorePending {
                id,
                kind: "fault".into(),
                text: format!("write the fault for finding {} ({}): {}", finding.id, cut(&finding.title, 80), missing.join(", ")),
                missing,
            });
            continue;
        }
        let mut fault = json!({"kind": "fault", "id": id, "area": area, "title": first_line(&finding.title, 120),
            "symptom": cut(&finding.scenario, 600), "cause": cut(&finding.title, 600), "fix": cut(&fix, 800),
            "check": cut(&check.unwrap_or_default(), 300), "status": "cured"});
        let file = finding.file.replace('\\', "/");
        let relative = !file.is_empty() && !file.starts_with('/') && !file.contains(':') && !file.split('/').any(|part| part == "..");
        if relative {
            fault["refs"] = json!({"files": [file]});
        }
        records.push(fault);
    }
    // The causes of reworks: each rejected attempt of an accepted task, with
    // the reviewer's exact reason.
    for task in team.tasks.iter().filter(|task| task.status == TaskStatus::Accepted) {
        for attempt in task.attempts.iter().filter(|attempt| attempt.state == AttemptState::Rejected && !attempt.rework_reason.trim().is_empty()) {
            let id = format!("{}rework-{}-{}", prefix(team), task.id, attempt.n);
            let check = passing_run(task);
            let fix = report_text(task).unwrap_or_default();
            let mut missing = Vec::new();
            if fix.trim().is_empty() {
                missing.push("fix".to_owned());
            }
            if check.is_none() {
                missing.push("check".to_owned());
            }
            if !missing.is_empty() {
                pending.push(LorePending {
                    id,
                    kind: "fault".into(),
                    text: format!("write the fault for the rework of #{} (attempt {}): {}", task.id, attempt.n, missing.join(", ")),
                    missing,
                });
                continue;
            }
            records.push(json!({"kind": "fault", "id": id, "area": area,
                "title": first_line(&format!("Rework of #{}: {}", task.id, attempt.rework_reason), 120),
                "symptom": cut(&format!("#{} {} was sent back for rework (attempt {})", task.id, task.title, attempt.n), 600),
                "cause": cut(&attempt.rework_reason, 600), "fix": cut(&fix, 800),
                "check": cut(&check.unwrap_or_default(), 300), "status": "cured"}));
        }
    }
    // The newest passing reading of each test template, at its commit.
    let mut readings: BTreeMap<String, (f64, Value)> = BTreeMap::new();
    for task in &team.tasks {
        for attempt in task.attempts.iter().filter(|attempt| attempt.state != AttemptState::Revoked) {
            let Some(commit) = attempt.commits.last().filter(|commit| !commit.sha.is_empty()) else { continue };
            for run in attempt.runs.iter().filter(|run| run.exit == Some(0) && !run.timed_out && test_template(&run.template)) {
                if readings.get(&run.template).is_some_and(|(at, _)| *at >= run.at) {
                    continue;
                }
                let args = if run.args.is_null() || run.args == json!({}) { String::new() } else { format!(" {}", run.args) };
                let reading = json!({"kind": "reading", "id": format!("{}reading-{}", prefix(team), run.template), "area": area,
                    "title": first_line(&format!("{} duration, the latest passing run", run.template), 120),
                    "what": first_line(&format!("{} wall time on task #{}", run.template, task.id), 300),
                    "command": cut(&format!("{}{args}", run.template), 300), "value": format!("{} ms", run.ms),
                    "measured_at": commit.sha, "status": "current"});
                readings.insert(run.template.clone(), (run.at, reading));
            }
        }
    }
    records.extend(readings.into_values().map(|(_, reading)| reading));
    pending.truncate(PENDING_MAX);
    (records, pending)
}

/// The records the workspace holds, by id (bounded read of its JSONL).
pub(crate) fn current(workspace: &Path) -> BTreeMap<String, Value> {
    let mut found = BTreeMap::new();
    let mut budget = READ_BYTES;
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in [workspace.join("lore"), workspace.join("lore").join("plans")] {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut names: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|ext| ext == "jsonl")).collect();
            names.sort();
            files.extend(names);
        }
    }
    for file in files {
        let size = std::fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0);
        if size > budget {
            break;
        }
        budget -= size;
        for line in std::fs::read_to_string(&file).unwrap_or_default().lines() {
            if let Ok(record) = serde_json::from_str::<Value>(line) {
                if let Some(id) = record["id"].as_str() {
                    found.insert(id.to_owned(), record);
                }
            }
        }
    }
    found
}

/// The fields a write sets itself, never compared.
const STAMPS: [&str; 5] = ["v", "by", "created", "updated", "area"];

/// What changes: records to add, patches of records to update, and lore
/// commands (an item reopened, a plan closed). Only this team's ids are
/// touched.
pub(crate) fn diff(team: &Team, desired: &[Value], current: &BTreeMap<String, Value>) -> (Vec<Value>, Vec<Value>, Vec<Vec<String>>) {
    let prefix = prefix(team);
    let (mut adds, mut updates, mut commands) = (Vec::new(), Vec::new(), Vec::new());
    for record in desired {
        let id = record["id"].as_str().unwrap_or_default();
        if !id.starts_with(&prefix) {
            continue;
        }
        let Some(old) = current.get(id) else {
            adds.push(record.clone());
            continue;
        };
        let mut patch = Map::new();
        for (key, value) in record.as_object().into_iter().flatten() {
            if !STAMPS.contains(&key.as_str()) && old.get(key) != Some(value) {
                patch.insert(key.clone(), value.clone());
            }
        }
        // Fields the record no longer has are removed.
        for key in old.as_object().into_iter().flatten().map(|(key, _)| key) {
            if !STAMPS.contains(&key.as_str()) && key != "id" && key != "kind" && record.get(key).is_none() && matches!(key.as_str(), "commits" | "reason" | "decided_by" | "outcome") {
                patch.insert(key.clone(), Value::Null);
            }
        }
        if patch.is_empty() {
            continue;
        }
        let was = old["status"].as_str().unwrap_or_default();
        let now = record["status"].as_str().unwrap_or_default();
        if record["kind"] == "item" && matches!(was, "done" | "dropped") && matches!(now, "open" | "deferred") {
            commands.push(vec!["item".into(), "reopen".into(), id.to_owned(), "--reason".into(), "the task was reopened in its team".into()]);
            continue;
        }
        if record["kind"] == "item" && matches!(was, "done" | "dropped") {
            continue; // a decided item stays as it was decided
        }
        if record["kind"] == "plan" && was == "closed" {
            continue; // a closed plan stays closed
        }
        if record["kind"] == "plan" && now == "closed" {
            let outcome = record["outcome"].as_str().unwrap_or("closed").to_owned();
            commands.push(vec!["plan".into(), "close".into(), id.to_owned(), "--outcome".into(), outcome]);
            continue;
        }
        patch.insert("id".into(), json!(id));
        updates.push(Value::Object(patch));
    }
    (adds, updates, commands)
}

/// The folders a cited commit or file may be in: the workspace, the team's
/// folder and its members' own folders.
fn roots(team: &Team, workspace: &Path) -> Vec<PathBuf> {
    let mut folders = vec![workspace.to_path_buf()];
    let local = team.members.iter().filter(|member| member.computer.is_empty()).map(|member| member.directory.as_str());
    for folder in std::iter::once(team.folder.as_str()).chain(local) {
        let path = PathBuf::from(folder);
        if !folder.is_empty() && path.is_dir() && !folders.contains(&path) {
            folders.push(path);
        }
        if folders.len() >= 20 {
            break;
        }
    }
    folders
}

/// Runs `lore -C <workspace> --json --by agent:zerus ...`; its JSON answer,
/// or the refusal's first line.
pub(crate) fn run_lore(bin: &Path, workspace: &Path, args: &[&str], stdin: &str, roots: &[PathBuf], trailer: Option<&str>) -> Result<Value, String> {
    let mut command = Command::new(bin);
    command.arg("-C").arg(workspace).args(["--json", "--by", BY]);
    if let Some(trailer) = trailer {
        command.args(["--trailer", trailer]);
    }
    command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Ok(joined) = std::env::join_paths(roots) {
        command.env("LORE_ROOTS", joined);
    }
    // The store's git never takes the caller's identity or repository.
    for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_NAME", "GIT_COMMITTER_EMAIL"] {
        command.env_remove(name);
    }
    let mut child = command.spawn().map_err(|error| format!("lore did not start: {error}"))?;
    if let Some(mut input) = child.stdin.take() {
        use std::io::Write;
        let _ = input.write_all(stdin.as_bytes());
    }
    let output = child.wait_with_output().map_err(|error| format!("lore failed: {error}"))?;
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    if output.status.success() {
        return Ok(value);
    }
    let reason = value["error"].as_str().map(str::to_owned).unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("lore refused").to_owned());
    let details: Vec<String> = value["details"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    Err(if details.is_empty() { reason } else { format!("{reason}: {}", details.join("; ")) })
}

/// Adds the team's area to a store's `lore.toml` when it lacks it, and
/// commits that in a store that is its own repository.
pub(crate) fn ensure_area(store: &Path, area: &str) -> Result<(), String> {
    let path = store.join("lore.toml");
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let Some(line) = text.lines().find(|line| line.trim_start().starts_with("areas")) else {
        return Err(format!("{} has no areas line", path.display()));
    };
    let mut names: Vec<String> = line.split('"').skip(1).step_by(2).map(str::to_owned).collect();
    if names.iter().any(|name| name == area) {
        return Ok(());
    }
    names.push(area.to_owned());
    let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
    let updated = text.replacen(line, &format!("areas = [{}]", quoted.join(", ")), 1);
    std::fs::write(&path, updated).map_err(|error| format!("{}: {error}", path.display()))?;
    // Committed where the store owns its history (a private store).
    super::super::project_memory::commit_config(store, &format!("lore: area {area}")).map_err(|error| error.to_string())
}

fn digest_of(record: &Value) -> String {
    format!("{:x}", Sha256::digest(record.to_string().as_bytes()))
}

/// What one pass did for a team.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Synced {
    pub written: Vec<String>,
    pub refused: Vec<(String, String)>,
    pub skipped: bool,
}

/// One pass for one team: its records into its workspace, bounded. The team
/// record keeps the pass's hash (nothing changed: nothing runs), the waiting
/// records and the refused ones.
pub(crate) fn sync(store: &mut Store, team_id: &str, bin: Option<&Path>) -> anyhow::Result<Synced> {
    let Some(team) = store.load(team_id)? else { return Ok(Synced::default()) };
    let Some((workspace, writes)) = workspace(&team) else { return Ok(Synced { skipped: true, ..Synced::default() }) };
    if !writes {
        return Ok(Synced { skipped: true, ..Synced::default() });
    }
    let (records, pending) = desired(&team);
    let hash = digest_of(&json!([records, pending.iter().map(|p| &p.id).collect::<Vec<_>>()]));
    if team.lore.hash == hash {
        return Ok(Synced { skipped: true, ..Synced::default() });
    }
    let Some(bin) = bin else {
        // Plain memory behaviour until lore is there; said once.
        if !team.lore.unavailable_told {
            store.mutate(None, team_id, None, |team, events| {
                team.lore.unavailable_told = true;
                events.push(Event::new("lore_unavailable", json!({"reason":"the lore binary was not found (HGS_LORE, beside hgs, PATH); the team keeps plain memory until it is"})));
                Ok(Value::Null)
            })?;
        }
        return Ok(Synced { skipped: true, ..Synced::default() });
    };
    let current = current(&workspace);
    let (adds, updates, commands) = diff(&team, &records, &current);
    let mut outcome = Synced::default();
    let refused_before = team.lore.refused.clone();
    let fresh = |record: &Value| refused_before.get(record["id"].as_str().unwrap_or_default()) != Some(&digest_of(record));
    let adds: Vec<Value> = adds.into_iter().filter(|record| fresh(record)).collect();
    let updates: Vec<Value> = updates.into_iter().filter(|record| fresh(record)).collect();
    // Bounded: what is left over goes in the next pass.
    let truncated = adds.len() + updates.len() + commands.len() > PASS_RECORDS;
    let adds: Vec<Value> = adds.into_iter().take(PASS_RECORDS).collect();
    let updates: Vec<Value> = updates.into_iter().take(PASS_RECORDS - adds.len()).collect();
    let commands: Vec<Vec<String>> = commands.into_iter().take(PASS_RECORDS - adds.len() - updates.len()).collect();
    let roots = roots(&team, &workspace);
    let trailer = format!("Zerus-Team: {}", team.id);
    let team_area = area(&team);
    if adds.iter().chain(updates.iter()).any(|record| record["area"].as_str() == Some(team_area.as_str())) {
        if let Err(error) = ensure_area(&workspace, &team_area) {
            outcome.refused.push(("area".into(), error));
        }
    }
    let mut refused: BTreeMap<String, String> = team.lore.refused.clone();
    // lore did not run at all: nothing is remembered, the next pass retries.
    let mut transient = false;
    let mut write = |args: &[&str], batch: Vec<Value>, jsonl: bool| {
        if batch.is_empty() || transient {
            return;
        }
        let input = if jsonl { batch.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") } else { Value::Array(batch.clone()).to_string() };
        match run_lore(bin, &workspace, args, &input, &roots, Some(&trailer)) {
            Ok(_) => outcome.written.extend(batch.iter().filter_map(|record| record["id"].as_str().map(str::to_owned))),
            Err(error) if is_transient(&error) => {
                transient = true;
                outcome.refused.push(("lore".into(), error));
            }
            Err(_) => {
                // One by one, so one bad record does not hold the rest back.
                for record in batch {
                    let single = if jsonl { record.to_string() } else { Value::Array(vec![record.clone()]).to_string() };
                    let id = record["id"].as_str().unwrap_or_default().to_owned();
                    match run_lore(bin, &workspace, args, &single, &roots, Some(&trailer)) {
                        Ok(_) => outcome.written.push(id),
                        Err(error) => {
                            if !is_transient(&error) {
                                refused.insert(id.clone(), digest_of(&record));
                            }
                            outcome.refused.push((id, error));
                        }
                    }
                }
            }
        }
    };
    // Plans come before their items in the batch: one add keeps the order.
    write(&["add", "any", "--stdin", "--new"], adds, false);
    write(&["update", "--batch"], updates, true);
    for command in &commands {
        if transient {
            break;
        }
        let args: Vec<&str> = command.iter().map(String::as_str).collect();
        let id = command.get(2).cloned().unwrap_or_default();
        match run_lore(bin, &workspace, &args, "", &roots, Some(&trailer)) {
            Ok(_) => outcome.written.push(id),
            Err(error) => {
                transient |= is_transient(&error);
                outcome.refused.push((id, error));
            }
        }
    }
    // Task 156: lore's meaning hits over the decisions for open tasks
    // (bounded: ten tasks a pass, only when their text or the decisions changed).
    let hits = decision_hits(&team, bin, &workspace, &roots);
    if !hits.is_empty() {
        store.mutate(None, team_id, None, |team, _| {
            for (task, found) in &hits {
                team.decision_hits.insert(*task, found.clone());
            }
            team.decision_hits.retain(|task, _| team.tasks.iter().any(|found| found.id == *task && !found.status.finished()));
            Ok(Value::Null)
        })?;
    }
    // A finished team's workspace is kept read only (docs/lore.md 4.5).
    if matches!(team.status, TeamStatus::Done | TeamStatus::Stopped) && workspace.is_dir() && !workspace.join(".closed").exists() {
        let _ = std::fs::write(workspace.join(".closed"), format!("team {} ended; kept read only and searchable\n", team.id));
    }
    let done = outcome.clone();
    store.mutate(None, team_id, None, |team, events| {
        team.lore.pending = pending;
        team.lore.refused = refused;
        // A pass that wrote everything settles the hash; refused records are
        // remembered by content, so they never hold it open.
        if !truncated && !transient {
            team.lore.hash = hash;
        }
        if !done.written.is_empty() {
            team.lore.written_at = super::super::now();
            team.lore.recent = done.written.iter().rev().take(10).cloned().collect();
            events.push(Event::new("lore_written", json!({"count":done.written.len(),"ids":done.written.iter().take(20).collect::<Vec<_>>()})));
        }
        for (id, reason) in done.refused.iter().take(10) {
            events.push(Event::new("lore_refused", json!({"id":id,"reason":cut(reason, 500)})));
        }
        Ok(Value::Null)
    })?;
    // Task #397: the records changed, so each store builds its index once, in
    // the background: a settled pass, no write held the store when it starts.
    if !done.written.is_empty() && !truncated && !transient {
        start_index(bin, &workspace);
        if let Some(memory) = team.settings.memory.as_ref() {
            let project = Path::new(&memory.store);
            if matches!(memory.mode.as_str(), "project" | "read-only") && !memory.store.is_empty() && project != workspace.as_path() {
                start_index(bin, project);
            }
        }
    }
    Ok(outcome)
}

/// lore did not start or did not finish: not a refusal of the record.
fn is_transient(error: &str) -> bool {
    error.starts_with("lore did not start")
        || error.starts_with("lore failed")
        || error.contains("another writer holds the store")
        || error.contains("another process holds the file")
}

/// Task 156: `lore find` over the decision records for the open tasks whose
/// text or decisions changed since their last hits.
fn decision_hits(team: &Team, bin: &Path, workspace: &Path, roots: &[PathBuf]) -> Vec<(u32, super::model::DecisionHits)> {
    if team.decisions.is_empty() {
        return Vec::new();
    }
    let decisions: Vec<&str> = team.decisions.iter().map(|decision| decision.id.as_str()).collect();
    let mut out = Vec::new();
    for task in team.tasks.iter().filter(|task| !task.status.finished()) {
        if out.len() >= 10 {
            break;
        }
        let query = cut(&format!("{} {}", task.title, task.description), 400);
        let hash = digest_of(&json!([query, decisions]));
        if team.decision_hits.get(&task.id).is_some_and(|found| found.hash == hash) {
            continue;
        }
        let Ok(value) = run_lore(bin, workspace, &["find", &query, "--kind", "decision", "-n", "5"], "", roots, None) else { continue };
        let ids: Vec<String> = value["results"].as_array().into_iter().flatten()
            .filter_map(|hit| hit["record"]["id"].as_str())
            .filter_map(|id| id.strip_prefix(&prefix(team)))
            .filter(|id| id.starts_with('d'))
            .map(str::to_owned)
            .collect();
        out.push((task.id, super::model::DecisionHits { hash, ids }));
    }
    out
}

/// The background writer: every 30 s, each active team with a workspace.
pub(crate) fn start_background(log: impl Fn(&str) + Send + 'static) {
    // A test's state folder is the test thread's own: no writer beside it.
    if cfg!(test) {
        return;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(PASS_EVERY);
        let Ok(mut store) = Store::open(true) else { continue };
        let Ok(teams) = store.memory_teams(super::super::now() - FINISHED_WINDOW, 1000) else { continue };
        let bin = lore_bin();
        for team in teams.iter().filter(|team| workspace(team).is_some_and(|(_, writes)| writes)) {
            if let Err(error) = sync(&mut store, &team.id, bin.as_deref()) {
                log(&format!("lore sync of {} failed: {error}", team.id));
            }
        }
    });
}

/// Task 128: the lead's digest block: the workspace's open items and newest
/// decisions, at most 10 lines; empty without a workspace.
pub(crate) fn digest(team: &Team) -> String {
    // It runs in the dispatcher's inbox step: no file is read, the records
    // come from the team as the writer computes them.
    let Some((workspace, writes)) = workspace(team) else { return String::new() };
    let records = if writes { desired(team).0 } else { Vec::new() };
    let open = records.iter().filter(|record| record["kind"] == "item" && matches!(record["status"].as_str(), Some("open" | "deferred"))).count();
    let mut decided: Vec<(f64, &Value)> = team
        .proposals
        .iter()
        .filter(|proposal| matches!(proposal["state"].as_str(), Some("accepted" | "rejected")))
        .map(|proposal| (proposal["decided_at"].as_f64().unwrap_or(0.0), proposal))
        .collect();
    decided.sort_by(|a, b| b.0.total_cmp(&a.0));
    let decisions: Vec<&Value> = decided
        .iter()
        .filter_map(|(_, proposal)| {
            let id = format!("{}{}", prefix(team), proposal["id"].as_str().unwrap_or_default().to_lowercase());
            records.iter().find(|record| record["id"] == id.as_str())
        })
        .take(3)
        .collect();
    if open == 0 && decisions.is_empty() && team.lore.pending.is_empty() {
        return String::new();
    }
    let mut lines = vec![format!("Team memory ({}): {open} open item(s); hgs team lore plans recalls them, hgs team lore find searches.", workspace.display())];
    for record in decisions {
        lines.push(format!("- decision {}: {} ({})", record["id"].as_str().unwrap_or_default(), first_line(record["choice"].as_str().unwrap_or_default(), 120),
            first_line(record["reason"].as_str().unwrap_or_default(), 80)));
    }
    for pending in team.lore.pending.iter().take(3) {
        lines.push(format!("- waits for you: {}", pending.text));
    }
    lines.truncate(10);
    lines.join("\n")
}

/// At most this many bytes of the plain memory digest (docs/lore.md 5).
pub(crate) const PLAIN_DIGEST_BYTES: usize = 6 * 1024;

/// Task 128 (docs/lore.md 5): plain memory's one generated text: the plan with
/// each task's outcome and why, the owner's decisions with their reasons,
/// open and fixed findings and the last test result per template; at most
/// 6 KiB, newest phases kept when it is cut.
pub(crate) fn plain_digest(team: &Team) -> String {
    let mut sections: Vec<String> = Vec::new();
    let mut head = format!("Team {} ({}): {}", team.name, team.id, first_line(&team.goal, 200));
    if head.ends_with(": ") {
        head.truncate(head.len() - 2);
    }
    sections.push(head);
    for (n, title) in plan_phases(team, true) {
        let mut lines = vec![format!("Phase {n} {}", first_line(&title, 100)).trim_end().to_owned()];
        for task in team.tasks.iter().filter(|task| in_phase(task, n)) {
            let outcome = match task.status {
                TaskStatus::Accepted => format!("accepted by {}", decider(task)),
                TaskStatus::Rejected => format!("rejected by {}", decider(task)),
                TaskStatus::Cancelled => format!("cancelled by {}", decider(task)),
                _ if task.hold.is_some() => "held".to_owned(),
                _ => "open".to_owned(),
            };
            let why = if matches!(task.status, TaskStatus::Accepted | TaskStatus::Rejected | TaskStatus::Cancelled) && !task.decision_reason.trim().is_empty() {
                format!(": {}", first_line(&task.decision_reason, 160))
            } else {
                task.hold.as_ref().filter(|hold| !hold.reason.is_empty()).map(|hold| format!(": {}", first_line(&hold.reason, 160))).unwrap_or_default()
            };
            lines.push(format!("- #{} {}: {outcome}{why}", task.id, first_line(&task.title, 100)));
        }
        sections.push(lines.join("
"));
    }
    let decisions: Vec<String> = team
        .proposals
        .iter()
        .filter(|proposal| matches!(proposal["state"].as_str(), Some("accepted" | "rejected")))
        .rev()
        .take(20)
        .map(|proposal| {
            let summary = proposal["change"]["summary"].as_str().filter(|text| !text.is_empty()).or(proposal["reason"].as_str()).unwrap_or_default();
            let reason = proposal["note"].as_str().filter(|note| !note.trim().is_empty()).unwrap_or("no reason given");
            format!("- {} {}: {} ({})", if proposal["state"] == "accepted" { "allowed" } else { "denied" },
                proposal["kind"].as_str().unwrap_or("proposal"), first_line(summary, 120), first_line(reason, 120))
        })
        .collect();
    if !decisions.is_empty() {
        sections.push(format!("Your decisions, newest first:\n{}", decisions.join("\n")));
    }
    let findings: Vec<String> = team
        .findings
        .iter()
        .rev()
        .filter(|finding| finding.unresolved() || finding.state == "fixed")
        .take(20)
        .map(|finding| format!("- {} {} [{}]: {}", finding.id, finding.severity, finding.state, first_line(&finding.title, 140)))
        .collect();
    if !findings.is_empty() {
        sections.push(format!("Findings, newest first:\n{}", findings.join("\n")));
    }
    let mut last: BTreeMap<String, (f64, String)> = BTreeMap::new();
    for task in &team.tasks {
        for run in task.attempts.iter().flat_map(|attempt| attempt.runs.iter()).filter(|run| test_template(&run.template)) {
            if last.get(&run.template).is_none_or(|(at, _)| *at < run.at) {
                let result = match run.exit {
                    Some(0) => "passed".to_owned(),
                    _ if run.timed_out => "timed out".to_owned(),
                    Some(code) => format!("failed (exit {code})"),
                    None => "no result".to_owned(),
                };
                last.insert(run.template.clone(), (run.at, format!("- {} on #{}: {result}, {} ms", run.template, task.id, run.ms)));
            }
        }
    }
    if !last.is_empty() {
        sections.push(format!("Last test result per template:\n{}", last.into_values().map(|(_, line)| line).collect::<Vec<_>>().join("\n")));
    }
    // Over 6 KiB: the oldest phases go first, then whole tails are cut.
    let mut text = sections.join("\n\n");
    let mut drop_at = 1;
    while text.len() > PLAIN_DIGEST_BYTES && drop_at < sections.len() && sections[drop_at].starts_with("Phase ") && sections.len() > 2 {
        sections.remove(drop_at);
        if !sections[drop_at - 1].starts_with("(earlier phases") {
            sections.insert(drop_at, "(earlier phases left out)".into());
            drop_at += 1;
        }
        text = sections.join("\n\n");
    }
    if text.len() > PLAIN_DIGEST_BYTES {
        let mut end = PLAIN_DIGEST_BYTES - 3;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('\u{2026}');
    }
    text
}

/// Task #397 (owner, 2026-11-08): the store a hit came from, as the reader
/// names it. A team's workspace lives in the project store as `teams/<team8>`,
/// so a search run at the project root finds the team records too; without a
/// name a hit would not say which memory it came from.
pub(crate) fn store_label(root: &str, relative: &str, team: &Team) -> String {
    let relative = relative.trim().trim_start_matches("./");
    if relative.is_empty() || relative == "." {
        return if root == "project" { "project".into() } else { format!("team {}", team.name) };
    }
    let own = &team.id[..8.min(team.id.len())];
    if let Some(rest) = relative.strip_prefix("teams/") {
        let id = rest.split('/').next().unwrap_or(rest);
        return if id == own { format!("team {}", team.name) } else { format!("team {id}") };
    }
    relative.to_owned()
}

/// Names the store of every hit of `find --stores all` in one run.
fn label_hits(value: &mut Value, root: &str, team: &Team) {
    let Some(results) = value["results"].as_array_mut() else { return };
    for hit in results {
        let relative = hit["store"].as_str().unwrap_or(".").to_owned();
        hit["store"] = Value::String(store_label(root, &relative, team));
    }
}

/// The arguments of a lore read. `find` searches every sub-store (task #397),
/// so the project's team records are found from the project root; `show` and
/// `plans` read their own store.
pub(crate) fn read_args<'a>(command: &'a str, words: &'a [String]) -> Vec<&'a str> {
    let mut args: Vec<&str> = std::iter::once(command).chain(words.iter().map(String::as_str)).collect();
    if command == "find" {
        args.extend(["--stores", "all"]);
    }
    args
}

/// `hgs team lore find|plans|show`: lore's own answer from the workspace (and
/// the project store in `project` and `read-only`), each result marked with
/// the store that holds it. `find` searches the sub-stores too (`--stores
/// all`); in project mode one search of the project root answers for the
/// whole project, the team's own sub-store included, so its hits are not
/// returned twice.
pub(crate) fn read(team: &Team, command: &str, words: &[String]) -> anyhow::Result<Value> {
    let (workspace, _) = workspace(team).ok_or_else(|| anyhow::anyhow!("This team keeps plain memory: it has no lore workspace (memory mode {})",
        team.settings.memory.as_ref().map_or("plain", |memory| memory.mode.as_str())))?;
    let bin = lore_bin().ok_or_else(|| anyhow::anyhow!("The lore binary was not found (HGS_LORE, beside hgs, PATH)"))?;
    let memory = team.settings.memory.clone().unwrap_or_default();
    let find = command == "find";
    let project = matches!(memory.mode.as_str(), "project" | "read-only").then(|| PathBuf::from(&memory.store)).filter(|store| !memory.store.is_empty() && store != &workspace);
    let stores: Vec<(&str, PathBuf)> = match (find, project.clone()) {
        // The project root's search answers for the whole project, its teams included.
        (true, Some(project)) => vec![("project", project)],
        (_, project) => {
            let mut stores = vec![("workspace", workspace.clone())];
            if let Some(project) = project {
                stores.push(("project", project));
            }
            stores
        }
    };
    let args = read_args(command, words);
    let roots = roots(team, &workspace);
    let mut answers = Map::new();
    let mut notes = Vec::new();
    for (name, store) in stores {
        match run_lore(&bin, &store, &args, "", &roots, None) {
            Ok(mut value) => {
                if find {
                    label_hits(&mut value, name, team);
                }
                answers.insert(name.into(), value);
            }
            Err(error) => notes.push(format!("{name}: {error}")),
        }
    }
    Ok(json!({"team": team.id, "command": command, "stores": answers, "notes": notes}))
}

/// Task #397: one detached `lore index` for a store whose records changed, so
/// its status reads built without the reader waiting; it starts only after a
/// settled pass (no write holds the store then) and lore's own marker keeps a
/// second search from starting another.
fn start_index(bin: &Path, store: &Path) {
    let mut command = Command::new(bin);
    command.arg("-C").arg(store).arg("index");
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_NAME", "GIT_COMMITTER_EMAIL"] {
        command.env_remove(name);
    }
    let _ = command.spawn();
}

/// Task 128 (docs/lore.md 4.5): the owner's export of a team's knowledge
/// records into the project store's team area, idempotent by id.
pub(crate) fn export(team: &Team) -> anyhow::Result<Value> {
    let (workspace, _) = workspace(team).ok_or_else(|| anyhow::anyhow!("This team has no lore workspace to export"))?;
    ensure_project(team)?;
    let project = super::super::project_memory::entry(&team.project_id)?;
    let store = project.store_path(&team.project_id);
    let bin = lore_bin().ok_or_else(|| anyhow::anyhow!("The lore binary was not found (HGS_LORE, beside hgs, PATH)"))?;
    let there = current(&store);
    let area = area(team);
    let records: Vec<Value> = current(&workspace)
        .into_values()
        .filter(|record| super::super::project_memory::KNOWLEDGE_KINDS.contains(&record["kind"].as_str().unwrap_or_default()))
        .filter(|record| !there.contains_key(record["id"].as_str().unwrap_or_default()))
        .map(|mut record| {
            if let Some(map) = record.as_object_mut() {
                for stamp in ["v", "by", "created", "updated"] {
                    map.remove(stamp);
                }
                map.insert("area".into(), json!(area));
            }
            record
        })
        .collect();
    if records.is_empty() {
        return Ok(json!({"status":"nothing new","exported":0}));
    }
    ensure_area(&store, &area).map_err(|error| anyhow::anyhow!(error))?;
    let roots = roots(team, &store);
    let subject = format!("Zerus-Team: {}", team.id);
    let answer = run_lore(&bin, &store, &["add", "any", "--stdin", "--new"], &Value::Array(records.clone()).to_string(), &roots, Some(&subject))
        .map_err(|error| anyhow::anyhow!("The export was refused, nothing was written: {error}"))?;
    Ok(json!({"status":"exported","exported":records.len(),"area":area,"commit":answer["commit"]}))
}

fn ensure_project(team: &Team) -> anyhow::Result<()> {
    anyhow::ensure!(!team.project_id.is_empty() && super::super::project_memory::project_enabled(&team.project_id),
        "The project's memory is off: turn RustCodeLore on for the project before exporting into it");
    Ok(())
}

#[cfg(test)]
#[path = "tests/lore_sync.rs"]
mod tests;
