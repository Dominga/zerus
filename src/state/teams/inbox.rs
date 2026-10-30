//! WhiteWh fork: the lead's load (C1-C4). A report, question, permission
//! request, the lead's own task and a stopped member go to one inbox, not to
//! one message each; status says what every open task waits for; an undecided
//! report reminds the lead once and then shows on the owner's attention list.
use super::model::*;
use super::plan;
use serde_json::{json, Value};

/// The most items the inbox reports at once.
const ITEM_LIMIT: usize = 100;
/// The most items one coalesced message lists, with the rest named by count.
const MESSAGE_ITEMS: usize = 10;
/// The most characters of a summary the inbox keeps.
const SUMMARY_CHARS: usize = 300;

/// One thing on the lead's desk. `key` names it across ticks, so the coalesced
/// message never names it twice.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    /// `report`, `question`, `permission`, `lead_task`, `stalled`, `budget`
    /// or `task_waits` (spec 0: U45, U46, U48).
    pub kind: &'static str,
    pub key: String,
    pub task: Option<u32>,
    pub member: String,
    pub since: f64,
    pub summary: String,
    pub command: String,
    /// Extra lines the message shows under the item: B1 puts the exact
    /// permission command and its source here (4000 characters cut).
    pub detail: String,
    /// U19 (#520): a subagent permission's rows ({id,type,model,effort?,
    /// description}); null for every other item.
    pub subagents: Value,
}

impl Item {
    /// The item as `hgs team inbox --json` lists it.
    pub(crate) fn value(&self) -> Value {
        let mut value = json!({"kind":self.kind,"task":self.task,"member":self.member,"since":self.since,
            "summary":self.summary,"command":self.command});
        // U18: every item says how long it has waited, so the desktop and the
        // lead see the stuck ones without reading timestamps.
        value["age"] = json!(age(crate::state::now(), self.since));
        if !self.detail.is_empty() {
            value["detail"] = json!(self.detail);
        }
        if self.subagents.is_array() {
            value["subagents"] = self.subagents.clone();
        }
        value
    }
}

/// One line of a text, never a paragraph: the inbox message stays readable.
fn summary(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let flat = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > SUMMARY_CHARS {
        flat.chars().take(SUMMARY_CHARS - 1).collect::<String>() + "\u{2026}"
    } else {
        flat
    }
}

/// The request id the lead replaces when it runs a command from a message.
const PLACEHOLDER: &str = "UUID";

/// The report command of a task, with the attempt when there is one.
fn report_command(team: &Team, task: &Task, attempt: Option<&Attempt>, accept: bool) -> String {
    let mut body = json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"text":"..."});
    if let Some(attempt) = attempt {
        body["attempt"] = json!(attempt.id);
    }
    if accept {
        body["accept"] = json!(true);
    }
    format!("{body} | hgs team report --json")
}

/// The decision command of a report: the lead accepts or rejects it.
fn decide_command(team: &Team, task: &Task, accept: bool) -> String {
    format!(
        "{} | hgs team {} --json",
        json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id}),
        if accept { "accept" } else { "reject" }
    )
}

/// A7: what a captured report means for its member, for the reviewer.
pub(crate) fn captured_hold_note(team: &Team, member: &str) -> String {
    let role = team.name_of(member);
    format!(
        "Captured from {role}'s last reply, which may only acknowledge the task. {role} gets no new work until you \
         accept, reject, retry, reassign or cancel this task, or {role} replaces the report with hgs team report."
    )
}

/// How many decided proposals the lead's inbox lists (C9), the latest first.
const DECIDED_PROPOSALS: usize = 5;

/// The items waiting for the lead, oldest first, at most `ITEM_LIMIT`: the
/// team's own and those of its direct reports (spec 5.6).
pub(crate) fn items(team: &Team) -> Vec<Item> {
    items_for(team, &team.lead)
}

/// The items waiting for `reviewer` - the lead or a sub-lead (spec 5.6): a
/// report, question or stall goes to whoever reviews the task's assignee;
/// everything else is the lead's.
pub(crate) fn items_for(team: &Team, reviewer: &str) -> Vec<Item> {
    let mut items = all_items(team);
    items.retain(|item| reviewer_of(team, item) == reviewer);
    items
}

fn reviewer_of(team: &Team, item: &Item) -> String {
    match (item.kind, item.task.and_then(|id| team.task(id))) {
        // #538 N3: a report a sub-lead may not accept is the lead's.
        ("report", Some(task)) if super::pipeline::lead_accepts_only(task) => team.lead.clone(),
        // The lead's own task has no reviewer: it stays the lead's.
        ("report" | "question" | "stalled", Some(task)) if !task.assignee.is_empty() && task.assignee != team.lead => {
            team.reviewer_of(&task.assignee)
        }
        _ => team.lead.clone(),
    }
}

fn all_items(team: &Team) -> Vec<Item> {
    let active = matches!(team.status, TeamStatus::Running | TeamStatus::Paused);
    // N5: a finished team has no open work, but the owner's decision on the
    // lead's proposal is still the lead's to read (spec 4).
    let finished = matches!(team.status, TeamStatus::Done | TeamStatus::Stopped);
    if !active && !finished {
        return Vec::new();
    }
    let mut items: Vec<Item> = Vec::new();
    // C9: the team's own items, once each, not once per task.
    // F1.4: a member whose launch failed (the folder check) is an item.
    for member in team
        .members
        .iter()
        .filter(|member| member.id != team.lead)
        .filter(|_| active)
        .filter(|member| member.state != MemberState::Removed && !member.error.is_empty() && member.session.is_none())
    {
        let role = team.name_of(&member.id);
        items.push(Item {
            subagents: Value::Null,
            kind: "member_error",
            key: format!("member_error:{}", member.id),
            task: None,
            member: role.clone(),
            since: crate::state::now(),
            summary: summary(&format!("{role} was not started: {}", member.error)),
            command: format!(
                "{} | hgs team relaunch --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"member":member.id})
            ),
            detail: member.error.clone(),
        });
    }
    // Spec 0: a task that waits for a step Zerus could not take (U45 model,
    // U46 context, U48 branch) is one item for the lead, with the reason and
    // the retry that starts the step again. A context action that still runs
    // waits for nobody.
    for task in team.tasks.iter().filter(|_| active).filter(|task| task.status == TaskStatus::Pending) {
        let Some(wait) = task.waits.as_ref().filter(|wait| !wait.running()) else { continue };
        let attempt = task.attempts.iter().rev().find(|attempt| wait.attempt > 0 && attempt.n == wait.attempt);
        let member = match attempt {
            Some(attempt) => team.name_of(&attempt.member),
            None if task.assignee.is_empty() => task.role.clone(),
            None => team.name_of(&task.assignee),
        };
        // U56/N3: the command fits the cause, in whole sentences: a free peer
        // takes the task, a paused member is resumed, a stopped session is
        // relaunched, and a stuck or offline member is the owner's call.
        let (command, detail) = if wait.waits_for == "member" {
            let target = task.assignee.clone();
            let found = team.member(&target);
            let state = found.map(|member| member.state).unwrap_or(MemberState::NotCreated);
            let live = found.and_then(|member| member.seen).is_some_and(|seen| seen.live);
            let free = team
                .members
                .iter()
                .find(|other| {
                    other.id != target
                        && other.state == MemberState::Active
                        && !other.draining
                        && other.stuck.is_none()
                        && other.seen.is_some_and(|seen| seen.live)
                        && team.role_of(&other.id) == team.role_of(&target)
                })
                .map(|other| other.id.clone());
            let resume = format!(
                "{} | hgs team resume-member --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"member":target})
            );
            let relaunch = format!(
                "{} | hgs team relaunch --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"member":target})
            );
            let owner = format!(
                "{} | hgs team settings --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"settings":{"lead_restarts_stuck":true}})
            );
            match (free, state) {
                (Some(peer), _) => (
                    format!(
                        "{} | hgs team reassign --json",
                        json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"member":peer})
                    ),
                    format!("{} Reassigning it to {peer} frees it now.", wait.reason),
                ),
                (None, MemberState::Paused) => (resume, format!("{} Resume the member to free it.", wait.reason)),
                (None, _) if !live => (relaunch, format!("{} Relaunch its session to free it.", wait.reason)),
                (None, _) => (
                    owner,
                    format!(
                        "{} No same-role member can take it. This is the owner's call: ask the owner to set lead_restarts_stuck, then interrupt the member.",
                        wait.reason
                    ),
                ),
            }
        } else {
            (
                format!("{} | hgs team retry --json", json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id})),
                wait.reason.clone(),
            )
        };
        items.push(Item {
            subagents: Value::Null,
            kind: "task_waits",
            key: format!("task_waits:{}:{}", task.id, wait.at),
            task: Some(task.id),
            member,
            since: wait.at,
            summary: summary(&format!("#{} {} waits: {}", task.id, task.title, wait.reason)),
            command,
            detail,
        });
    }
    // N3: a draining member whose tasks still keep it open is one item for the
    // lead, naming them, so a drain never waits silently; when no same-role peer
    // can take them, the item says that too.
    for member in team.members.iter().filter(|_| active).filter(|member| member.draining && member.state != MemberState::Removed) {
        let (role, name) = (team.role_of(&member.id), team.name_of(&member.id));
        let waiting: Vec<u32> = team
            .tasks
            .iter()
            .filter(|task| task.assignee == member.id && !task.status.finished())
            .map(|task| task.id)
            .collect();
        if waiting.is_empty() {
            continue;
        }
        let peer = team
            .members
            .iter()
            .any(|other| other.id != member.id && other.state != MemberState::Removed && !other.draining && team.role_of(&other.id) == role);
        let listed = waiting.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(" ");
        let why = if peer { "they wait for a decision".to_owned() } else { format!("no other {role} can take them") };
        items.push(Item {
            subagents: Value::Null,
            kind: "drain_held",
            key: format!("drain_held:{}", member.id),
            task: waiting.first().copied(),
            member: name.clone(),
            since: crate::state::now(),
            summary: summary(&format!("Draining {name}: {listed} still to decide; {why}")),
            command: format!("{} | hgs team status --json", json!({"team":team.id})),
            detail: format!("{name} is draining. {listed} keep it open: {why}."),
        });
    }
    // U56: a stuck member is one item with its id, phase, duration and the
    // tasks that wait for it, and the commands that free it.
    for member in team.members.iter().filter(|member| member.state != MemberState::Removed && member.stuck.is_some()) {
        let stuck = member.stuck.as_ref().unwrap();
        let waiting: Vec<u32> = team
            .tasks
            .iter()
            .filter(|task| task.assignee == member.id && !task.status.finished())
            .map(|task| task.id)
            .collect();
        let minutes = ((crate::state::now() - stuck.at) / 60.0).max(0.0).round() as u64;
        let why = if stuck.reason == "compaction_timeout" { "is stuck in compaction" } else { "has made no progress" };
        let phase = if stuck.phase.is_empty() { "unknown".to_owned() } else { stuck.phase.clone() };
        let label = format!("{} ({})", member.id, team.role_of(&member.id));
        let tasks = if waiting.is_empty() {
            String::new()
        } else {
            format!(
                " Its tasks wait: {}.",
                waiting.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(" ")
            )
        };
        // N3: whole sentences, and the command fits the cause: the owner's
        // setting route while it is off, the lead's restart once the owner set it.
        let (command, route) = if team.settings.lead_restarts_stuck {
            (
                format!(
                    "{} | hgs team relaunch --json",
                    json!({"request_id":PLACEHOLDER,"team":team.id,"member":member.id})
                ),
                format!("You may interrupt it and relaunch it: hgs team relaunch {}.", member.id),
            )
        } else {
            (
                format!(
                    "{} | hgs team settings --json",
                    json!({"request_id":PLACEHOLDER,"team":team.id,"settings":{"lead_restarts_stuck":true}})
                ),
                "This is the owner's call: ask the owner to set lead_restarts_stuck, with hgs team settings."
                    .to_owned(),
            )
        };
        items.push(Item {
            subagents: Value::Null,
            kind: "member_stuck",
            key: format!("member_stuck:{}", member.id),
            task: waiting.first().copied(),
            member: label.clone(),
            since: stuck.at,
            summary: summary(&format!("{label} {why} for {minutes} min in phase {phase}.{tasks}")),
            command,
            detail: format!(
                "{}{route}{tasks}",
                if stuck.reason == "compaction_timeout" { "Its context compaction did not finish. " } else { "" }
            ),
        });
    }
    // U62 (#717): each test result is one card linking its record - suite,
    // status, time, the submitter - never the log, which stays in the database.
    for notice in team.test_notices.iter().rev().take(20) {
        items.push(Item {
            subagents: Value::Null,
            kind: "test_result",
            key: format!("test_result:{}", notice.record),
            task: None,
            member: notice.asked_by.clone(),
            since: notice.at,
            summary: summary(&format!(
                "Test {} {} in {} s (record {})",
                notice.suite,
                notice.status,
                notice.duration_ms / 1000,
                notice.record
            )),
            command: format!(
                "{} | hgs team test results --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"record":notice.record})
            ),
            detail: format!(
                "Asked by {}. The log and excerpt stay in record {}; read them with hgs team test results --json, not from this card.",
                if notice.asked_by.is_empty() { "an unknown caller" } else { notice.asked_by.as_str() },
                notice.record
            ),
        });
    }
    // U52: a member whose session crashed too often this hour stays stopped
    // until the owner or the lead starts it again.
    for member in team.members.iter().filter(|member| member.id != team.lead && member.crash_told).filter(|_| active) {
        let role = team.name_of(&member.id);
        let at = member.crash.unwrap_or_else(crate::state::now);
        let restarts = member.restarts.iter().filter(|restart| crate::state::now() - **restart <= 3600.0).count();
        let command = member
            .session
            .as_ref()
            .map(|session| format!("hgs resume {} -d", session.name))
            .unwrap_or_default();
        items.push(Item {
            subagents: Value::Null,
            kind: "crashed",
            key: format!("crashed:{}", member.id),
            task: None,
            member: role.clone(),
            since: at,
            summary: summary(&format!(
                "{role}'s session crashed again after {restarts} restart(s) this hour and stays stopped"
            )),
            command,
            detail: "The team's restart_crashed setting launches a crashed member at most three times an hour; a fourth crash waits for you.".into(),
        });
    }
    // F8: each decided proposal reaches the lead with the decision and the
    // owner's reason, not only as a feed event; the latest few, by decision.
    let mut decided: Vec<&Value> = team
        .proposals
        .iter()
        .filter(|proposal| matches!(proposal["state"].as_str(), Some("accepted") | Some("rejected")))
        .collect();
    let decided_at = |proposal: &Value| proposal["decided_at"].as_f64().or(proposal["at"].as_f64()).unwrap_or_default();
    decided.sort_by(|a, b| decided_at(b).total_cmp(&decided_at(a)));
    for proposal in decided.into_iter().take(DECIDED_PROPOSALS) {
        let id = proposal["id"].as_str().unwrap_or_default().to_owned();
        let state = proposal["state"].as_str().unwrap_or_default().to_owned();
        let kind = proposal["kind"].as_str().unwrap_or_default();
        let note = proposal["note"].as_str().unwrap_or_default();
        let why = if note.is_empty() { String::new() } else { format!(": {note}") };
        items.push(Item {
            subagents: Value::Null,
            kind: "proposal_decided",
            key: format!("proposal:{id}"),
            task: None,
            member: "owner".into(),
            since: decided_at(proposal),
            summary: summary(&format!("The owner {state} your {kind} proposal {id}{why}")),
            command: format!("{} | hgs team status --json", json!({"request_id":PLACEHOLDER,"team":team.id})),
            detail: proposal["note"].as_str().unwrap_or_default().to_owned(),
        });
    }
    if !active {
        // N5: the decision above is all a finished team still has to say.
        return items;
    }
    // U25: a phase the owner sent back is a review item: the comment and the
    // command that returns the reworked phase to the owner as a draft.
    for phase in team.phases.iter().filter(|phase| phase.state == PhaseState::Rejected) {
        let Some(rejection) = &phase.rejection else { continue };
        items.push(Item {
            subagents: Value::Null,
            kind: "phase_rejected",
            key: format!("phase_rejected:{}:{}", phase.n, rejection.at),
            task: None,
            member: "owner".into(),
            since: rejection.at,
            summary: summary(&format!("The owner sent {} back: {}", phase_name(phase), rejection.comment)),
            command: format!(
                "{} | hgs team rework-phase --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"phase":phase.n,"note":"..."})
            ),
            detail: format!(
                "Change the draft with edit-draft (or extend \"phase\":{}), then rework-phase returns it to the owner.\n{}",
                phase.n, rejection.comment
            ),
        });
    }
    for task in &team.tasks {
        // A report to decide (C2).
        if task.status == TaskStatus::Submitted {
            if let Some(attempt) = task.attempts.iter().rev().find(|attempt| attempt.state == AttemptState::Reported) {
                if let Some(report) = &attempt.report {
                    items.push(Item {
                        subagents: Value::Null,
                        kind: "report",
                        // A replaced report (A6: the member's own report after a
                        // captured reply) is a new item: the key carries how many
                        // texts it replaced, so the digest tells the lead again.
                        key: format!("report:{}:{}:{}", task.id, attempt.id, report.replaced.len()),
                        task: Some(task.id),
                        member: team.name_of(&attempt.member),
                        since: report.at,
                        // U31: a report made ahead of its draft phase waits for
                        // the owner's approval of that phase.
                        summary: if attempt.worked_ahead {
                            format!("worked ahead of phase {}: {}", task.phase, summary(&report.text))
                        } else if !report.replaced.is_empty() {
                            format!("replaces the earlier report: {}", summary(&report.text))
                        } else {
                            summary(&report.text)
                        },
                        command: decide_command(team, task, true),
                        // A7: a captured reply may be only an acknowledgement;
                        // its member waits for this decision. U36: after the
                        // round limit the lead reads every round's findings
                        // with the report.
                        detail: [
                            if report.captured && attempt.member != team.lead {
                                captured_hold_note(team, &attempt.member)
                            } else {
                                String::new()
                            },
                            super::pipeline::rounds_text(task),
                        ]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    });
                }
            }
        }
        for attempt in &task.attempts {
            // A member's question the lead answers with `reply` (C2).
            for question in attempt.questions.iter().filter(|question| question.open()) {
                items.push(Item {
                    subagents: Value::Null,
                    kind: "question",
                    key: format!("question:{}", question.id),
                    task: Some(task.id),
                    member: team.name_of(&attempt.member),
                    since: question.at,
                    summary: summary(&question.text),
                    command: format!(
                        "{} | hgs team reply --json",
                        json!({"request_id":PLACEHOLDER,"team":team.id,"question":question.id,"text":"..."})
                    ),
                    detail: String::new(),
                });
            }
        }
        // #630 N6: a stage member's question, on its stage run.
        if let Some(pipeline) = task.pipeline_run.as_ref() {
            for run in &pipeline.runs {
                for question in run.questions.iter().filter(|question| question.open()) {
                    items.push(Item {
                        subagents: Value::Null,
                        kind: "question",
                        key: format!("question:{}", question.id),
                        task: Some(task.id),
                        member: if run.member.is_empty() { "Zerus".into() } else { team.name_of(&run.member) },
                        since: question.at,
                        summary: summary(&question.text),
                        command: format!(
                            "{} | hgs team reply --json",
                            json!({"request_id":PLACEHOLDER,"team":team.id,"question":question.id,"text":"..."})
                        ),
                        detail: String::new(),
                    });
                }
            }
        }
        // The lead's own task: handed out without a message, reported and
        // accepted in one step (C1).
        if task.status == TaskStatus::InProgress {
            if let Some(attempt) = task.open_attempt().filter(|attempt| attempt.member == team.lead) {
                items.push(Item {
                    subagents: Value::Null,
                    kind: "lead_task",
                    key: format!("lead_task:{}:{}", task.id, attempt.id),
                    task: Some(task.id),
                    member: "Lead".into(),
                    since: attempt.delivered_at.unwrap_or(attempt.sent_at),
                    summary: summary(&format!("#{} {}", task.id, task.title)),
                    command: report_command(team, task, Some(attempt), true),
                    detail: String::new(),
                });
            }
        }
        // A member that stopped on its task (C2), unless it waits for an
        // answer: that is the question item above.
        let Some(attempt) = task.open_attempt() else { continue };
        if attempt.member == team.lead || task.status.finished() || attempt.open_question().is_some() {
            continue;
        }
        let member = team.member(&attempt.member);
        let kind = if attempt.no_report {
            Some("no_report")
        } else if member.and_then(|member| member.seen).is_some_and(|seen| !seen.live) {
            Some("stopped")
        } else {
            None
        };
        if let Some(kind) = kind {
            let role = team.name_of(&attempt.member);
            let what = if kind == "no_report" {
                format!("{role} ended its turn on #{} with neither a report nor a reply", task.id)
            } else {
                format!("{role}'s session stopped on #{}", task.id)
            };
            items.push(Item {
                subagents: Value::Null,
                kind: "stalled",
                key: format!("stalled:{}:{}:{kind}", task.id, attempt.id),
                task: Some(task.id),
                member: role,
                since: attempt.turn_ended_at.or(attempt.delivered_at).unwrap_or(attempt.sent_at),
                summary: summary(&format!("{what}: {}", task.title)),
                command: format!(
                    "{} | hgs team retry --json",
                    json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id})
                ),
                detail: String::new(),
            });
        }
    }
    // #630 N7: a stage whose member was removed or paused, or whose hand-off
    // was never confirmed, went back to Waiting; the lead frees it or lets the
    // next free member take it.
    for task in team.tasks.iter().filter(|_| active) {
        let Some(run) = task.pipeline_run.as_ref().and_then(|pipeline| pipeline.open_run()) else { continue };
        let (stalled, silent) = (!run.stalled.is_empty(), !run.silent.is_empty());
        if !stalled && !silent {
            continue;
        }
        let member = if !run.member.is_empty() { &run.member } else { &run.stalled_member };
        let role = if member.is_empty() { "Zerus".to_owned() } else { team.name_of(member) };
        if stalled {
            items.push(Item {
                kind: "stage_stalled",
                key: format!("stage_stalled:{}:{}", task.id, run.id),
                task: Some(task.id),
                member: role.clone(),
                since: run.delivered_at.unwrap_or(run.at),
                summary: summary(&format!("The {} stage of #{} is stalled: {}; it waits for a free member", run.kind, task.id, run.stalled)),
                command: format!("{} | hgs team send-back --json", json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"reason":"..."})),
                detail: String::new(),
                subagents: Value::Null,
            });
        }
        if silent {
            // #635 N1: the member's turn ended without a verdict; the stage is
            // still its own, so the lead may skip it or send the task back.
            items.push(Item {
                kind: "stage_stalled",
                key: format!("stage_stalled:{}:{}:silent", task.id, run.id),
                task: Some(task.id),
                member: role,
                since: run.delivered_at.unwrap_or(run.at),
                summary: summary(&format!("The {} stage of #{} stalled: {}", run.kind, task.id, run.silent)),
                command: format!("{} | hgs team send-back --json", json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"reason":"..."})),
                detail: String::new(),
                subagents: Value::Null,
            });
        }
    }
    // The permission requests the dispatcher read from the members' sessions
    // (C2): each is answerable by its own question id and hash.
    for member in &team.members {
        let Some(pending) = &member.pending else { continue };
        let asked = pending.card_question();
        items.push(Item {
            kind: "permission",
            key: format!("permission:{}:{}", member.id, pending.question_id),
            task: None,
            member: team.name_of(&member.id),
            since: pending.at,
            summary: if asked.is_empty() { "asks for permission".into() } else { summary(&asked) },
            command: pending.answer_command(team, &member.id),
            // B1: the exact command the permission asks about and its source.
            detail: super::dispatcher::permission_command_line(&pending.card).trim_end().to_owned(),
            subagents: if pending.card["subagents"].is_array() { pending.card["subagents"].clone() } else { Value::Null },
        });
    }
    // U20: a delivered attempt whose turn ended long ago and still has no
    // report is an item, whatever the capture did.
    let now = crate::state::now();
    for task in &team.tasks {
        let Some(attempt) = task.open_attempt() else { continue };
        if attempt.member == team.lead || attempt.report.is_some() {
            continue;
        }
        let Some(ended) = attempt.turn_ended_at else { continue };
        if now - ended < team.settings.report_timeout_seconds as f64 * 2.0 {
            continue;
        }
        let role = team.name_of(&attempt.member);
        let minutes = ((now - ended) / 60.0).round() as u64;
        items.push(Item {
            subagents: Value::Null,
            kind: "no_report",
            key: format!("no_report:{}:{}", task.id, attempt.id),
            task: Some(task.id),
            member: role.clone(),
            since: ended,
            summary: summary(&format!("{role} ended its turn {minutes} min ago on #{} without a report: {}", task.id, task.title)),
            command: format!(
                "{} | hgs team report --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"attempt":attempt.id,"text":"..."})
            ),
            detail: attempt.turn_reply.clone(),
        });
    }
    // H1: a member stopped by a usage limit is one item, with its reset time
    // when the session reports one.
    for member in &team.members {
        let Some(limit) = &member.limited else { continue };
        let role = team.name_of(&member.id);
        let reset = match limit.resets_at {
            Some(at) => format!("; resets at {}", local_time(at)),
            None => "; reset time unknown".to_owned(),
        };
        items.push(Item {
            subagents: Value::Null,
            kind: "member_limited",
            key: format!("member_limited:{}:{}", member.id, limit.at),
            task: None,
            member: role.clone(),
            since: limit.at,
            summary: summary(&format!("{role} stopped by a usage limit{reset}")),
            command: format!("{} | hgs team status --json", json!({"team":team.id})),
            detail: if limit.detail.is_empty() { String::new() } else { summary(&limit.detail) },
        });
    }
    items.sort_by(|a, b| a.since.total_cmp(&b.since).then_with(|| a.key.cmp(&b.key)));
    items.truncate(ITEM_LIMIT);
    items
}

impl PendingQuestion {
    /// The first question of the card and its option ids, as the inbox
    /// summarizes it: the lead picks one of these ids in its answer.
    fn card_question(&self) -> String {
        let mut text = String::new();
        for item in self.card["questions"].as_array().into_iter().flatten() {
            if let Some(header) = item["header"].as_str().filter(|header| !header.is_empty()) {
                text += &format!("{header}: ");
            }
            text += item["question"].as_str().unwrap_or_default();
            for option in item["options"].as_array().into_iter().flatten() {
                text += &format!(
                    " [{}] {}",
                    option["id"].as_str().unwrap_or_default(),
                    option["label"].as_str().unwrap_or_default()
                );
            }
        }
        text
    }

    /// The answer command for the lead: the exact id, hash and answers `hgs
    /// team answer` expects, with the option ids to replace.
    fn answer_command(&self, team: &Team, member: &str) -> String {
        let answers: Vec<Value> = self.card["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| json!({"question_id":item["id"],"selected_option_ids":["<option id>"],"text":""}))
            .collect();
        format!(
            "{} | hgs team answer --json",
            json!({"request_id":PLACEHOLDER,"team":team.id,"member":member,
                "question_id":self.question_id,"expected_question_hash":self.question_hash,"answers":answers})
        )
    }
}

impl std::fmt::Display for Item {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{}", self.kind.replace('_', " "))
    }
}

/// How long an item has waited, as the message says it.
fn age(now: f64, since: f64) -> String {
    let minutes = ((now - since).max(0.0) / 60.0).round() as u64;
    match minutes {
        0 => "just now".into(),
        1 => "1 min".into(),
        minutes => format!("{minutes} min"),
    }
}

/// A reset time as a person reads it, in this computer's zone; status keeps
/// the numeric `resets_at`.
fn local_time(at: f64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(at.round() as i64, 0)
        .map(|time| time.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| format!("{at:.0}"))
}

/// The single message that lists the lead's new items (C2).
pub(crate) fn text(team: &Team, items: &[Item], now: f64) -> String {
    let mut text = format!(
        "[Zerus team \u{b7} from: Zerus \u{b7} inbox]\nTeam: {}. {} item(s) wait for you, oldest first:\n",
        team.name,
        items.len()
    );
    for (index, item) in items.iter().take(MESSAGE_ITEMS).enumerate() {
        let task = item.task.map(|task| format!("task #{task} \u{b7} ")).unwrap_or_default();
        text += &format!(
            "\n{}. {} \u{b7} {task}{} \u{b7} {}\n   {}\n",
            index + 1,
            item,
            item.member,
            age(now, item.since),
            item.summary
        );
        // B1: a permission item carries the exact command and its source.
        if !item.detail.is_empty() {
            for line in item.detail.lines() {
                text += &format!("   {line}\n");
            }
        }
        text += &format!("   {}\n", item.command);
        // N7: the lead must be able to read the whole report, not only the
        // summary line.
        if item.kind == "report" {
            if let Some(task) = item.task {
                text += &format!("   Full report: task #{task} in {} | hgs team status --json\n", json!({"team":team.id}));
            }
        }
    }
    if items.len() > MESSAGE_ITEMS {
        text += &format!("\n[{} more item(s); hgs team inbox --json lists them all]\n", items.len() - MESSAGE_ITEMS);
    }
    text += &format!(
        "\nAll items: {} | hgs team inbox --json\n\
         A report is not decided by reading it: accept or reject each one. One message lists the new items, \
         at most once per {} s.",
        json!({"team":team.id}),
        team.settings.inbox_interval_seconds
    );
    text
}

/// What one open task waits for and which action unblocks it (C3).
pub(crate) fn waits(team: &Team, task: &Task, ready: bool) -> Option<Value> {
    if task.status.finished() {
        return None;
    }
    if let Some(hold) = &task.hold {
        return Some(json!({"for":"hold","on":[hold.by],"unblock":format!("release #{}", task.id)}));
    }
    // Spec 0: a step before the hand-out was refused or never confirmed; U46:
    // while Zerus still prepares the context it waits for nobody.
    if let Some(wait) = task.waits.as_ref().filter(|_| task.status == TaskStatus::Pending) {
        if wait.state == "waiting" {
            return Some(json!({"for":wait.waits_for,"on":wait.on,"reason":"Zerus is preparing the context","unblock":"wait"}));
        }
        return Some(json!({"for":wait.waits_for,"on":wait.on,"reason":wait.reason,"unblock":format!("retry or reassign #{}", task.id)}));
    }
    // U36: a test holds the assignee's folder; its next task waits for it.
    if matches!(task.status, TaskStatus::Pending) {
        if let Some(held) = super::stages::held(team, &task.assignee).filter(|held| *held != task.id) {
            return Some(json!({"for":"folder","on":[format!("#{held}")],"unblock":format!("folder held by the stage of #{held}")}));
        }
    }
    if task.status == TaskStatus::Submitted {
        return Some(json!({
            "for":"lead","on":[team.name_of(&team.lead)],"unblock":format!("accept #{}", task.id)
        }));
    }
    // U36: a stage waits for its member (or for one with its role).
    if let Some(run) = task.pipeline_run.as_ref().filter(|_| task.status.in_stage()) {
        let open = run.open_run();
        let who = match open {
            Some(stage) if !stage.member.is_empty() => team.name_of(&stage.member),
            Some(stage) => run.stages.get(stage.stage).map(|spec| spec.role.clone()).unwrap_or_default(),
            None => String::new(),
        };
        let kind = open.map(|stage| stage.kind.clone()).unwrap_or_default();
        return Some(json!({"for":"stage","on":[who],"unblock":format!("the {kind} of #{} by {who}", task.id)}));
    }
    if let Some(attempt) = task.open_attempt() {
        let role = team.name_of(&attempt.member);
        if let Some(question) = attempt.open_question() {
            return Some(json!({"for":"lead","on":[role],"unblock":format!("answer {}", question.id)}));
        }
        return Some(json!({"for":"member","on":[role],"unblock":format!("wait for {role}")}));
    }
    if task.status == TaskStatus::Draft {
        return Some(json!({
            "for":"owner","on":[format!("phase {}", task.phase)],
            "unblock":format!("approve phase {}", task.phase)
        }));
    }
    // A pending task: the dependency that is not satisfied names the action.
    let unmet: Vec<u32> = task.deps.iter().filter(|dep| !plan::satisfied(team, dep.task, dep.kind)).map(|dep| dep.task).collect();
    if !unmet.is_empty() {
        let first = unmet[0];
        let hard = team.settings.all_hard
            || task.deps.iter().find(|dep| dep.task == first).is_some_and(|dep| dep.kind == DependencyKind::Hard);
        return Some(json!({
            "for":"deps","on":unmet,
            "unblock":format!("{} #{}", if hard { "accept" } else { "start" }, first)
        }));
    }
    if !ready {
        let blockers = plan::barrier_blockers(team, task);
        if let Some(first) = blockers.first() {
            return Some(json!({
                "for":"deps","on":blockers,"unblock":format!("finish #{}", first)
            }));
        }
    }
    let role = team.name_of(&task.assignee);
    Some(json!({"for":"member","on":[role],"unblock":format!("wait for {role}")}))
}

/// Records the items a coalesced message named, and L2: keeps the key of an
/// item that is still open. A closed item's key leaves at once, so a later
/// re-tell only covers what is genuinely new.
pub(crate) fn mark_told(told: &mut Vec<String>, named: &[Item], open: &std::collections::BTreeSet<String>) {
    for item in named {
        if !told.iter().any(|key| key == &item.key) {
            told.push(item.key.clone());
        }
    }
    told.retain(|key| open.contains(key));
}

/// The reports the owner's attention list shows: undecided for at least
/// `remind_owner_minutes` (C4); 0 turns the list off.
/// "Phase N" or "Phase N · Title", as the feed and the prototype name it.
fn phase_name(phase: &Phase) -> String {
    if phase.title.is_empty() {
        format!("Phase {}", phase.n)
    } else {
        format!("Phase {} \u{b7} {}", phase.n, phase.title)
    }
}

pub(crate) fn attention(team: &Team, now: f64) -> Vec<Value> {
    if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return Vec::new();
    }
    // U25: the next draft waits for the owner's Accept ("Needs your answer"),
    // whatever the report reminder says; a draft behind it waits for it.
    let mut drafts: Vec<&Phase> = team.phases.iter().filter(|phase| phase.state.is_draft()).collect();
    drafts.sort_by_key(|phase| (phase.order(), phase.n));
    let phase_items: Vec<Value> = drafts
        .first()
        .filter(|phase| phase.state == PhaseState::Draft)
        .map(|phase| {
            json!({"kind":"phase_draft","phase":phase.n,"since":phase.added_at,
                "summary":format!("{} \u{2014} draft waits for your Accept", phase_name(phase))})
        })
        .into_iter()
        .collect();
    let report_minutes = team.settings.remind_owner_minutes;
    let permission_minutes = team.settings.remind_owner_permission_minutes;
    let mut out: Vec<Value> = Vec::new();
    if report_minutes > 0 {
        out.extend(
            team.tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Submitted)
                .filter_map(|task| {
                    let attempt = task.attempts.iter().rev().find(|attempt| attempt.state == AttemptState::Reported)?;
                    let report = attempt.report.as_ref()?;
                    let waited = (now - report.at).max(0.0);
                    (waited >= report_minutes as f64 * 60.0).then(|| {
                        json!({"kind":"report","task":task.id,"member":team.role_of(&attempt.member),"member_id":attempt.member,
                            "since":report.at,"minutes":(waited / 60.0).round() as u64})
                    })
                }),
        );
    }
    // D2: deferred findings reach the owner here, not only in the feed: one
    // item naming how many wait for a fix.
    let deferred: Vec<&crate::state::teams::model::Finding> =
        team.findings.iter().filter(|finding| finding.state == "deferred").collect();
    if !deferred.is_empty() {
        let latest = deferred.iter().map(|finding| finding.at).fold(0.0_f64, f64::max);
        out.push(json!({"kind":"findings","count":deferred.len(),"since":latest,
            "summary":format!("{} findings deferred", deferred.len())}));
    }
    // N17 (spec 8): a replan pauses the team for the owner's proposal; the
    // owner sees it here until the team runs again.
    if team.status == crate::state::teams::model::TeamStatus::Paused {
        for finding in team.findings.iter().filter(|finding| finding.state == "replan" && !finding.proposal.is_empty()) {
            out.push(json!({"kind":"replan","finding":finding.id,"title":finding.title,"severity":finding.severity,
                "proposal":finding.proposal,"since":finding.at,
                "summary":format!("Replan proposed for {}: {}", finding.id, finding.proposal)}));
        }
    }
    // U18: a member's permission request the lead has not answered for
    // `remind_owner_permission_minutes` goes to the owner, with the member, the
    // command and its age; the owner may answer it from here.
    if permission_minutes > 0 {
        for member in &team.members {
            let Some(pending) = &member.pending else { continue };
            let waited = (now - pending.at).max(0.0);
            if waited < permission_minutes as f64 * 60.0 {
                continue;
            }
            let role = team.role_of(&member.id);
            let asked = pending.card_question();
            // U56: the desktop finds the member's card by "member"; the summary
            // names it by id.
            let mut item = json!({"kind":"permission","member":role,"member_id":member.id,"since":pending.at,
                "minutes":(waited / 60.0).round() as u64,
                "summary":format!("{} waits for your answer: {}", team.name_of(&member.id),
                    if asked.is_empty() { "a permission request".to_owned() } else { asked }),
                "command":pending.answer_command(team, &member.id)});
            if pending.card["subagents"].is_array() {
                item["subagents"] = pending.card["subagents"].clone();
            }
            out.push(item);
        }
    }
    out.extend(phase_items);
    out.sort_by(|a, b| a["since"].as_f64().unwrap_or_default().total_cmp(&b["since"].as_f64().unwrap_or_default()));
    out
}

#[cfg(test)]
mod tests {
    use super::super::ops::{self, Actor};
    use super::*;
    use std::collections::BTreeSet;

    fn team() -> Team {
        let (mut team, _) = ops::create(
            "t",
            &json!({"name":"Checkout","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
        )
        .unwrap();
        ops::apply(
            &mut team,
            "plan",
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}}],
                "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"Tests","assignee":"dev","deps":[{"task":1}]}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut Vec::new(),
        )
        .unwrap();
        ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
        team
    }

    #[test]
    fn an_ended_turn_without_a_report_is_an_owner_visible_item() {
        let mut team = team();
        delivered(&mut team, 1);
        let ended = crate::state::now() - 10_000.0;
        {
            let attempt = &mut team.task_mut(1).unwrap().attempts[0];
            attempt.turn_ended_at = Some(ended);
            attempt.turn_reply = "Done: the report is the last message".into();
        }
        let list = items(&team);
        let found = list.iter().find(|item| item.kind == "no_report").unwrap();
        assert!(found.summary.contains("without a report"), "{}", found.summary);
        assert_eq!(found.detail, "Done: the report is the last message");
        // A report ends the item.
        team.task_mut(1).unwrap().attempts[0].report = Some(crate::state::teams::model::Report {
            text: "Done".into(),
            at: crate::state::now(),
            ..Default::default()
        });
        assert!(!items(&team).iter().any(|item| item.kind == "no_report"));
    }

    #[test]
    fn decided_proposals_are_listed_once_each_however_many_tasks() {
        // C9: neither once per task nor without bound: the five latest, listed
        // oldest first like every item.
        let mut team = team();
        for n in 0..25 {
            team.tasks.push(Task { id: 10 + n, title: "More".into(), assignee: "dev".into(), ..Default::default() });
        }
        for n in 0..8 {
            team.proposals.push(json!({"id":format!("p{n}"),"kind":"add-member","state":"accepted","at":1.0,"decided_at":100.0 + f64::from(n)}));
        }
        let decided: Vec<String> = items(&team).into_iter().filter(|item| item.kind == "proposal_decided").map(|item| item.key).collect();
        assert_eq!(decided, ["proposal:p3", "proposal:p4", "proposal:p5", "proposal:p6", "proposal:p7"]);
        // A team without tasks still lists them.
        team.tasks.clear();
        assert_eq!(items(&team).iter().filter(|item| item.kind == "proposal_decided").count(), 5);
    }

    #[test]
    fn a_decided_proposal_reaches_the_lead() {
        let mut team = team();
        team.proposals.push(json!({"id":"p1","kind":"add-member","state":"rejected","reason":"needs help","note":"not now","at":10.0}));
        let items = items(&team);
        let found = items.iter().find(|item| item.kind == "proposal_decided").unwrap();
        assert!(found.summary.contains("rejected") && found.summary.contains("not now"), "{}", found.summary);
        assert_eq!(found.detail, "not now");
    }

    #[test]
    fn deferred_findings_reach_the_owners_attention_list() {
        use crate::state::teams::model::Finding;
        let mut team = team();
        team.settings.remind_owner_minutes = 10;
        team.findings.push(Finding {
            id: "f1".into(),
            severity: "later".into(),
            title: "Polish".into(),
            scenario: "s".into(),
            state: "deferred".into(),
            at: 10.0,
            ..Default::default()
        });
        let items = attention(&team, 1000.0);
        let found = items.iter().find(|item| item["kind"] == "findings").unwrap();
        assert_eq!(found["count"], 1);
        assert!(found["summary"].as_str().unwrap_or_default().contains("1 findings deferred"));
    }

    #[test]
    fn a_failed_launch_is_a_member_error_item() {
        let mut team = team();
        {
            let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.session = None;
            dev.error = "/x belongs to another repository".into();
        }
        let items = items(&team);
        let found = items.iter().find(|item| item.kind == "member_error").unwrap();
        assert!(found.summary.contains("was not started"), "{}", found.summary);
        assert!(found.command.contains("relaunch"), "{}", found.command);
        assert_eq!(found.detail, "/x belongs to another repository");
    }

    /// U34: a DeepSeek member whose folder the sandbox cannot use is not
    /// started; the Lead's inbox carries the reason and the one-line fix.
    #[test]
    fn a_deepseek_sandbox_folder_error_names_the_fix_in_the_inbox() {
        let mut team = team();
        let error = "the DeepSeek sandbox needs full control of D:\\work\\team-abcdef12-dev (missing WRITE_OWNER); fix it for this folder with:\n  icacls \"D:\\work\\team-abcdef12-dev\" /grant *S-1-5-21-1-2-3-1001:(OI)(CI)F";
        {
            let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.agent = "dsh".into();
            dev.session = None;
            dev.error = error.into();
        }
        let items = items(&team);
        let found = items.iter().find(|item| item.kind == "member_error").unwrap();
        assert!(found.summary.contains("the DeepSeek sandbox needs full control of"), "{}", found.summary);
        assert!(found.detail.contains("/grant *S-1-5-21-1-2-3-1001:(OI)(CI)F"), "{}", found.detail);
        assert!(found.command.contains("relaunch"), "{}", found.command);
    }

    fn delivered(team: &mut Team, id: u32) {
        let task = team.task_mut(id).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: format!("00000000-0000-4000-8000-{id:012}"),
            member: "dev".into(),
            state: AttemptState::Delivered,
            sent_at: 10.0,
            delivered_at: Some(10.0),
            started_at: Some(11.0),
            ..Default::default()
        });
        task.status = TaskStatus::InProgress;
    }

    fn submitted(team: &mut Team, id: u32, at: f64) {
        delivered(team, id);
        let attempt = team.task_mut(id).unwrap().attempts.last_mut().unwrap();
        attempt.state = AttemptState::Reported;
        attempt.report = Some(Report { text: "Done and checked".into(), at, ..Default::default() });
        team.task_mut(id).unwrap().status = TaskStatus::Submitted;
    }

    #[test]
    fn the_inbox_lists_reports_questions_lead_tasks_and_stalled_work_oldest_first() {
        let mut team = team();
        // #1 waits for a decision since 100; #2 has a question since 200.
        submitted(&mut team, 1, 100.0);
        delivered(&mut team, 2);
        {
            let attempt = team.task_mut(2).unwrap().attempts.last_mut().unwrap();
            attempt.questions.push(Question {
                id: "q1".into(),
                text: "Which database?".into(),
                at: 200.0,
                ..Default::default()
            });
        }
        // The lead's own task, handed out without a message (C1).
        team.tasks.push(Task { id: 3, title: "Plan review".into(), assignee: "lead".into(), status: TaskStatus::InProgress, ..Default::default() });
        team.task_mut(3).unwrap().attempts.push(Attempt {
            n: 1,
            id: "L3".into(),
            member: "lead".into(),
            state: AttemptState::Delivered,
            sent_at: 150.0,
            delivered_at: Some(150.0),
            started_at: Some(150.0),
            ..Default::default()
        });
        // A stopped member's open task.
        team.tasks.push(Task { id: 4, title: "Docs".into(), assignee: "dev".into(), status: TaskStatus::InProgress, ..Default::default() });
        team.task_mut(4).unwrap().attempts.push(Attempt {
            n: 1,
            id: "S4".into(),
            member: "dev".into(),
            state: AttemptState::Delivered,
            sent_at: 50.0,
            delivered_at: Some(50.0),
            ..Default::default()
        });
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 60.0, live: false });
        let items = super::items(&team);
        let kinds: Vec<&str> = items.iter().map(|item| item.kind).collect();
        // Oldest first: the stopped task at 50, the report at 100, the lead's
        // task at 150, the question at 200.
        assert_eq!(kinds, ["stalled", "report", "lead_task", "question"]);
        assert_eq!(items[1].task, Some(1));
        assert_eq!(items[1].member, "dev (Backend)");
        assert!(items[1].summary.contains("Done and checked"));
        assert!(items[1].command.contains("hgs team accept --json"), "{}", items[1].command);
        assert!(items[3].command.contains("\"question\":\"q1\"") && items[3].command.contains("hgs team reply --json"));
        assert!(items[2].command.contains("\"accept\":true") && items[2].command.contains("hgs team report --json"));
        assert!(items[0].command.contains("hgs team retry --json"));
        // `hgs team inbox --json` shows the same fields.
        let value = items[1].value();
        assert_eq!((value["kind"].as_str(), value["task"].as_u64(), value["since"].as_f64()), (Some("report"), Some(1), Some(100.0)));
        // A paused team still has a lead to decide; a finished one does not.
        team.status = TeamStatus::Paused;
        assert!(!super::items(&team).is_empty());
        team.status = TeamStatus::Done;
        assert!(super::items(&team).is_empty());
    }

    #[test]
    fn an_unanswered_permission_reaches_the_owner_after_its_own_reminder() {
        // U18: a member permission the lead has not answered for
        // remind_owner_permission_minutes (10 by default) goes to the owner's
        // attention list, with the member, the command and its age; the oldest
        // one comes first, and it does not change the report reminder.
        let mut team = team();
        assert_eq!((team.settings.remind_owner_minutes, team.settings.remind_owner_permission_minutes), (60, 10));
        let now = 5_000.0;
        team.members.push(Member { id: "qa".into(), role: "Tester".into(), ..Default::default() });
        let card = json!({"questions":[{"id":"decision","header":"Bash","question":"Run the build?",
            "options":[{"id":"allow","label":"Allow"},{"id":"deny","label":"Deny"}]}]});
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().pending = Some(PendingQuestion {
            question_id: "q9".into(),
            question_hash: "h9".into(),
            approval: true,
            at: now - 12.0 * 60.0,
            card: card.clone(),
        });
        team.members.iter_mut().find(|member| member.id == "qa").unwrap().pending =
            Some(PendingQuestion { question_id: "q8".into(), question_hash: "h8".into(), approval: true, at: now - 30.0 * 60.0, card });
        // A report 20 minutes old is not yet the owner's; the permissions are.
        submitted(&mut team, 1, now - 20.0 * 60.0);
        let attention = super::attention(&team, now);
        assert!(!attention.iter().any(|item| item["kind"] == "report"), "{attention:?}");
        let permissions: Vec<&Value> = attention.iter().filter(|item| item["kind"] == "permission").collect();
        assert_eq!(permissions.len(), 2, "{attention:?}");
        // The longest wait is first.
        assert_eq!(permissions[0]["member"], "Tester");
        assert_eq!(permissions[0]["minutes"], 30);
        assert_eq!(permissions[1]["member"], "Backend");
        // U56: the desktop finds the card by role; the summary names the id.
        assert_eq!(permissions[1]["member_id"], "dev");
        assert!(permissions[1]["summary"].as_str().unwrap().starts_with("dev (Backend) waits"), "{}", permissions[1]);
        assert!(permissions[1]["summary"].as_str().unwrap().contains("waits for your answer"), "{}", permissions[1]);
        // The owner may answer it from the attention list.
        assert!(permissions[1]["command"].as_str().unwrap().contains("hgs team answer --json"), "{}", permissions[1]);
        // Before the reminder the permission is only the lead's inbox item.
        let early = super::attention(&team, now - 40.0 * 60.0);
        assert!(!early.iter().any(|item| item["kind"] == "permission"), "{early:?}");
        // 0 turns only the permission list off: the report appears at 60.
        team.settings.remind_owner_permission_minutes = 0;
        let late = super::attention(&team, now + 61.0 * 60.0);
        assert!(!late.iter().any(|item| item["kind"] == "permission"), "{late:?}");
        assert!(late.iter().any(|item| item["kind"] == "report"), "{late:?}");
    }

    #[test]
    fn a_permission_request_is_answerable_from_the_inbox() {
        let mut team = team();
        let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        member.pending = Some(PendingQuestion {
            question_id: "q9".into(),
            question_hash: "h9".into(),
            approval: true,
            at: 20.0,
            card: json!({"questions":[{"id":"decision","header":"Bash","question":"Run the build?",
                "options":[{"id":"allow","label":"Allow"},{"id":"deny","label":"Deny"}]}]}),
        });
        let items = super::items(&team);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!((item.kind, item.member.as_str(), item.task), ("permission", "dev (Backend)", None));
        assert!(item.summary.contains("Bash: Run the build?"), "{}", item.summary);
        // The option ids the answer needs are in the summary.
        assert!(item.summary.contains("[allow] Allow") && item.summary.contains("[deny] Deny"), "{}", item.summary);
        assert!(item.command.contains("\"question_id\":\"q9\"") && item.command.contains("\"expected_question_hash\":\"h9\""));
        assert!(item.command.contains("hgs team answer --json") && item.command.contains("\"question_id\":\"decision\""));
        // No stored request, no item.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().pending = None;
        assert!(super::items(&team).is_empty());
    }

    #[test]
    fn the_coalesced_message_names_each_new_item_with_its_age_and_command() {
        let mut team = team();
        submitted(&mut team, 1, 600.0);
        delivered(&mut team, 2);
        team.task_mut(2).unwrap().attempts.last_mut().unwrap().questions.push(Question {
            id: "q1".into(),
            text: "Which database?".into(),
            at: 900.0,
            ..Default::default()
        });
        let items = super::items(&team);
        let text = super::text(&team, &items, 1200.0);
        assert!(text.contains("from: Zerus \u{b7} inbox") && text.contains("2 item(s)"), "{text}");
        assert!(text.contains("1. report") && text.contains("2. question"), "{text}");
        assert!(text.contains("10 min") && text.contains("5 min"), "{text}");
        assert!(text.contains("Done and checked") && text.contains("Which database?"), "{text}");
        assert!(text.contains("hgs team accept --json") && text.contains("hgs team reply --json"), "{text}");
        // N7: the message says how to read the whole report.
        assert!(text.contains("Full report: task #1") && text.contains("hgs team status --json"), "{text}");
        assert!(text.contains("hgs team inbox --json"), "{text}");
        // A long list is summarized, never sent whole.
        let mut many = team.clone();
        for id in 10..40u32 {
            many.tasks.push(Task {
                id,
                title: format!("Task {id}"),
                assignee: "dev".into(),
                status: TaskStatus::Submitted,
                attempts: vec![Attempt {
                    n: 1,
                    id: format!("A{id}"),
                    member: "dev".into(),
                    state: AttemptState::Reported,
                    report: Some(Report { text: "Done".into(), at: id as f64, ..Default::default() }),
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
        let text = super::text(&many, &super::items(&many), 40.0);
        assert!(text.contains("[20 more item(s)") || text.contains("more item(s)"), "{text}");
    }

    #[test]
    fn a_report_that_replaces_a_captured_one_is_told_again() {
        // Owner, 2026-10-30: the member's own report replaced the reply Zerus
        // had captured, and the lead never heard of it: both had one key.
        let mut team = team();
        submitted(&mut team, 1, 100.0);
        team.task_mut(1).unwrap().attempts.last_mut().unwrap().report.as_mut().unwrap().captured = true;
        let first = super::items(&team);
        let mut told = Vec::new();
        let open: BTreeSet<String> = first.iter().map(|item| item.key.clone()).collect();
        mark_told(&mut told, &first, &open);
        let (attempt, member) = {
            let attempt = team.task(1).unwrap().attempts.last().unwrap();
            (attempt.id.clone(), attempt.member.clone())
        };
        let replaced = ops::apply(
            &mut team,
            "report",
            &serde_json::json!({"task":1,"attempt":attempt,"text":"Done: the real report."}),
            &Actor::Member(member),
            &BTreeSet::new(),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(replaced["status"], "submitted");
        let items = super::items(&team);
        let report = items.iter().find(|item| item.kind == "report").unwrap();
        assert!(!told.contains(&report.key), "the replacement must be a new item: {told:?} {}", report.key);
        assert!(report.summary.starts_with("replaces the earlier report"), "{}", report.summary);
    }

    #[test]
    fn the_told_keys_keep_still_open_items() {
        use std::collections::BTreeSet;
        let mut team = team();
        submitted(&mut team, 1, 100.0);
        delivered(&mut team, 2);
        team.task_mut(2).unwrap().attempts.last_mut().unwrap().questions.push(Question {
            id: "q1".into(),
            text: "Which database?".into(),
            at: 200.0,
            ..Default::default()
        });
        let items = super::items(&team);
        assert_eq!(items.len(), 2);
        let mut told = Vec::new();
        let open: BTreeSet<String> = items.iter().map(|item| item.key.clone()).collect();
        mark_told(&mut told, &items, &open);
        assert_eq!(told.len(), 2);
        // A decided report's key leaves; the still-open question's key stays,
        // so the question is never told twice (L2).
        team.task_mut(1).unwrap().status = TaskStatus::Accepted;
        let items = super::items(&team);
        let open: BTreeSet<String> = items.iter().map(|item| item.key.clone()).collect();
        mark_told(&mut told, &[], &open);
        assert_eq!(told, vec![items[0].key.clone()], "{told:?}");
        // A new item is added beside the still-open one.
        team.tasks.push(Task {
            id: 9,
            title: "New".into(),
            assignee: "dev".into(),
            status: TaskStatus::Submitted,
            attempts: vec![Attempt {
                n: 1,
                id: "A9".into(),
                member: "dev".into(),
                state: AttemptState::Reported,
                report: Some(Report { text: "New work".into(), at: 300.0, ..Default::default() }),
                ..Default::default()
            }],
            ..Default::default()
        });
        let items = super::items(&team);
        let open: BTreeSet<String> = items.iter().map(|item| item.key.clone()).collect();
        mark_told(&mut told, &items, &open);
        assert_eq!(told.len(), 2, "{told:?}");
        assert!(items.iter().all(|item| told.contains(&item.key)));
    }

    #[test]
    fn every_open_task_says_what_it_waits_for() {
        let mut team = team();
        // #1 waits for a free member; #2 waits for #1.
        let tasks = team.tasks.clone();
        assert_eq!(waits(&team, &tasks[0], true).unwrap(), json!({"for":"member","on":["dev (Backend)"],"unblock":"wait for dev (Backend)"}));
        assert_eq!(waits(&team, &tasks[1], false).unwrap(), json!({"for":"deps","on":[1],"unblock":"accept #1"}));
        // A soft dependency still pending names the start that frees it.
        team.task_mut(2).unwrap().deps[0].kind = DependencyKind::Soft;
        assert_eq!(waits(&team, &team.task(2).unwrap().clone(), false).unwrap()["unblock"], "start #1");
        team.task_mut(2).unwrap().deps[0].kind = DependencyKind::Hard;
        // A hard dependency that is in progress still waits for acceptance.
        delivered(&mut team, 1);
        assert_eq!(waits(&team, &team.task(2).unwrap().clone(), false).unwrap()["unblock"], "accept #1");
        // A report waits for the lead, a question too.
        submitted(&mut team, 1, 20.0);
        assert_eq!(waits(&team, &team.task(1).unwrap().clone(), true).unwrap()["for"], "lead");
        assert_eq!(waits(&team, &team.task(1).unwrap().clone(), true).unwrap()["unblock"], "accept #1");
        delivered(&mut team, 2);
        team.task_mut(2).unwrap().attempts.last_mut().unwrap().questions.push(Question {
            id: "q3".into(),
            text: "Which?".into(),
            at: 30.0,
            ..Default::default()
        });
        assert_eq!(waits(&team, &team.task(2).unwrap().clone(), false).unwrap(), json!({"for":"lead","on":["dev (Backend)"],"unblock":"answer q3"}));
        // A held task, a draft phase and a finished task.
        let mut held = team.task(1).unwrap().clone();
        held.status = TaskStatus::Pending;
        held.hold = Some(Hold { by: "Lead".into(), at: 1.0, reason: "Later".into() });
        assert_eq!(waits(&team, &held, false).unwrap()["for"], "hold");
        assert_eq!(waits(&team, &held, false).unwrap()["unblock"], "release #1");
        let draft = Task { id: 9, title: "Later".into(), phase: 2, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() };
        assert_eq!(waits(&team, &draft, false).unwrap()["for"], "owner");
        assert_eq!(waits(&team, &draft, false).unwrap()["unblock"], "approve phase 2");
        let mut done = team.task(1).unwrap().clone();
        done.status = TaskStatus::Accepted;
        assert!(waits(&team, &done, true).is_none());
    }

    #[test]
    fn a_barrier_phase_waits_for_the_earlier_tasks() {
        let mut team = team();
        team.phases.push(Phase { n: 2, title: "QA".into(), barrier: true, approval: Approval::Auto, state: PhaseState::Approved, ..Default::default() });
        team.tasks.push(Task { id: 3, title: "Verify".into(), phase: 2, assignee: "dev".into(), ..Default::default() });
        delivered(&mut team, 1);
        team.task_mut(2).unwrap().status = TaskStatus::Cancelled;
        let wait = waits(&team, &team.task(3).unwrap().clone(), false).unwrap();
        assert_eq!(wait["for"], "deps");
        assert_eq!(wait["on"], json!([1]));
        assert!(wait["unblock"].as_str().unwrap().starts_with("finish #"), "{wait}");
    }

    #[test]
    fn an_owner_attention_item_appears_after_the_owner_reminder_time() {
        let mut team = team();
        submitted(&mut team, 1, 1000.0);
        // Before the default 60 minutes: nothing for the owner.
        assert!(attention(&team, 1000.0 + 59.0 * 60.0).is_empty());
        let listed = attention(&team, 1000.0 + 60.0 * 60.0);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], json!({"kind":"report","task":1,"member":"Backend","member_id":"dev","since":1000.0,"minutes":60}));
        // 0 turns the owner's list off.
        team.settings.remind_owner_minutes = 0;
        assert!(attention(&team, 100_000.0).is_empty());
        // A decided report leaves it.
        team.settings.remind_owner_minutes = 60;
        team.task_mut(1).unwrap().status = TaskStatus::Accepted;
        assert!(attention(&team, 100_000.0).is_empty());
    }
}
