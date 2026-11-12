//! U62, #760 (re-added #722; owner, 2026-10-30): the merge queue. With the
//! project's `[merge_queue] enabled`, the lead's (or the owner's) accept of a
//! report does not decide it at once: Zerus merges the commit its author
//! reported into the lead's branch, runs the task's affected tests on the
//! merged commit through the test queue, and then keeps the merge and accepts
//! the task, or rolls the merge back and sends the task back to its author
//! with the failure. A conflict goes back the same way with its files. One
//! merge is under test at a time, so a rollback never drops another's work,
//! and a phase integration only checks what came after the last tested merge.

use super::model::{AttemptState, Decider, Landed, MemberState, MergeEntry, MergeLog, MergeRange, MergeWaitReason, OpenNotRun, Task, TaskStatus, Team};
use super::store::Event;
use super::test_queue::{Request, TestStore};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// How long one git step of the queue may take.
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Task #267: the integration line's branch and head: the one base every
/// hand-out starts from, and the line accepted work lands on.
pub(crate) fn integration_branch(team: &Team) -> Option<String> {
    // Task 377 (owner, 2026-11-08): the line is a named branch - the team's
    // configured base branch, or the branch recorded when the integration
    // folder was granted - never whatever the folder happens to have checked
    // out (a merger's work branch is not the line).
    let configured = team.settings.base_branch.trim();
    if !configured.is_empty() {
        return Some(configured.to_owned());
    }
    let recorded = team.line_branch.trim();
    (!recorded.is_empty()).then(|| recorded.to_owned())
}

/// The integration line's current head, or None when it cannot be read.
pub(crate) fn integration_head(team: &Team) -> Option<String> {
    let folder = team.integration_folder();
    if folder.is_empty() {
        return None;
    }
    let line = integration_branch(team)?;
    git_text(folder, &["rev-parse", &format!("refs/heads/{line}")]).filter(|head| !head.is_empty())
}

/// Task #267: once a conflict's resolve task is decided, its landing is back in
/// the queue and retries even when nothing else changed.
fn release_conflicts(team: &mut Team) -> bool {
    let decided: Vec<(u32, u32)> = team
        .tasks
        .iter()
        .filter_map(|task| {
            let entry = task.merge.as_ref().filter(|entry| entry.state == "conflict")?;
            let resolve = entry.resolve_task?;
            team.tasks.iter().any(|other| other.id == resolve && other.status.finished()).then_some((task.id, resolve))
        })
        .collect();
    for (id, _resolve) in &decided {
        if let Some(entry) = team.task_mut(*id).and_then(|task| task.merge.as_mut()) {
            entry.state = "queued".into();
            entry.retry = true;
            entry.resolve_task = None;
            entry.wait_reason = MergeWaitReason::Other;
            entry.note.clear();
        }
    }
    !decided.is_empty()
}

/// Task #19 (owner, 2026-11-10): the lead lands accepted work by hand on a
/// named line (`manual_line`).
pub(crate) fn manual(team: &Team) -> bool {
    !team.settings.manual_line.trim().is_empty()
}

/// Task #19: the line a landing names, for messages.
pub(crate) fn line_name(team: &Team) -> String {
    if manual(team) {
        format!("the line {}", team.settings.manual_line.trim())
    } else {
        "the integration line".into()
    }
}

/// Task #19: a lead who lands by hand, or a team created with working mode
/// "none": the queue's and the working mode's playbook rules (a), (b) and (h)
/// say nothing.
pub(crate) fn hand_landing(team: &Team) -> bool {
    manual(team)
        || team.working_mode.trim() == "none"
        || team.mode_kept.as_ref().is_some_and(|kept| kept["by"] == "New team")
}

/// Task #267: whether a commit is on the integration line. Task #19: on the
/// manual line's branch when the lead lands by hand, whatever the team
/// folder has checked out (it may be on another team's detached line).
pub(crate) fn landed(team: &Team, commit: &str) -> bool {
    if manual(team) {
        let line = format!("refs/heads/{}", team.settings.manual_line.trim());
        return !commit.is_empty() && ancestry_at(team.integration_folder(), commit, &line) == Ancestry::Yes;
    }
    !commit.is_empty() && ancestry(team.integration_folder(), commit) == Ancestry::Yes
}

/// Task #370: whether the integration repository knows the commit at all. Work
/// another repository holds is never this line's to land.
fn in_repository(team: &Team, commit: &str) -> bool {
    let folder = team.integration_folder();
    !folder.is_empty() && !commit.is_empty() && git(folder, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).exit == Some(0)
}

/// Task #370: whether the line already holds the change of `commit` as a
/// cherry-pick: `git cherry -v` marks a commit with an equivalent patch on the
/// other side with '-'.
fn cherry_landed(team: &Team, commit: &str) -> bool {
    let folder = team.integration_folder();
    if folder.is_empty() || commit.is_empty() {
        return false;
    }
    let Some(list) = git_text(folder, &["cherry", "-v", "HEAD", commit]) else { return false };
    list.lines().any(|line| line.starts_with('-') && line.split_whitespace().nth(1) == Some(commit))
}

/// Task #370: the commits of rejected attempts of every task in the team: a
/// merge whose range carries one would land rejected code.
///
/// Owner's team, 2026-11-10: an attempt the pipeline returned (its review or
/// test round) is not the lead's rejection. Its member fixes it with commits
/// on top, so it is the base of the accepted work; a cherry-pick of the last
/// fix alone would drop it.
fn rejected_commits(team: &Team) -> Vec<String> {
    team.tasks
        .iter()
        .flat_map(|task| task.attempts.iter())
        .filter(|attempt| attempt.state == AttemptState::Rejected && !attempt.pipeline_return && !attempt.folder_head.is_empty())
        .map(|attempt| attempt.folder_head.clone())
        .collect()
}

/// Task #443: the revision whose history is "on the line": the configured
/// branch when the line is on, or the team folder's HEAD when the line is off
/// (no `integration_folder`, the owner's p92: a hand landing in the team's own
/// folder).
fn line_revision(team: &Team) -> String {
    if manual(team) {
        return format!("refs/heads/{}", team.settings.manual_line.trim());
    }
    if team.settings.integration_folder.trim().is_empty() {
        return "HEAD".to_owned();
    }
    integration_branch(team).map_or_else(|| "HEAD".to_owned(), |line| format!("refs/heads/{line}"))
}

/// Task #443: every commit on the line in one read.
fn line_commits(team: &Team) -> Option<std::collections::HashSet<String>> {
    let folder = team.integration_folder();
    if folder.is_empty() {
        return None;
    }
    super::dispatcher::git_at(folder, &["rev-list", &line_revision(team)])
        .map(|text| text.lines().map(str::to_owned).collect())
}

/// Task #443: the commit whose landing the task waits for: the queue's entry,
/// else the reported commit its accept recorded.
fn landing_commit(task: &Task) -> &str {
    task.merge
        .as_ref()
        .map(|entry| entry.commit.as_str())
        .filter(|commit| !commit.is_empty())
        .or_else(|| {
            task.attempts
                .iter()
                .rev()
                .find(|attempt| attempt.state == AttemptState::Reported && !attempt.folder_head.is_empty())
                .map(|attempt| attempt.folder_head.as_str())
        })
        .unwrap_or_default()
}

/// Task #443 (owner, 2026-11-09): a task whose report the lead accepted and
/// whose work waits for a landing nobody decided is settled: the queue's
/// landing (queued or testing, with no failure note - it will land it) or a
/// hand landing (an accept outside the queue, no merge entry). It is never a
/// "decision waiting": it gets no report reminder and no digest item; the
/// lead's status names it in one "accepted, waiting to land" line with its
/// age. A conflict, a blocked landing or a failed one stays the lead's
/// decision (its `merge_waiting` item and the report).
pub(crate) fn waits_for_landing(task: &Task) -> bool {
    if task.landed.is_some() || landing_commit(task).is_empty() {
        return false;
    }
    match task.merge.as_ref() {
        Some(entry) => entry.note.is_empty() && matches!(entry.state.as_str(), "queued" | "testing"),
        None => task.status == TaskStatus::Accepted,
    }
}

/// Task #443: when the landing started waiting: the queue's accept, or the
/// lead's accept outside the queue.
fn landing_since(task: &Task) -> f64 {
    if let Some(entry) = task.merge.as_ref() {
        return entry.at;
    }
    task.attempts
        .iter()
        .rev()
        .find(|attempt| attempt.state == AttemptState::Reported)
        .and_then(|attempt| attempt.decided_at.or_else(|| attempt.report.as_ref().map(|report| report.at)))
        .unwrap_or_default()
}

thread_local! {
    /// Task #443: when each line was last read for its landed work. A commit
    /// becomes an ancestor only when the line moves, and one `rev-list` over a
    /// long history is not a tick's cost to pay every three seconds: the check
    /// runs at most once in `LANDING_EVERY` seconds per team.
    static LANDING_CHECKED: std::cell::RefCell<std::collections::HashMap<String, f64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Task #443: how often a tick reads the line for work it already holds.
const LANDING_EVERY: f64 = 20.0;

/// Task #443: whether this tick reads the line for `team` (at most once in
/// `LANDING_EVERY` seconds).
fn landing_due(team: &Team, now: f64) -> bool {
    let key = format!("{}\n{}", team.id, team.integration_folder());
    LANDING_CHECKED.with(|checked| {
        let mut checked = checked.borrow_mut();
        if checked.get(&key).is_some_and(|at| now - at < LANDING_EVERY) {
            return false;
        }
        checked.insert(key, now);
        true
    })
}

/// Task #443: the accepted work whose commit is already on the line but whose
/// record still waits: (task, commit), the tasks Zerus accepts and marks
/// landed by itself. One Git read, none when nothing waits, and at most one in
/// `LANDING_EVERY` seconds; a landed commit needs no second accept with
/// "merged":true.
pub(crate) fn landed_by_ancestry(team: &Team, now: f64) -> Vec<(u32, String)> {
    let waiting: Vec<(u32, String)> = team
        .tasks
        .iter()
        .filter(|task| waits_for_landing(task))
        .map(|task| (task.id, landing_commit(task).to_owned()))
        .filter(|(_, commit)| !commit.is_empty())
        .collect();
    if waiting.is_empty() || !landing_due(team, now) {
        return Vec::new();
    }
    let Some(on_line) = line_commits(team) else { return Vec::new() };
    waiting.into_iter().filter(|(_, commit)| on_line.contains(commit)).collect()
}

/// Task #443: accepts the task and records its landing by itself (no second
/// accept with "merged":true); the lead's status and the playbook read
/// `task.landed` from here on.
pub(crate) fn record_landed(team: &mut Team, landed: &[(u32, String)], now: f64, events: &mut Vec<Event>) -> anyhow::Result<bool> {
    let mut changed = false;
    for (id, commit) in landed {
        let Some(task) = team.task(*id) else { continue };
        if task.landed.as_ref().is_some_and(|landed| landed.commit == *commit) {
            continue;
        }
        if task.status == TaskStatus::Submitted {
            let short: String = commit.chars().take(9).collect();
            let reason = format!("Zerus landed it by itself: {short} is on the integration line");
            let input = json!({"task":id,"reason":reason,"merged":true});
            let actor = super::ops::Actor::Member(team.lead.clone());
            super::ops::apply(team, "accept", &input, &actor, &Default::default(), events)?;
        }
        let Some(task) = team.task_mut(*id) else { continue };
        task.landed = Some(Landed { commit: commit.clone(), by: "Zerus".into(), at: now });
        drop_landed_entry(task);
        changed = true;
    }
    if changed {
        events.push(Event::new("tasks_landed", json!({"by":"Zerus","tasks":landed.iter().map(|(id, _)| *id).collect::<Vec<_>>()})));
    }
    Ok(changed)
}

/// DCB-svn report (2026-11-12): a landing queued for an accepted task (`hgs
/// team land`) that reached the line by another way is over: its entry goes,
/// so the queue never merges it again. A merge under test keeps its entry.
pub(crate) fn drop_landed_entry(task: &mut Task) -> bool {
    let over = task.status == TaskStatus::Accepted
        && task.landed.is_some()
        && task.merge.as_ref().is_some_and(|entry| !matches!(entry.state.as_str(), "testing" | "conflict"));
    if over {
        task.merge = None;
    }
    over
}

/// Task #443: the accepted work waiting to land, as ONE line of the lead's
/// status with each task's age, and its entries; None when nothing waits. The
/// record only: the tick's `landed_by_ancestry` drops the landed tasks, and no
/// status poll starts Git for this line.
pub(crate) fn waiting_to_land(team: &Team, now: f64) -> Option<Value> {
    let mut waiting: Vec<(u32, String, f64, String)> = team
        .tasks
        .iter()
        .filter(|task| waits_for_landing(task))
        .map(|task| {
            let state = task.merge.as_ref().map(|entry| entry.state.clone()).unwrap_or_else(|| "not_queued".into());
            (task.id, landing_commit(task).to_owned(), landing_since(task), state)
        })
        .filter(|(_, commit, _, _)| !commit.is_empty())
        .collect();
    waiting.sort_by(|a, b| a.2.total_cmp(&b.2));
    if waiting.is_empty() {
        return None;
    }
    let minutes = |since: f64| ((now - since).max(0.0) / 60.0).round() as u64;
    let named: Vec<String> = waiting.iter().take(8).map(|(id, _, since, _)| format!("#{id} ({} min)", minutes(*since))).collect();
    let more = waiting.len().saturating_sub(named.len());
    let mut line = format!("accepted, waiting to land: {}", named.join(", "));
    if more > 0 {
        line.push_str(&format!(" and {more} more"));
    }
    Some(json!({"line":line, "more":more,
        "tasks":waiting.iter().map(|(id, commit, since, state)| json!({"task":id,"commit":commit,
            "age":minutes(*since),"state":state})).collect::<Vec<_>>()}))
}

/// Task #464 (owner, 2026-11-09): whether a task is a landing one - its own
/// title names the landing ("Rolling landing batch N", "Land 352 and 347").
/// Its member is the merger: hand landings are its business, never the
/// authors' and never the lead's review.
pub(crate) fn is_landing(task: &Task) -> bool {
    let title = task.title.to_lowercase();
    title.contains("rolling landing") || title.trim_start().starts_with("land ")
}

/// Task #464: the open landing task and the member that holds it, in number
/// order; None when nobody has one (the lead's inbox names the wait instead).
pub(crate) fn landing_task(team: &Team) -> Option<(String, u32)> {
    let mut found: Vec<&Task> = team
        .tasks
        .iter()
        .filter(|task| !task.status.finished() && !task.assignee.is_empty() && task.assignee != team.lead && is_landing(task))
        .collect();
    found.sort_by_key(|task| (task.status != TaskStatus::InProgress && task.status != TaskStatus::Assigned, task.id));
    found.first().map(|task| (task.assignee.clone(), task.id))
}

/// Task #464: the commit an accepted task's report named, when the task is
/// accepted and its work still waits to land.
pub(crate) fn accepted_commit(task: &Task) -> Option<String> {
    if task.status != TaskStatus::Accepted || task.landed.is_some() {
        return None;
    }
    task.attempts
        .iter()
        .rev()
        .find(|attempt| attempt.state == AttemptState::Reported && !attempt.folder_head.is_empty())
        .map(|attempt| attempt.folder_head.clone())
}

/// Task #464: one plain line the lead's starved digest uses for accepted work
/// that waits for a hand landing: how many, the merger that holds the landing
/// task and whether it is cold, or that nobody has one. None when nothing
/// waits.
pub(crate) fn landing_line(team: &Team, now: f64) -> Option<String> {
    let waiting = waiting_to_land(team, now)?;
    let hand: Vec<Value> = waiting["tasks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|task| task["state"] == "not_queued")
        .collect();
    if hand.is_empty() {
        return None;
    }
    let named: Vec<String> = hand.iter().take(8).map(|task| format!("#{} ({} min)", task["task"], task["age"])).collect();
    let mut line = format!("accepted, waiting to land: {}", named.join(", "));
    let more = hand.len().saturating_sub(named.len());
    if more > 0 {
        line.push_str(&format!(" and {more} more"));
    }
    match landing_task(team) {
        Some((member, task)) => {
            let who = team.name_of(&member);
            let cold = team.member(&member).is_some_and(|member| member.starved_since.is_some());
            Some(format!(
                "{line}; {who} {} #{task} - wake it with a hand-out (hgs team retry/continue) or land them by hand, then accept with \"merged\":true",
                if cold { "is cold with" } else { "holds" }
            ))
        }
        None => Some(format!("{line}; no merger task is open: land them by hand and accept with \"merged\":true, or add a landing task")),
    }
}

/// Task #267: the integration line for status and the lead's inbox: its branch
/// and head, the accepted work that is on it, and every accepted task that is
/// not, with the reason (the queue's state, its resolve task, or no queue).
pub(crate) fn landing(team: &Team) -> Value {    let folder = team.integration_folder();
    let branch = integration_branch(team).unwrap_or_default();
    let head = integration_head(team).unwrap_or_default();
    let mut landed = Vec::new();
    let mut unlanded = Vec::new();
    // Task #393: the line's commits in one read. A status or an inbox over
    // hundreds of accepted tasks names each task's commit and checks it
    // against this set, instead of starting Git once per task. Task #443: the
    // configured branch is the line; when the line is off it is the team
    // folder's HEAD.
    let line_set = line_commits(team);
    let on_the_line = |commit: &str| -> bool {
        match &line_set {
            Some(set) => set.contains(commit),
            None => !commit.is_empty() && ancestry(folder, commit) == Ancestry::Yes,
        }
    };
    for task in team.tasks.iter().filter(|task| task.status == TaskStatus::Accepted || task.merge.is_some()) {
        let commit = task
            .attempts
            .iter()
            .rev()
            .find(|attempt| attempt.state == AttemptState::Reported && !attempt.folder_head.is_empty())
            .map(|attempt| attempt.folder_head.clone())
            .unwrap_or_default();
        let merged = task.merge.as_ref().map(|entry| entry.merged.clone()).unwrap_or_default();
        // Task #393: a landing the queue recorded is on the line already (its
        // merged commit is what the queue kept); the finished task is skipped
        // instead of read from Git again. The landed commit is the merge the
        // queue kept, or (a task accepted before the queue, or outside it) its
        // own commit once it is on the line.
        let recorded = !merged.is_empty()
            && task.merge.as_ref().is_some_and(|entry| matches!(entry.state.as_str(), "landed" | "merged" | "accepted"));
        let on_line = if recorded {
            true
        } else if !merged.is_empty() {
            on_the_line(&merged)
        } else {
            on_the_line(&commit)
        };
        // Task #370: the line holding an equivalent patch of the accepted
        // commit (a cherry-pick) counts as landed, and so does a landing the
        // lead marked by a named line commit.
        let equivalent = !on_line && in_repository(team, &commit) && cherry_landed(team, &commit);
        let marked = task.landed.as_ref().map(|landed| landed.commit.clone()).filter(|named| !named.is_empty() && ancestry(folder, named) == Ancestry::Yes);
        if on_line || equivalent || marked.is_some() {
            landed.push(json!({"task":task.id,"commit":if merged.is_empty() { commit.clone() } else { merged.clone() },
                "equivalent":equivalent,"marked":marked}));
            continue;
        }
        if task.status != TaskStatus::Accepted {
            continue;
        }
        // Task #370: a task whose output is outside this repository (another
        // project's work) never waits as unlanded.
        if !commit.is_empty() && !in_repository(team, &commit) {
            landed.push(json!({"task":task.id,"commit":commit,"outside":true}));
            continue;
        }
        let (state, why, resolve) = match task.merge.as_ref() {
            Some(entry) if !entry.note.is_empty() => (entry.state.clone(), entry.note.clone(), entry.resolve_task),
            Some(entry) => (
                entry.state.clone(),
                match entry.state.as_str() {
                    "queued" => "queued for the integration line".to_owned(),
                    "testing" => "merged into the integration line; its affected tests run".to_owned(),
                    "conflict" => "the integration line conflicts; a resolve task waits".to_owned(),
                    other => format!("the queue is {other}"),
                },
                entry.resolve_task,
            ),
            None => ("not_queued".to_owned(), "accepted, but no landing is queued".to_owned(), None),
        };
        unlanded.push(json!({"task":task.id,"commit":commit,"state":state,"why":why,"resolve":resolve,
            "blocked":task.merge.as_ref().is_some_and(|entry| entry.blocked)}));
    }
    json!({"folder":folder,"branch":branch,"head":head,"landed":landed,"unlanded":unlanded})
}

/// Task #267: ONE resolve task per conflict, for the reviewer (the lead when the
/// team has none), naming both commits and the files. The conflicting landing
/// stays undecided, and later landings wait behind this task.
fn resolve_task(team: &mut Team, id: u32, entry: &MergeEntry, head: &str, files: &[String], events: &mut Vec<Event>) -> anyhow::Result<u32> {
    let short = |commit: &str| commit.chars().take(9).collect::<String>();
    let next = team.tasks.iter().map(|task| task.id).max().unwrap_or(0).checked_add(1).ok_or_else(|| anyhow::anyhow!("Task numbers are exhausted"))?;
    let phase_n = team.task(id).map(|task| task.phase).unwrap_or(1);
    let reviewer = team
        .members
        .iter()
        .filter(|member| member.state == MemberState::Active && member.id != team.lead)
        .find(|member| member.role.eq_ignore_ascii_case("reviewer"))
        .map(|member| member.id.clone())
        .unwrap_or_else(|| team.lead.clone());
    let files_text = if files.is_empty() { "the files git named".to_owned() } else { files.join(", ") };
    let task = Task {
        id: next,
        phase: phase_n,
        title: format!("Resolve the integration conflict of #{id}"),
        description: format!(
            "The accepted work of #{id} ({}) conflicts with the integration line {} in: {}.\n\n\
             Merge the integration line into the work of #{id} (the git-merge-lead template), resolve these files, commit and report. \
             The queue lands #{id} once this task is decided; later landings wait behind it.",
            short(&entry.commit),
            short(head),
            files_text
        ),
        criteria: vec![format!("The integration line merges into #{} without conflicts, and its affected tests pass on the landing", id)],
        output: files.to_vec(),
        origin: Some(id),
        assignee: reviewer.clone(),
        status: TaskStatus::Pending,
        ..Default::default()
    };
    let resolve = task.id;
    team.tasks.push(task);
    if let Some(record) = team.phases.iter_mut().find(|phase| phase.n == phase_n) {
        record.tasks.push(resolve);
        record.tasks.sort_unstable();
    }
    events.push(Event::new("merge_resolve_task", json!({"task":id,"resolve":resolve,"reviewer":reviewer,"files":files,"commit":entry.commit,"head":head})));
    Ok(resolve)
}

/// The catalog merge driver `.gitattributes` names `i18n`: the reviewed
/// supplement merges as a key union and the generated catalog is regenerated
/// (scripts/i18n-merge.py). Zerus passes it with `-c` on its own merge paths and
/// never writes it into a repository config: C2 refuses a repository's own merge
/// drivers, which are arbitrary commands.
pub(crate) const I18N_MERGE_DRIVER: &str = if cfg!(windows) {
    "python scripts/i18n-merge.py %O %A %B %L %P"
} else {
    "python3 scripts/i18n-merge.py %O %A %B %L %P"
};

/// The `-c` argument that enables [`I18N_MERGE_DRIVER`] for one Git command.
pub(crate) fn i18n_driver_config() -> String {
    format!("merge.i18n.driver={I18N_MERGE_DRIVER}")
}

/// A bounded per-suite tested-point map; the merge log is bounded too.
const MERGE_SUITES: usize = 64;
/// One bounded batch of open ranges (the ordinary team task limit).
const MERGE_RANGES: usize = 500;

/// The project turned the queue on (the owner may override it in the team).
pub(crate) fn enabled(team: &Team) -> bool {
    // Task #19: a team whose lead lands by hand runs no queue.
    if team.integration_folder().is_empty() || manual(team) {
        return false;
    }
    let root = Path::new(team.integration_folder());
    let overrides = team.settings.tests.as_ref();
    let Ok(loaded) = crate::test_discipline::config::load_project(root, overrides) else { return false };
    if loaded.active() && loaded.config.merge_queue.enabled {
        return true;
    }
    // Owner, 2026-11-09: when neither the project nor the team says, a team
    // whose members work in their own clones or worktrees has the queue on:
    // their accepted branches live there, so nothing else lands them.
    let said = overrides.is_some_and(|fields| fields.get("merge_queue").is_some())
        || std::fs::read_to_string(root.join(crate::test_discipline::config::FILE)).is_ok_and(|text| text.contains("merge_queue"));
    !said && team.members.iter().any(|member| {
        member.id != team.lead && matches!(member.folder, super::model::MemberFolder::Worktree | super::model::MemberFolder::Clone)
    })
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
    if folder.is_empty() || super::dispatcher::same_path(&folder, team.integration_folder()) {
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

/// Task 174: why the owner's integration folder cannot take a merge, or None
/// (unset, or a git checkout of the team's repository on a branch).
pub(crate) fn integration_problem(team: &Team) -> Option<String> {
    let folder = team.settings.integration_folder.trim();
    if folder.is_empty() {
        return None;
    }
    if !Path::new(folder).is_dir() {
        return Some(format!("the integration folder {folder} does not exist"));
    }
    if !team.folder.is_empty() && !super::dispatcher::same_repository(folder, &team.folder, Instant::now() + GIT_TIMEOUT) {
        return Some(format!("the integration folder {folder} is not a checkout of the team's repository ({})", team.folder));
    }
    match git_text(folder, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Some(branch) if !branch.is_empty() => None,
        _ => Some(format!("the integration folder {folder} is not on a branch (detached HEAD)")),
    }
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
    /// Task #370: the range carries a rejected attempt's commit; the queue
    /// refuses the merge and offers a cherry-pick of the task's own commit.
    Blocked(String),
    Failed(String),
}

/// Task #1 (team ede724c4): a merge-queue git about to start. One started while
/// this thread holds the team store's write lock is a warning in the lock log.
fn note_git(folder: &str, args: &[&str]) {
    let mut named = vec![folder.to_owned()];
    named.extend(args.iter().take(7).map(|arg| (*arg).to_owned()));
    super::store::note_process("git", &named);
}

fn git(folder: &str, args: &[&str]) -> super::templates::Outcome {
    note_git(folder, args);
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
    // Task #393: a blank folder is no repository; no `git -C ""`.
    if folder.trim().is_empty() {
        return None;
    }
    note_git(folder, args);
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
pub(crate) fn merge(lead: &str, entry: &MergeEntry, message: &str, rejected: &[String]) -> Merged {
    let listing = git_text(lead, &["config", "--list", "--show-scope"]).unwrap_or_default();
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
    // Task #370: never merge a range that carries a commit of a rejected
    // attempt of any task; cherry-pick the task's own commit instead.
    if !entry.cherry_pick {
        for commit in rejected.iter().filter(|commit| **commit != entry.commit) {
            let carried = git(lead, &["merge-base", "--is-ancestor", commit, &entry.commit]).exit == Some(0)
                && git(lead, &["merge-base", "--is-ancestor", commit, "HEAD"]).exit != Some(0);
            if carried {
                return Merged::Blocked(format!(
                    "The accepted commit {} carries {}, a commit of a rejected attempt; a merge would land rejected code. Land the task's own commit as a cherry-pick instead: hgs team land --json with \"cherry_pick\":true.",
                    entry.commit.chars().take(9).collect::<String>(),
                    commit.chars().take(9).collect::<String>()
                ));
            }
        }
    }
    if git(lead, &["merge-base", "--is-ancestor", &entry.commit, "HEAD"]).exit == Some(0) {
        return Merged::Done { merged: prev.clone(), prev };
    }
    let driver = i18n_driver_config();
    let outcome = if entry.cherry_pick {
        git(lead, &["-c", driver.as_str(), "cherry-pick", "-x", &entry.commit])
    } else {
        git(lead, &["-c", driver.as_str(), "merge", "--no-edit", "-m", message, &entry.commit])
    };
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
    let _ = if entry.cherry_pick { git(lead, &["cherry-pick", "--abort"]) } else { git(lead, &["merge", "--abort"]) };
    if files.is_empty() {
        Merged::Failed(format!("The merge failed:\n{}", outcome.tail))
    } else {
        Merged::Conflict(files)
    }
}

fn range_files(team: &Team, prev: &str, merged: &str) -> anyhow::Result<Vec<String>> {
    let changed = git_text(team.integration_folder(), &["diff", "--name-only", "--no-renames", prev, merged])
        .ok_or_else(|| anyhow::anyhow!("git could not read the range {prev}..{merged}"))?;
    Ok(changed.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect())
}

/// Fixed metadata per task, and at most one ordinary task batch per operation.
pub(crate) fn open_merge_ranges(team: &Team) -> Vec<MergeRange> {
    team.tasks.iter().filter_map(|task| task.untested_merge.clone()).take(MERGE_RANGES).collect()
}

/// Clear only the exact snapshots proved by this kept merge or accepted build.
pub(crate) fn close_merge_ranges(team: &mut Team, ranges: &[MergeRange]) {
    for range in ranges.iter().take(MERGE_RANGES) {
        if let Some(task) = team.task_mut(range.task) {
            if task.untested_merge.as_ref() == Some(range) {
                task.untested_merge = None;
            }
        }
    }
}

/// Round-4 saves represented an untested merge as a synthetic suite. Move the
/// exact old shape, including its original start, without reintroducing it from
/// the bounded log on the next keep.
pub(crate) fn migrate_legacy_ranges(team: &mut Team) -> bool {
    let legacy: Vec<_> = team.merge_not_run.iter().filter_map(|(suite, open)| {
        let id = suite.strip_prefix("merge-")?.parse::<u32>().ok()?;
        let prefix = format!("not run here: {suite} runs on all; the merge of ");
        let (_, merged) = open.message.strip_prefix(&prefix)?.split_once(" was not tested on ")?;
        (!open.since.is_empty() && !merged.is_empty() && team.task(id).is_some())
            .then(|| (suite.clone(), open.message.clone(), MergeRange { task: id, prev: open.since.clone(), merged: merged.to_owned() }))
    }).take(MERGE_RANGES).collect();
    for (suite, message, range) in &legacy {
        if let Some(task) = team.task_mut(range.task) {
            if task.untested_merge.is_none() { task.untested_merge = Some(range.clone()); }
        }
        team.merge_not_run.remove(suite);
        for log in &mut team.merge_log { log.not_run.retain(|old| old != message); }
    }
    !legacy.is_empty()
}

/// The requests a merge queues, and the suites of another platform it records
/// as not run here because no test runner of their class exists.
#[derive(Debug, Default)]
pub(crate) struct Affected {
    pub requests: Vec<Request>,
    pub not_run: Vec<String>,
    pub tested_ranges: Vec<MergeRange>,
    pub not_run_since: std::collections::BTreeMap<String, String>,
}

/// The changed merge range and the HEAD on which its tests will run.
#[derive(Clone, Copy)]
pub(crate) struct TestRange<'a> {
    prev: &'a str,
    merged: &'a str,
    run_at: &'a str,
}

/// The task's affected tests on the merged range, as test queue requests: the
/// project's map over `prev..merged`, each run's command in its folder. A suite
/// whose declared platforms exclude this computer is queued only when a
/// configured test runner of its class exists; otherwise it is named as not run
/// here and the merge decides on the suites that did run.
pub(crate) fn affected_requests(
    team: &Team,
    id: u32,
    range: TestRange<'_>,
    now: f64,
    host: &[String],
    runners: &[super::test_runners::Runner],
) -> anyhow::Result<Affected> {
    use crate::test_discipline::{affected, config, durations, map};
    let TestRange { prev, merged, run_at } = range;
    let lead = Path::new(team.integration_folder());
    let loaded = config::load(lead, team.settings.tests.as_ref())?;
    let config = &loaded.config;
    let mut files: std::collections::BTreeSet<String> = range_files(team, prev, merged)?.into_iter().collect();
    // Select once over the union, so overlapping suites/targets are not run twice.
    // A provider selected only by an older open range still has to run here.
    let mut ranges = Vec::new();
    for range in open_merge_ranges(team) {
        if ancestry_at(team.integration_folder(), &range.merged, run_at) == Ancestry::Yes {
            let changed = range_files(team, &range.prev, &range.merged)?;
            files.extend(changed.iter().cloned());
            ranges.push((range, changed));
        }
    }
    let files: Vec<String> = files.into_iter().collect();
    if files.is_empty() && ranges.is_empty() {
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
    let available: std::collections::BTreeSet<&str> = config.providers.iter().filter(|provider| {
        let platforms = provider.platform_classes();
        super::test_runners::runs_here(&platforms, host) || runners.iter().any(|runner| platforms.contains(&runner.platform))
    }).map(|provider| provider.name.as_str()).collect();
    let mut tested_ranges = Vec::new();
    let mut not_run_since: std::collections::BTreeMap<String, String> = Default::default();
    for (range, files) in ranges {
        let selected = affected::select(lead, config, &test_map, &durations, &files, &Default::default(), false);
        for run in &selected.runs {
            if !available.contains(run.provider.as_str()) {
                let start = not_run_since.entry(run.provider.clone()).or_insert_with(|| range.prev.clone());
                if *start != range.prev && ancestry_at(team.integration_folder(), &range.prev, start) == Ancestry::Yes {
                    *start = range.prev.clone();
                }
            }
        }
        if selected.unmapped.is_empty() && selected.manual.is_empty() && selected.elsewhere.is_empty()
            && selected.runs.iter().all(|run| available.contains(run.provider.as_str())) {
            tested_ranges.push(range);
        }
    }
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
            performance: provider.performance,
            compare: String::new(),
        });
    }
    Ok(Affected { requests, not_run, tested_ranges, not_run_since })
}

/// One step of the queue for a team: the merge under test decides when all its
/// results are in; else the oldest queued entry merges. `host` is this
/// computer's test classes and `runners` the owner's configured test runners;
/// a suite of another class without a runner is recorded as not run here. True
/// when it changed something.
#[cfg(test)]
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
#[cfg(test)]
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
    let mut queue = Vec::new();
    let changed = step_into(team, database, now, events, affected_hosts, &mut queue)?;
    for request in &queue {
        database.enqueue(request)?;
    }
    Ok(changed)
}

/// Task #1 (team ede724c4): why a step's outcome, computed on `base` (as
/// `ours`) outside the store's write lock, no longer applies to `theirs`, the
/// team as stored under the lock; None when it applies. It does not apply when
/// another writer changed a task the step changed, in its status, its landing
/// entry, its landed record or any field the step also wrote; when a task the
/// step added took an id another writer used meanwhile; or when another writer
/// changed a team field the step also wrote. Every other change of theirs (a
/// member's report, another accept queued, a message) merges with it.
pub(crate) fn superseded(base: &Value, ours: &Value, theirs: &Value) -> Option<String> {
    let empty = serde_json::Map::new();
    let (base_map, our_map, their_map) =
        (base.as_object().unwrap_or(&empty), ours.as_object().unwrap_or(&empty), theirs.as_object().unwrap_or(&empty));
    for (key, our) in our_map {
        if matches!(key.as_str(), "tasks" | "revision" | "updated") {
            continue;
        }
        let was = base_map.get(key).unwrap_or(&Value::Null);
        if our != was && their_map.get(key).unwrap_or(&Value::Null) != was {
            return Some(format!("the team's {key} changed meanwhile"));
        }
    }
    let by_id = |team: &Value| -> std::collections::BTreeMap<String, Value> {
        team["tasks"].as_array().into_iter().flatten().map(|task| (task["id"].to_string(), task.clone())).collect()
    };
    let (base_tasks, our_tasks, their_tasks) = (by_id(base), by_id(ours), by_id(theirs));
    for (id, our) in &our_tasks {
        let Some(was) = base_tasks.get(id) else {
            if their_tasks.contains_key(id) {
                return Some(format!("task #{id}, which this step added, was added by another writer meanwhile"));
            }
            continue;
        };
        if our == was {
            continue;
        }
        let Some(their) = their_tasks.get(id) else { return Some(format!("task #{id} is no longer in the team")) };
        for key in ["status", "merge", "landed"] {
            if their.get(key) != was.get(key) {
                return Some(format!("task #{id}'s {key} changed meanwhile"));
            }
        }
        for (key, value) in our.as_object().unwrap_or(&empty) {
            if was.get(key) != Some(value) && their.get(key) != was.get(key) {
                return Some(format!("task #{id}'s {key} changed meanwhile"));
            }
        }
    }
    for id in base_tasks.keys() {
        if !our_tasks.contains_key(id) && their_tasks.get(id) != base_tasks.get(id) {
            return Some(format!("task #{id} changed meanwhile"));
        }
    }
    None
}

/// Task #1 (team ede724c4): a superseded outcome is retried on the next tick
/// from the team as stored. A merge commit this step made that the stored team
/// does not record is rolled back first while the line still stands on it, so
/// the retry merges (and tests) the landing again instead of finding it on the
/// line already; a task another writer marked landed keeps it.
pub(crate) fn undo_superseded(base: &Team, ours: &Team, theirs: Option<&Team>) -> Option<String> {
    let folder = ours.integration_folder();
    for task in &ours.tasks {
        let Some(entry) = task.merge.as_ref().filter(|entry| !entry.merged.is_empty() && entry.prev != entry.merged) else { continue };
        let before = base.task(task.id).and_then(|task| task.merge.as_ref()).map(|entry| entry.merged.as_str()).unwrap_or_default();
        if before == entry.merged {
            continue;
        }
        let stored = theirs.and_then(|team| team.task(task.id));
        if stored.is_some_and(|task| task.landed.is_some() || task.merge.as_ref().is_some_and(|found| found.merged == entry.merged)) {
            continue;
        }
        return Some(roll_back(folder, entry).note());
    }
    None
}

/// Task #1 (team ede724c4): the step whose test requests are collected in
/// `queue` instead of enqueued: the dispatcher runs it on a copy of the team
/// outside the store's write lock, and enqueues them only once its outcome is
/// written.
pub(crate) fn step_into<F>(
    team: &mut Team,
    database: &TestStore,
    now: f64,
    events: &mut Vec<Event>,
    affected_hosts: F,
    queue: &mut Vec<Request>,
) -> anyhow::Result<bool>
where
    F: FnOnce() -> anyhow::Result<(Vec<String>, Vec<super::test_runners::Runner>)>,
{
    let mut migrated = migrate_legacy_ranges(team);
    // Task 377 (owner, 2026-11-08): the line's branch is recorded once, from
    // the folder it was granted on; later checkouts there never change it.
    if team.line_branch.trim().is_empty() && !team.settings.integration_folder.trim().is_empty() {
        let folder = team.integration_folder();
        if let Some(branch) = git_text(folder, &["symbolic-ref", "--quiet", "--short", "HEAD"]).filter(|branch| !branch.is_empty()) {
            team.line_branch = branch;
            migrated = true;
        }
    }
    // Task #267: a conflict's resolve task decided puts its landing back in the
    // queue; nothing lands while a conflict waits for its resolve task.
    let released = release_conflicts(team);
    migrated = migrated || released;
    // DCB-svn report (2026-11-12): the oldest entry was an accepted task the
    // lead had marked landed; the queue merged it again every tick and every
    // later landing waited behind it.
    let mut dropped = Vec::new();
    for task in &mut team.tasks {
        if drop_landed_entry(task) {
            dropped.push(task.id);
        }
    }
    if !dropped.is_empty() {
        events.push(Event::new("merge_dropped_landed", json!({"tasks":dropped})));
        migrated = true;
    }
    let legacy: Vec<_> = team.tasks.iter().filter_map(|task| task.merge.as_ref()
        .filter(|entry| matches!(entry.state.as_str(), "waiting" | "reverting"))
        .map(|entry| (task.id, task.status, entry.clone()))).collect();
    for (id, status, entry) in legacy {
        if entry.state == "reverting" && (matches!(status, TaskStatus::Pending | TaskStatus::Cancelled)
            || entry.merged.is_empty() || ancestry(team.integration_folder(), &entry.merged) == Ancestry::No) {
            record_untested_range(team, id, &entry, events);
            team.task_mut(id).unwrap().merge = None;
            events.push(Event::new("merge_migration_dropped", json!({"task":id})));
        } else if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
            found.state = "queued".into();
            if found.note.is_empty() { found.note = "A saved waiting merge was requeued for the current rule".into(); }
            found.retry = false;
            found.requests.clear(); found.not_run.clear(); found.not_run_since.clear(); found.tested_ranges.clear();
        }
        migrated = true;
    }
    if let Some(id) = team.tasks.iter().find(|task| task.merge.as_ref().is_some_and(|entry| entry.state == "testing")).map(|task| task.id) {
        let mut entry = team.task(id).and_then(|task| task.merge.clone()).unwrap_or_default();
        let mut results = Vec::new();
        for request in &entry.requests {
            match database.result_of(request)? {
                Some(record) => results.push(record),
                None => return Ok(migrated),
            }
        }
        for record in results.iter().filter(|record| record.status == "not_run") {
            push_not_run(&mut entry.not_run, &record.excerpt);
            entry.not_run_since.entry(record.suite.clone()).or_insert_with(|| entry.prev.clone());
        }
        let not_run: Vec<&str> = results.iter().filter(|record| record.status == "not_run").map(|record| record.excerpt.as_str()).collect();
        if !not_run.is_empty() {
            // The persisted entry has no suite selection per range. Keep all
            // ranges open rather than closing one whose missing suite is unknown.
            entry.tested_ranges.clear();
            events.push(Event::new("merge_not_run", json!({"task":id,"suites":not_run})));
        }
        let failed: Vec<String> = results
            .iter()
            .filter(|record| !matches!(record.status.as_str(), "passed" | "flaky" | "reused" | "not_run"))
            .map(|record| format!("{} {} (record {})", record.suite, record.status, record.id))
            .collect();
        if failed.is_empty() {
            keep(team, id, &entry, results.iter().filter(|record| record.status != "not_run").map(|record| record.suite.clone()).collect(), now, events)?;
        } else {
            // Task 374 (owner, 2026-11-08): a run that could not start - a
            // missing module, a missing toolchain, nothing in a report - is an
            // environment failure, not the work's. The merge is rolled back,
            // the landing stays queued waiting for the lead's retry after the
            // fix, and the task is never reopened.
            let environment: Vec<&super::test_queue::Record> = results
                .iter()
                .filter(|record| {
                    !matches!(record.status.as_str(), "passed" | "flaky" | "reused")
                        && record.report.is_none()
                        && environment_excerpt(&record.excerpt)
                })
                .collect();
            if !environment.is_empty() && environment.len() == failed.len() {
                let rolled = roll_back(team.integration_folder(), &entry).note();
                let reason = format!(
                    "The merge queue's tests could not start: an environment failure, not this work's. {rolled} The landing stays queued; the task is not reopened. Fix the dependency or toolchain and retry (hgs team merge --json with retry).\n{}",
                    environment
                        .iter()
                        // Task #18: an environment failure Zerus named keeps its
                        // whole text, the owner's step included.
                        .map(|record| match record.excerpt.find(super::environment::MARKER) {
                            Some(at) => format!("{}: {}", record.suite, &record.excerpt[at..]),
                            None => format!("{}: {}", record.suite, record.excerpt.lines().last().unwrap_or_default()),
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                events.push(Event::new("merge_environment_failed", json!({"task":id,"reason":reason})));
                return Ok(set_queue_note(team, id, &reason, MergeWaitReason::Other, "merge_environment_failed", events));
            }
            let excerpt = results
                .iter()
                .find(|record| !matches!(record.status.as_str(), "passed" | "flaky" | "reused" | "not_run"))
                .map(|record| record.excerpt.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
                .unwrap_or_default();
            let rolled = roll_back(team.integration_folder(), &entry).note();
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
    // Task #267: one landing at a time, in acceptance order: while a conflict
    // waits for its resolve task, later landings queue behind it.
    if team.tasks.iter().any(|task| task.merge.as_ref().is_some_and(|entry| entry.state == "conflict")) {
        return Ok(migrated);
    }
    let Some(id) = team
        .tasks
        .iter()
        .filter_map(|task| task.merge.as_ref().filter(|entry| entry.state == "queued").map(|entry| (entry.at, task.id)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
    else {
        return Ok(migrated);
    };
    let mut entry = team.task(id).and_then(|task| task.merge.clone()).unwrap_or_default();
    let (head, map) = retry_stamps(team);
    if !entry.merged.is_empty() {
        let reachable = ancestry(team.integration_folder(), &entry.merged);
        if reachable != Ancestry::No {
            return resolve_reachable(team, id, RecordedMerge { entry, head: &head, reachable }, now, events, affected_hosts, queue).map(|changed| changed || migrated);
        }
        // A hand reset or revert removed the merge: the commit can be merged again.
        if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
            found.note.clear();
        found.wait_reason = MergeWaitReason::Other;
        found.tested_ranges.clear();
            found.merged.clear();
            found.prev.clear();
            found.retry = false;
        }
        entry = team.task(id).and_then(|task| task.merge.clone()).unwrap_or_default();
    }
    if entry.wait_reason != MergeWaitReason::Busy && !entry.note.is_empty() && !entry.retry && !entry.wait_head.is_empty() && entry.wait_head == head && entry.wait_map == map {
        return Ok(migrated);
    }
    if entry.retry {
        if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
            found.retry = false;
        }
    }
    // Task 174: an owner's integration folder that is not a checkout of the
    // team's repository holds the queue with that note; nothing is merged.
    if let Some(note) = integration_problem(team) {
        if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
            if found.note == note && found.wait_reason == MergeWaitReason::Busy {
                return Ok(migrated);
            }
            found.note = note;
            found.wait_reason = MergeWaitReason::Busy;
        }
        return Ok(true);
    }
    let title = team.task(id).map(|task| task.title.clone()).unwrap_or_default();
    let short: String = entry.commit.chars().take(8).collect();
    let message = format!("Merge accepted work from {} ({short}) (#{id}): {title}", team.name_of(&entry.author));
    // Task 377 (owner, 2026-11-08): the line is a named branch; a folder on
    // another checkout lands nothing and says so, instead of merging the task
    // into a work branch.
    if let Some(line) = integration_branch(team) {
        let folder = team.integration_folder().to_owned();
        let on = git_text(&folder, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_default();
        if !on.is_empty() && on != line {
            let note = format!(
                "The integration folder is on {on}, not the line {line}: check out {line} there and the queue lands this task (hgs team status shows the line)."
            );
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                if found.note != note || found.wait_reason != MergeWaitReason::Other {
                    found.note = note;
                    found.wait_reason = MergeWaitReason::Other;
                    events.push(Event::new("merge_wrong_checkout", json!({"task":id,"folder":folder,"on":on,"line":line})));
                }
            }
            return Ok(migrated);
        }
    }
    let rejected = rejected_commits(team);
    match merge(team.integration_folder(), &entry, &message, &rejected) {
        Merged::Busy(note) => {
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                if found.note == note && found.wait_reason == MergeWaitReason::Busy {
                    return Ok(migrated);
                }
                found.note = note;
                found.wait_reason = MergeWaitReason::Busy;
            }
            Ok(true)
        }
        // Task #370: the landing refuses and waits for a cherry-pick of the
        // task's own commit; nothing is merged and the task stays accepted.
        Merged::Blocked(note) => {
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                if found.note == note && found.blocked {
                    return Ok(migrated);
                }
                found.state = "blocked".into();
                found.blocked = true;
                found.note = note;
                found.wait_reason = MergeWaitReason::Other;
                found.retry = false;
            }
            events.push(Event::new("landing_blocked", json!({"task":id,"commit":entry.commit})));
            Ok(true)
        }
        Merged::Conflict(files) => {
            // Task #267: ONE resolve task for the reviewer, naming both
            // commits and the files; the landing stays undecided and later
            // landings wait behind it instead of meeting the same conflict.
            let head = integration_head(team).unwrap_or_default();
            let resolve = match team.task(id).and_then(|task| task.merge.as_ref()).and_then(|entry| entry.resolve_task) {
                Some(resolve) if team.tasks.iter().any(|task| task.id == resolve && !task.status.finished()) => resolve,
                _ => resolve_task(team, id, &entry, &head, &files, events)?,
            };
            let note = format!(
                "the integration line {} conflicts in {}; resolve task #{resolve} waits",
                head.chars().take(9).collect::<String>(),
                if files.is_empty() { "the files git named".to_owned() } else { files.join(", ") }
            );
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                found.state = "conflict".into();
                found.conflict_files = files.clone();
                found.resolve_task = Some(resolve);
                found.note = note;
                found.wait_reason = MergeWaitReason::Conflict;
            }
            events.push(Event::new("merge_conflict", json!({"task":id,"files":files,"resolve":resolve,"head":head})));
            Ok(true)
        }
        Merged::Failed(text) => {
            return_to_author(team, id, &format!("The merge queue could not merge it: {text}"), events);
            Ok(true)
        }
        Merged::Done { prev, merged } => {
            // Task 377: a merge regenerates the project's generated files and
            // keeps them in the merge commit.
            let merged = regenerate_and_amend(Path::new(team.integration_folder())).unwrap_or(merged);
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                found.prev = prev.clone();
                found.merged = merged.clone();
            }
            let affected = if prev == merged && open_merge_ranges(team).is_empty() {
                Affected::default()
            } else {
                let (host, runners) = match affected_hosts() {
                    Ok(pair) => pair,
                    Err(error) => {
                        let reason = format!("The test runners could not be read: {error}");
                        return requeue_after_error(team, id, &prev, &merged, &reason, false, events);
                    }
                };
                match affected_requests(team, id, TestRange { prev: &prev, merged: &merged, run_at: &merged }, now, &host, &runners) {
                    Ok(affected) => affected,
                    Err(error) => {
                        let reason = format!("The affected tests could not be selected: {error}");
                        return requeue_after_error(team, id, &prev, &merged, &reason, true, events);
                    }
                }
            };
            queue.extend(affected.requests.iter().cloned());
            events.push(Event::new("merged", json!({"task":id,"prev":prev,"merged":merged,"tests":affected.requests.len(),"not_run":affected.not_run.clone()})));
            if !affected.not_run.is_empty() {
                events.push(Event::new("merge_not_run", json!({"task":id,"suites":affected.not_run.clone()})));
            }
            if affected.requests.is_empty() {
                let entry = MergeEntry { prev, merged, not_run: affected.not_run, tested_ranges: affected.tested_ranges, not_run_since: affected.not_run_since, ..entry };
                keep(team, id, &entry, Vec::new(), now, events)?;
            } else if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                found.state = "testing".into();
                found.note.clear();
                found.wait_reason = MergeWaitReason::Other;
                found.tested_ranges.clear();
                found.retry = false;
                found.wait_head.clear();
                found.wait_map.clear();
                found.selection_failed = 0;
                found.prev = prev;
                found.merged = merged;
                found.requests = affected.requests.iter().map(|request| request.id.clone()).collect();
                found.not_run = affected.not_run;
                found.not_run_since = affected.not_run_since;
                found.tested_ranges = affected.tested_ranges;
            }
            Ok(true)
        }
    }
}

/// Task 374 (owner, 2026-11-08): the output of a suite that never started -
/// its module, its toolchain or its test tool was missing - so the result says
/// nothing about the merged work and must not reopen the task.
fn environment_excerpt(excerpt: &str) -> bool {
    if excerpt.contains(super::environment::MARKER) {
        return true;
    }
    let text = excerpt.to_lowercase();
    [
        "err_module_not_found",
        "cannot find module",
        "module_not_found",
        "is not recognized as an internal or external command",
        "command not found",
        "toolchain",
    ]
    .iter()
    .any(|needle| text.contains(needle))
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
    // Task #393: the answers a process already read, per (folder, commit,
    // HEAD): a status or an inbox asks the same line for every accepted task,
    // and the worker's next tick asks again while HEAD stands.
    if folder.trim().is_empty() || commit.is_empty() {
        return Ancestry::Unknown;
    }
    let key = (folder.to_owned(), commit.to_owned(), super::dispatcher::head_key(folder));
    if let Ok(cache) = ancestry_cache().lock() {
        if let Some(answer) = cache.get(&key) {
            return *answer;
        }
    }
    let answer = ancestry_at(folder, commit, "HEAD");
    // Unknown is not remembered: a transient failure is no answer.
    if answer != Ancestry::Unknown {
        if let Ok(mut cache) = ancestry_cache().lock() {
            if cache.len() > 4096 {
                cache.clear();
            }
            cache.insert(key, answer);
        }
    }
    answer
}

type AncestryKey = (String, String, String);

fn ancestry_cache() -> &'static std::sync::Mutex<std::collections::HashMap<AncestryKey, Ancestry>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<AncestryKey, Ancestry>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn ancestry_at(folder: &str, commit: &str, head: &str) -> Ancestry {
    if commit.is_empty() || head.is_empty() {
        return Ancestry::Unknown;
    }
    match git(folder, &["merge-base", "--is-ancestor", commit, head]).exit {
        Some(0) => Ancestry::Yes,
        Some(1) => Ancestry::No,
        _ => Ancestry::Unknown,
    }
}

/// Task 377 (owner, 2026-11-08): after a merge, run the project's declared
/// generators (a generated catalog, for one) in the line's folder and fold
/// their output into the merge commit, so a merge keeps every side's strings.
/// Returns the amended head when the generators changed anything.
pub(crate) fn regenerate_and_amend(folder: &Path) -> Option<String> {
    let loaded = crate::test_discipline::load(folder, None).ok()?;
    let commands = loaded.config.generated;
    if commands.is_empty() {
        return None;
    }
    for command in &commands {
        let mut process = super::test_queue::shell(command, folder);
        process.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        let _ = process.status();
    }
    let folder = folder.to_string_lossy().to_string();
    let changed = git_text(&folder, &["status", "--porcelain", "--untracked-files=no"]).unwrap_or_default();
    if changed.is_empty() {
        return None;
    }
    let hooks = format!("core.hooksPath={}", if cfg!(windows) { "NUL" } else { "/dev/null" });
    let _ = git(&folder, &["-c", hooks.as_str(), "add", "-A"]);
    let amended = git(&folder, &["-c", hooks.as_str(), "-c", "commit.gpgsign=false", "commit", "--amend", "--no-edit"]);
    if amended.exit != Some(0) {
        return None;
    }
    git_text(&folder, &["rev-parse", "HEAD"]).filter(|head| !head.is_empty())
}

/// A stable fingerprint of the project's tests map and the team's override.
fn tests_stamp(team: &Team) -> String {
    let text = std::fs::read_to_string(Path::new(team.integration_folder()).join(".zerus/tests.toml")).unwrap_or_default();
    let settings = team.settings.tests.as_ref().map(|value| value.to_string()).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(text.as_bytes());
    digest.update(settings.as_bytes());
    format!("{:x}", digest.finalize())
}

/// The HEAD and tests-map fingerprint a queued retry remembers.
fn retry_stamps(team: &Team) -> (String, String) {
    (git_text(team.integration_folder(), &["rev-parse", "HEAD"]).unwrap_or_default(), tests_stamp(team))
}

/// Store a queued entry's reason once; a changed HEAD or map refreshes the
/// stamps without repeating the event.
fn set_queue_note(team: &mut Team, id: u32, reason: &str, wait_reason: MergeWaitReason, kind: &'static str, events: &mut Vec<Event>) -> bool {
    let (head, map) = retry_stamps(team);
    let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) else { return false };
    if found.state == "queued" && found.note == reason && found.wait_reason == wait_reason && found.wait_head == head && found.wait_map == map {
        return false;
    }
    let announce = found.state != "queued" || found.note != reason;
    found.state = "queued".into();
    found.note = reason.to_owned();
    found.wait_reason = wait_reason;
    found.tested_ranges.clear();
    found.wait_head = head;
    found.wait_map = map;
    found.retry = false;
    found.requests.clear();
    found.not_run.clear(); found.not_run_since.clear();
    if announce {
        events.push(Event::new(kind, json!({"task":id,"reason":reason})));
    }
    true
}

/// A saved merge and Git's current reachability result for its checkout.
struct RecordedMerge<'a> {
    entry: MergeEntry,
    head: &'a str,
    reachable: Ancestry,
}

/// An untested merge still reachable from HEAD: never merge it again. Try the
/// rollback while HEAD is still that merge; once HEAD moved, run the recorded
/// prev..merged tests on HEAD. A successful run keeps the merge normally; a
/// selection error returns the task and records the open range.
fn resolve_reachable<F>(
    team: &mut Team,
    id: u32,
    merge: RecordedMerge<'_>,
    now: f64,
    events: &mut Vec<Event>,
    affected_hosts: F,
    queue: &mut Vec<Request>,
) -> anyhow::Result<bool>
where
    F: FnOnce() -> anyhow::Result<(Vec<String>, Vec<super::test_runners::Runner>)>,
{
    let RecordedMerge { entry, head, reachable } = merge;
    if head == entry.merged {
        // A failed reset is retried only for a new HEAD or the lead's retry,
        // never because the note or tests-map text happened to change.
        if entry.wait_reason == MergeWaitReason::Rollback && !entry.retry && entry.wait_head == head {
            return Ok(false);
        }
        return match roll_back(team.integration_folder(), &entry) {
            Rollback::Done => {
                let head = git_text(team.integration_folder(), &["rev-parse", "HEAD"]).unwrap_or_default();
                let map = tests_stamp(team);
                if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                    found.state = "queued".into();
                    found.note = "The merge was rolled back after its tests could not be selected; retry after a change or the lead's action.".into();
                    found.wait_reason = MergeWaitReason::Selection;
                    found.tested_ranges.clear();
                    found.wait_head = head.clone();
                    found.wait_map = map;
                    found.retry = false;
                    found.requests.clear();
                    found.not_run.clear(); found.not_run_since.clear();
                    found.prev = head;
                    found.merged.clear();
                }
                events.push(Event::new("merge_requeued", json!({"task":id,"reason":"rolled back"})));
                Ok(true)
            }
            Rollback::Failed(note) => {
                let reason = format!("The merge could not be rolled back: {note}");
                Ok(set_queue_note(team, id, &reason, MergeWaitReason::Rollback, "merge_rollback_blocked", events))
            }
        };
    }
    if reachable == Ancestry::Unknown {
        let reason = format!("git could not tell whether the merge {} is in the lead branch; the merge waits: {head}", entry.merged);
        return Ok(set_queue_note(team, id, &reason, MergeWaitReason::Ancestry, "merge_rollback_blocked", events));
    }
    let (host, runners) = match affected_hosts() {
        Ok(pair) => pair,
        Err(error) => {
            let reason = format!("The test runners could not be read: {error}; the merge waits on HEAD {head}");
            return Ok(set_queue_note(team, id, &reason, MergeWaitReason::Runners, "merge_rollback_blocked", events));
        }
    };
    let affected = match affected_requests(team, id, TestRange { prev: &entry.prev, merged: &entry.merged, run_at: head }, now, &host, &runners) {
        Ok(affected) => affected,
        Err(error) => {
            let reason = format!("The affected tests could not be selected: {error}");
            return_to_author(team, id, &reason, events);
            events.push(Event::new("merge_affected_failed", json!({"task":id,"reason":reason})));
            return Ok(true);
        }
    };
    queue.extend(affected.requests.iter().cloned());
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
            &MergeEntry { prev: entry.prev, merged: entry.merged, not_run: affected.not_run, tested_ranges: affected.tested_ranges, not_run_since: affected.not_run_since, ..entry },
            Vec::new(),
            now,
            events,
        )?;
    } else if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
        found.state = "testing".into();
        found.note.clear();
        found.wait_reason = MergeWaitReason::Other;
        found.tested_ranges.clear();
        found.retry = false;
        found.wait_head.clear();
        found.wait_map.clear();
        found.selection_failed = 0;
        found.requests = affected.requests.iter().map(|request| request.id.clone()).collect();
        found.not_run = affected.not_run;
        found.not_run_since = affected.not_run_since;
        found.tested_ranges = affected.tested_ranges;
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
    let head = git_text(team.integration_folder(), &["rev-parse", "HEAD"]).unwrap_or_default();
    let rolled = if head == merged {
        roll_back(team.integration_folder(), &MergeEntry { prev: prev.to_owned(), merged: merged.to_owned(), ..Default::default() })
    } else {
        match ancestry(team.integration_folder(), merged) {
            Ancestry::No => Rollback::Done,
            Ancestry::Yes => Rollback::Failed(format!("The lead branch moved since; revert {merged} by hand.")),
            Ancestry::Unknown => {
                let note = format!("{reason}; git could not tell whether {merged} is in the lead branch; the merge waits");
                return Ok(set_queue_note(team, id, &note, MergeWaitReason::Ancestry, "merge_rollback_blocked", events));
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
            let head = git_text(team.integration_folder(), &["rev-parse", "HEAD"]).unwrap_or_default();
            let map = tests_stamp(team);
            if let Some(found) = team.task_mut(id).and_then(|task| task.merge.as_mut()) {
                found.state = "queued".into();
                found.note = format!("{reason}; the merge was rolled back and will be retried");
                found.wait_reason = if selection { MergeWaitReason::Selection } else { MergeWaitReason::Runners };
                found.tested_ranges.clear();
                found.wait_head = head.clone();
                found.wait_map = map;
                found.retry = false;
                found.selection_failed = failed;
                found.requests.clear();
                found.not_run.clear(); found.not_run_since.clear();
                found.prev = head;
                found.merged.clear();
            }
            events.push(Event::new("merge_requeued", json!({"task":id,"reason":reason})));
            Ok(true)
        }
        Rollback::Failed(note) => {
            let reason = format!("{reason}; {note}");
            Ok(set_queue_note(team, id, &reason, MergeWaitReason::Rollback, "merge_rollback_failed", events))
        }
    }
}

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
    close_merge_ranges(team, &entry.tested_ranges);
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
            let candidate = if entry.not_run.iter().any(|message| not_run_suite(message) == suite) {
                entry.not_run_since.get(suite).cloned().unwrap_or_else(|| entry.prev.clone())
            } else {
                suite_since(team, suite)
            };
            let since = previous.get(suite).map(|open| open.since.clone()).filter(|since| !since.is_empty())
                .map(|old| if candidate != old && ancestry_at(team.integration_folder(), &candidate, &old) == Ancestry::Yes { candidate.clone() } else { old })
                .unwrap_or(candidate);
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
    // DCB-svn report (2026-11-12): `hgs team land` queues a landing for a task
    // already accepted; its merge records the landing instead of a second
    // accept, which refused ("has no report to decide") and rolled the whole
    // step back every tick.
    if team.task(id).is_some_and(|task| task.status == TaskStatus::Accepted) {
        let commit = if entry.merged.is_empty() || entry.prev == entry.merged { entry.commit.clone() } else { entry.merged.clone() };
        if let Some(task) = team.task_mut(id) {
            task.merge = None;
            task.landed = Some(Landed { commit: commit.clone(), by: "Zerus".into(), at: now });
        }
        events.push(Event::new("tasks_landed", json!({"by":"Zerus","tasks":[id],"commit":commit})));
        return Ok(());
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
    !entry.merged.is_empty() && ancestry(team.integration_folder(), &entry.merged) != Ancestry::No
}

/// Keep a returned task's untested merge range open for the phase build.
fn record_untested_range(team: &mut Team, id: u32, entry: &MergeEntry, _events: &mut Vec<Event>) {
    if entry.merged.is_empty() || entry.prev.is_empty() || ancestry(team.integration_folder(), &entry.merged) == Ancestry::No {
        return;
    }
    let previous = team.task(id).and_then(|task| task.untested_merge.as_ref()).cloned();
    // Only a definite non-ancestor permits replacing the older start. A Git
    // read failure must preserve it while extending the end to this merge.
    let prev = previous
        .filter(|range| ancestry_at(team.integration_folder(), &range.merged, &entry.merged) != Ancestry::No)
        .map(|range| range.prev)
        .unwrap_or_else(|| entry.prev.clone());
    if let Some(task) = team.task_mut(id) {
        task.untested_merge = Some(MergeRange { task: id, prev, merged: entry.merged.clone() });
    }
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
    pub merges: Vec<MergeRange>,
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
    let merges = open_merge_ranges(team);
    if since.is_empty() && not_run.is_empty() && merges.is_empty() {
        return None;
    }
    let per_suite = not_run
        .iter()
        .filter_map(|message| {
            let suite = not_run_suite(message);
            (!suite.is_empty()).then(|| (suite.to_owned(), suite_since(team, suite)))
        })
        .collect();
    Some(Untested { since, not_run, per_suite, merges })
}

/// A queued or tested entry, for status.
pub(crate) fn view(task: &Task) -> Option<Value> {
    let entry = task.merge.as_ref()?;
    let mut value = json!({"state":entry.state,"author":entry.author,"commit":entry.commit,"merged":entry.merged,"tests":entry.requests.len(),"note":entry.note});
    if entry.wait_reason != MergeWaitReason::Other { value["wait_reason"] = json!(entry.wait_reason); }
    if !entry.not_run.is_empty() {
        value["not_run"] = json!(entry.not_run.clone());
    }
    Some(value)
}

#[cfg(test)]
#[path = "merge_queue_tests.rs"]
mod tests;

