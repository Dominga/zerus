//! Task 134: how a team message reaches its member. The text goes into the
//! working tier first (`working.rs`); the terminal gets the doorbell, one
//! line under the message's own request id, so a stuck or lost paste loses
//! nothing and receipts keep their request identities. An agent whose hooks
//! take added context at the turn start (Claude, Codex) gets the unread text
//! from Zerus in that turn; any other agent gets the text after the doorbell
//! line (the hand-out text), stored all the same.
use super::dispatcher::{send, Sent};
use super::model::{Member, SessionRef, Team};
use super::store::{Event, Store};
use super::working::{self, Message, NewMessage, Urgency};
use serde_json::json;

/// Agents whose turn-start hook (`UserPromptSubmit`) returns added context
/// that the agent reads (`hookSpecificOutput.additionalContext`), as their
/// SessionStart context already does.
pub(crate) fn turn_hook(agent: &str) -> bool {
    matches!(agent, "claude" | "codex")
}

/// What a doorbell pastes: the line, and for an agent without a turn hook
/// the unread text after it (bounded as a turn's context is).
pub(crate) fn paste(agent: &str, header: &str, list: &[Message]) -> String {
    let mut text = working::doorbell(header, list);
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
        Ok((working::unread(tx, team_id, member_id, at)?, agent))
    });
    let (list, agent) = match posted {
        Ok(found) => found,
        Err(error) => return Sent::Refused(format!("the team message was not stored, so nothing was sent: {error:#}")),
    };
    let outcome = send(session, request_id, &paste(&agent, working::header_of(text), &list));
    settle(store, team_id, member_id, &agent, request_id, &list, &outcome, "idle", false);
    outcome
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
    let outcome = send(&session, &request_id, &paste(&member.agent, "", &list));
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

#[cfg(test)]
#[path = "doorbell_tests.rs"]
mod tests;
