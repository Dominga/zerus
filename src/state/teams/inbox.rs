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

/// Task 134: items a member is blocked on make the lead's inbox message
/// urgent: its doorbell says URGENT and rings at once.
pub(crate) const URGENT_KINDS: [&str; 7] = ["question", "permission", "terminal_question", "crashed", "input_blocked", "member_stuck", "urgent_unread"];

pub(crate) fn urgency<'a>(items: impl IntoIterator<Item = &'a Item>) -> super::working::Urgency {
    // Task #460 (owner, 2026-11-09): an item whose command asks nothing of the
    // lead ("no action") is information, whatever its kind: it never makes the
    // digest urgent.
    if items.into_iter().any(|item| !informational(item) && URGENT_KINDS.contains(&item.kind)) {
        super::working::Urgency::Urgent
    } else {
        super::working::Urgency::Normal
    }
}

/// Task #460: an item the lead cannot act on is information for the digest:
/// its command starts with "no action". It never makes the digest urgent, and
/// its key is fixed while it is open, so it is told once.
pub(crate) fn informational(item: &Item) -> bool {
    item.command.starts_with("no action")
}

/// One thing on the lead's desk. `key` names it across ticks, so the coalesced
/// message never names it twice.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    /// `report`, `question`, `permission`, `lead_task`, `stalled`, `budget`,
    /// `interim_report`, `merge_waiting` or `task_waits` (spec 0: U45, U46, U48).
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
        // Task #289: the lead knows a notice from a decision. Task #29 (team
        // ede724c4, owner 2026-11-10): every item carries its key; every
        // informational one closes with `hgs team seen --json` and comes back
        // only when its content changes (its signature). A decision is
        // answered, never closed.
        let closes = closable(self);
        value["notice"] = json!(closes);
        value["key"] = json!(self.key);
        if closes {
            value["seen"] = json!(format!(
                "{{\"request_id\":\"UUID\",\"team\":\"TEAM\",\"items\":[\"{}\"]}} | hgs team seen --json",
                self.key
            ));
        }
        if !self.detail.is_empty() {
            value["detail"] = json!(self.detail);
        }
        if self.subagents.is_array() {
            value["subagents"] = self.subagents.clone();
        }
        value
    }
}

/// Task #289 (owner, 2026-11-08): notices are information for the lead, never a
/// decision waiting: a decided proposal, a cancelled dependency, a member that
/// does not read and the digests. They can be marked seen and return only when
/// their content changes.
const NOTICES: [&str; 4] = ["proposal_decided", "dependency_cancelled", "unread_messages", "lead_changes"];

pub(crate) fn notice(kind: &str) -> bool {
    NOTICES.contains(&kind)
}

/// Task #29 (team ede724c4; the lead's answer q11, 2026-11-10): information
/// the lead may close besides the notices: test results, playbook items,
/// records lore cannot write yet, accepted work waiting to land, refused
/// queued calls, idle notices and recovered attempts. Every item whose
/// command asks nothing ("no action", task #460) closes too.
const INFORMATIONAL: [&str; 7] = [
    "test_result",
    "playbook",
    "lore_missing",
    "waiting_to_land",
    "refused_queued_call",
    "idle_with_ready_work",
    "attempt_recovered",
];

/// Task #29: whether the lead or the owner may close this item. A decision (a
/// report, question, permission, a card or anything else that waits for an
/// accept, reject or answer) is not: it clears only by being decided.
pub(crate) fn closable(item: &Item) -> bool {
    notice(item.kind) || INFORMATIONAL.contains(&item.kind) || informational(item)
}

/// An item's change signature: the same key closed comes back when its words,
/// member or task change. Task #29 (team ede724c4): it leaves out the clock.
/// Playbook, member_error, drain_held and build_folders items are dated
/// now() and waiting_to_land now minus its age, and summaries count minutes
/// ("idle 12 min", "#1 (26 min)"), so a signature with them changed on every
/// read and a closed item came straight back.
pub(crate) fn signature(item: &Item) -> String {
    use sha2::{Digest, Sha256};
    let text = format!("{}\n{}\n{}\n{}\n{}\n{:?}", item.kind, item.key, steady(&item.summary), steady(&item.detail), item.member, item.task);
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Task #29: a text with its elapsed times blanked ("12 min" reads "N min"),
/// so an item that only grew older keeps its signature.
fn steady(text: &str) -> String {
    static ELAPSED: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    ELAPSED
        .get_or_init(|| regex::Regex::new(r"\b\d+(?:\.\d+)?\s?(min|minutes?|s|sec|seconds?|h|hours?)\b").expect("a valid pattern"))
        .replace_all(text, "N $1")
        .into_owned()
}

/// Whether the lead or the owner closed this item with exactly this content.
fn is_seen(team: &Team, item: &Item) -> bool {
    let signature = signature(item);
    team.seen_notices.iter().any(|seen| seen.key == item.key && seen.signature == signature)
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
/// Task #29 (team ede724c4): how long a test result stays in the inbox when
/// nothing settles it sooner.
pub(crate) const TEST_RESULT_HOURS: f64 = 12.0;

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
    // Task #289, task #29: an informational item the lead or the owner
    // closed stays out until it changes.
    items.retain(|item| !closable(item) || !is_seen(team, item));
    items
}

fn reviewer_of(team: &Team, item: &Item) -> String {
    match (item.kind, item.task.and_then(|id| team.task(id))) {
        // #538 N3: a report a sub-lead may not accept is the lead's.
        ("report", Some(task)) if super::pipeline::lead_accepts_only(task) => team.lead.clone(),
        // The lead's own task has no reviewer: it stays the lead's.
        // Task #362: a member's missing report is its reviewer's, so the lead
        // or the sub-lead it reports to can use the member's last reply.
        ("report" | "question" | "stalled" | "interim_report" | "no_report", Some(task)) if !task.assignee.is_empty() && task.assignee != team.lead => {
            team.reviewer_of(&task.assignee)
        }
        _ => team.lead.clone(),
    }
}

/// Every item, seen notices included: `hgs team seen` marks what it finds here,
/// and `items` hides what is already seen (task #289).
pub(crate) fn all_items(team: &Team) -> Vec<Item> {
    let active = matches!(team.status, TeamStatus::Running | TeamStatus::Paused);
    // N5: a finished team has no open work, but the owner's decision on the
    // lead's proposal is still the lead's to read (spec 4).
    let finished = matches!(team.status, TeamStatus::Done | TeamStatus::Stopped);
    if !active && !finished && team.iron_rules_error.is_empty() {
        return Vec::new();
    }
    let mut items: Vec<Item> = Vec::new();
    if !team.iron_rules_error.is_empty() {
        items.push(Item { kind:"stalled", key:"iron_rules".into(), task:None, member:team.lead.clone(), since:team.iron_rules_error_at.unwrap_or_else(crate::state::now),
            summary:"Iron rules block team delivery; the owner must select its project".into(),
            detail:team.iron_rules_error.clone(),
            command:format!("Owner: {{\"team\":\"{}\",\"project_id\":\"STABLE_ID\",\"by\":\"human\",\"request_id\":\"UUID\"}} | hgs team settings --json",team.id),
            subagents:Value::Null });
    }

    // Task #406 (owner, 2026-11-09): one alarm when ready tasks wait while a
    // phase of the worker's tick has held it for five minutes, naming it.
    if team.status == TeamStatus::Running {
        if let Some(item) = worker_stall(team, crate::state::now()) {
            items.push(item);
        }
    }

    // A queued merge that failed and was rolled back waits for the lead to fix
    // the runner/tests map or to retry it.
    if active {
        for task in &team.tasks {
            let Some(entry) = task.merge.as_ref().filter(|entry| {
                (entry.state == "queued" || entry.state == "blocked")
                    && !entry.note.is_empty()
                    && entry.wait_reason != super::model::MergeWaitReason::Busy
            }) else { continue };
            // Task #370: a blocked landing offers the cherry-pick of the task's own commit.
            let next = if entry.blocked {
                "Land the task's own commit as a cherry-pick: the merge would carry a rejected attempt's commit.".to_owned()
            } else {
                match entry.wait_reason {
                super::model::MergeWaitReason::Rollback => "The merge could not be rolled back; revert it, then retry or reject.",
                super::model::MergeWaitReason::Runners => "Fix the runner or machine config, then accept or retry the merge.",
                super::model::MergeWaitReason::Selection => "Fix the tests map, then accept or retry the merge.",
                super::model::MergeWaitReason::Ancestry => "Check the lead branch and git access, then accept or retry the merge.",
                _ => "Fix the cause, then accept or retry the merge.",
                }.to_owned()
            };
            items.push(Item {
                subagents: Value::Null,
                kind: "merge_waiting",
                key: format!("merge_waiting:{}:{}", task.id, entry.commit),
                task: Some(task.id),
                member: team.name_of(&entry.author),
                since: entry.at,
                summary: summary(&format!("#{} merge waits: {}", task.id, entry.note)),
                command: format!(
                    "{} | hgs team land --json",
                    json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"cherry_pick":entry.blocked})
                ),
                detail: format!("{} {next}", entry.note),
            });
        }
    }
    // Task #267: accepted work that is not on the integration line waits for
    // the lead: the queue, its resolve task, or a landing nobody made.
    if active && !team.settings.integration_folder.trim().is_empty() {
        let landing = super::merge_queue::landing(team);
        for entry in landing["unlanded"].as_array().into_iter().flatten() {
            let task = entry["task"].as_u64().unwrap_or_default() as u32;
            let why = entry["why"].as_str().unwrap_or_default();
            let resolve = entry["resolve"].as_u64();
            // Task #370: a blocked landing offers a cherry-pick of the task's
            // own commit; the merge would carry a rejected attempt's commit.
            let blocked = entry["blocked"].as_bool() == Some(true);
            let next = if blocked {
                "The accepted commit carries a commit of a rejected attempt, so the queue refuses to merge its branch. Land the task's own commit as a cherry-pick.".to_owned()
            } else {
                match resolve {
                    Some(resolve) => format!("The resolve task #{resolve} resolves the conflict; the landing retries once it is decided."),
                    None => "Land it again, or resolve it by hand and accept again.".to_owned(),
                }
            };
            items.push(Item {
                subagents: Value::Null,
                kind: "unlanded",
                key: format!("unlanded:{task}"),
                task: Some(task),
                member: team.name_of(team.task(task).map(|task| task.assignee.as_str()).unwrap_or_default()),
                since: team.task(task).and_then(|task| task.merge.as_ref()).map_or(team.updated, |entry| entry.at),
                summary: summary(&format!("#{task} is accepted but not on the integration line: {why}")),
                command: format!("{} | hgs team land --json", json!({"request_id":PLACEHOLDER,"team":team.id,"task":task,"cherry_pick":blocked})),
                detail: format!("Accepted work lands on {} ({}) at acceptance through the queue. {next}", landing["branch"].as_str().unwrap_or_default(), landing["head"].as_str().unwrap_or_default()),
            });
        }
    }
    // Task #464 (owner, 2026-11-09): accepted work waits for a hand landing and
    // no merger holds a landing task: one line for the lead, with the ready
    // command. Information, never a decision; the task's own report is decided.
    // Only a hand landing (no queue entry) reaches here: the queue lands its own.
    if active && super::merge_queue::landing_task(team).is_none() {
        let hand: Vec<Value> = super::merge_queue::waiting_to_land(team, crate::state::now())
            .and_then(|waiting| waiting["tasks"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|task| task["state"] == "not_queued")
            .collect();
        if let Some(first) = hand.first().cloned() {
            let task = first["task"].as_u64().unwrap_or_default() as u32;
            let age = first["age"].as_f64().unwrap_or_default();
            let since = crate::state::now() - age * 60.0;
            let named: Vec<String> = hand.iter().take(8).map(|task| format!("#{} ({} min)", task["task"], task["age"])).collect();
            let more = hand.len().saturating_sub(named.len());
            let mut line = format!("accepted, waiting to land: {}", named.join(", "));
            if more > 0 {
                line.push_str(&format!(" and {more} more"));
            }
            items.push(Item {
                subagents: Value::Null,
                kind: "waiting_to_land",
                key: "waiting_to_land".into(),
                task: None,
                member: "Zerus".into(),
                since,
                summary: summary(&line),
                command: format!(
                    "{{\"request_id\":\"UUID\",\"team\":\"{}\",\"task\":{task},\"commit\":\"<the line's merge commit>\"}} | hgs team landed --json  (merge the commits into the line by hand first; Zerus marks each landed once it is on the line)",
                    team.id
                ),
                detail: String::new(),
            });
        }
    }
    // A rework keeps its member indefinitely. Show the wait even when that
    // member has an approval or usage limit rather than a stored task wait.
    for task in team.tasks.iter().filter(|task| active && task.rework() && task.status == TaskStatus::Pending
        && task.hold.is_none() && task.waits.as_ref().is_none_or(|wait| wait.waits_for == "member")) {
        let unavailable = team.member(&task.assignee).is_some_and(|member| {
            matches!(member.state, MemberState::Removed | MemberState::Paused) || member.session.is_none()
                || member.limited.is_some() || member.stuck.is_some() || member.seen.is_some_and(|seen| !seen.live)
                || matches!(member.seen_phase.as_str(), "approval" | "question" | "input" | "error" | "working" | "tool" | "compacting")
                || matches!(member.seen_activity.as_str(), "busy" | "attention" | "background")
        });
        if !unavailable { continue; }
        let since = task.rework_since.unwrap();
        let minutes = ((crate::state::now() - since) / 60.0).max(0.0).floor() as u64;
        items.push(Item {
            subagents: Value::Null,
            kind: "rework_waits",
            key: format!("rework_waits:{}:{since}", task.id),
            task: Some(task.id),
            member: team.name_of(&task.assignee),
            since,
            summary: format!("#{} waits for its member, {minutes} min", task.id),
            command: format!("{} | hgs team reassign --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"member":"MEMBER"})),
            detail: format!("{} keeps this rework without a time limit. Wait for that member or explicitly reassign the task. {}",
                team.name_of(&task.assignee), task.waits.as_ref().map(|wait| wait.reason.as_str()).unwrap_or_default()).trim_end().to_owned(),
        });
    }
    // Task #220: a tool a team's run found missing waits for its lead, not the
    // owner: the lead denies it with a reason, or asks the owner on a card.
    if active {
        // A card the lead already asked for answers it; the offer is not the
        // lead's again until the owner's answer says so.
        let asked: Vec<String> = team
            .proposals
            .iter()
            .filter(|proposal| proposal["state"] == "open" && proposal["kind"] == "install-tool")
            .filter_map(|proposal| proposal["change"]["offer"].as_str().map(str::to_owned))
            .collect();
        for offer in crate::test_discipline::tools::offers_of(&team.id).into_iter().filter(|offer| !asked.contains(&offer.id)) {
            let who = if offer.member.is_empty() { team.name.clone() } else { team.name_of(&offer.member) };
            let needs = format!("{who}'s run needs {}", offer.tool);
            items.push(Item {
                subagents: Value::Null,
                kind: "tool_missing",
                key: format!("tool_missing:{}", offer.id),
                task: offer.task,
                member: who.clone(),
                since: offer.since,
                summary: summary(&format!("{needs} in {}: {}", offer.project_name, offer.why)),
                command: format!(
                    "{} | hgs team decide-tool --json",
                    json!({"request_id":PLACEHOLDER,"team":team.id,"offer":offer.id,"allow":false,
                        "reason":format!("We work without {} for now", offer.tool)})
                ),
                detail: format!(
                    "Project: {} ({}).\nThe install command: {}\nAsk the owner to install it: {} | hgs team propose --json",
                    offer.project_name,
                    offer.folder,
                    offer.command.join(" "),
                    json!({"request_id":PLACEHOLDER,"team":team.id,"kind":"install-tool","offer":offer.id,
                        "reason":format!("{needs}: {}", offer.why)})
                ),
            });
        }
    }
    // Task #364: one notice when the team's build folders pass the owner's
    // limit; it stays while they do and is never repeated per folder.
    if active {
        if let Some((bytes, limit)) = crate::state::build_dirs::team_over(&team.id) {
            items.push(Item {
                subagents: Value::Null,
                kind: "build_folders",
                key: format!("build_folders:{}", team.id),
                task: None,
                member: String::new(),
                since: crate::state::now(),
                summary: summary(&format!(
                    "The team's build folders hold {} (over your {} limit); Clean removes the finished tasks' targets",
                    crate::state::build_dirs::gigabytes(bytes),
                    crate::state::build_dirs::gigabytes(limit)
                )),
                command: format!("{} | hgs team disk --json", json!({"action":"list","by":"human"})),
                detail: "Settings > Build folders shows each team's folders; Clean… removes the targets of its finished tasks after your confirmation.".into(),
            });
        }
    }
    // Task 259 (owner, 2026-11-08): no idle member while ready work waits.
    // Zerus moves a ready task with free outputs to an idle member itself; the
    // one case it leaves to the lead is a task an explicit assignment keeps
    // with its own member (pinned, keep_assignees or a rework). An idle member
    // that may take such a task is named here with the task, so the lead can
    // hand it over instead of leaving both waiting.
    // Task 263 (owner, 2026-11-08): a member whose own ready work another open
    // task's outputs hold is idle too; that item names the holder, so an
    // hour-long silent hold like 198 against 218-226 is visible and freeable.
    if active {
        let ready = plan::dispatch_order(team, plan::ready(team));
        let outputs = InProgressOutputs::new(team);
        let staged = super::stages::busy(team);
        for member in team.members.iter().filter(|member| member.id != team.lead && member.state != MemberState::Removed) {
            let held = plan::held_ready(team, &member.id);
            let idle = member.starved_since.is_some()
                || (!held.is_empty() && member.seen_activity == "idle" && member.session.is_some());
            if !idle {
                continue;
            }
            // Task #24 (team ede724c4): the notice is current when it is sent.
            // `starved_since` ends only on a later tick, so a member handed a
            // task (or a stage) in this tick still carries it: it is not idle.
            if has_open_work(team, &member.id) || staged.contains(&member.id.as_str()) {
                continue;
            }
            let since = member
                .starved_since
                .or(member.seen_event_at)
                .or_else(|| member.seen.as_ref().map(|seen| seen.at))
                .unwrap_or_else(crate::state::now);
            let role = team.role_of(&member.id);
            let mut best: Option<(u8, usize, &Task)> = None;
            for (index, id) in ready.iter().enumerate() {
                let Some(task) = team.task(*id) else { continue };
                if task.assignee.is_empty() || task.assignee == member.id || task.assignee == team.lead { continue; }
                if !team.requires_assignee(task) {
                    // Zerus hands this one over on its own next tick.
                    continue;
                }
                if !plan::output_holders(team, task).is_empty() || outputs.blocks_for(&member.id, task) { continue; }
                if !super::rights::missing(task, member).is_empty() { continue; }
                let found = team.role_of(&task.assignee);
                // Task 259: a compatible role - the same one, or one doing the
                // same kind of work - is what lets a member take the task.
                if found != role && !related_roles(&role, &found) { continue; }
                let rank = if found == role { 0 } else { 1 };
                if best.is_none_or(|(best_rank, best_index, _)| (rank, index) < (best_rank, best_index)) {
                    best = Some((rank, index, task));
                }
            }
            if let Some((_, _, task)) = best {
                let why = if task.rework() { "a rework stays with the member that got it" } else { "an explicit assignment (pinned or keep_assignees) keeps it" };
                let minutes = ((crate::state::now() - since).max(0.0) / 60.0).round() as u64;
                items.push(Item {
                    subagents: Value::Null,
                    kind: "idle_with_ready_work",
                    key: format!("idle_with_ready_work:{}:{}", member.id, task.id),
                    task: Some(task.id),
                    member: team.name_of(&member.id),
                    since,
                    summary: summary(&format!("#{} {} is ready and {} is idle {minutes} min: it may take it", task.id, task.title, team.name_of(&member.id))),
                    command: format!("{} | hgs team reassign --json",
                        json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"member":member.id})),
                    detail: format!("{} has no ready task of its own while #{} waits for {}. {}; Zerus does not move it by itself. Its outputs are free for {} ({role}) and its launch can do it. Reassign it to hand the task over, or leave it with its member.",
                        team.name_of(&member.id), task.id, team.name_of(&task.assignee), why, team.name_of(&member.id)).trim_end().to_owned(),
                });
                continue;
            }
            // Task 263: the member's own ready work is held by another open
            // task's outputs; name the holder and the action that frees it.
            let Some((task_id, holder)) = held.first().copied() else { continue };
            let Some(task) = team.task(task_id) else { continue };
            let note = plan::holder_note(team, holder);
            let waiting = plan::holder_waits_for_member(team, holder);
            let minutes = ((crate::state::now() - since).max(0.0) / 60.0).round() as u64;
            let command = if waiting {
                format!("{} | hgs team hold --json", json!({"request_id":PLACEHOLDER,"team":team.id,"task":holder,
                    "reason":format!("#{holder} waits for its member and holds #{}'s outputs for {}", task_id, team.name_of(&member.id))}))
            } else {
                format!("{} | hgs team look --json",
                    json!({"team":team.id,"member":team.task(holder).map(|task| task.assignee.clone()).unwrap_or_default()}))
            };
            items.push(Item {
                subagents: Value::Null,
                kind: "idle_with_ready_work",
                key: format!("idle_with_ready_work:{}:{}:outputs", member.id, task_id),
                task: Some(task_id),
                member: team.name_of(&member.id),
                since,
                summary: summary(&format!("#{} {} is ready for {} but held by #{}'s outputs", task_id, task.title, team.name_of(&member.id), holder)),
                command,
                detail: format!("#{} waits for #{}'s output; {}. {} is idle {minutes} min. Hold or reassign #{} to let #{} start, or wait for it.",
                    task_id, holder, note, team.name_of(&member.id), holder, task_id).trim_end().to_owned(),
            });
        }
    }
    // Task 277 (owner, 2026-11-08): an urgent message unread past the alarm
    // bound is an alarm: the member has not acted on it and Zerus did not get
    // its new turn through. Ordinary unread notices stay folded into the one
    // "does not read" item; only an urgent one is shouted.
    if active && team.settings.urgent_alarm_minutes > 0 {
        let now = crate::state::now();
        let bound = team.settings.urgent_alarm_minutes as f64 * 60.0;
        for member in team.members.iter().filter(|member| member.id != team.lead && member.state != MemberState::Removed) {
            let Some(unread) = member.unread.as_ref().filter(|unread| unread.urgent > 0) else { continue };
            let Some(at) = unread.urgent_at else { continue };
            let waited = (now - at).max(0.0);
            if waited < bound {
                continue;
            }
            let minutes = (waited / 60.0).round() as u64;
            let who = if unread.urgent_by == "owner" { "you" } else { "the lead" };
            let why = match member.urgent.as_ref().map(|urgent| urgent.state.as_str()).unwrap_or_default() {
                "requested" => "its turn was interrupted and the message waits for the stop",
                "sent" => "it was delivered in a new turn but not acted on",
                "failed" => "Zerus could not interrupt its turn",
                _ => "it waits for the member's turn to end",
            };
            items.push(Item {
                subagents: Value::Null,
                kind: "urgent_unread",
                key: format!("urgent_unread:{}:{}", member.id, unread.urgent_oldest),
                task: None,
                member: team.name_of(&member.id),
                since: at,
                summary: summary(&format!("{} has an urgent message from {who} unread {minutes} min: {why}", team.name_of(&member.id))),
                command: format!("{} | hgs team look --json", json!({"team":team.id,"member":member.id})),
                detail: format!("{} unread urgent message(s); {why}. The message is in the member's team inbox. Open its look to see the session, or stop it natively.", unread.urgent),
            });
        }
    }
    // #752: two or more members in one folder build each other's half-done
    // edits; the lead proposes the build broker once (a switch card), and a
    // card the owner denied is not proposed again.
    let proposed = team.proposals.iter().any(|proposal| proposal["kind"] == "switch-setting" && proposal["change"]["setting"] == "build_broker");
    if active && !team.settings.build_broker && !proposed {
        // Feedback 19: only a folder with something to build gets the offer.
        if let Some((folder, members)) =
            super::broker::shared_folders(team).into_iter().find(|(folder, _)| super::broker::builds(std::path::Path::new(folder)))
        {
            let names: Vec<String> = members.iter().map(|id| team.name_of(id)).collect();
            items.push(Item {
                subagents: Value::Null,
                kind: "build_broker",
                key: "build_broker".into(),
                task: None,
                member: names.join(", "),
                since: 0.0,
                summary: summary(&format!("{} work in one folder: propose the build broker", names.join(", "))),
                command: format!(
                    "{} | hgs team propose --json",
                    json!({"request_id":PLACEHOLDER,"team":team.id,"kind":"switch-setting","setting":"build_broker","value":true,
                        "reason":format!("{} share {folder}; their builds compile each other's half-done edits", names.join(", "))})
                ),
                detail: broker_detail(&super::broker::config(std::path::Path::new(&folder))),
            });
        }
    }
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
    // Task 134: a member that does not read its team messages (two doorbells
    // and the bound again, or an urgent one its turn kept back) is one item for
    // the lead, with the member's state; the messages stay unread for it.
    for member in team.members.iter().filter(|member| member.id != team.lead).filter(|_| active) {
        let Some(why) = super::doorbell::unread_stalled(member, crate::state::now(), super::working::urgent_bound(team)) else { continue };
        let unread = member.unread.as_ref().expect("a stall has unread messages");
        let role = team.name_of(&member.id);
        let state = serde_json::to_value(member.state).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_default();
        let session = member.session.as_ref().map(|session| session.name.clone()).unwrap_or_else(|| "no session".into());
        items.push(Item {
            subagents: Value::Null,
            kind: "unread_messages",
            key: format!("unread_messages:{}:{}", member.id, unread.oldest),
            task: None,
            member: role.clone(),
            since: unread.oldest_at,
            summary: summary(&format!("{role} does not read: {why}")),
            command: format!("{} | hgs team look --json", json!({"team":team.id,"member":member.id})),
            detail: format!(
                "Member state: {}, session {session}. Unread: {} ({} urgent). The member reads them with hgs team inbox;                  Zerus never marks them read for it. Look at the member, or stop it natively so the messages reach it at the turn end.",
                if state.is_empty() { "unknown".to_owned() } else { state },
                unread.count,
                unread.urgent
            ),
        });
    }
    // Spec 0: a task that waits for a step Zerus could not take (U45 model,
    // U46 context, U48 branch) is one item for the lead, with the reason and
    // the retry that starts the step again. A context action that still runs
    // waits for nobody.
    for task in team.tasks.iter().filter(|_| active).filter(|task| task.status == TaskStatus::Pending) {
        let Some(wait) = task.waits.as_ref().filter(|wait| !wait.running()) else { continue };
        if task.rework() && wait.waits_for == "member" { continue; }
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
            let member_stuck = found.is_some_and(|member| member.stuck.is_some());
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
                (None, _) if !live => (relaunch, format!("{} Relaunch its session in a fresh conversation with a handoff brief to free it.", wait.reason)),
                (None, _) if team.settings.lead_restarts_stuck && member_stuck => (
                    relaunch,
                    format!("{} Relaunch it: a fresh conversation with a handoff brief continues the task.", wait.reason),
                ),
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
        // Task 112: a paused team hands nothing out; the lead learns that is why.
        let paused = if team.status == TeamStatus::Paused { " The team is paused, so nothing is handed out until the owner resumes it." } else { "" };
        items.push(Item {
            subagents: Value::Null,
            kind: "task_waits",
            key: format!("task_waits:{}:{}", task.id, wait.at),
            task: Some(task.id),
            member,
            since: wait.at,
            summary: summary(&format!("#{} {} waits: {}{paused}", task.id, task.title, wait.reason)),
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
            command: format!("{} | hgs team status --json", json!({"team":team.id,"task":waiting[0]})),
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
        // #728: the minutes since the member's last progress, never the time
        // since the mark; under the stuck bound there is no item at all.
        let quiet = crate::state::now() - stuck.since.unwrap_or(stuck.at);
        if quiet < super::ops::stuck_minutes(team) * 60.0 {
            continue;
        }
        let minutes = (quiet / 60.0).round() as u64;
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
                format!(
                    "You may relaunch it: hgs team relaunch {} gives it a fresh conversation with a handoff brief that continues the task.",
                    member.id
                ),
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
    // Task #24 (team ede724c4): a task's results are one item with its
    // counts, the newest suite results and its records, not one per run; it
    // is told again only when a new result arrives (the key names the newest).
    let mut by_task: Vec<(u32, Vec<&TestNotice>)> = Vec::new();
    for notice in team.test_notices.iter().rev() {
        let Some(task) = notice.task else { continue };
        match by_task.iter_mut().find(|(id, _)| *id == task) {
            Some((_, list)) => list.push(notice),
            None => by_task.push((task, vec![notice])),
        }
    }
    // Task #29 (team ede724c4): results are information. A task's results
    // clear once it is accepted and landed, and any result after
    // TEST_RESULT_HOURS.
    let fresh_after = crate::state::now() - TEST_RESULT_HOURS * 3600.0;
    by_task.retain(|(task, list)| list[0].at >= fresh_after && !team.task(*task).is_some_and(|task| task.status == TaskStatus::Accepted && task.landed.is_some()));
    for (task, list) in by_task.iter().take(20) {
        items.push(test_results_item(team, *task, list));
    }
    // U62 (#717): a result for no task is one card linking its record - suite,
    // status, time, the submitter - never the log, which stays in the database.
    for notice in team.test_notices.iter().rev().filter(|notice| notice.task.is_none() && notice.at >= fresh_after).take(20) {
        items.push(Item {
            subagents: Value::Null,
            kind: "test_result",
            key: format!("test_result:{}", notice.record),
            task: None,
            member: notice.asked_by.clone(),
            since: notice.at,
            summary: summary(&{
                // #718: what the result is beyond its status, named on the card.
                let flags = super::test_results::flag_text(&notice.flags, notice.recorded_seconds, notice.duration_ms);
                let base = if notice.reused_from.is_empty() {
                    format!("Test {} {} in {} s (record {})", notice.suite, notice.status, notice.duration_ms / 1000, notice.record)
                } else {
                    format!("Test {} reused from record {} (record {})", notice.suite, notice.reused_from, notice.record)
                };
                if flags.is_empty() { base } else { format!("{base}: {flags}") }
            }),
            command: format!(
                "{} | hgs team test --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"action":"results","record":notice.record})
            ),
            detail: format!(
                "Asked by {}. The log and excerpt stay in record {}; read them with hgs team test --json and action results, record {}, not from this card.",
                if notice.asked_by.is_empty() { "an unknown caller" } else { notice.asked_by.as_str() },
                notice.record,
                notice.record
            ),
        });
    }
    // #759: a pending task a cancelled dependency no longer holds, so the
    // lead can re-plan it before it starts without that dependency's work.
    for task in team.tasks.iter().filter(|task| task.status == TaskStatus::Pending) {
        let released = plan::cancelled_deps(team, task);
        if released.is_empty() {
            continue;
        }
        let names: Vec<String> = released.iter().map(|id| format!("#{id}")).collect();
        let since = released
            .iter()
            .filter_map(|id| team.task(*id))
            .flat_map(|dep| dep.attempts.iter().filter_map(|attempt| attempt.decided_at))
            .fold(team.created, f64::max);
        items.push(Item {
            subagents: Value::Null,
            kind: "dependency_cancelled",
            key: format!("dependency_cancelled:{}:{}", task.id, names.join(",")),
            task: Some(task.id),
            member: String::new(),
            since,
            summary: summary(&format!(
                "#{} {} no longer waits on cancelled {}: it starts once its other dependencies are done",
                task.id,
                task.title,
                names.join(", ")
            )),
            command: format!("{} | hgs team keep --json", json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id})),
            detail: format!(
                "Keep it if it does not need the cancelled work: the keep command above records that and clears this notice. Otherwise re-plan it: hold it with {} | hgs team hold --json, or cancel it too with {} | hgs team cancel --json.",
                json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id}),
                json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"reason":"..."})
            ),
        });
    }
    // #718: flaky suites to file a task for, suites over their budgets, the
    // quarantine and the proposal of a full run.
    for alert in team.test_alerts.iter().filter(|alert| !alert.dismissed) {
        items.push(Item {
            subagents: Value::Null,
            kind: "test_alert",
            key: format!("test_alert:{}:{}:{}", alert.kind, alert.suite, alert.at),
            task: None,
            member: String::new(),
            since: alert.at,
            summary: summary(&alert.detail),
            command: format!(
                "{} | hgs team test --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"action":"dismiss","kind":alert.kind,"suite":alert.suite})
            ),
            detail: if alert.kind == "flaky" {
                "File a task to fix it (hgs team extend or add-to-phase), then dismiss this item; the next one counts newer episodes only.".into()
            } else {
                "Split the suite or change its budget in .zerus/tests.toml, then dismiss this item.".into()
            },
        });
    }
    for quarantine in team.test_quarantine.iter() {
        items.push(Item {
            subagents: Value::Null,
            kind: "test_quarantine",
            key: format!("test_quarantine:{}:{}", quarantine.suite, quarantine.until),
            task: None,
            member: String::new(),
            since: quarantine.since,
            summary: summary(&format!(
                "{} is quarantined as flaky ({} episode(s)) until {}",
                quarantine.suite,
                quarantine.episodes,
                chrono::DateTime::from_timestamp(quarantine.until as i64, 0).map(|time| time.format("%Y-%m-%d").to_string()).unwrap_or_default()
            )),
            command: format!("{} | hgs team test --json", json!({"request_id":PLACEHOLDER,"team":team.id,"action":"results"})),
            detail: "Its failures still show; the quarantine expires by itself.".into(),
        });
    }
    if let Some(proposal) = &team.test_full_proposal {
        items.push(Item {
            subagents: Value::Null,
            kind: "test_full_run",
            key: format!("test_full_run:{}", proposal.at),
            task: None,
            member: String::new(),
            since: proposal.at,
            summary: summary(&full_run_text(proposal)),
            command: format!("{} | hgs team test --json", json!({"request_id":PLACEHOLDER,"team":team.id,"action":"full-done"})),
            detail: "The full run is the owner's call: ask the owner with this time, then record it with full-done once it ran.".into(),
        });
    }
    // Task 98: a member whose composer holds a draft or dialog Zerus did not
    // leave refuses every hand-out; after a few refusals the lead hears it once,
    // with the screen's last lines and what to do.
    for member in team.members.iter().filter(|member| member.input_block.as_ref().is_some_and(|block| block.raised)).filter(|_| active) {
        let block = member.input_block.as_ref().unwrap();
        let name = team.name_of(&member.id);
        let terminal = member.session.as_ref().map(|session| session.name.clone()).unwrap_or_default();
        items.push(Item {
            subagents: Value::Null,
            kind: "input_blocked",
            key: format!("input_blocked:{}", member.id),
            task: None,
            member: name.clone(),
            since: block.since,
            summary: summary(&format!("{name}'s terminal is busy: {}", block.reason)),
            command: String::new(),
            detail: input_block_action(&terminal, &block.screen),
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
        .filter(|proposal| matches!(proposal["state"].as_str(), Some("accepted") | Some("rejected") | Some("answered") | Some("withdrawn")))
        .collect();
    let decided_at = |proposal: &Value| proposal["decided_at"].as_f64().or(proposal["at"].as_f64()).unwrap_or_default();
    // A stable order: proposals decided in one go keep their places.
    decided.sort_by(|a, b| decided_at(b).total_cmp(&decided_at(a)).then_with(|| a["id"].as_str().cmp(&b["id"].as_str())));
    for proposal in decided.into_iter().take(DECIDED_PROPOSALS) {
        let id = proposal["id"].as_str().unwrap_or_default().to_owned();
        let state = proposal["state"].as_str().unwrap_or_default().to_owned();
        let kind = proposal["kind"].as_str().unwrap_or_default();
        let note = proposal["note"].as_str().unwrap_or_default();
        let why = if note.is_empty() { String::new() } else { format!(": {note}") };
        // #740: the owner's answer to the lead's question, with the question.
        let text = if state == "answered" {
            let question = proposal["change"]["question"].as_str().unwrap_or_default();
            format!("The owner answered your question {id} ({question}): {note}")
        } else if state == "withdrawn" {
            // Task 111: never shown to the owner; the lead learns why, once,
            // and proposes again against the plan as it is now.
            format!("Your {kind} proposal {id} was withdrawn before the owner saw it: it can no longer apply ({note}). Propose again against the plan as it is now")
        } else {
            format!("The owner {state} your {kind} proposal {id}{why}")
        };
        items.push(Item {
            subagents: Value::Null,
            kind: "proposal_decided",
            key: format!("proposal:{id}"),
            task: None,
            member: "owner".into(),
            since: decided_at(proposal),
            summary: summary(&text),
            command: format!("{} | hgs team inbox --json", json!({"request_id":PLACEHOLDER,"team":team.id})),
            detail: proposal["note"].as_str().unwrap_or_default().to_owned(),
        });
    }
    if !active {
        // N5: the decision above is all a finished team still has to say.
        return items;
    }
    // Task 112 (owner, 2026-11-06: #21 waited 529 minutes on a paused team and
    // the lead did not know why): a paused team hands out nothing; the lead is
    // told once per pause, with what waits for it.
    if team.status == TeamStatus::Paused {
        let at = team.paused.as_ref().map_or(team.updated, |pause| pause.at);
        let waiting: Vec<String> = team
            .tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Pending)
            .take(10)
            .map(|task| format!("#{}", task.id))
            .collect();
        let waits = if waiting.is_empty() { String::new() } else { format!("; {} wait to be handed out", waiting.join(", ")) };
        items.push(Item {
            subagents: Value::Null,
            kind: "team_paused",
            key: format!("team_paused:{}", at as u64),
            task: None,
            member: "owner".into(),
            since: at,
            summary: summary(&format!(
                "The team is paused {}: Zerus hands out no task until the owner resumes it{waits}",
                paused_words(team)
            )),
            command: format!("{} | hgs team status --json", json!({"team":team.id})),
            detail: "Ask the owner to resume the team (Resume team on the team bar or in Waiting for you); resume it yourself only on the owner's word.".into(),
        });
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
    // U63 (owner, 2026-10-31): a phase the owner sent back at its acceptance
    // is reworked in the same phase; one that waits for the acceptance holds
    // every later phase, and the lead accepts it only on the owner's word.
    for phase in team.phases.iter().filter(|phase| phase.acceptance_rework) {
        let Some(rejection) = &phase.rejection else { continue };
        items.push(Item {
            subagents: Value::Null,
            kind: "phase_acceptance_sent_back",
            key: format!("phase_acceptance_sent_back:{}:{}", phase.n, rejection.at),
            task: None,
            member: "owner".into(),
            since: rejection.at,
            summary: summary(&format!("The owner sent {} back at its acceptance: {}", phase_name(phase), rejection.comment)),
            command: format!(
                "{} | hgs team extend --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"phase":phase.n,"tasks":[{"id":0,"title":"...","assignee":"..."}]})
            ),
            detail: format!(
                "Add the rework to phase {} itself (extend with \"phase\":{}); it waits for the owner's acceptance again once that work is decided. No later phase starts meanwhile.\n{}",
                phase.n, phase.n, rejection.comment
            ),
        });
    }
    for phase in team.phases.iter().filter(|phase| plan::awaits_acceptance(team, phase)) {
        let since = phase.awaiting_since.unwrap_or(team.updated);
        items.push(Item {
            subagents: Value::Null,
            kind: "phase_awaiting_acceptance",
            key: format!("phase_awaiting_acceptance:{}:{}", phase.n, since),
            task: None,
            member: "owner".into(),
            since,
            summary: summary(&format!("{} waits for the owner's acceptance; no later phase starts until then", phase_name(phase))),
            command: format!(
                "{} | hgs team accept-phase --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"phase":phase.n,"owner_word":{"text":"..."}})
            ),
            detail: format!("The owner accepts phase {} or sends it back in Zerus; you accept it only on the owner's word naming the phase.", phase.n),
        });
    }
    for task in &team.tasks {
        // A report to decide (C2). Task #443: a report the lead accepted whose
        // work waits for its landing (the queue's, or a hand landing) is
        // decided: it is never a decision waiting and gets no reminder; the
        // lead's status names it in one "accepted, waiting to land" line.
        if task.status == TaskStatus::Submitted && !super::merge_queue::waits_for_landing(task) {
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
                        // U31: a report made ahead of its draft phase (an older
                        // record's work ahead; task 158 runs no draft) waits
                        // for the owner's approval of that phase.
                        summary: if team.phases.iter().any(|phase| phase.n == task.phase && phase.state.is_draft()) {
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
                            report.tests.as_ref().map(tests_text).unwrap_or_default(),
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
                    // Task 161: a member blocked on an outside event is
                    // answered with a park until its condition, with an ETA.
                    detail: if super::wait_loops::waiting_reply(&question.text) || super::wait_loops::lead_wait_instruction(&question.text).is_some() {
                        let (condition, what) = super::wait_loops::guess_condition(&question.text, question.at);
                        format!(
                            "Blocked on an outside event? Answer with a park until {what}, ETA: {} | hgs team reply --json",
                            json!({"request_id":PLACEHOLDER,"team":team.id,"question":question.id,"text":"",
                                "park":{"condition":condition,"eta_minutes":60,"note":"..."}})
                        )
                    } else {
                        String::new()
                    },
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
        // Owner (2026-11-01): an interim report (progress or a question) keeps
        // its attempt open; the lead reads it here, the owner only in the feed.
        if task.status == TaskStatus::InProgress {
            if let Some(attempt) = task.open_attempt() {
                if let Some(interim) = &attempt.interim {
                    items.push(Item {
                        subagents: Value::Null,
                        kind: "interim_report",
                        key: format!("interim:{}:{}", attempt.id, interim.at),
                        task: Some(task.id),
                        member: team.name_of(&attempt.member),
                        since: interim.at,
                        summary: summary(&format!("Interim: {}", interim.text)),
                        command: format!("{} | hgs team status --json", json!({"team":team.id,"task":task.id})),
                        detail: interim.text.clone(),
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
        // Owner, 2026-11-07: a turn that ended without a report is No report
        // (the item below offers the lead its last reply); it is not a stalled
        // session and Zerus never stores the reply as the report.
        let kind = if member.and_then(|member| member.seen).is_some_and(|seen| !seen.live) {
            Some("stopped")
        } else {
            None
        };
        if let Some(kind) = kind {
            let role = team.name_of(&attempt.member);
            let what = format!("{role}'s session stopped on #{}", task.id);
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
        // #740: the lead's own question is the owner's, never its own mailbox's.
        if member.id == team.lead {
            continue;
        }
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
    // A request only the member's Terminal takes (Kimi's Edit and Write
    // approvals): the lead cannot answer it, so it says who can and where.
    for member in team.members.iter().filter(|member| member.id != team.lead) {
        let Some(asked) = &member.terminal_question else { continue };
        let name = team.name_of(&member.id);
        items.push(Item {
            subagents: Value::Null,
            kind: "terminal_question",
            key: format!("terminal_question:{}:{}", member.id, asked.question_id),
            task: None,
            member: name.clone(),
            since: asked.at,
            summary: summary(&format!("{name} waits for an answer only its Terminal takes: {}", terminal_title(asked))),
            command: format!("{} | hgs team inbox --json", json!({"team":team.id})),
            detail: format!(
                "Zerus cannot answer it, so neither can you: {} Tell the owner to open {name}'s Terminal and answer it there; \
                 the member continues once it is answered.{}",
                asked.card["answer_unavailable_reason"].as_str().unwrap_or("Answer it in Terminal."),
                // Task 113: after the defer time the owner has it with the screen.
                if super::terminal_prompt::of(member).is_some_and(|(_, raised)| raised.is_some()) {
                    " Zerus raised it to the owner with the screen and a phone alert."
                } else {
                    ""
                }
            ),
        });
    }
    // U20: a delivered attempt whose turn ended long ago and still has no
    // report is an item. Owner, 2026-11-07: it is No report, never a capture
    // from the member's last reply; the reply is shown here and the one action
    // is to use it as the report. Waiting for the answer to the member's own
    // question or for its Background work is not a stall, so that wait is no
    // missing report; the budget or the block timeout reaches the lead as the
    // blocked message instead.
    let now = crate::state::now();
    for task in &team.tasks {
        let Some(attempt) = task.open_attempt() else { continue };
        if attempt.member == team.lead || attempt.report.is_some() {
            continue;
        }
        if attempt.open_question().is_some() || attempt.blocked_since.is_some() {
            continue;
        }
        if team.member(&attempt.member).is_some_and(|member| member.background_since.is_some()) {
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
            // One action: this reply as the report. The CLI reads the attempt's
            // kept reply (or its session's last one); the lead, or the member's
            // sub-lead, decides (task #362: the lead may run it too).
            command: format!(
                "{} | hgs team use-last-reply --json",
                json!({"request_id":PLACEHOLDER,"team":team.id,"task":task.id,"attempt":attempt.id})
            ),
            detail: summary(&attempt.last_reply),
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
            command: format!("{} | hgs team inbox --json", json!({"team":team.id})),
            detail: if limit.detail.is_empty() { String::new() } else { summary(&limit.detail) },
        });
    }
    // Task 160: a parked task whose condition failed is the lead's to decide;
    // one well past its estimate is named too.
    let now = crate::state::now();
    for task in team.tasks.iter().filter(|_| active) {
        let Some(park) = task.park.as_ref() else { continue };
        let overdue = park.state == super::park::WAITING && now > park.eta + (park.eta - park.at).max(1800.0);
        if park.state != super::park::FAILED && !overdue {
            continue;
        }
        let kind = if overdue { "park_overdue" } else { "park_failed" };
        items.push(Item {
            subagents: Value::Null,
            kind,
            key: format!("{kind}:{}:{}", task.id, park.at as i64),
            task: Some(task.id),
            member: park.member.clone(),
            since: if overdue { park.eta } else if park.checked_at > 0.0 { park.checked_at } else { park.at },
            summary: summary(&format!("#{} {}", task.id, super::park::summary(park, now))),
            command: format!(
                "{{\"team\":\"{}\",\"task\":{}}} | hgs team unpark --json (give it back now), or reassign or cancel it",
                team.id, task.id
            ),
            detail: String::new(),
        });
    }
    // Task 128 (docs/lore.md 6.1): a record Zerus cannot write until a field
    // is known waits here, with the workspace to write it in.
    if let Some((workspace, true)) = super::lore_sync::workspace(team) {
        // Task #29: the stored list is the last lore pass's; a task that has
        // its commit since is not named while the next pass is still due.
        for pending in team.lore.pending.iter().filter(|pending| super::lore_sync::still_pending(team, pending)) {
            items.push(Item {
                subagents: Value::Null,
                kind: "lore_missing",
                key: format!("lore_missing:{}:{}", pending.id, pending.missing.join(",")),
                task: None,
                member: "Zerus".into(),
                since: if team.lore.written_at > 0.0 { team.lore.written_at } else { team.updated },
                summary: summary(&pending.text),
                command: format!("lore -C \"{}\" add {} --stdin  # id {}; give {}", workspace.display(), pending.kind, pending.id, pending.missing.join(", ")),
                detail: String::new(),
            });
        }
    }
    // Task 173: a member working with nothing handed to it, at once.
    // Task #227: when its folder's branch names the task it works on, the item
    // names it and one command binds the work in progress to that task.
    for member in team.members.iter().filter(|_| active) {
        let Some(flag) = member.unassigned.as_ref() else { continue };
        let named = flag.task.and_then(|id| team.task(id).map(|task| (id, task.title.clone())));
        let (summary, command, detail) = match &named {
            Some((task, title)) => (
                summary(&format!("{} {} on #{} {} with no attempt (unassigned work)", team.name_of(&member.id), flag.what, task, title)),
                format!(
                    "{} | hgs team retry --json (binds the work in progress to #{}), or reassign",
                    json!({"request_id":"UUID","team":team.id,"task":task}),
                    task
                ),
                format!("Its branch shows #{}; the retry keeps the work and tells the member to report under it, never to start over.", task),
            ),
            None => (
                summary(&super::shadow::lead_line(team, member, flag)),
                "{\"request_id\":\"UUID\",\"team\":\"ID\",\"task\":N} | hgs team retry --json (or reassign); hgs team look MEMBER shows what it runs".into(),
                String::new(),
            ),
        };
        items.push(Item {
            subagents: Value::Null,
            kind: "unassigned_work",
            key: format!("unassigned_work:{}:{}", member.id, flag.since as i64),
            task: flag.task,
            member: member.id.clone(),
            since: flag.since,
            summary,
            command,
            detail,
        });
    }
    // Task #227: the attempts a restart gave back by itself, as one item: what
    // Zerus re-sent or bound, with no action for the lead.
    if active {
        let cutoff = crate::state::now() - 30.0 * 60.0;
        let recovered: Vec<&Value> = team
            .crash_recovered
            .iter()
            .filter(|entry| entry["at"].as_f64().unwrap_or(0.0) >= cutoff)
            .collect();
        if !recovered.is_empty() {
            let since = recovered.iter().filter_map(|entry| entry["at"].as_f64()).fold(f64::INFINITY, f64::min);
            let named: Vec<String> = recovered
                .iter()
                .filter_map(|entry| {
                    let task = entry["task"].as_u64()?;
                    let attempt = entry["attempt"].as_u64().unwrap_or_default();
                    Some(match entry["how"].as_str().unwrap_or("") {
                        "resent" => format!("#{task} attempt {attempt} sent again"),
                        "continued" => format!("#{task} attempt {attempt} continued on its own"),
                        _ => format!("#{task} attempt {attempt}"),
                    })
                })
                .collect();
            items.push(Item {
                subagents: Value::Null,
                kind: "attempt_recovered",
                key: format!("attempt_recovered:{:.0}", since),
                task: None,
                member: "Zerus".into(),
                since,
                summary: summary(&format!("The restart gave {} attempt(s) back by itself: {}", named.len(), named.join(", "))),
                command: "no action: Zerus kept each attempt's number; hgs team status --json shows them".into(),
                detail: String::new(),
            });
        }
    }
    // Task #275: a sandboxed member's queued call the worker refused or that
    // failed; the member is told as a team message and the lead reads why.
    // Task #460 (owner, 2026-11-09: 'junk reminders'): each failure is one
    // informational item, keyed by its own request id - never by the oldest
    // failure's time. A failure ageing out of the 24 h window closes its item
    // and makes no new notice; only a genuinely new failure is told, once. Its
    // command asks nothing of the lead, so it never makes the digest urgent.
    if active || finished {
        let cutoff = crate::state::now() - 24.0 * 3600.0;
        for failure in team.outbox_failures.iter().filter(|failure| failure["at"].as_f64().unwrap_or(0.0) >= cutoff) {
            let at = failure["at"].as_f64().unwrap_or(0.0);
            let member = failure["member"].as_str().unwrap_or_default();
            let op = failure["op"].as_str().unwrap_or("call");
            let error = failure["error"].as_str().unwrap_or("no reason recorded");
            // A record from before task #275 carries no id; its time still
            // names it, and every failure with an id keeps that id for good.
            let id = failure["request_id"].as_str().filter(|id| !id.is_empty()).map(str::to_owned).unwrap_or_else(|| format!("{at:.0}"));
            items.push(Item {
                subagents: Value::Null,
                kind: "refused_queued_call",
                key: format!("refused_queued_call:{id}"),
                task: None,
                member: "Zerus".into(),
                since: at,
                summary: summary(&format!("A queued {op} from {member} was refused: {error}")),
                command: "no action: the member was told at its next safe point; hgs team status --json shows the team".into(),
                detail: String::new(),
            });
        }
    }
    // Task #228: the scratch teams a QA member created for this team's tasks.
    // The lead reads each creation and stop here; a team whose sessions could
    // not be stopped is listed for the owner.
    if active || finished {
        let cutoff = crate::state::now() - 7.0 * 86400.0;
        let recent: Vec<&super::model::ScratchChild> = team
            .scratch_teams
            .iter()
            .filter(|record| record.at >= cutoff)
            .collect();
        if !recent.is_empty() {
            let since = recent.iter().map(|record| record.at).fold(f64::INFINITY, f64::min);
            let mut names = Vec::new();
            for record in &recent {
                let task = if record.task == 0 { String::new() } else { format!(" for task #{}", record.task) };
                names.push(match record.state.as_str() {
                    "stopped" => format!("{} ({}, stopped{task})", record.name, record.id),
                    "listed" => format!("{} ({}, listed: its sessions could not be stopped{task})", record.name, record.id),
                    _ => format!("{} ({}, created{task})", record.name, record.id),
                });
            }
            let listed = recent.iter().any(|record| record.state == "listed");
            items.push(Item {
                subagents: Value::Null,
                kind: "scratch_team",
                key: format!("scratch_team:{since:.0}"),
                task: None,
                member: "Zerus".into(),
                since,
                summary: summary(&format!("Scratch teams created by the QA check: {}", names.join("; "))),
                command: if listed {
                    "The owner stops a listed team: {\"team\":\"<scratch team>\",\"by\":\"human\"} | hgs team stop --json".into()
                } else {
                    "no action: hgs team status --json shows them; Zerus stops them when the creating task is decided".into()
                },
                detail: names.join("; "),
            });
        }
    }
    // Task 156: a report that contradicts a decision, before its accept.
    for task in team.tasks.iter().filter(|_| active) {
        let Some(attempt) = task.attempts.iter().rev().find(|attempt| attempt.state == AttemptState::Reported && !attempt.decision_flags.is_empty()) else { continue };
        let named: Vec<String> = attempt.decision_flags.iter().map(|flag| format!("{} (adds \"{}\")", flag["decision"].as_str().unwrap_or_default(), flag["term"].as_str().unwrap_or_default())).collect();
        items.push(Item {
            subagents: Value::Null,
            kind: "decision_conflict",
            key: format!("decision_conflict:{}:{}", task.id, attempt.n),
            task: Some(task.id),
            member: attempt.member.clone(),
            since: attempt.report.as_ref().map_or(attempt.sent_at, |report| report.at),
            summary: summary(&format!("#{} {}: the report contradicts decision {}", task.id, task.title, named.join(", "))),
            command: format!("{{\"request_id\":\"UUID\",\"team\":\"{}\",\"task\":{},\"reason\":\"...\"}} | hgs team reject --json, or accept with \"against_decisions\":true", team.id, task.id),
            detail: attempt.decision_flags.iter().map(|flag| format!("- {}: {}", flag["decision"].as_str().unwrap_or_default(), flag["choice"].as_str().unwrap_or_default())).collect::<Vec<_>>().join("\n"),
        });
    }
    // Task 271 (owner, 2026-11-08): the lead playbook; every breach is one item
    // with the exact fix, and only what the owner must decide is marked so.
    // Only a team that carries a playbook is coached: a team from before the
    // playbook (or one built in a test) reads as it did.
    if active && team.playbook.is_some() {
        for breach in super::playbook::breaches(team) {
            items.push(Item {
                subagents: Value::Null,
                kind: "playbook",
                key: breach.key(),
                task: None,
                member: team.lead.clone(),
                since: crate::state::now(),
                summary: summary(&format!("Playbook ({}): {}{}", breach.rule, breach.title, if breach.owner { " — the owner's decision" } else { "" })),
                command: breach.fix,
                detail: breach.detail,
            });
        }
    }
    items.sort_by(|a, b| a.since.total_cmp(&b.since).then_with(|| a.key.cmp(&b.key)));
    items.truncate(ITEM_LIMIT);
    items
}

/// What a Terminal-only request asks: its tool and title ("Edit: Apply these
/// edits?").
/// Task #24: whether `member` holds an open attempt now.
fn has_open_work(team: &Team, member: &str) -> bool {
    team.tasks.iter().any(|task| task.attempts.iter().any(|attempt| attempt.member == member && attempt.state.open()))
}

/// Task #24 (team ede724c4): one task's test results (newest first) as one
/// item: how many runs passed and failed, each suite's newest status, and the
/// records to read.
fn test_results_item(team: &Team, task: u32, list: &[&TestNotice]) -> Item {
    let newest = list[0];
    let passed = list.iter().filter(|notice| notice.status == "passed").count();
    let failed = list.iter().filter(|notice| notice.status == "failed").count();
    let other = list.len() - passed - failed;
    let mut suites: Vec<(&str, &str)> = Vec::new();
    for notice in list {
        if !suites.iter().any(|(suite, _)| *suite == notice.suite) {
            suites.push((notice.suite.as_str(), notice.status.as_str()));
        }
    }
    let counts = if other > 0 { format!("{passed} passed, {failed} failed, {other} other") } else { format!("{passed} passed, {failed} failed") };
    let latest: Vec<String> = suites.iter().map(|(suite, status)| format!("{suite} {status}")).collect();
    let title = team.task(task).map(|found| format!(" {}", found.title)).unwrap_or_default();
    let records: Vec<String> = list
        .iter()
        .take(10)
        .map(|notice| {
            let flags = super::test_results::flag_text(&notice.flags, notice.recorded_seconds, notice.duration_ms);
            let reused = if notice.reused_from.is_empty() { String::new() } else { format!(", reused from {}", notice.reused_from) };
            let flags = if flags.is_empty() { String::new() } else { format!(": {flags}") };
            format!("{} {} in {} s, record {}{reused}{flags}", notice.suite, notice.status, notice.duration_ms / 1000, notice.record)
        })
        .collect();
    let more = if list.len() > records.len() { format!("\n{} older run(s) in the task's records.", list.len() - records.len()) } else { String::new() };
    Item {
        subagents: Value::Null,
        kind: "test_result",
        key: format!("test_results:{task}:{}", newest.record),
        task: Some(task),
        member: newest.asked_by.clone(),
        since: newest.at,
        summary: summary(&format!("#{task}{title}: {} test run(s), {counts}; newest per suite: {}", list.len(), latest.join(", "))),
        command: format!("{} | hgs team test --json", json!({"request_id":PLACEHOLDER,"team":team.id,"action":"results","task":task})),
        detail: format!("Runs, newest first:\n{}{more}\nOne record with its log: {} | hgs team test --json",
            records.join("\n"), json!({"request_id":PLACEHOLDER,"team":team.id,"action":"results","record":newest.record})),
    }
}

pub(crate) fn terminal_title(asked: &PendingQuestion) -> String {
    let question = &asked.card["questions"][0];
    let header = question["header"].as_str().unwrap_or_default();
    let title = question["question"].as_str().unwrap_or("a permission request");
    if header.is_empty() { title.to_owned() } else { format!("{header}: {title}") }
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

/// Task 112: "since 2026-11-06 09:12 by You" for a paused team.
pub(crate) fn paused_words(team: &Team) -> String {
    match &team.paused {
        Some(pause) if pause.by.is_empty() => format!("since {}", local_time(pause.at)),
        Some(pause) => format!("since {} by {}", local_time(pause.at), pause.by),
        None => String::new(),
    }
}

/// A reset time as a person reads it, in this computer's zone; status keeps
/// the numeric `resets_at`.
pub(crate) fn local_time(at: f64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(at.round() as i64, 0)
        .map(|time| time.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| format!("{at:.0}"))
}

/// An item that stops its member until the lead answers it.
pub(crate) fn blocks_member(item: &Item) -> bool {
    matches!(item.kind, "permission" | "question")
}

/// The owner, 2026-11-05: a lead answered a member's permissions late and
/// left a question unanswered among many reports. A permission or question
/// the lead was told of and still has not answered after this long is told
/// once more, when the lead is free.
pub(crate) const REMIND_BLOCKING_SECONDS: f64 = 180.0;

/// The told permissions and questions still open past the reminder time and
/// not reminded yet.
pub(crate) fn overdue(items: &[Item], told: &[String], now: f64) -> Vec<Item> {
    items
        .iter()
        .filter(|item| blocks_member(item) && now - item.since >= REMIND_BLOCKING_SECONDS)
        .filter(|item| told.contains(&item.key) && !told.contains(&format!("remind:{}", item.key)))
        .cloned()
        .collect()
}

/// The single message that lists the lead's new items (C2): what stops a
/// member first, then the rest, each part oldest first.
pub(crate) fn text(team: &Team, items: &[Item], now: f64) -> String {
    text_with(team, items, &[], now)
}

/// `text`, with `again` the overdue items told once more.
pub(crate) fn text_with(team: &Team, items: &[Item], again: &[Item], now: f64) -> String {
    let mut ordered: Vec<(&Item, bool)> = again.iter().map(|item| (item, true)).collect();
    ordered.extend(items.iter().filter(|item| !again.iter().any(|old| old.key == item.key)).map(|item| (item, false)));
    ordered.sort_by(|(a, _), (b, _)| blocks_member(b).cmp(&blocks_member(a)).then(a.since.total_cmp(&b.since)));
    let blocking = ordered.iter().filter(|(item, _)| blocks_member(item)).count();
    let mut text = format!(
        "[Zerus team \u{b7} from: Zerus \u{b7} inbox]\nTeam: {}. {} item(s) wait for you{}:\n",
        team.name,
        ordered.len(),
        if blocking > 0 {
            format!("; the {blocking} permission(s) and question(s) first: each stops its member until you answer it")
        } else {
            ", oldest first".to_owned()
        }
    );
    for (index, (item, reminded)) in ordered.iter().take(MESSAGE_ITEMS).enumerate() {
        let item = *item;
        if *reminded {
            text += &format!("\n{}. STILL UNANSWERED, told before:", index + 1);
        }
        let task = item.task.map(|task| format!("task #{task} \u{b7} ")).unwrap_or_default();
        text += &format!(
            "{}{} \u{b7} {task}{} \u{b7} {}\n   {}\n",
            if *reminded { " ".to_owned() } else { format!("\n{}. ", index + 1) },
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
                text += &format!("   Full report: task #{task} in {} | hgs team status --json\n", json!({"team":team.id,"task":task}));
            }
        }
    }
    if ordered.len() > MESSAGE_ITEMS {
        text += &format!("\n[{} more item(s); hgs team inbox --json lists them all]\n", ordered.len() - MESSAGE_ITEMS);
    }
    if blocking > 0 {
        text += "\nAnswer every permission and question before you end your turn, and check that each answer \
                 went through. One Zerus refuses as Terminal-only goes to the owner by itself: tell the owner in \
                 your reply which member waits in its Terminal.\n";
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
    // U63 (owner, 2026-10-31): a paused phase waits for its pause to be
    // removed, and a phase behind one awaiting the owner's acceptance for it.
    if let Some(pause) = plan::phase_pause(team, task.phase) {
        let mut value = json!({
            "for":if pause.owner { "owner" } else { "lead" },"on":[format!("phase {}", task.phase)],
            "unblock":format!("remove the pause of phase {}", task.phase)
        });
        if !pause.reason.is_empty() {
            value["reason"] = json!(format!("Paused by {}: {}", pause.by, pause.reason));
        } else {
            value["reason"] = json!(format!("Paused by {}", pause.by));
        }
        return Some(value);
    }
    if let Some(held) = plan::acceptance_block(team, task.phase) {
        return Some(json!({
            "for":"owner","on":[format!("phase {held}")],
            "reason":format!("Phase {held} waits for the owner's acceptance"),
            "unblock":format!("accept phase {held}")
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
    // Task 263 (owner, 2026-11-08): another open task's outputs hold this one;
    // name the holder and why it has not started, never a silent wait.
    if task.status == TaskStatus::Pending && task.open_attempt().is_none() {
        if let Some(holder) = plan::output_holders(team, task).first().copied() {
            return Some(json!({
                "for":"outputs","on":[holder],
                "unblock":plan::holder_unblock(team, holder)
            }));
        }
    }
    // Task 295: a ready task whose files another open task writes waits for
    // that task, not for its member.
    if ready && task.status == TaskStatus::Pending {
        let queue = plan::output_queue(team, task);
        if let Some(first) = queue.first() {
            return Some(json!({"for":"deps","on":queue,
                "reason":format!("#{first} writes the same files; tasks writing the same files never run side by side"),
                "unblock":format!("finish #{first}")}));
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
    // Owner feedback 2026-11-04 (20): a decided proposal is final. Only the
    // latest few are listed, so one could leave the list and come back when
    // several were decided at once; forgetting it then repeated the whole
    // batch to the lead. Its key is kept (the newest 200) while other items
    // are forgotten once they close, so an item that opens again is new.
    // A reminder's mark stays while its item is open.
    told.retain(|key| {
        open.contains(key) || key.starts_with("proposal:") || key.strip_prefix("remind:").is_some_and(|item| open.contains(item))
    });
    let decided = told.iter().filter(|key| key.starts_with("proposal:")).count();
    if decided > 200 {
        let mut drop = decided - 200;
        told.retain(|key| {
            if drop > 0 && key.starts_with("proposal:") && !open.contains(key) {
                drop -= 1;
                return false;
            }
            true
        });
    }
}

/// Task #24 (team ede724c4): `mark_told` for an inbox, remembering the told
/// items that close for `CLOSED_MEMORY_SECONDS`, so one that opens again
/// unchanged meanwhile is not told again; an item open again is listed as
/// told once more.
pub(crate) fn mark_told_inbox(inbox: &mut Inbox, named: &[Item], open: &std::collections::BTreeSet<String>, now: f64) {
    for key in open {
        if !inbox.told.contains(key) && inbox.closed.iter().any(|closed| &closed.key == key && now - closed.at < CLOSED_MEMORY_SECONDS) {
            inbox.told.push(key.clone());
        }
    }
    let before = inbox.told.clone();
    mark_told(&mut inbox.told, named, open);
    for key in before.into_iter().filter(|key| !inbox.told.contains(key) && !key.starts_with("remind:")) {
        inbox.closed.retain(|closed| closed.key != key);
        inbox.closed.push(ClosedItem { key, at: now });
    }
    inbox.closed.retain(|closed| now - closed.at < CLOSED_MEMORY_SECONDS && !inbox.told.contains(&closed.key));
    let extra = inbox.closed.len().saturating_sub(500);
    inbox.closed.drain(0..extra);
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

/// Task 98: what the lead or the owner does about a blocked composer, with
/// the screen's last lines.
pub(crate) fn input_block_action(terminal: &str, screen: &[String]) -> String {
    let mut text = format!(
        "Open {terminal}'s Terminal. Zerus never submits a draft it did not paste: submit it if it is a message the member \
         should get, or clear it (Esc, or Ctrl+U), close any dialog and return to an empty prompt. The next hand-out then goes."
    );
    if !screen.is_empty() {
        text += "\nLast lines:\n";
        text += &screen.join("\n");
    }
    text
}

/// Task 111: the member was removed or its session was seen to end: a question
/// it asked can no longer be answered and waits for nobody.
fn session_ended(member: &Member) -> bool {
    member.state == MemberState::Removed || member.seen.as_ref().is_some_and(|seen| !seen.live)
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
    if permission_minutes > 0 || team.members.iter().any(|member| member.id == team.lead && member.pending.is_some()) {
        for member in &team.members {
            let Some(pending) = &member.pending else { continue };
            // Task 111: a question whose session ended can no longer be answered.
            if session_ended(member) {
                continue;
            }
            let waited = (now - pending.at).max(0.0);
            // #740: the lead's own question waits for the owner from the start.
            let lead = member.id == team.lead;
            if !lead && (permission_minutes == 0 || waited < permission_minutes as f64 * 60.0) {
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
    // The owner, 2026-11-05: a running team whose lead's session stopped has
    // nobody to read its reports or hand out its tasks; only the owner can
    // resume it (or restart_crashed does, on by default).
    // Task 95: the lead stalled while its decisions wait: the owner's item
    // names what it waits on; it covers a stopped lead too.
    let stalled = super::lead_stall::attention(team, now);
    let stalled_now = stalled.is_some();
    out.extend(stalled);
    if team.status == TeamStatus::Running && !stalled_now {
        if let Some(lead) = team.member(&team.lead).filter(|lead| lead.state == MemberState::Active && lead.session.is_some()) {
            if let Some(seen) = lead.seen.filter(|seen| !seen.live) {
                out.push(json!({"kind":"lead_stopped","member":team.role_of(&lead.id),"member_id":lead.id,"since":seen.at,
                    "minutes":((now - seen.at).max(0.0) / 60.0).round() as u64,
                    "summary":format!("The lead's session stopped: nobody reads reports or hands out tasks. Resume {}{}",
                        lead.session.as_ref().map(|session| session.name.as_str()).unwrap_or_default(),
                        if team.settings.restart_crashed { "" } else { " (or turn on restart_crashed)" })}));
            }
        }
    }
    // Task 98: a composer that keeps refusing hand-outs waits for the owner
    // too: only a person decides what to do with a draft Zerus did not paste.
    if team.status == TeamStatus::Running {
        for member in &team.members {
            let Some(block) = member.input_block.as_ref().filter(|block| block.raised) else { continue };
            let terminal = member.session.as_ref().map(|session| session.name.clone()).unwrap_or_default();
            out.push(json!({"kind":"input_blocked","member":team.role_of(&member.id),"member_id":member.id,"since":block.since,
                "minutes":((now - block.since).max(0.0) / 60.0).round() as u64,"count":block.count,"reason":block.reason,
                "screen":block.screen,"session":terminal,
                "summary":format!("{}'s Terminal refused {} hand-outs: a draft or dialog Zerus did not leave. Open it and clear or submit it",
                    team.name_of(&member.id), block.count),
                "command":input_block_action(&terminal, &block.screen)}));
        }
    }
    // A request only the member's Terminal takes waits for the owner at once:
    // nobody else can answer it.
    for member in &team.members {
        let Some(asked) = &member.terminal_question else { continue };
        if session_ended(member) {
            continue;
        }
        let name = team.name_of(&member.id);
        let mut item = json!({"kind":"permission_terminal","member":team.role_of(&member.id),"member_id":member.id,
            "since":asked.at,"minutes":((now - asked.at).max(0.0) / 60.0).round() as u64,
            "summary":format!("{name} waits for your answer in its Terminal: {}", terminal_title(asked))});
        // Task 113: raised after the defer time, with the session and screen.
        if let Some(Value::Object(fields)) = super::terminal_prompt::owner_fields(team, member, now) {
            for (key, value) in fields {
                item[key] = value;
            }
        }
        out.push(item);
    }
    out.extend(phase_items);
    // U63 (owner, 2026-10-31): a phase ready for the owner's acceptance.
    for phase in team.phases.iter().filter(|phase| plan::awaits_acceptance(team, phase)) {
        out.push(json!({"kind":"phase_acceptance","phase":phase.n,"since":phase.awaiting_since.unwrap_or(team.updated),
            "summary":format!("{} is ready for your acceptance", phase_name(phase)),
            "outcome":acceptance_summary(team, phase, now)}));
    }
    // Task 277 (owner, 2026-11-08): your own urgent message to a member that
    // stays unread past the alarm bound is on your Waiting list too.
    if team.settings.urgent_alarm_minutes > 0 {
        let bound = team.settings.urgent_alarm_minutes as f64 * 60.0;
        for member in team.members.iter().filter(|member| member.id != team.lead && member.state != MemberState::Removed) {
            let Some(unread) = member.unread.as_ref().filter(|unread| unread.urgent > 0 && unread.urgent_by == "owner") else { continue };
            let Some(at) = unread.urgent_at else { continue };
            let waited = (now - at).max(0.0);
            if waited < bound {
                continue;
            }
            let minutes = (waited / 60.0).round() as u64;
            out.push(json!({"kind":"urgent_message","member":team.role_of(&member.id),"member_id":member.id,"since":at,"minutes":minutes,
                "summary":format!("{} has your urgent message unread {minutes} min", team.name_of(&member.id))}));
        }
    }
    out.sort_by(|a, b| a["since"].as_f64().unwrap_or_default().total_cmp(&b["since"].as_f64().unwrap_or_default()));
    out
}

/// U63: what the owner reads before accepting a phase: its tasks by outcome,
/// its test stages and how long it took from its approval.
pub(crate) fn acceptance_summary(team: &Team, phase: &Phase, now: f64) -> Value {
    let mut outcome = super::ops::phase_outcome(team, phase.n);
    let (mut passed, mut returned) = (0, 0);
    for task in team.tasks.iter().filter(|task| task.phase == phase.n) {
        for run in task.pipeline_run.iter().flat_map(|pipeline| pipeline.runs.iter()).filter(|run| run.kind == "test") {
            match run.state {
                StageState::Passed => passed += 1,
                StageState::Returned => returned += 1,
                _ => {}
            }
        }
    }
    outcome["tests"] = json!(if passed + returned == 0 { String::new() } else { format!("Tests: {passed} passed, {returned} returned") });
    let end = phase.awaiting_since.unwrap_or(now);
    outcome["minutes"] = json!(phase.decided_at.map_or(0, |start| ((end - start).max(0.0) / 60.0).round() as u64));
    outcome
}

/// #718: the proposal of a full run, for the lead's card and the owner's list.
pub(crate) fn full_run_text(proposal: &super::model::FullRunProposal) -> String {
    let minutes = (proposal.seconds / 60.0).ceil() as u64;
    let unknown = if proposal.unknown > 0 { format!(" and {} target(s) without a recorded time", proposal.unknown) } else { String::new() };
    let why = if proposal.reason == "age" { "the oldest reused measurement is too old" } else { "enough results were reused" };
    format!("A full test run is proposed: {} result(s) reused since the last one ({why}); about {minutes} min{unknown}", proposal.reused)
}

/// #718: what a report's change does to the tests, on the lead's card.
/// Task #406: the alarm for ready tasks behind a worker held by one phase.
pub(crate) fn worker_stall(team: &Team, now: f64) -> Option<Item> {
    let ready = super::plan::ready(team);
    let first = *ready.first()?;
    let (step, held) = super::tick_clock::stall(now)?;
    let minutes = (held / 60.0).floor() as u64;
    Some(Item {
        kind: "stalled",
        key: "worker_stall".into(),
        task: Some(first),
        member: team.lead.clone(),
        since: now - held,
        summary: format!(
            "{} ready task(s) wait (#{first} first): the team worker has been held by '{step}' for {minutes} min and hands out nothing",
            ready.len()
        ),
        command: "hgs team worker --stop (it ends within 5 s; the next team command starts it again)".into(),
        detail: "hgs team status --json shows worker_tick: the phase running now, the last tick and the slowest recent one.".into(),
        subagents: Value::Null,
    })
}

pub(crate) fn tests_text(tests: &Value) -> String {
    if tests["files"].as_u64().unwrap_or(0) == 0 {
        return "Tests: no test file changed.".into();
    }
    // Task #401: a change too large to read file by file has its counts only.
    if tests["bounded"] == true {
        return format!(
            "Tests: +{} -{} lines in {} test file(s) of {} changed; functions not listed for a change this large.",
            tests["lines_added"].as_u64().unwrap_or(0),
            tests["lines_removed"].as_u64().unwrap_or(0),
            tests["files"].as_u64().unwrap_or(0),
            tests["changed_files"].as_u64().unwrap_or(0)
        );
    }
    let added = tests["functions_added"]["total"].as_u64().unwrap_or(0);
    let removed = tests["functions_removed"]["total"].as_u64().unwrap_or(0);
    let names = |key: &str| -> String {
        let list: Vec<&str> = tests[key]["names"].as_array().into_iter().flatten().filter_map(Value::as_str).take(5).collect();
        if list.is_empty() { String::new() } else { format!(" ({})", list.join(", ")) }
    };
    format!(
        "Tests: +{} -{} lines in {} file(s); functions +{added}{} -{removed}{}.",
        tests["lines_added"].as_u64().unwrap_or(0),
        tests["lines_removed"].as_u64().unwrap_or(0),
        tests["files"].as_u64().unwrap_or(0),
        names("functions_added"),
        names("functions_removed")
    )
}

/// #752: what the build broker does in this folder and what it costs, for the
/// lead's item (owner, 2026-10-31: say the cost plainly).
fn broker_detail(config: &super::broker::Config) -> String {
    let assets = if config.assets.is_empty() { String::new() } else { format!(" Asset folders are never copied: {}.", config.assets.join(", ")) };
    if config.shared {
        format!(
            "With the build broker, builds run in this folder itself{}, one at a time. Each build first waits (at most {} s) until \
             every other member has asked for a build since it last changed its files, so nobody compiles another's half-written \
             file.{assets} Cost: no copy and no second build folder, but builds queue behind each other, and a member in the middle \
             of an edit delays the others' builds up to that wait. The owner's Allow on your switch card turns it on \
             (hgs guide team-broker).",
            if config.unreal { " (an Unreal project: no second Intermediate, and the editor loads the DLL that was built)" } else { "" },
            config.ready_wait.as_secs()
        )
    } else {
        format!(
            "With the build broker each member builds its own snapshot of the folder (its own changed files, the rest at HEAD) \
             in its own build folder, one build at a time; a failure in another member's file is told as theirs.{assets} \
             Cost: each member gets a copy of the tracked sources and its own build folder, so each member's first build is a \
             full build and its outputs take disk once per member. For a large project, mode = \"shared\" in [build_broker] of \
             .zerus/tests.toml builds in the folder itself instead. The owner's Allow on your switch card turns it on \
             (hgs guide team-broker)."
        )
    }
}

#[cfg(test)]
mod tests {

    /// Task #24 (team ede724c4): a task's test results are one inbox item with
    /// its counts, each suite's newest status and its records; a result for no
    /// task keeps its own card. A new result changes the item's key (it is
    /// told again); nothing new keeps it.
    #[test]
    fn a_tasks_test_results_are_one_item_with_counts() {
        let mut team = Team { id: "t".into(), name: "T".into(), status: TeamStatus::Running, ..Default::default() };
        team.tasks.push(Task { id: 7, title: "Composer".into(), status: TaskStatus::Accepted, ..Default::default() });
        let notice = |record: &str, suite: &str, status: &str, at: f64, task: Option<u32>| TestNotice {
            record: record.into(), suite: suite.into(), status: status.into(), duration_ms: 12_000, asked_by: "merge queue".into(), at, task,
            ..Default::default()
        };
        team.test_notices = vec![
            notice("r1", "client-core", "failed", 10.0, Some(7)),
            notice("r2", "client-web", "failed", 20.0, Some(7)),
            notice("r3", "client-core", "passed", 30.0, Some(7)),
            notice("r4", "hgs", "passed", 40.0, None),
            notice("r5", "client-core", "failed", 50.0, Some(8)),
        ];
        // Task #29: results clear after 12 h; read at this test's own clock.
        let items = crate::state::with_test_now(100.0, || items(&team));
        let tests: Vec<&Item> = items.iter().filter(|item| item.kind == "test_result").collect();
        assert_eq!(tests.len(), 3, "#7, #8 and the result for no task: {tests:?}");
        let seven = tests.iter().find(|item| item.task == Some(7)).unwrap();
        assert_eq!(seven.key, "test_results:7:r3");
        assert!(seven.summary.contains("#7 Composer: 3 test run(s), 1 passed, 2 failed"), "{}", seven.summary);
        assert!(seven.summary.contains("newest per suite: client-core passed, client-web failed"), "{}", seven.summary);
        assert!(seven.command.contains("\"action\":\"results\"") && seven.command.contains("\"task\":7"), "{}", seven.command);
        assert!(seven.detail.contains("record r1") && seven.detail.contains("record r3"), "{}", seven.detail);
        assert_eq!(seven.since, 30.0);
        assert!(tests.iter().any(|item| item.task.is_none() && item.key == "test_result:r4"));
        // A new result for #7: one item still, with a new key.
        team.test_notices.push(notice("r6", "client-web", "passed", 60.0, Some(7)));
        let again = crate::state::with_test_now(100.0, || super::items(&team));
        let seven: Vec<&Item> = again.iter().filter(|item| item.kind == "test_result" && item.task == Some(7)).collect();
        assert_eq!(seven.len(), 1);
        assert_eq!(seven[0].key, "test_results:7:r6");
        assert!(seven[0].summary.contains("4 test run(s), 2 passed, 2 failed"), "{}", seven[0].summary);
    }

    /// Task #24: an item told before that closes and opens again unchanged
    /// (a member between turns, a rework's member idle for a moment) is not
    /// new: the inbox never repeats an unchanged list; after the memory it is.
    #[test]
    fn an_item_that_closes_and_opens_again_is_not_told_twice() {
        let item = |key: &str| Item { subagents: Value::Null, kind: "rework_waits", key: key.into(), task: Some(3), member: "dev".into(), since: 1.0,
            summary: "#3 waits for its member, 5 min".into(), command: "reassign".into(), detail: String::new() };
        let mut inbox = Inbox::default();
        let open: std::collections::BTreeSet<String> = ["rework_waits:3:1".to_owned()].into();
        mark_told_inbox(&mut inbox, &[item("rework_waits:3:1")], &open, 100.0);
        assert!(inbox.knows("rework_waits:3:1", 100.0));
        // A later message while the item is closed (its member idle a moment).
        mark_told_inbox(&mut inbox, &[item("report:9")], &["report:9".to_owned()].into(), 200.0);
        assert!(!inbox.told.contains(&"rework_waits:3:1".to_owned()), "closed: no longer listed as told");
        // It opens again unchanged: known, so the next list does not name it.
        assert!(inbox.knows("rework_waits:3:1", 300.0));
        let both: std::collections::BTreeSet<String> = ["rework_waits:3:1".to_owned(), "report:9".to_owned()].into();
        mark_told_inbox(&mut inbox, &[], &both, 300.0);
        assert!(inbox.told.contains(&"rework_waits:3:1".to_owned()) && inbox.closed.is_empty(), "{inbox:?}");
        // Closed for longer than the memory: told again when it returns.
        mark_told_inbox(&mut inbox, &[], &Default::default(), 400.0);
        assert!(!inbox.knows("rework_waits:3:1", 400.0 + CLOSED_MEMORY_SECONDS));
        assert!(inbox.knows("rework_waits:3:1", 400.0 + CLOSED_MEMORY_SECONDS - 1.0));
        mark_told_inbox(&mut inbox, &[], &Default::default(), 400.0 + CLOSED_MEMORY_SECONDS);
        assert!(inbox.closed.is_empty(), "the memory is bounded by time");
    }
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
    fn the_broker_card_names_the_mode_and_its_cost() {
        let shared = super::super::broker::Config { shared: true, unreal: true, assets: vec!["Content".into()], ..Default::default() };
        let text = broker_detail(&shared);
        assert!(text.contains("in this folder itself") && text.contains("no second Intermediate") && text.contains("Cost:") && text.contains("never copied: Content"), "{text}");
        let text = broker_detail(&Default::default());
        assert!(text.contains("own snapshot") && text.contains("Cost:") && text.contains("mode = \"shared\""), "{text}");
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
            attempt.last_reply = "Done: the report is the last message".into();
        }
        let list = items(&team);
        let found = list.iter().find(|item| item.kind == "no_report").unwrap();
        assert!(found.summary.contains("without a report"), "{}", found.summary);
        // The lead sees the kept reply and the one action that uses it.
        assert_eq!(found.detail, "Done: the report is the last message");
        assert!(found.command.contains("hgs team use-last-reply"), "{}", found.command);
        assert!(!found.command.contains("team report"), "{}", found.command);
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

    /// The owner, 2026-11-04: a Kimi Edit approval waited half an hour with
    /// an idle lead. Zerus cannot answer it, so it is the lead's item naming
    /// who can and the owner's at once, never silence.
    #[test]
    fn a_request_only_the_terminal_takes_reaches_the_lead_and_the_owner_at_once() {
        let mut team = team();
        let now = 5_000.0;
        let card = json!({"question_id":"k1","question_hash":"hk","approval":true,"can_answer":false,
            "answer_unavailable_reason":"Zerus answers only Kimi's Bash command approvals here. Answer this request in Terminal.",
            "questions":[{"id":"approval","header":"Edit","question":"Apply these edits?","options":[]}]});
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().terminal_question =
            Some(PendingQuestion { question_id: "k1".into(), question_hash: "hk".into(), approval: true, at: now - 60.0, card });
        let items = super::items(&team);
        let item = items.iter().find(|item| item.kind == "terminal_question").expect("the lead hears of it");
        assert!(item.summary.contains("dev (Backend)") && item.summary.contains("Edit: Apply these edits?"), "{}", item.summary);
        assert!(item.detail.contains("Terminal") && item.detail.contains("Bash command approvals"), "{}", item.detail);
        assert!(!item.command.contains("hgs team answer"), "the lead cannot answer it: {}", item.command);
        // The owner sees it at once, without the permission reminder.
        let attention = super::attention(&team, now);
        let found = attention.iter().find(|item| item["kind"] == "permission_terminal").expect("the owner sees it");
        assert_eq!(found["member_id"], "dev");
        assert!(found["summary"].as_str().unwrap().contains("in its Terminal"), "{found}");
        let waiting = super::super::owner_waiting::items(std::slice::from_ref(&team), now);
        let listed = waiting.iter().find(|item| item["kind"] == "permission_escalated").expect("in the owner's list");
        assert_eq!(listed["action"], "open_terminal");
        // No request, no item.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().terminal_question = None;
        assert!(!super::items(&team).iter().any(|item| item.kind == "terminal_question"));
        assert!(!super::attention(&team, now).iter().any(|item| item["kind"] == "permission_terminal"));
    }

    /// The owner, 2026-11-05: the lead's session stopped and the team still
    /// showed Running; nobody read reports. The owner sees it at once.
    #[test]
    fn a_stopped_lead_waits_for_the_owner_at_once() {
        let mut team = team();
        let now = 5_000.0;
        team.members.iter_mut().find(|member| member.id == "lead").unwrap().seen = Some(MemberSeen { at: now - 120.0, live: false });
        // Off, the item names the setting that would restart the lead.
        team.settings.restart_crashed = false;
        let attention = super::attention(&team, now);
        let found = attention.iter().find(|item| item["kind"] == "lead_stopped").expect("the owner sees the stopped lead");
        assert_eq!(found["minutes"], 2);
        assert!(found["summary"].as_str().unwrap().contains("restart_crashed"), "{found}");
        let waiting = super::super::owner_waiting::items(std::slice::from_ref(&team), now);
        assert!(waiting.iter().any(|item| item["kind"] == "lead_stopped" && item["action"] == "resume_lead"), "{waiting:?}");
        // A live lead, or a paused team, is no item.
        team.members.iter_mut().find(|member| member.id == "lead").unwrap().seen = Some(MemberSeen { at: now, live: true });
        assert!(!super::attention(&team, now).iter().any(|item| item["kind"] == "lead_stopped"));
    }

    /// Task 111: a question whose session ended, or whose member was
    /// removed, can no longer be answered and waits for nobody.
    #[test]
    fn a_question_whose_session_ended_waits_for_nobody() {
        let mut team = team();
        let now = 5_000.0;
        team.settings.remind_owner_permission_minutes = 1;
        let question = |at: f64| PendingQuestion {
            question_id: "q1".into(),
            question_hash: "h1".into(),
            approval: true,
            at,
            card: json!({"questions":[{"id":"decision","header":"Bash","question":"Run the build?",
                "options":[{"id":"allow","label":"Allow"},{"id":"deny","label":"Deny"}]}]}),
        };
        let asked = |team: &Team| super::attention(team, now).iter().filter(|item| item["kind"] == "permission").count();
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().pending = Some(question(now - 600.0));
        assert_eq!(asked(&team), 1);
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: now - 30.0, live: false });
        assert_eq!(asked(&team), 0, "its session ended");
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.seen = Some(MemberSeen { at: now, live: true });
        assert_eq!(asked(&team), 1);
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().state = MemberState::Removed;
        assert_eq!(asked(&team), 0, "its member was removed");
        let waiting = super::super::owner_waiting::items(std::slice::from_ref(&team), now);
        assert!(!waiting.iter().any(|item| item["kind"] == "permission_escalated"), "{waiting:?}");
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
        // The question stops its member: it comes before the older report.
        assert!(text.contains("1. question") && text.contains("2. report"), "{text}");
        assert!(text.contains("10 min") && text.contains("5 min"), "{text}");
        assert!(text.contains("Done and checked") && text.contains("Which database?"), "{text}");
        assert!(text.contains("hgs team accept --json") && text.contains("hgs team reply --json"), "{text}");
        // N7: the message says how to read the whole report.
        assert!(text.contains("Full report: task #1") && text.contains("hgs team status --json") && text.contains("\"task\":1"), "{text}");
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

    /// Task #286: the screenshot's merge-conflict wait is the lead's inbox
    /// item, never the owner's Waiting, however long it stays unresolved.
    #[test]
    fn a_merge_conflict_wait_lands_in_the_leads_inbox_not_the_owners_waiting() {
        let mut team = team();
        team.tasks[0].status = TaskStatus::Pending;
        team.tasks[0].waits = Some(TaskWait::zerus(
            "dependencies",
            "refused",
            "#229's accepted commit abcdef conflicts in src/x.rs; merge it into the member's branch (folder) and resolve it, or reassign the task, then retry",
            500.0,
        ));
        let lead = super::items(&team);
        assert!(lead.iter().any(|item| item.kind == "task_waits" && item.task == Some(1)), "{lead:?}");
        let owner = super::super::owner_waiting::items(std::slice::from_ref(&team), 500.0 + 600.0 * 60.0);
        assert!(!owner.iter().any(|item| item["kind"] == "task_wait_escalated"), "{owner:?}");
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

    /// #740: the lead's own question (its AskUserQuestion) waits for the owner
    /// at once, with its questions and the member's id, and never in the
    /// lead's own mailbox.
    #[test]
    fn the_leads_own_question_waits_for_the_owner_at_once() {
        let mut team = team();
        let now = 5_000.0;
        let card = json!({"questions":[{"id":"q_0","header":"Cache","question":"Install the build cache?","multi_select":false,
            "allow_other":true,"options":[{"id":"1","label":"Yes, on D:"},{"id":"2","label":"No"}]}]});
        let lead = team.lead.clone();
        team.members.iter_mut().find(|member| member.id == lead).unwrap().pending =
            Some(PendingQuestion { question_id: "toolu_1".into(), question_hash: "h1".into(), approval: false, at: now - 5.0, card: card.clone() });
        // Not the lead's mailbox.
        assert!(!super::items(&team).iter().any(|item| item.kind == "permission"), "the lead's question is not its own item");
        // The owner's attention, five seconds after it was asked.
        let attention = super::attention(&team, now);
        let asked: Vec<&Value> = attention.iter().filter(|item| item["kind"] == "permission").collect();
        assert_eq!(asked.len(), 1, "{attention:?}");
        assert_eq!(asked[0]["member_id"], json!(lead));
        assert!(asked[0]["summary"].as_str().unwrap().contains("Install the build cache?"), "{}", asked[0]);
        // Even with the members' reminder off.
        team.settings.remind_owner_permission_minutes = 0;
        assert!(super::attention(&team, now).iter().any(|item| item["kind"] == "permission"));
        // Waiting for you lists it, bound to the lead's id.
        let waiting = super::super::owner_waiting::items(&[team.clone()], now);
        let found = waiting.iter().find(|item| item["kind"] == "permission_escalated").expect("the lead's question waits for you");
        assert_eq!(found["member_id"], json!(lead));
        assert_eq!(found["action"], "answer_permission");
        // The record the desktop answers from carries the questions and Other.
        let record = super::super::dispatcher::permission_record(team.member(&lead).unwrap().pending.as_ref().unwrap());
        assert_eq!(record["questions"], card["questions"]);
    }
}

#[cfg(test)]
mod told_tests {
    use super::*;

    fn item(key: &str) -> Item {
        Item { subagents: Value::Null, kind: "proposal_decided", key: key.into(), task: None, member: String::new(), since: 0.0,
            summary: String::new(), command: String::new(), detail: String::new() }
    }

    /// Owner feedback 2026-11-04 (20): a decided proposal told once is not
    /// told again when it leaves the listed few and comes back.
    #[test]
    fn a_told_decision_stays_told_after_it_leaves_the_list() {
        let mut told = Vec::new();
        let open: std::collections::BTreeSet<String> = ["proposal:p1".to_owned(), "report:1".to_owned()].into();
        mark_told(&mut told, &[item("proposal:p1"), item("report:1")], &open);
        // Both leave the open list: the report is forgotten, the decision kept.
        mark_told(&mut told, &[], &Default::default());
        assert_eq!(told, vec!["proposal:p1".to_owned()]);
    }

    /// Task #364: the lead hears once when a team's build folders pass the
    /// owner's limit.
    #[test]
    fn the_build_folders_notice_is_one_item_while_a_team_is_over_the_limit() {
        let state = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(state.path(), || {
            use crate::state::teams::ops;
            let (mut team, _) = ops::create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
            ops::apply(&mut team, "plan", &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],"tasks":[{"id":1,"title":"A","assignee":"dev"}]}), &ops::Actor::Human, &std::collections::BTreeSet::new(), &mut Vec::new()).unwrap();
            ops::apply(&mut team, "approve", &json!({}), &ops::Actor::Human, &std::collections::BTreeSet::new(), &mut Vec::new()).unwrap();
            team.status = crate::state::teams::model::TeamStatus::Running;
            let target = work.path().join("target");
            std::fs::create_dir_all(&target).unwrap();
            crate::state::build_dirs::note(crate::state::build_dirs::Use {
                path: target.clone(), kind: "cargo_target", origin: "team_run", team: team.id.clone(), ..Default::default()
            })
            .unwrap();
            assert!(!items(&team).iter().any(|item| item.kind == "build_folders"));
            crate::state::build_dirs::test_measure(&target, 200 * 1024 * 1024 * 1024, 100).unwrap();
            let found: Vec<Item> = items(&team).into_iter().filter(|item| item.kind == "build_folders").collect();
            assert_eq!(found.len(), 1, "{found:?}");
            assert!(found[0].summary.contains("over your 100.0 GB limit"), "{}", found[0].summary);
            // Still one on the next read: the notice fires once while it holds.
            assert_eq!(items(&team).iter().filter(|item| item.kind == "build_folders").count(), 1);
        });
    }
}

