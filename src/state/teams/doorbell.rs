//! Task 134: how a team message reaches its member. The text goes into the
//! working tier first (`working.rs`); the terminal gets the doorbell, one
//! line under the message's own request id, so a stuck or lost paste loses
//! nothing and receipts keep their request identities. An agent whose hooks
//! take added context at the turn start (Claude, Codex) gets the unread text
//! from Zerus in that turn; any other agent gets the text after the doorbell
//! line (the hand-out text), stored all the same.
use super::dispatcher::{hgs, send, Sent};
use super::model::{AttemptState, Member, SessionRef, StageState, Team, UrgentInterrupt};
use super::store::{Event, Store};
use super::working::{self, Message, NewMessage, Urgency};
use serde_json::json;
use std::time::Duration;

/// Task 277 (owner, 2026-11-08): the line a stopped turn continues with.
pub(crate) const CONTINUE: &str = "Continue your task from where you stopped.";

/// Agents whose turn-start hook (`UserPromptSubmit`) returns added context
/// that the agent reads (`hookSpecificOutput.additionalContext`), as their
/// SessionStart context already does.
pub(crate) fn turn_hook(agent: &str) -> bool {
    matches!(agent, "claude" | "codex")
}

/// What a doorbell pastes: the line, and for an agent without a turn hook
/// the unread text after it (bounded as a turn's context is). `urgent`: a
/// message it rings for is urgent.
pub(crate) fn paste(agent: &str, header: &str, list: &[Message], urgent: bool) -> String {
    let mut text = working::doorbell(header, list, urgent);
    if !turn_hook(agent) {
        if let Some((context, _)) = working::turn_context(list, false) {
            text.push_str("\n\n");
            text.push_str(&context);
        }
    }
    text
}

/// Stores a team message for `member_id` and rings its doorbell under
/// `request_id`. A message that could not be stored is a refused send (the
/// caller retries as for any refusal); a refused doorbell withdraws the
/// message it just stored, so a retry with a new request id is the only copy.
#[allow(clippy::too_many_arguments)]
pub(crate) fn post_and_ring(
    store: &mut Store,
    team_id: &str,
    member_id: &str,
    session: &SessionRef,
    request_id: &str,
    kind: &str,
    by: &str,
    urgency: Urgency,
    text: &str,
) -> Sent {
    let at = super::super::now();
    // Task 184: the message is working-tier text (docs/lore.md 11.9); the
    // stored text, its header and the paste are the redacted one.
    let text = super::secrets::redact_working(text);
    #[cfg(test)]
    POSTED.with(|posted| posted.borrow_mut().insert(request_id.to_owned(), text.clone()));
    let posted = store.mutate_working(team_id, |tx, team, events| {
        events.extend(working::cleanup(tx, team_id, at)?);
        let new = NewMessage { request_id, by, to: member_id, kind, urgency, text: &text };
        // Zerus's own delivery journals as it always has (handed_out,
        // delivered, inbox_told...); the message adds no event of its own.
        working::post(tx, team_id, &new, at)?;
        working::refresh(tx, team, member_id, at)?;
        let agent = team.member(member_id).map(|member| member.agent.clone()).unwrap_or_default();
        Ok((working::unread(tx, team_id, member_id, at)?, agent, working::urgent_bound(team)))
    });
    let (list, agent, bound) = match posted {
        Ok(found) => found,
        Err(error) => return Sent::Refused(format!("the team message was not stored, so nothing was sent: {error:#}")),
    };
    // Task #24 (team ede724c4): the doorbell rings for this message and the
    // unread ones due; only those count a ring and only they make it URGENT.
    // Every unread message used to count one per post (a lead's message 11
    // rings in an hour, a member 'does not read' after two quick posts).
    let rung = ringing(&list, request_id, bound, at);
    let urgent = rung.iter().any(Message::urgent);
    let outcome = send(session, request_id, &paste(&agent, working::header_of(&text), &list, urgent));
    settle(store, team_id, member_id, &agent, request_id, &rung, &outcome, "idle", false);
    outcome
}

/// Task #24: the unread messages a doorbell posted with `request_id` rings
/// for: that message, and those whose own schedule is due by `now`.
fn ringing(list: &[Message], request_id: &str, bound: f64, now: f64) -> Vec<Message> {
    let last = working::last_ring(list);
    list.iter()
        .filter(|message| message.request_id == request_id || working::next_ring(message, last, bound).is_some_and(|at| at <= now))
        .cloned()
        .collect()
}

/// Task 186 (docs/lore.md hard rule 3, 11.11): stores a team message for a
/// recipient that is working, without typing anything: it waits for the next
/// safe point. A Claude session takes an urgent one at its next tool
/// boundary and every unread one at its next turn start (the hooks' added
/// context); otherwise the dispatcher rings at the turn end
/// (`RingMessages`). An error means nothing was stored.
#[allow(clippy::too_many_arguments)]
pub(crate) fn post_waiting(
    store: &mut Store,
    team_id: &str,
    member_id: &str,
    request_id: &str,
    kind: &str,
    by: &str,
    urgency: Urgency,
    text: &str,
) -> anyhow::Result<()> {
    let at = super::super::now();
    #[cfg(test)]
    POSTED.with(|posted| posted.borrow_mut().insert(request_id.to_owned(), text.to_owned()));
    store.mutate_working(team_id, |tx, team, events| {
        events.extend(working::cleanup(tx, team_id, at)?);
        let new = NewMessage { request_id, by, to: member_id, kind, urgency, text };
        working::post(tx, team_id, &new, at)?;
        working::refresh(tx, team, member_id, at)?;
        events.push(Event::new(
            "message_waits",
            json!({"to":team.role_of(member_id),"kind":kind,"urgent":urgency == Urgency::Urgent,"request_id":request_id}),
        ));
        Ok(())
    })
}

/// After a doorbell: rung (and, without a turn hook, put into the turn) on a
/// delivery that may have arrived; withdrawn when refused before anything
/// reached the member and it never rang before.
#[allow(clippy::too_many_arguments)]
fn settle(store: &mut Store, team_id: &str, member_id: &str, agent: &str, request_id: &str, list: &[Message], outcome: &Sent, safe_point: &str, journal: bool) {
    let at = super::super::now();
    let ids: Vec<String> = list.iter().map(|message| message.id.clone()).collect();
    let result = store.mutate_working(team_id, |tx, team, events| {
        let role = team.role_of(member_id);
        match outcome {
            Sent::Refused(_) => {
                working::withdraw(tx, request_id)?;
            }
            Sent::Delivered | Sent::Uncertain(_) => {
                working::rung(tx, &ids, safe_point, at)?;
                if !turn_hook(agent) {
                    working::injected(tx, &ids, safe_point, at)?;
                }
                if journal {
                    events.push(Event::new(
                    "message_rung",
                    json!({"to":role,"count":ids.len(),"urgent":list.iter().any(Message::urgent),"request_id":request_id,
                        "certain":matches!(outcome, Sent::Delivered),"safe_point":safe_point}),
                    ));
                }
            }
        }
        working::refresh(tx, team, member_id, at)
    });
    if let Err(error) = result {
        eprintln!("hgs team worker: {member_id}: the doorbell's state was not recorded: {error:#}");
    }
}

/// The dispatcher's ring of a member's unread messages (the first ring of a
/// message posted while the member worked, or the one ring more after the
/// bound), only at an idle moment: never a paste into a busy composer.
/// Task 186: each message by its own schedule; the doorbell names every
/// unread one, and only the messages due count a ring. `safe_point` is
/// `turn_end` for a message that waited for a turn, `idle` otherwise.
pub(crate) fn ring(store: &mut Store, team: &Team, member_id: &str) -> anyhow::Result<Sent> {
    let member = team.member(member_id).context("unknown member")?;
    let session = member.session.clone().context("the member has no session")?;
    let now = super::super::now();
    let list = working::unread(store.connection(), &team.id, member_id, now)?;
    anyhow::ensure!(!list.is_empty(), "nothing unread");
    let bound = working::urgent_bound(team);
    let last = working::last_ring(&list);
    let due: Vec<Message> = list.iter().filter(|message| working::next_ring(message, last, bound).is_some_and(|at| at <= now)).cloned().collect();
    anyhow::ensure!(!due.is_empty(), "nothing is due to ring");
    let waited = due.iter().any(|message| {
        message.rings == 0 && message.injected_at.is_none() && last.is_none_or(|last| message.at >= last + working::COALESCE_SECONDS)
    });
    let request_id = uuid::Uuid::new_v4().to_string();
    let outcome = send(&session, &request_id, &paste(&member.agent, "", &list, due.iter().any(Message::urgent)));
    settle(store, &team.id, member_id, &member.agent, &request_id, &due, &outcome, if waited { "turn_end" } else { "idle" }, true);
    Ok(outcome)
}

use anyhow::Context as _;

// A test reads what a team message said: the text stored under the request id
// its doorbell carried.
#[cfg(test)]
thread_local! {
    static POSTED: std::cell::RefCell<std::collections::HashMap<String, String>> = std::cell::RefCell::new(Default::default());
}

/// The text a send stands for in a test: the stored message its doorbell
/// rang for (by the send's request id), else what was pasted.
#[cfg(test)]
pub(crate) fn posted_text(input: &serde_json::Value) -> String {
    let request = input["request_id"].as_str().unwrap_or_default();
    POSTED.with(|posted| posted.borrow().get(request).cloned()).unwrap_or_else(|| input["text"].as_str().unwrap_or_default().to_owned())
}

/// Whether the dispatcher rings `member` now: unread messages that never
/// rang (posted while it worked), or one ring more once the oldest has waited
/// past its bound (`RING_AGAIN_SECONDS`, or `URGENT_BOUND_SECONDS` for an
/// urgent one). At most two rings; the lead's inbox takes it from there.
pub(crate) fn due(member: &Member, now: f64) -> bool {
    let Some(unread) = member.unread.as_ref() else { return false };
    // Task 186: each message's own schedule (`working::next_ring`).
    if unread.planned {
        return unread.ring_at.is_some_and(|at| now >= at);
    }
    if unread.rings == 0 {
        return true;
    }
    let bound = if unread.urgent > 0 { working::URGENT_BOUND_SECONDS } else { working::RING_AGAIN_SECONDS };
    unread.rings < 2 && now - unread.rung_at.unwrap_or(unread.oldest_at) >= bound
}

/// Whether the lead hears that `member` does not read (the inbox item): two
/// rings and the bound again, or an urgent message the member's turn kept
/// from ringing past `urgent_bound`.
pub(crate) fn unread_stalled(member: &Member, now: f64, urgent_bound: f64) -> Option<String> {
    let unread = member.unread.as_ref()?;
    let minutes = ((now - unread.oldest_at).max(0.0) / 60.0).round() as u64;
    // Task 186: by each message's own rings and the team's urgent_bound.
    if unread.planned {
        if unread.stall_at.is_some_and(|at| now >= at) {
            return Some(format!("{} unread team message(s), the oldest {minutes} min old, rang twice", unread.count));
        }
        let held = unread.held_urgent_at.filter(|at| now - at >= urgent_bound)?;
        let minutes = ((now - held).max(0.0) / 60.0).round() as u64;
        return Some(format!(
            "an urgent team message is undelivered: the member has worked {minutes} min without a safe point; stop it natively to deliver it at the turn end"
        ));
    }
    let bound = if unread.urgent > 0 { working::URGENT_BOUND_SECONDS } else { working::RING_AGAIN_SECONDS };
    if unread.rings >= 2 && now - unread.rung_at.unwrap_or(unread.oldest_at) >= bound {
        return Some(format!("{} unread team message(s), the oldest {minutes} min old, rang twice", unread.count));
    }
    if unread.urgent > 0 && unread.rings == 0 && now - unread.oldest_at >= working::URGENT_BOUND_SECONDS {
        return Some(format!(
            "an urgent team message is undelivered: the member has worked {minutes} min without a safe point; stop it natively to deliver it at the turn end"
        ));
    }
    None
}

/// Task 186 (11.11): an urgent message to the lead that has reached no safe
/// point within the team's `urgent_bound`: the owner's Waiting item (Zerus
/// never stops the lead). Answers since when it waits and its minutes.
pub(crate) fn lead_undelivered(team: &Team, now: f64) -> Option<(f64, u64)> {
    let lead = team.member(&team.lead)?;
    let held = lead.unread.as_ref().filter(|unread| unread.planned)?.held_urgent_at?;
    (now - held >= working::urgent_bound(team)).then(|| (held, ((now - held).max(0.0) / 60.0).round() as u64))
}

/// Task 277: the unread urgent messages of `member`, oldest first.
fn urgent_unread(store: &Store, team: &Team, member_id: &str, at: f64) -> anyhow::Result<Vec<Message>> {
    let list = working::unread(store.connection(), &team.id, member_id, at)?;
    Ok(list.into_iter().filter(Message::urgent).collect())
}

/// Task 277 (owner, 2026-11-08): interrupt the member's busy turn natively
/// (Claude/Codex/Kimi stop, DeepSeek aborts, OpenCode stops) for its unread
/// urgent messages, pinned to the displayed turn. The stop is not waited for
/// here: the dispatcher sees the turn end and `continue_after_interrupt` opens
/// the new turn, while the attempt itself goes on. A refusal is recorded and
/// not repeated for the same message (the alarm covers it); a newer urgent
/// message may still try once more.
pub(crate) fn interrupt(store: &mut Store, team: &Team, member_id: &str, turn: f64, run: &str, conversation: &str) -> anyhow::Result<()> {
    let at = super::super::now();
    let urgent = urgent_unread(store, team, member_id, at)?;
    anyhow::ensure!(!urgent.is_empty(), "nothing urgent is unread");
    let Some(session) = team.member(member_id).and_then(|member| member.session.clone()) else { return Ok(()) };
    let request = json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":run,
        "expected_conversation_id":conversation,"expected_turn_started":turn});
    let outcome = hgs(&session.host, &["interrupt", session.name.as_str(), "--json"], Some(&request), Duration::from_secs(60));
    let error = match &outcome {
        Ok(output) if output.status.success() => String::new(),
        Ok(output) => String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        Err(error) => error.to_string(),
    };
    let ids: Vec<String> = urgent.iter().map(|message| message.id.clone()).collect();
    let oldest = urgent.first().map(|message| message.id.clone()).unwrap_or_default();
    let count = urgent.len();
    store.mutate_working(&team.id, |_, team, events| {
        let role = team.role_of(member_id);
        if let Some(found) = team.members.iter_mut().find(|found| found.id == member_id) {
            found.urgent = Some(UrgentInterrupt {
                ids,
                oldest,
                at,
                turn,
                state: if error.is_empty() { "requested".into() } else { "failed".into() },
                error: error.clone(),
            });
        }
        events.push(Event::new("urgent_interrupt", json!({"member":role,"count":count,"ok":error.is_empty(),"error":error})));
        Ok(())
    })?;
    Ok(())
}

/// Task 277: after the stop, open a new turn that carries the urgent message
/// on top and says to continue the task; the attempt is untouched. A message
/// that was read meanwhile is skipped; nothing urgent left clears the record.
pub(crate) fn continue_after_interrupt(store: &mut Store, team: &Team, member_id: &str) -> anyhow::Result<()> {
    let Some(member) = team.member(member_id) else { return Ok(()) };
    let Some(record) = member.urgent.clone().filter(|record| record.state == "requested") else { return Ok(()) };
    let at = super::super::now();
    let list: Vec<Message> = urgent_unread(store, team, member_id, at)?
        .into_iter()
        .filter(|message| record.ids.contains(&message.id))
        .collect();
    let Some(session) = member.session.clone().filter(|_| !list.is_empty()) else {
        // Nothing left to carry, or the member has no session any more.
        store.mutate_working(&team.id, |_, team, events| {
            let role = team.role_of(member_id);
            if let Some(found) = team.members.iter_mut().find(|found| found.id == member_id) {
                found.urgent = None;
            }
            events.push(Event::new("urgent_delivered", json!({"member":role,"count":0,"certain":true,"skipped":true})));
            Ok(())
        })?;
        return Ok(());
    };
    let task = team.tasks.iter().find_map(|task| task.open_attempt().filter(|attempt| attempt.member == member_id).map(|_| task.id));
    let header = match task {
        Some(id) => format!("[Zerus team \u{b7} from: Zerus \u{b7} task: #{id} \u{b7} urgent]"),
        None => "[Zerus team \u{b7} from: Zerus \u{b7} urgent]".to_owned(),
    };
    let agent = member.agent.clone();
    let count = list.len();
    let mut text = working::doorbell(&header, &list, list.iter().any(Message::urgent));
    text.push('\n');
    text.push_str(CONTINUE);
    if !turn_hook(&agent) {
        if let Some((context, _)) = working::turn_context(&list, false) {
            text.push_str("\n\n");
            text.push_str(&context);
        }
    }
    let request = uuid::Uuid::new_v4().to_string();
    let outcome = send(&session, &request, &text);
    let ids: Vec<String> = list.iter().map(|message| message.id.clone()).collect();
    let error = match &outcome {
        Sent::Refused(error) => error.clone(),
        Sent::Uncertain(error) => error.clone(),
        Sent::Delivered => String::new(),
    };
    let delivered = !matches!(outcome, Sent::Refused(_));
    store.mutate_working(&team.id, |tx, team, events| {
        let role = team.role_of(member_id);
        if delivered {
            working::rung(tx, &ids, "interrupt", at)?;
            if !turn_hook(&agent) {
                working::injected(tx, &ids, "interrupt", at)?;
            }
        }
        if let Some(found) = team.members.iter_mut().find(|found| found.id == member_id) {
            found.urgent = Some(UrgentInterrupt {
                state: if delivered { "sent".into() } else { "failed".into() },
                error: error.clone(),
                ..record.clone()
            });
        }
        working::refresh(tx, team, member_id, at)?;
        events.push(Event::new(
            "urgent_delivered",
            json!({"member":role,"count":count,"certain":matches!(outcome, Sent::Delivered),"error":error}),
        ));
        Ok(())
    })?;
    Ok(())
}

/// Task 277 (owner, 2026-11-08): a hand-out or stage message that already
/// reached the member's turn (`injected_at`) for an attempt it holds is
/// stale, not unread: mark it read, so "does not read" counts only messages
/// that never reached it. A message the turn hook has not injected yet stays
/// unread (it still has to reach a turn).
pub(crate) fn mark_taken(store: &mut Store, team: &Team) -> anyhow::Result<()> {
    let at = super::super::now();
    let mut by_member: std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>> = Default::default();
    for task in &team.tasks {
        if let Some(attempt) = task
            .open_attempt()
            .filter(|attempt| matches!(attempt.state, AttemptState::Delivered | AttemptState::Unconfirmed))
        {
            by_member.entry(attempt.member.as_str()).or_default().insert(attempt.id.as_str());
        }
        if let Some(run) = task.pipeline_run.as_ref().and_then(|run| run.open_run()).filter(|run| run.state == StageState::Delivered) {
            by_member.entry(run.member.as_str()).or_default().insert(run.id.as_str());
        }
    }
    for (member, requests) in by_member {
        let list = working::unread(store.connection(), &team.id, member, at)?;
        let ids: Vec<String> = list
            .iter()
            .filter(|message| matches!(message.kind.as_str(), "hand_out" | "stage"))
            .filter(|message| message.injected_at.is_some() && requests.contains(message.request_id.as_str()))
            .map(|message| message.id.clone())
            .collect();
        if ids.is_empty() {
            continue;
        }
        store.mutate_working(&team.id, |tx, team, _| {
            working::mark_read(tx, &team.id, member, &ids, "taken", "", at)?;
            working::refresh(tx, team, member, at)
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "doorbell_tests.rs"]
mod tests;
