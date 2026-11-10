//! WhiteWh fork: U50, one list of what waits for the owner across every team
//! (docs/superpowers/specs/2026-10-28-team-customization.md section 7). It
//! reads only what the teams record (the attention list, phases, proposals,
//! holds and decisions) and stores nothing; `hgs team waiting --json` pages it
//! newest first. Task #246: every item carries a `state` fingerprint and the
//! owner may dismiss an item with the state they saw; it stays out of the list
//! (and the count) until that state changes.
use super::model::{PhaseState, TaskStatus, TaskWait, Team, TeamStatus};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A page holds at most this many items.
pub(crate) const MAX_PAGE: usize = 200;
const DEFAULT_PAGE: usize = 50;

/// Every item of these teams that waits for the owner, newest first; ties
/// in their stable id order, so a cursor never skips or repeats one.
pub(crate) fn items(teams: &[Team], now: f64) -> Vec<Value> {
    let mut out = Vec::new();
    for team in teams {
        let start = out.len();
        team_items(team, now, &mut out);
        for found in &mut out[start..] {
            let kind = found["kind"].as_str().unwrap_or_default();
            let state = match kind {
                // Task #246: a plan or phase waits for a new revision only.
                "plan_draft" => format!("plan:{}", team.revision),
                "phase_draft" | "phase_acceptance" => format!("{kind}:{}:{}", found["phase"], team.revision),
                // A re-pause has a new time; an unrelated edit does not.
                "team_paused" => format!("paused:{}", found["paused_at"]),
                "report_escalated" => format!("report:{}:{}", found["task"], found["since"]),
                "permission_escalated" => format!("permission:{}:{}", found["member_id"], found["since"]),
                "owner_word_asked" => {
                    if found.get("proposal").is_some() {
                        format!("proposal:{}:{}", found["proposal"], found["since"])
                    } else {
                        format!("addition:{}:{}", found["phase"], found["since"])
                    }
                }
                "task_waits_owner" => format!("task:{}:{}", found["task"], found["since"]),
                "lead_changes" => format!("digest:{}", found["since"]),
                "crit_added" => format!("crit:{}:{}", found["task"], found["since"]),
                "team_finished" => format!("finished:{}", found["since"]),
                "test_full_run" => format!("full:{}", found["since"]),
                "test_quarantine" => format!("quarantine:{}:{}", found["id"], found["since"]),
                _ => format!("{kind}:{}:{}", found["id"], found["since"]),
            };
            found["state"] = json!(state);
        }
    }
    sorted(out)
}

/// Task #246: a dismissal is kept with the item's state at that moment. The
/// item stays out of the live list while its state is unchanged; a changed
/// state (a new plan revision, a second pause, a lower disk band, a new offer)
/// returns it. A decision that blocks work stays in the dismissed section with
/// the team or member it still blocks.
pub(crate) fn split(items: Vec<Value>, dismissals: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let by_id: BTreeMap<&str, &Value> = dismissals.iter().filter_map(|record| Some((record["id"].as_str()?, record))).collect();
    let mut active = Vec::new();
    let mut dismissed = Vec::new();
    for item in items {
        let Some(record) = item["id"].as_str().and_then(|id| by_id.get(id)) else {
            active.push(item);
            continue;
        };
        if record["state"] != item["state"] {
            active.push(item);
            continue;
        }
        let mut gone = item;
        gone["dismissed_at"] = record["at"].clone();
        if blocking(&gone) {
            gone["blocking"] = json!(true);
            gone["blocks"] = json!(blocks(&gone));
        }
        dismissed.push(gone);
    }
    (active, sorted(dismissed))
}

/// The decisions that still block a team or member when dismissed.
fn blocking(item: &Value) -> bool {
    matches!(
        item["kind"].as_str().unwrap_or_default(),
        "plan_draft" | "phase_draft" | "phase_acceptance" | "owner_word_asked" | "permission_escalated"
    )
}

fn blocks(item: &Value) -> String {
    let team = item["team_name"].as_str().filter(|name| !name.is_empty()).unwrap_or("the team");
    match item["member"].as_str().filter(|member| !member.is_empty()) {
        Some(member) => format!("{team}/{member}"),
        None => team.to_owned(),
    }
}

/// Task #286: a merge conflict in an accepted predecessor's commit is the
/// lead's or the merger's resolve task (#267), never the owner's.
fn is_merge_conflict(wait: &TaskWait) -> bool {
    wait.reason.contains("accepted commit")
        && (wait.reason.contains("conflicts in") || wait.reason.contains("merge it into the member's branch"))
}

/// The live page plus the dismissed items apart, with their own count. The
/// badge and the notifications read `total`, which never counts a dismissal.
pub(crate) fn page_with_dismissed(active: &[Value], dismissed: &[Value], before: Option<&Value>, limit: Option<u64>) -> Value {
    let mut page = page(active, before, limit);
    page["dismissed"] = json!(dismissed.iter().take(MAX_PAGE).cloned().collect::<Vec<_>>());
    page["dismissed_total"] = json!(dismissed.len());
    page
}

/// Newest first, ties in id order: the page order, also of items from beyond
/// the teams (#742's tool offers).
pub(crate) fn sorted(mut items: Vec<Value>) -> Vec<Value> {
    items.sort_by(|a, b| since(b).total_cmp(&since(a)).then_with(|| id(a).cmp(id(b))));
    items
}

fn since(item: &Value) -> f64 {
    item["since"].as_f64().unwrap_or_default()
}

fn id(item: &Value) -> &str {
    item["id"].as_str().unwrap_or_default()
}

/// One page after the cursor `{since, id}` (the last item of the previous
/// page): items that arrive later are newer and never move an older page.
pub(crate) fn page(items: &[Value], before: Option<&Value>, limit: Option<u64>) -> Value {
    let limit = limit.map_or(DEFAULT_PAGE, |limit| (limit as usize).clamp(1, MAX_PAGE));
    let start = match before.filter(|cursor| cursor.is_object()) {
        None => 0,
        Some(cursor) => {
            let (at, key) = (since(cursor), id(cursor));
            items.iter().position(|item| since(item) < at || (since(item) == at && id(item) > key)).unwrap_or(items.len())
        }
    };
    let shown: Vec<Value> = items[start..].iter().take(limit).cloned().collect();
    let next = (start + shown.len() < items.len())
        .then(|| shown.last().map(|item| json!({"since":item["since"],"id":item["id"]})))
        .flatten();
    json!({"items":shown,"next":next,"total":items.len()})
}

fn item(team: &Team, kind: &str, key: &str, title: String, since: f64, action: &str) -> Value {
    json!({"id":format!("{}/{kind}/{key}", team.id),"team":team.id,"team_name":team.name,"kind":kind,
        "title":title,"since":since,"action":action})
}

fn team_items(team: &Team, now: f64, out: &mut Vec<Value>) {
    match team.status {
        // The plan waits for Approve & Run when the owner reviews it.
        TeamStatus::Planning => {
            if !team.tasks.is_empty() && team.settings.approval == super::model::Approval::Review {
                let tasks = team.tasks.len();
                out.push(item(
                    team,
                    "plan_draft",
                    "plan",
                    format!("Plan of {tasks} task{} waits for Approve & Run", if tasks == 1 { "" } else { "s" }),
                    team.created,
                    "approve_plan",
                ));
            }
            return;
        }
        TeamStatus::Running | TeamStatus::Paused => {}
        _ => return,
    }
    // Task 112: a paused team waits for the owner's Resume team, whatever its
    // sessions do; the item says since when and by whom.
    if team.status == TeamStatus::Paused {
        let at = team.paused.as_ref().map_or(team.updated, |pause| pause.at);
        let words = super::inbox::paused_words(team);
        let title = if words.is_empty() { "Team paused: members get no tasks".to_owned() } else { format!("Team paused {words}: members get no tasks") };
        let mut found = item(team, "team_paused", &(at as u64).to_string(), title, at, "resume_team");
        found["paused_at"] = json!(at);
        found["paused_by"] = json!(team.paused.as_ref().map(|pause| pause.by.clone()).unwrap_or_default());
        out.push(found);
    }
    // Task 186 (docs/lore.md 11.11): an urgent message to the lead that has
    // reached no safe point within the team's urgent_bound is the owner's:
    // Zerus never stops the lead, the owner may.
    if let Some((held, minutes)) = super::doorbell::lead_undelivered(team, now) {
        let lead = team.member(&team.lead).map(|lead| team.name_of(&lead.id)).unwrap_or_else(|| "The lead".into());
        let mut found = item(
            team,
            "lead_message_undelivered",
            &(held as u64).to_string(),
            format!("URGENT team message to {lead} is undelivered: the lead has worked {minutes} min without a safe point"),
            held,
            "open_terminal",
        );
        found["member_id"] = json!(team.lead);
        // Its Terminal is where the owner may stop the lead natively.
        found["session"] = json!(team.member(&team.lead).and_then(|lead| lead.session.as_ref()).map(|session| session.name.clone()).unwrap_or_default());
        found["urgent"] = json!(true);
        out.push(found);
    }
    // The attention list already decides when a report or a permission is
    // escalated to the owner, and which draft phase is next.
    for attention in super::inbox::attention(team, now) {
        let at = attention["since"].as_f64().unwrap_or_default();
        let summary = attention["summary"].as_str().unwrap_or_default().to_owned();
        match attention["kind"].as_str().unwrap_or_default() {
            "phase_draft" => {
                let phase = attention["phase"].as_u64().unwrap_or_default();
                let mut found = item(team, "phase_draft", &phase.to_string(), summary, at, "approve_phase");
                found["phase"] = json!(phase);
                out.push(found);
            }
            // U63 (owner, 2026-10-31): a phase ready for the owner's acceptance.
            "phase_acceptance" => {
                let phase = attention["phase"].as_u64().unwrap_or_default();
                let mut found = item(team, "phase_acceptance", &phase.to_string(),
                    format!("Phase {phase} is ready for your acceptance"), at, "accept_phase");
                found["phase"] = json!(phase);
                found["summary"] = attention["outcome"].clone();
                out.push(found);
            }
            "report" => {
                let task = attention["task"].as_u64().unwrap_or_default();
                let title = team.task(task as u32).map(|task| task.title.clone()).unwrap_or_default();
                let member = attention["member"].as_str().unwrap_or_default();
                // U56: the summary names the member by id with its role.
                let named = attention["member_id"].as_str().map(|id| team.name_of(id)).unwrap_or_else(|| member.to_owned());
                let mut found = item(
                    team,
                    "report_escalated",
                    &task.to_string(),
                    format!("Report on #{task} {title} from {named} waits for your decision"),
                    at,
                    "decide_report",
                );
                found["task"] = json!(task);
                found["member"] = json!(member);
                out.push(found);
            }
            "permission" => {
                let member = attention["member"].as_str().unwrap_or_default().to_owned();
                let mut found = item(team, "permission_escalated", &member, summary, at, "answer_permission");
                found["member"] = json!(member);
                // #740: the member's id, which the desktop's answer is bound to.
                found["member_id"] = attention["member_id"].clone();
                out.push(found);
            }
            // The lead's session stopped: the team waits for the owner to resume it.
            "lead_stopped" => {
                let member = attention["member"].as_str().unwrap_or_default().to_owned();
                let mut found = item(team, "lead_stopped", &member, summary, at, "resume_lead");
                found["member"] = json!(member);
                found["member_id"] = attention["member_id"].clone();
                out.push(found);
            }
            // Task 98: a member's composer refuses every hand-out; only its
            // Terminal clears it.
            "input_blocked" => {
                let member = attention["member"].as_str().unwrap_or_default().to_owned();
                let mut found = item(team, "input_blocked", &format!("{member}-{}", at as u64), summary, at, "open_terminal");
                for key in ["member_id", "count", "reason", "screen", "session", "command"] {
                    found[key] = attention[key].clone();
                }
                found["member"] = json!(member);
                out.push(found);
            }
            // Owner, 2026-11-11: a member's start waits for the session it
            // replaces; its Terminal is where the owner stops that session.
            "replaced_waits" => {
                let member = attention["member"].as_str().unwrap_or_default().to_owned();
                let mut found = item(team, "replaced_waits", &member, summary, at, "open_terminal");
                for key in ["member_id", "session", "minutes", "detail"] {
                    found[key] = attention[key].clone();
                }
                found["member"] = json!(member);
                out.push(found);
            }
            // Task 277 (owner, 2026-11-08): your own urgent message to a member
            // that stayed unread; the button opens the team to see it.
            "urgent_message" => {
                let member_id = attention["member_id"].as_str().unwrap_or_default().to_owned();
                let mut found = item(team, "urgent_message", &member_id, summary, at, "open_team");
                found["member"] = attention["member"].clone();
                found["member_id"] = attention["member_id"].clone();
                found["minutes"] = attention["minutes"].clone();
                out.push(found);
            }
            // Task 95: the lead stalled while its decisions wait. The round is
            // part of the id, so each round (at most hourly) is notified once
            // by the desktops, which notify a waiting id once.
            "lead_stalled" => {
                let member = attention["member"].as_str().unwrap_or_default().to_owned();
                let round = attention["round"].as_u64().unwrap_or(1);
                let action = if attention["reason"] == "exited" { "resume_lead" } else { "open_lead" };
                let mut found = item(team, "lead_stalled", &format!("{member}-{round}"), summary, at, action);
                for key in ["member_id", "reason", "detail", "items", "members", "round", "minutes", "session"] {
                    found[key] = attention[key].clone();
                }
                found["member"] = json!(member);
                out.push(found);
            }
            // Only the member's Terminal takes this answer: the button opens the
            // team, and the title says where to answer.
            "permission_terminal" => {
                let member = attention["member"].as_str().unwrap_or_default().to_owned();
                let mut found = item(team, "permission_escalated", &format!("{member}-terminal"), summary, at, "open_terminal");
                found["member"] = json!(member);
                found["member_id"] = attention["member_id"].clone();
                // Task 113: once raised, the session and the screen's last lines.
                for key in ["escalated", "raised_at", "minutes", "screen", "session", "host", "tool", "reason"] {
                    if !attention[key].is_null() {
                        found[key] = attention[key].clone();
                    }
                }
                out.push(found);
            }
            // N17: a replan pauses the team for the owner's word on the proposal.
            "replan" => {
                let finding = attention["finding"].as_str().unwrap_or_default();
                let mut found = item(team, "owner_word_asked", &format!("replan-{finding}"), summary, at, "decide_replan");
                found["finding"] = json!(finding);
                out.push(found);
            }
            _ => {}
        }
    }
    // Task 162: the lead's changes of the plan inside the approved goal, in
    // one digest that blocks nothing; each can be reverted.
    if let Some((title, at, changes)) = super::lead_plan::digest(team) {
        let mut found = item(team, "lead_changes", "digest", title, at, "open_team");
        // Task #286: one grouped row: the counts per kind and the newest few.
        found["groups"] = super::lead_plan::groups(changes.as_array().map(Vec::as_slice).unwrap_or_default());
        found["changes"] = changes;
        found["seen"] = json!(format!("{{\"team\":\"{}\",\"by\":\"human\"}} | hgs team seen-lead-changes --json", team.id));
        out.push(found);
    }
    // Task #229: the lead's critical follow-ups, one item each, blocking nothing
    // for the owner but naming the task, its phase and its origin; the desktop's
    // notification for a Waiting item comes from here.
    for task in team.tasks.iter().filter(|task| task.severity == "crit" && !task.status.finished()) {
        let origin = task.origin.map(|origin| format!(" (from #{origin})")).unwrap_or_default();
        let title = format!("The lead added critical task #{} to phase {}{origin}: {}", task.id, task.phase, task.title);
        let at = task.added_late.as_ref().map_or(team.updated, |late| late.at);
        let mut found = item(team, "crit_added", &task.id.to_string(), title, at, "open_task");
        found["task"] = json!(task.id);
        found["phase"] = json!(task.phase);
        found["severity"] = json!("crit");
        if let Some(origin) = task.origin {
            found["origin"] = json!(origin);
        }
        out.push(found);
    }
    // F8: the lead's open proposals wait for the owner's Accept or Reject.
    for proposal in team.proposals.iter().filter(|proposal| proposal["state"] == "open") {
        let key = proposal["id"].as_str().unwrap_or_default();
        let kind = proposal["kind"].as_str().unwrap_or("change").replace('-', " ");
        let reason = proposal["reason"].as_str().unwrap_or_default();
        // #740: a question names itself.
        let asked = proposal["change"]["question"].as_str().filter(|_| proposal["kind"] == "question");
        // Task #220: an install card names the tool, why and the command Allow runs.
        let install = proposal["change"]["tool"].as_str().filter(|_| proposal["kind"] == "install-tool");
        let title = match (asked, install) {
            (Some(question), _) => format!("The lead asks: {}", question.split_whitespace().collect::<Vec<_>>().join(" ")),
            (None, Some(tool)) => format!(
                "The lead asks to install {tool}: {}",
                proposal["change"]["why"].as_str().filter(|why| !why.is_empty()).unwrap_or(reason)
            ),
            _ if reason.is_empty() => format!("The lead proposes: {kind}"),
            _ => format!("The lead proposes: {kind} \u{2014} {reason}"),
        };
        let mut found = item(team, "owner_word_asked", &format!("proposal-{key}"), title, proposal["at"].as_f64().unwrap_or_default(), "decide_proposal");
        found["proposal"] = json!(key);
        if install.is_some() {
            // Task #220: the card's own fields, for the desktop's renderer.
            found["offer"] = proposal["change"]["offer"].clone();
            found["tool"] = proposal["change"]["tool"].clone();
            found["folder"] = proposal["change"]["folder"].clone();
            let words: Vec<String> = proposal["change"]["command"].as_array().into_iter().flatten().filter_map(|word| word.as_str().map(str::to_owned)).collect();
            found["install_command"] = json!(words.join(" "));
        }
        out.push(found);
    }
    // U29: an addition to a running phase waits for the owner's confirmation
    // until it expires; an expired one can no longer be confirmed and is not
    // listed (it leaves the team on its next change).
    for addition in team.pending_additions.iter().filter(|addition| super::ops::addition_fresh(addition, now)) {
        let count = addition.tasks.len();
        let mut found = item(
            team,
            "owner_word_asked",
            &format!("addition-{}", addition.id),
            format!("Adding {count} task{} to running phase {} waits for your confirmation", if count == 1 { "" } else { "s" }, addition.phase),
            addition.warned_at,
            "confirm_addition",
        );
        found["phase"] = json!(addition.phase);
        out.push(found);
    }
    // A5: a task the owner held waits for the owner's release.
    for task in team.tasks.iter().filter(|task| !task.status.finished()) {
        let Some(hold) = task.hold.as_ref().filter(|hold| hold.by == "You") else { continue };
        let reason = if hold.reason.is_empty() { String::new() } else { format!(": {}", hold.reason) };
        let mut found = item(team, "task_waits_owner", &task.id.to_string(), format!("#{} {} is held by you{reason}", task.id, task.title), hold.at, "release_task");
        found["task"] = json!(task.id);
        out.push(found);
    }
    // #581 N7, spec 7: a task waiting for a step Zerus could not take (U45
    // model, U46 context, U48 branch) waits for the lead or the owner, with
    // its reason and the retry that starts the step again, as in the lead's
    // inbox. A context action that still runs waits for nobody.
    // Task #286 (owner, 2026-11-08): a task's wait for the lead or the merger
    // is not the owner's. It is in the lead's inbox (spec 0's `task_waits`);
    // only a wait the lead leaves unresolved for `owner_escalate_minutes`
    // reaches the owner, as one item naming the lead. A merge-conflict wait
    // never does: the lead's resolve task (#267) handles it.
    for task in team.tasks.iter().filter(|task| task.status == TaskStatus::Pending) {
        let Some(wait) = task.waits.as_ref().filter(|wait| !wait.running()) else { continue };
        if is_merge_conflict(wait) {
            continue;
        }
        let minutes = team.settings.owner_escalate_minutes;
        if minutes == 0 || now - wait.at < minutes as f64 * 60.0 {
            continue;
        }
        let lead = team.member(&team.lead).map(|lead| team.name_of(&lead.id)).unwrap_or_else(|| "the lead".into());
        let mut found = item(
            team,
            "task_wait_escalated",
            &format!("escalated-{}", task.id),
            format!("#{} {} still waits after {minutes} min: {} ({lead} has not acted)", task.id, task.title, wait.reason),
            wait.at,
            "open_team",
        );
        found["task"] = json!(task.id);
        found["lead"] = json!(lead);
        found["reason"] = json!(wait.reason);
        found["waits_for"] = json!(wait.waits_for);
        found["minutes"] = json!(minutes);
        out.push(found);
    }
    // #718: the proposal of a full test run is the owner's call too, and a
    // flaky suite's quarantine shows the owner until it expires.
    if let Some(proposal) = &team.test_full_proposal {
        let mut found = item(team, "test_full_run", &format!("{}", proposal.at as i64), super::inbox::full_run_text(proposal), proposal.at, "open_team");
        found["seconds"] = json!(proposal.seconds);
        found["reused"] = json!(proposal.reused);
        out.push(found);
    }
    for quarantine in team.test_quarantine.iter().filter(|quarantine| quarantine.until > now) {
        let until = chrono::DateTime::from_timestamp(quarantine.until as i64, 0).map(|time| time.format("%Y-%m-%d").to_string()).unwrap_or_default();
        let title = format!("{} is quarantined as flaky ({} episode(s)) until {until}", quarantine.suite, quarantine.episodes);
        out.push(item(team, "test_quarantine", &quarantine.suite, title, quarantine.since, "open_team"));
    }
    // Every task decided: the team waits for the owner (or the lead) to finish
    // it or add the next phase; no draft phase is waiting.
    // U63: a phase that waits for the owner's acceptance (its own item) or
    // for its rework is not the end yet.
    let accepting = team.phases.iter().any(|phase| super::plan::awaits_acceptance(team, phase) || phase.acceptance_rework);
    if super::ops::all_decided(team) && !accepting && !team.phases.iter().any(|phase| phase.state == PhaseState::Draft) {
        let decided = team
            .tasks
            .iter()
            .flat_map(|task| task.attempts.iter().filter_map(|attempt| attempt.decided_at))
            .fold(0.0_f64, f64::max);
        let tasks = team.tasks.len();
        out.push(item(
            team,
            "team_finished",
            "finish",
            format!("All {tasks} task{} decided: waits for you to finish", if tasks == 1 { "" } else { "s" }),
            if decided > 0.0 { decided } else { team.updated },
            "finish",
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::teams::model::{Attempt, AttemptState, Hold, Member, PendingAddition, Phase, Report, Task, TaskWait, TeamPause};

    fn team(id: &str, status: TeamStatus) -> Team {
        Team {
            id: id.into(),
            name: format!("Team {id}"),
            status,
            lead: "lead".into(),
            created: 100.0,
            updated: 100.0,
            members: vec![
                Member { id: "lead".into(), role: "Lead".into(), ..Default::default() },
                Member { id: "dev".into(), role: "Backend".into(), ..Default::default() },
            ],
            tasks: vec![Task { id: 1, title: "API".into(), assignee: "dev".into(), ..Default::default() }],
            ..Default::default()
        }
    }

    fn kinds(items: &[Value]) -> Vec<&str> {
        items.iter().map(|item| item["kind"].as_str().unwrap()).collect()
    }

    /// One fixture per kind, across several teams, each naming its team.
    #[test]
    fn every_kind_across_teams_names_its_team_newest_first() {
        // Within the addition's confirmation window (warned at 4500).
        let now = 6_200.0;
        let planning = team("t-plan", TeamStatus::Planning);

        let mut phased = team("t-phase", TeamStatus::Running);
        phased.tasks.push(Task { id: 2, phase: 2, title: "UI".into(), ..Default::default() });
        phased.phases.push(Phase { n: 2, title: "Screens".into(), state: PhaseState::Draft, tasks: vec![2], added_at: 1000.0, ..Default::default() });

        let mut reported = team("t-report", TeamStatus::Running);
        reported.settings.remind_owner_minutes = 30;
        reported.tasks[0].status = TaskStatus::Submitted;
        reported.tasks[0].attempts.push(Attempt {
            n: 1,
            member: "dev".into(),
            state: AttemptState::Reported,
            report: Some(Report { text: "Done".into(), at: 2000.0, ..Default::default() }),
            ..Default::default()
        });

        let mut asking = team("t-perm", TeamStatus::Running);
        asking.settings.remind_owner_permission_minutes = 5;
        asking.members[1].pending = Some(super::super::model::PendingQuestion { at: 3000.0, ..Default::default() });

        let mut proposing = team("t-word", TeamStatus::Paused);
        proposing.proposals.push(json!({"id":"p1","kind":"add-member","state":"open","at":4000.0,"reason":"More tests"}));
        proposing.proposals.push(json!({"id":"p0","kind":"remove-member","state":"accepted","at":10.0}));
        proposing.pending_additions.push(PendingAddition { id: "a1".into(), phase: 1, tasks: vec![Task::default()], warned_at: 4500.0, ..Default::default() });

        let mut held = team("t-held", TeamStatus::Running);
        held.tasks[0].hold = Some(Hold { by: "You".into(), at: 5000.0, reason: "After the release".into() });
        held.tasks.push(Task { id: 2, title: "Lead's".into(), hold: Some(Hold { by: "Lead".into(), at: 5100.0, ..Default::default() }), ..Default::default() });

        let mut finished = team("t-done", TeamStatus::Running);
        finished.tasks[0].status = TaskStatus::Accepted;
        finished.tasks[0].attempts.push(Attempt { n: 1, state: AttemptState::Reported, decided_at: Some(6000.0), ..Default::default() });

        // Nothing waits for the owner here: a running team at work, an interim
        // report on an open attempt, a stopped team and an archived one.
        let mut quiet = team("t-quiet", TeamStatus::Running);
        quiet.tasks[0].status = TaskStatus::InProgress;
        quiet.tasks[0].attempts.push(Attempt {
            n: 1,
            id: "a1".into(),
            member: "dev".into(),
            state: AttemptState::Delivered,
            interim: Some(Report { text: "Half the API is in".into(), at: 7000.0, ..Default::default() }),
            ..Default::default()
        });
        let mut stopped = team("t-stop", TeamStatus::Stopped);
        stopped.tasks[0].hold = Some(Hold { by: "You".into(), at: 9000.0, ..Default::default() });

        let teams = vec![planning, phased, reported, asking, proposing, held, finished, quiet, stopped];
        let all = items(&teams, now);
        // Task 112: the paused team also waits for Resume team.
        let paused: Vec<&Value> = all.iter().filter(|item| item["kind"] == "team_paused").collect();
        assert_eq!(paused.len(), 1, "{all:#?}");
        assert_eq!((paused[0]["team"].as_str(), paused[0]["action"].as_str()), (Some("t-word"), Some("resume_team")));
        let list: Vec<Value> = all.into_iter().filter(|item| item["kind"] != "team_paused").collect();
        assert_eq!(
            kinds(&list),
            ["team_finished", "task_waits_owner", "owner_word_asked", "owner_word_asked", "permission_escalated", "report_escalated", "phase_draft", "plan_draft"],
            "{list:#?}"
        );
        assert!(!list.iter().any(|item| item["title"].as_str().unwrap_or_default().contains("Half the API is in")), "an interim report is not the owner's: {list:#?}");
        for item in &list {
            let team = item["team"].as_str().unwrap();
            assert_eq!(item["team_name"], format!("Team {team}"));
            assert!(!item["title"].as_str().unwrap().is_empty() && !item["action"].as_str().unwrap().is_empty(), "{item}");
        }
        assert_eq!(list[0]["since"], 6000.0);
        assert_eq!(list[1]["task"], 1);
        assert!(list[1]["title"].as_str().unwrap().contains("After the release"));
        assert_eq!((list[2]["action"].as_str(), list[3]["proposal"].as_str()), (Some("confirm_addition"), Some("p1")));
        assert_eq!(list[4]["member"], "Backend");
        assert_eq!((list[5]["task"].as_u64(), list[5]["member"].as_str()), (Some(1), Some("Backend")));
        assert_eq!(list[6]["phase"], 2);
        assert_eq!((list[7]["team"].as_str(), list[7]["action"].as_str()), (Some("t-plan"), Some("approve_plan")));
    }

    /// U29: an addition past its confirmation window is not listed: it can
    /// no longer be confirmed, and it leaves on the team's next change.
    #[test]
    fn an_expired_addition_is_not_listed() {
        let mut asking = team("t", TeamStatus::Running);
        asking.pending_additions.push(PendingAddition { id: "a1".into(), phase: 1, tasks: vec![Task::default()], warned_at: 1000.0, ..Default::default() });
        assert_eq!(kinds(&items(std::slice::from_ref(&asking), 1000.0 + 60.0)), ["owner_word_asked"]);
        let expired = 1000.0 + super::super::owner_word::FRESH_SECONDS + 1.0;
        assert!(items(&[asking], expired).is_empty());
    }

    /// A report or permission is the owner's only once the attention list
    /// escalates it; an auto-approved plan never waits.
    #[test]
    fn nothing_waits_before_it_is_escalated() {
        let mut reported = team("t", TeamStatus::Running);
        reported.settings.remind_owner_minutes = 30;
        reported.tasks[0].status = TaskStatus::Submitted;
        reported.tasks[0].attempts.push(Attempt {
            n: 1,
            state: AttemptState::Reported,
            report: Some(Report { text: "Done".into(), at: 1000.0, ..Default::default() }),
            ..Default::default()
        });
        let mut auto = team("a", TeamStatus::Planning);
        auto.settings.approval = super::super::model::Approval::Auto;
        let empty = Team { tasks: Vec::new(), ..team("e", TeamStatus::Planning) };
        assert!(items(&[reported.clone(), auto, empty], 1000.0 + 60.0).is_empty());
        assert_eq!(kinds(&items(&[reported], 1000.0 + 31.0 * 60.0)), ["report_escalated"]);
    }

    /// Task #286: a task's wait is the lead's, not the owner's; after the
    /// team's `owner_escalate_minutes` it reaches the owner as one item naming
    /// the lead, and a merge-conflict wait never does.
    #[test]
    fn a_task_wait_belongs_to_the_lead_and_escalates_after_the_setting() {
        let wait = |at: f64, reason: &str| TaskWait {
            waits_for: "zerus".into(),
            on: vec!["model".into()],
            reason: reason.into(),
            at,
            state: "refused".into(),
            ..Default::default()
        };
        let mut waiting = team("t", TeamStatus::Running);
        waiting.tasks[0].waits = Some(wait(700.0, "hgs: unknown model gpt-5.5 for this account"));
        waiting.tasks[0].hold = Some(Hold { by: "You".into(), at: 600.0, ..Default::default() });
        assert_eq!(waiting.settings.owner_escalate_minutes, 60);
        // Within the setting: only the owner's own hold is listed.
        let young = items(&[waiting.clone()], 700.0 + 59.0 * 60.0);
        assert_eq!(kinds(&young), ["task_waits_owner"], "{young:#?}");
        // Past it: one escalation item naming the lead, beside the hold.
        let old = items(&[waiting.clone()], 700.0 + 61.0 * 60.0);
        let escalated: Vec<&Value> = old.iter().filter(|item| item["kind"] == "task_wait_escalated").collect();
        assert_eq!(escalated.len(), 1, "{old:#?}");
        let found = escalated[0];
        assert_eq!((found["task"].as_u64(), found["lead"].as_str()), (Some(1), Some("Lead")));
        assert_eq!(found["minutes"], 60);
        assert!(found["title"].as_str().unwrap().contains("Lead has not acted"), "{}", found["title"]);
        assert!(found["reason"].as_str().unwrap().contains("unknown model"));
        // Still one, however long it stays unresolved: it escalates once.
        let later = items(&[waiting], 700.0 + 600.0 * 60.0);
        assert_eq!(later.iter().filter(|item| item["kind"] == "task_wait_escalated").count(), 1, "{later:#?}");
        // A merge conflict is the lead's/merger's resolve task: never, however old.
        let mut conflict = team("c", TeamStatus::Running);
        conflict.tasks[0].waits = Some(wait(
            700.0,
            "#229's accepted commit abcdef conflicts in src/x.rs; merge it into the member's branch (folder) and resolve it, or reassign the task, then retry",
        ));
        assert!(!kinds(&items(&[conflict], 700.0 + 600.0 * 60.0)).contains(&"task_wait_escalated"));
        // The setting at 0 never escalates.
        let mut off = team("o", TeamStatus::Running);
        off.settings.owner_escalate_minutes = 0;
        off.tasks[0].waits = Some(wait(700.0, "hgs: unknown model"));
        assert!(items(&[off], 700.0 + 600.0 * 60.0).is_empty());
    }

    /// Task #286: the lead-changes digest is one grouped row (added, moved,
    /// cancelled, ...) with counts and the newest few; Seen clears it.
    #[test]
    fn the_lead_changes_digest_is_one_grouped_row_that_seen_clears() {
        use crate::state::teams::model::LeadChange;
        let mut team = team("t", TeamStatus::Running);
        for (id, op, summary, at) in [
            ("lc1", "add-to-phase", "added #5 X to phase 1", 100.0),
            ("lc2", "move-task", "moved #6 Y from phase 1 to 2", 110.0),
            ("lc3", "add-to-phase", "added #7 Z to phase 2", 120.0),
            ("lc4", "cancel", "cancelled #8 W", 130.0),
        ] {
            team.lead_changes.push(LeadChange {
                id: id.into(), op: op.into(), summary: summary.into(), by: "Lead".into(), at, ..Default::default()
            });
        }
        let list = items(&[team.clone()], 200.0);
        let digest: Vec<&Value> = list.iter().filter(|item| item["kind"] == "lead_changes").collect();
        assert_eq!(digest.len(), 1, "one row: {list:#?}");
        let groups = digest[0]["groups"].as_array().expect("the groups");
        assert_eq!(groups.len(), 3, "{groups:?}");
        let counts: std::collections::BTreeMap<&str, u64> = groups
            .iter()
            .map(|group| (group["kind"].as_str().unwrap_or_default(), group["count"].as_u64().unwrap_or_default()))
            .collect();
        assert_eq!((counts.get("added"), counts.get("moved"), counts.get("cancelled")), (Some(&2), Some(&1), Some(&1)), "{groups:?}");
        let added = groups.iter().find(|group| group["kind"] == "added").expect("the added group");
        assert_eq!(added["newest"].as_array().unwrap().len(), 2, "the newest few of the kind");
        // Seen clears the row.
        let mut events = Vec::new();
        super::super::lead_plan::seen(&mut team, &json!({}), &crate::state::teams::ops::Actor::Human, &mut events).unwrap();
        assert!(!items(&[team], 400.0).iter().any(|item| item["kind"] == "lead_changes"));
    }

    #[test]
    fn an_empty_list_is_one_empty_page() {
        let page = page(&items(&[], 0.0), None, None);
        assert_eq!(page, json!({"items":[],"next":null,"total":0}));
    }

    /// Paging stays stable while items arrive: a newer item lands before the
    /// cursor and the next page neither repeats nor skips an older one; equal
    /// times keep their id order.
    #[test]
    fn pages_stay_stable_while_items_arrive() {
        let held = |n: u32, at: f64| {
            let mut team = team(&format!("t{n:02}"), TeamStatus::Running);
            team.tasks[0].hold = Some(Hold { by: "You".into(), at, ..Default::default() });
            team
        };
        // Two items share a time.
        let mut teams: Vec<Team> = (0..7).map(|n| held(n, 100.0 + f64::from(n / 2 * 2))).collect();
        let all = items(&teams, 0.0);
        let first = page(&all, None, Some(3));
        assert_eq!(first["total"], 7);
        assert_eq!(first["items"].as_array().unwrap().len(), 3);
        let cursor = first["next"].clone();
        // New items arrive, one newer than everything, one at the cursor's time.
        teams.push(held(50, 500.0));
        teams.push(held(51, cursor["since"].as_f64().unwrap()));
        let arrived = items(&teams, 0.0);
        let mut seen: Vec<String> = first["items"].as_array().unwrap().iter().map(|item| id(item).to_owned()).collect();
        let mut next = Some(cursor);
        while let Some(cursor) = next.filter(|cursor| !cursor.is_null()) {
            let page = page(&arrived, Some(&cursor), Some(3));
            seen.extend(page["items"].as_array().unwrap().iter().map(|item| id(item).to_owned()));
            next = Some(page["next"].clone());
        }
        let original: Vec<String> = all.iter().map(|item| id(item).to_owned()).collect();
        // Every original item exactly once, in the original order; the newer
        // arrival is not on the old pages.
        let kept: Vec<String> = seen.iter().filter(|id| original.contains(id)).cloned().collect();
        assert_eq!(kept, original);
        assert!(!seen.iter().any(|id| id.starts_with("t50/")), "{seen:?}");
        assert_eq!(page(&arrived, None, Some(1))["items"][0]["team"], "t50");
        // A page never exceeds the bound.
        assert_eq!(page(&arrived, None, Some(10_000))["items"].as_array().unwrap().len(), arrived.len().min(MAX_PAGE));
    }
    /// Task #246: an item's state is the situation the owner dismissed; only
    /// its change returns the row.
    #[test]
    fn every_item_carries_the_state_that_returns_it() {
        let mut paused = team("t", TeamStatus::Paused);
        paused.paused = Some(TeamPause { at: 100.0, by: "You".into(), ..Default::default() });
        let stored = items(&[paused.clone()], 200.0).into_iter().find(|item| item["kind"] == "team_paused").unwrap();
        assert_eq!(stored["state"], "paused:100.0", "{stored}");
        // A second pause is a new situation.
        paused.paused = Some(TeamPause { at: 300.0, ..Default::default() });
        let again = items(&[paused], 400.0).into_iter().find(|item| item["kind"] == "team_paused").unwrap();
        assert_eq!(again["state"], "paused:300.0");
        // A plan waits for a new revision.
        let mut planning = team("p", TeamStatus::Planning);
        planning.settings.approval = super::super::model::Approval::Review;
        planning.revision = 7;
        assert_eq!(items(&[planning.clone()], 100.0)[0]["state"], "plan:7");
        planning.revision = 8;
        assert_eq!(items(&[planning], 100.0)[0]["state"], "plan:8");
    }

    /// Task #246: a dismissal hides the row while its state is the same; a
    /// changed state returns it, and the old record stops hiding anything.
    #[test]
    fn a_dismissal_hides_the_row_until_its_state_changes() {
        let mut paused = team("t", TeamStatus::Paused);
        paused.paused = Some(TeamPause { at: 100.0, ..Default::default() });
        let list = items(&[paused.clone()], 200.0);
        let item = list.iter().find(|item| item["kind"] == "team_paused").unwrap().clone();
        let dismissal = json!({"id":item["id"],"state":item["state"],"at":150.0,"team":"t"});
        let (active, dismissed) = split(list, std::slice::from_ref(&dismissal));
        assert!(active.is_empty(), "{active:?}");
        assert_eq!(dismissed.len(), 1);
        assert_eq!(dismissed[0]["dismissed_at"], 150.0);
        assert!(dismissed[0].get("blocking").is_none(), "a paused team blocks nothing");
        paused.paused = Some(TeamPause { at: 300.0, ..Default::default() });
        let (active, dismissed) = split(items(&[paused], 400.0), &[dismissal]);
        assert_eq!(active.len(), 1, "the second pause returns the row");
        assert!(dismissed.is_empty());
    }

    /// Task #246: a dismissed owner decision stays in the dismissed section,
    /// naming what it still blocks.
    #[test]
    fn a_dismissed_decision_names_what_it_still_blocks() {
        let mut planning = team("t", TeamStatus::Planning);
        planning.settings.approval = super::super::model::Approval::Review;
        let list = items(&[planning], 100.0);
        let item = list[0].clone();
        let (active, dismissed) = split(list, &[json!({"id":item["id"],"state":item["state"],"at":10.0})]);
        assert!(active.is_empty());
        assert_eq!(dismissed[0]["blocking"], true, "{dismissed:?}");
        assert_eq!(dismissed[0]["blocks"], "Team t");
        // The live count excludes it; the dismissed count keeps it apart.
        let page = page_with_dismissed(&active, &dismissed, None, None);
        assert_eq!((page["total"].as_u64(), page["dismissed_total"].as_u64()), (Some(0), Some(1)));
    }

    #[test]
    fn the_store_keeps_and_removes_a_dismissal() {
        let dir = tempfile::tempdir().unwrap();
        let store = super::super::store::Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        store.dismiss("t/team_paused/100", "t", "paused:100.0", 5.0, "human").unwrap();
        let read = store.dismissed().unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!((read[0]["id"].as_str(), read[0]["state"].as_str()), (Some("t/team_paused/100"), Some("paused:100.0")));
        assert!(store.undismiss("t/team_paused/100").unwrap());
        assert!(store.dismissed().unwrap().is_empty());
    }
}
