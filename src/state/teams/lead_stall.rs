//! WhiteWh fork, task 95 (owner, 2026-11-06): a stalled lead never stalls the
//! team silently. The owner left, the lead's session stopped on a refused tool
//! call, and every member sat idle for ten hours: reports waited 560 minutes,
//! questions 600, and the starved notices went only to the stopped lead.
//!
//! When the lead's decisions (reports, questions, permissions) wait past the
//! team's `lead_stall_minutes` and the lead's session does not work on them
//! (idle, its own question open, a refused tool, an exited agent), the
//! dispatcher records one `LeadStall`. The owner sees it in Waiting for you,
//! on the team bar and on the phone (the lead's `mobile_attention`); each
//! `round` is notified once, a new round starts at most hourly, and the record
//! clears as soon as the lead moves or nothing waits past the threshold.
use super::dispatcher::Seen;
use super::inbox;
use super::model::{LeadStall, MemberState, Team, TeamStatus};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Task #289: the lead's waiting decisions. Notices (a decided proposal, a
/// cancelled dependency, an unread count, a digest) are information, never a
/// decision, so they neither start the stall nor count in its nag.
const DECISIONS: [&str; 3] = ["report", "question", "permission"];

/// The items the stall counts and names: the same set, so the nag's count and
/// its list agree.
pub(crate) fn decision(item: &inbox::Item) -> bool {
    DECISIONS.contains(&item.kind)
}

/// The stall this tick sees: `None` when it cannot decide (the lead's
/// computer did not answer the listing), `Some(None)` when the lead is not
/// stalled, `Some(Some(stall))` with the record to keep otherwise.
pub(crate) fn desired(team: &Team, seen: &[Seen], now: f64, listed: Option<&BTreeSet<String>>) -> Option<Option<LeadStall>> {
    let minutes = team.settings.lead_stall_minutes;
    if team.status != TeamStatus::Running || minutes == 0 {
        return Some(None);
    }
    let Some(lead) = team.member(&team.lead).filter(|lead| lead.state != MemberState::Removed) else { return Some(None) };
    let after = minutes as f64 * 60.0;
    let waiting: Vec<inbox::Item> = inbox::items(team)
        .into_iter()
        .filter(|item| decision(item) && now - item.since >= after)
        .collect();
    if waiting.is_empty() {
        return Some(None);
    }
    let found = lead
        .session
        .as_ref()
        .and_then(|session| seen.iter().find(|found| found.host == session.host && found.name == session.name));
    let own_question = lead.pending.as_ref().or(lead.terminal_question.as_ref());
    let (reason, detail) = match found {
        None => {
            // N7: an absent listing is not an exited agent.
            let host = lead.session.as_ref().map(|session| session.host.as_str()).unwrap_or(lead.computer.as_str());
            if listed.is_some_and(|hosts| !hosts.contains(host)) {
                return None;
            }
            ("exited", String::new())
        }
        Some(found) if !found.live || found.state == "stopped" => ("exited", String::new()),
        Some(found) if own_question.is_some() || found.asking || matches!(found.phase.as_str(), "approval" | "input" | "question") => {
            ("question", own_question.map(|asked| short(&inbox::terminal_title(asked))).unwrap_or_default())
        }
        Some(found) if found.phase == "interrupted" => ("rejected_tool", String::new()),
        // A turn, a Zerus build or its own Background jobs: it works.
        Some(found) if found.activity == "busy" || found.building || found.background => return Some(None),
        Some(_) => ("idle", String::new()),
    };
    let members: BTreeSet<&str> = waiting.iter().map(|item| item.member.as_str()).filter(|member| !member.is_empty()).collect();
    let oldest = waiting.iter().map(|item| item.since).fold(f64::INFINITY, f64::min);
    let (since, round, notified_at) = match &team.lead_stall {
        Some(old) if now - old.notified_at >= LeadStall::REPEAT_SECONDS => (old.since, old.round + 1, now),
        Some(old) => (old.since, old.round, old.notified_at),
        None => (now, 1, now),
    };
    Some(Some(LeadStall {
        since,
        reason: reason.into(),
        detail,
        items: waiting.len() as u32,
        members: members.len() as u32,
        oldest,
        round,
        notified_at,
        // Task 139: the lead hears of a round once.
        lead_told: team.lead_stall.as_ref().is_some_and(|old| old.round == round && old.lead_told),
    }))
}

fn short(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= 200 { line } else { format!("{}\u{2026}", line.chars().take(199).collect::<String>()) }
}

/// The attention item the owner's lists read (`inbox::attention`).
pub(crate) fn attention(team: &Team, now: f64) -> Option<Value> {
    let stall = team.lead_stall.as_ref().filter(|_| team.status == TeamStatus::Running)?;
    let lead = team.member(&team.lead)?;
    Some(json!({"kind":"lead_stalled","member":team.role_of(&lead.id),"member_id":lead.id,"since":stall.since,
        "minutes":((now - stall.oldest).max(0.0) / 60.0).round() as u64,"reason":stall.reason,"detail":stall.detail,
        "items":stall.items,"members":stall.members,"round":stall.round,"summary":stall.title(now),
        "session":lead.session.as_ref().map(|session| session.name.clone()).unwrap_or_default()}))
}

/// The phone's alert on the lead's session row (`mobile_attention`): one item
/// per round, so the relay pushes once per round and withdraws it when the
/// lead moves. Its `kind` says it is an alert, never a question to answer; it
/// carries counts and the reason's kind, never the lead's question text.
pub(crate) fn mobile_item(team: &Team, stall: &LeadStall) -> Value {
    use sha2::{Digest, Sha256};
    let id = format!("team-lead-stall:{}:{}", team.id, stall.round);
    let hash = format!("{:x}", Sha256::digest(format!("{id}\n{}", stall.since).as_bytes()));
    json!({"question_id":id,"question_hash":hash,"kind":"team_lead_stall","team":team.id,"team_name":team.name,
        "items":stall.items,"members":stall.members,"reason":stall.reason,"round":stall.round,"since":stall.since})
}

/// The feed event of a raised or repeated stall, or of its end.
pub(crate) fn event(old: Option<&LeadStall>, new: Option<&LeadStall>, now: f64) -> Option<(&'static str, Value)> {
    match (old, new) {
        (old, Some(new)) if old.is_none_or(|old| old.round != new.round) => Some((
            "lead_stalled",
            json!({"reason":new.reason,"detail":new.detail,"items":new.items,"members":new.members,"round":new.round,
                "minutes":((now - new.oldest).max(0.0) / 60.0).round() as u64}),
        )),
        (Some(old), None) => Some(("lead_stall_cleared", json!({"minutes":((now - old.since).max(0.0) / 60.0).round() as u64,"rounds":old.round}))),
        _ => None,
    }
}
