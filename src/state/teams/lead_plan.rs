//! Task 162 (owner, 2026-11-07): the owner gates only the team's goal and plan
//! approval, roster changes, wider rights, paid resources, release and
//! destructive actions. Inside an approved goal the lead plans by itself:
//! adding tasks to a running or accepted phase, moving tasks and phases,
//! splitting (add and cancel an empty task) and editing tasks of an accepted
//! phase are the lead's own operations, never a card.
//!
//! Each such change is recorded with what it replaced (`Team::lead_changes`)
//! and shown to the owner in one digest in Waiting, which blocks nothing; the
//! owner reverts any of them (`revert-lead-change`) while the work it touched
//! has not started, and marks the digest seen (`seen-lead-changes`).
use super::model::*;
use super::ops::Actor;
use super::store::Event;
use anyhow::{bail, ensure, Context};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The lead's planning operations inside an approved goal: recorded when the
/// lead does them on a running or paused team.
pub(crate) const LEAD_PLAN_OPS: [&str; 7] = ["extend", "edit-draft", "move-phase", "move-task", "add-to-phase", "cancel", "spawn"];

/// Whether `actor` is the lead planning inside the team's approved goal: the
/// plan is approved and the team runs (or is paused).
pub(crate) fn lead_plans(team: &Team, actor: &Actor) -> bool {
    actor.is_lead(team) && matches!(team.status, TeamStatus::Running | TeamStatus::Paused)
}

/// Owner, 2026-11-10: the lead relaunches by itself, without a card, a member
/// that did not start (its start failed, or the approved team never got its
/// session) or that Zerus marks stuck. Any other relaunch stays the owner's.
pub(crate) fn lead_relaunches(team: &Team, member_id: &str) -> bool {
    if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) || member_id == team.lead {
        return false;
    }
    let Some(member) = team.member(member_id) else { return false };
    if member.state == MemberState::Removed {
        return false;
    }
    // A planned member that simply has not been started yet is Zerus's to
    // start, not a relaunch: only a recorded start failure (task #13's
    // Member::failed_to_start) or Zerus's stuck mark counts.
    // Owner, 2026-11-11 (team 86821caa): so does a member a swap or relaunch
    // left without a session - its start after the swap has not happened, or
    // waits for the session it replaces. The guides say the lead relaunches a
    // member that did not start, and `hgs team relaunch` answered "The lead
    // changes the roster only on your word" for exactly such a member.
    member.failed_to_start() || member.stuck.is_some() || not_started_after_swap(member)
}

/// A member whose session a swap or relaunch took and whose new one has not
/// started (it failed, is still to be launched, or waits for the old one).
pub(crate) fn not_started_after_swap(member: &super::model::Member) -> bool {
    member.session.is_none() && member.state != MemberState::Removed && (member.launch_after_swap || member.replaced_busy)
}

/// The lead's card for planning inside the approved goal is refused at the
/// CLI with the operation to run instead (cards stored before stay
/// decidable).
pub(crate) fn refuse_plan_card(team: &Team, input: &Value) -> anyhow::Result<()> {
    let kind = input["kind"].as_str().unwrap_or_default();
    let into_phase = input.get("phase").is_some() || input["input"].get("phase").is_some();
    let planning = matches!(kind, "add-to-phase" | "edit-draft" | "move-phase" | "move-task") || (kind == "extend" && into_phase);
    if planning && matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        bail!(
            "{kind} inside the approved goal is your own operation, not a card: {{\"team\":\"{}\",...}} | hgs team {kind} --json. \
The owner sees it in the digest and can revert it (hgs guide team-lead).",
            team.id
        );
    }
    Ok(())
}

fn phase_state(phases: &[Phase], n: u32) -> Option<PhaseState> {
    phases.iter().find(|phase| phase.n == n).map(|phase| phase.state)
}

/// Whether a task in phase `n` is accepted work: phase 1 of a running team
/// (the approved plan) or a phase that is not a draft.
fn accepted(phases: &[Phase], n: u32) -> bool {
    match phase_state(phases, n) {
        Some(state) => !state.is_draft() && state != PhaseState::Discarded,
        None => n <= 1,
    }
}

fn short(text: &str) -> String {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 80 { format!("{}...", line.chars().take(77).collect::<String>()) } else { line }
}

/// Records the lead's change: the tasks and phases it changed in accepted
/// work, as they were. A change only of drafts (which the owner approves
/// anyway) is not recorded.
pub(crate) fn record(team: &mut Team, op: &str, tasks_before: &[Task], phases_before: &[Phase], by: &str, at: f64, events: &mut Vec<Event>) {
    let before: BTreeMap<u32, &Task> = tasks_before.iter().map(|task| (task.id, task)).collect();
    let mut changed = Vec::new();
    let mut added = Vec::new();
    let mut words = Vec::new();
    for task in &team.tasks {
        match before.get(&task.id) {
            None => {
                if accepted(&team.phases, task.phase) {
                    added.push(task.id);
                    words.push(format!("added #{} {} to phase {}", task.id, short(&task.title), task.phase));
                }
            }
            Some(old) if *old != task => {
                if accepted(phases_before, old.phase) || accepted(&team.phases, task.phase) {
                    changed.push((*old).clone());
                    words.push(if old.phase != task.phase {
                        format!("moved #{} {} from phase {} to {}", task.id, short(&task.title), old.phase, task.phase)
                    } else if old.status != task.status && task.status == TaskStatus::Cancelled {
                        format!("cancelled #{} {}", task.id, short(&task.title))
                    } else {
                        format!("edited #{} {}", task.id, short(&task.title))
                    });
                }
            }
            Some(_) => {}
        }
    }
    let mut phases = Vec::new();
    for old in phases_before {
        let now = team.phases.iter().find(|phase| phase.n == old.n);
        if now != Some(old) && !old.state.is_draft() {
            phases.push(old.clone());
            if now.is_some_and(|now| now.order != old.order) {
                words.push(format!("moved phase {}", old.n));
            }
        }
    }
    // Task #229: a phase the change opened (a Minor phase) is recorded so a
    // revert removes it with its tasks.
    let mut added_phases = Vec::new();
    for now in &team.phases {
        if !phases_before.iter().any(|old| old.n == now.n) && !now.state.is_draft() {
            added_phases.push(now.n);
            words.push(if now.kind == PhaseKind::Minor {
                format!("opened Minor phase {}", now.n)
            } else {
                format!("added phase {}", now.n)
            });
        }
    }
    if changed.is_empty() && added.is_empty() && phases.is_empty() && added_phases.is_empty() {
        return;
    }
    let n = team.lead_changes.iter().filter_map(|change| change.id.strip_prefix("lc")?.parse::<u32>().ok()).max().unwrap_or(0) + 1;
    let id = format!("lc{n}");
    let summary = if words.is_empty() { format!("changed phase {}", phases.iter().map(|phase| phase.n.to_string()).collect::<Vec<_>>().join(", ")) } else { words.join("; ") };
    events.push(Event::new("lead_changed_plan", json!({"change":id,"op":op,"by":by,"summary":summary,"added":added,
        "changed":changed.iter().map(|task| task.id).collect::<Vec<_>>()})));
    let (changed_ids, phases_changed) = (changed.iter().map(|task: &Task| task.id).collect(), phases.iter().map(|phase: &Phase| phase.n).collect());
    team.lead_changes.push(LeadChange {
        id,
        op: op.to_owned(),
        summary,
        by: by.to_owned(),
        at,
        tasks_before: changed,
        added,
        phases_before: phases,
        phases_added: added_phases,
        changed: changed_ids,
        phases_changed,
        ..Default::default()
    });
    trim(team);
}

/// Task #362 (owner, 2026-11-08): the lead, or a member's sub-lead, used that
/// member's last reply as the report. The owner sees it in the digest with the
/// reply quoted; it is no plan change and cannot be reverted.
pub(crate) fn report_recorded(team: &mut Team, task: u32, role: &str, reply: &str, by: &str) {
    let n = team.lead_changes.iter().filter_map(|change| change.id.strip_prefix("lc")?.parse::<u32>().ok()).max().unwrap_or(0) + 1;
    let id = format!("lc{n}");
    let summary = format!("{role}'s last reply as the report on #{task}: \"{}\"", clipped(reply, 160));
    team.lead_changes.push(LeadChange { id, op: "use-last-reply".into(), summary, by: by.to_owned(), at: super::super::now(), ..Default::default() });
    trim(team);
}

/// One line, at most `limit` characters.
fn clipped(text: &str, limit: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > limit {
        format!("{}...", line.chars().take(limit.saturating_sub(3)).collect::<String>())
    } else {
        line
    }
}

/// Bounded: the oldest seen or reverted changes leave first, then the oldest.
fn trim(team: &mut Team) {
    while team.lead_changes.len() > LEAD_CHANGES_KEPT {
        let at = team.lead_changes.iter().position(|change| change.seen || change.reverted_at.is_some()).unwrap_or(0);
        team.lead_changes.remove(at);
    }
}

/// The tasks a change touched.
/// Task #4 (team ede724c4): from the ids the head keeps, so a later change's
/// snapshots are never read for it.
fn touched(change: &LeadChange) -> BTreeSet<u32> {
    change.changed_tasks().into_iter().chain(change.added.iter().copied()).collect()
}

fn touched_phases(change: &LeadChange) -> BTreeSet<u32> {
    change.changed_phases().into_iter().chain(change.phases_added.iter().copied()).collect()
}

/// The owner reverts one of the lead's changes: the tasks it changed are as
/// they were, the tasks it added leave the plan, its phases are as they were.
/// Only while nothing it touched has started since, and newest first where
/// later changes touched the same work.
pub(crate) fn revert(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human), "Only you revert the lead's change");
    let id = input["change"].as_str().unwrap_or_default().trim().to_owned();
    let at = team.lead_changes.iter().position(|change| change.id == id).with_context(|| format!("Unknown lead change {id:?}"))?;
    let change = team.lead_changes[at].clone();
    ensure!(change.reverted_at.is_none(), "Lead change {id} is already reverted");
    // Task #362: a report is no plan change; it has nothing to restore.
    ensure!(change.op != "use-last-reply", "Lead change {id} is a report, not a plan change: it cannot be reverted");
    // Task #4 (team ede724c4): the store loads the change's snapshots from
    // `team_lead_changes` before the revert; without them it would restore nothing.
    ensure!(!change.history_missing(), "Lead change {id}'s history could not be read; nothing changed");
    let tasks = touched(&change);
    let phases = touched_phases(&change);
    if let Some(later) = team.lead_changes[at + 1..]
        .iter()
        .find(|later| later.reverted_at.is_none() && (!touched(later).is_disjoint(&tasks) || !touched_phases(later).is_disjoint(&phases)))
    {
        bail!("Lead change {} changed the same work later; revert it first", later.id);
    }
    for id in &tasks {
        let Some(task) = team.task(*id) else { continue };
        let started = task.attempts.iter().any(|attempt| attempt.sent_at >= change.at && !matches!(attempt.state, AttemptState::Revoked | AttemptState::Refused));
        ensure!(!started, "Task #{id} has started since the change; cancel or edit it instead");
    }
    for task in team.tasks.iter().filter(|task| !change.added.contains(&task.id)) {
        if let Some(dep) = task.deps.iter().find(|dep| change.added.contains(&dep.task)) {
            bail!("Task #{} depends on #{}, which the change added; edit #{} first", task.id, dep.task, task.id);
        }
    }
    team.tasks.retain(|task| !change.added.contains(&task.id));
    // Task #229: a phase the change opened leaves with its tasks.
    team.phases.retain(|phase| !change.phases_added.contains(&phase.n));
    for n in &change.phases_added {
        team.phase_modes.remove(n);
    }
    for phase in &mut team.phases {
        phase.tasks.retain(|task| !change.added.contains(task));
    }
    for old in &change.tasks_before {
        if let Some(task) = team.tasks.iter_mut().find(|task| task.id == old.id) {
            *task = old.clone();
        }
    }
    for old in &change.phases_before {
        if let Some(phase) = team.phases.iter_mut().find(|phase| phase.n == old.n) {
            *phase = old.clone();
        }
    }
    // A task restored to another phase is listed by that phase again.
    for old in &change.tasks_before {
        for phase in &mut team.phases {
            if phase.n == old.phase {
                if !phase.tasks.contains(&old.id) {
                    phase.tasks.push(old.id);
                    phase.tasks.sort_unstable();
                }
            } else if !change.phases_before.iter().any(|kept| kept.n == phase.n) {
                phase.tasks.retain(|task| *task != old.id);
            }
        }
    }
    let by = actor.label(team);
    let now = super::super::now();
    let found = &mut team.lead_changes[at];
    found.reverted_at = Some(now);
    found.reverted_by = by.clone();
    found.seen = true;
    events.push(Event::new("lead_change_reverted", json!({"change":id,"by":by,"summary":change.summary})));
    Ok(json!({"status":"reverted","change":id,"summary":change.summary,"removed":change.added,
        "restored":change.changed_tasks()}))
}

/// The owner has seen the digest: its changes leave Waiting (each stays
/// revertible).
pub(crate) fn seen(team: &mut Team, input: &Value, actor: &Actor, events: &mut Vec<Event>) -> anyhow::Result<Value> {
    ensure!(matches!(actor, Actor::Human), "Only you mark the lead's changes seen");
    let upto = input["change"].as_str().unwrap_or_default();
    let mut marked = Vec::new();
    for change in team.lead_changes.iter_mut().filter(|change| !change.seen) {
        change.seen = true;
        marked.push(change.id.clone());
        if change.id == upto {
            break;
        }
    }
    if !marked.is_empty() {
        events.push(Event::new("lead_changes_seen", json!({"changes":marked})));
    }
    Ok(json!({"status":"seen","changes":marked}))
}

/// The lead's changes the owner has not seen, newest first (at most 20 shown).
pub(crate) fn unseen(team: &Team) -> Vec<&LeadChange> {
    team.lead_changes.iter().rev().filter(|change| !change.seen && change.reverted_at.is_none()).collect()
}

/// The digest line of Waiting: one item per team, which blocks nothing.
pub(crate) fn digest(team: &Team) -> Option<(String, f64, Value)> {
    let list = unseen(team);
    let newest = list.first()?.at;
    let count = list.len();
    let shown: Vec<Value> = list
        .iter()
        .take(20)
        .map(|change| {
            let mut entry = json!({"id":change.id,"summary":change.summary,"at":change.at,"op":change.op});
            // Task #362: a report has nothing to revert; the plan changes do.
            if change.op != "use-last-reply" {
                entry["revert"] = json!(format!("{{\"team\":\"{}\",\"change\":\"{}\",\"by\":\"human\"}} | hgs team revert-lead-change --json", team.id, change.id));
            }
            entry
        })
        .collect();
    let first = list.first().map(|change| change.summary.clone()).unwrap_or_default();
    let only_report = count == 1 && list[0].op == "use-last-reply";
    let title = if only_report {
        "The lead used a member's last reply as the report".to_owned()
    } else if count == 1 {
        format!("The lead changed the plan: {first}")
    } else {
        format!("The lead changed the plan {count} times inside the approved goal, newest: {first}")
    };
    Some((title, newest, json!(shown)))
}

/// Task #286: the digest's changes grouped by kind (added, moved, cancelled,
/// re-issued, else changed), each with its count and its newest few, so the
/// owner's Waiting reads one grouped row.
pub(crate) fn groups(changes: &[Value]) -> Value {
    let mut order: Vec<&str> = Vec::new();
    let mut counted: std::collections::BTreeMap<&str, (usize, Vec<Value>)> = std::collections::BTreeMap::new();
    for change in changes {
        let kind = group_of(change);
        if !order.contains(&kind) {
            order.push(kind);
        }
        let entry = counted.entry(kind).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 3 {
            entry.1.push(change.clone());
        }
    }
    json!(
        order
            .into_iter()
            .map(|kind| {
                let (count, newest) = &counted[kind];
                json!({"kind":kind,"count":count,"newest":newest})
            })
            .collect::<Vec<Value>>()
    )
}

fn group_of(change: &Value) -> &'static str {
    let summary = change["summary"].as_str().unwrap_or_default();
    match summary.split_whitespace().next().unwrap_or_default() {
        "added" => "added",
        "moved" => "moved",
        "cancelled" => "cancelled",
        "re-issued" | "reissued" | "reopened" | "returned" => "re-issued",
        _ => match change["op"].as_str().unwrap_or_default() {
            "add-to-phase" | "extend" => "added",
            "move-task" | "move-phase" => "moved",
            _ => "changed",
        },
    }
}

/// `hgs team lead-changes --json`: the recorded changes, newest first.
pub(crate) fn list(team: &Team, limit: usize) -> Value {
    let shown: Vec<Value> = team
        .lead_changes
        .iter()
        .rev()
        .take(limit.clamp(1, 200))
        .map(|change| json!({"id":change.id,"op":change.op,"summary":change.summary,"by":change.by,"at":change.at,
            "seen":change.seen,"reverted_at":change.reverted_at,"added":change.added,"phases_added":change.phases_added,
            "changed":change.changed_tasks()}))
        .collect();
    json!({"team":team.id,"changes":shown,"total":team.lead_changes.len()})
}
