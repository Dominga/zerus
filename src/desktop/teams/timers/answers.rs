//! The member's doorbell: one guarded send that says how many of the lead's
//! answers wait and names `hgs team current`. It never carries the text of a
//! question or of an answer; the member's own read shows them. The same
//! rules as the lead's doorbell, per member: the team running, the member's
//! session live and idle with no pending request, a mark newer than the
//! binding's last doorbell, the interval passed, and at the spawn the member
//! still holding the attempt the mark names.
use super::{
    ledger::{
        answers::{AnswerBinding, AnswerReserved},
        Ledger,
    },
    ports::{Effects, Presence, Result, Ring, Sessions},
};
use crate::teams::store::{
    timers::{AnswerBell, AnswerMark, Gate},
    Store,
};
use zerus_team_plan::model::{Member, MemberState, Team, TeamStatus};

#[allow(clippy::too_many_arguments)]
pub(super) fn ring(
    store: &Store,
    ledger: &Ledger,
    sessions: &mut impl Sessions,
    effects: &mut impl Effects,
    epoch: &str,
    team: &Team,
    roster: &[Member],
    now: i64,
) -> Result<()> {
    ledger.answers_interrupted(&team.id)?;
    if team.status != TeamStatus::Running {
        return Ok(());
    }
    let mut after = String::new();
    for _ in 0..2 {
        let marks = store.answer_marks(&team.id, &after, 200)?;
        for mark in &marks {
            let member = roster.iter().find(|m| m.id == mark.member);
            if let Some(member) = member.filter(|m| m.state == MemberState::Active) {
                let seen = sessions.presence(team, member);
                one(
                    store,
                    ledger,
                    effects,
                    epoch,
                    team,
                    member,
                    seen.as_ref(),
                    mark,
                    now,
                )?;
            }
        }
        match marks.last() {
            Some(last) if marks.len() == 200 => after = last.member.clone(),
            _ => break,
        }
    }
    ledger.prune_answers(&team.id, now)
}
#[allow(clippy::too_many_arguments)]
fn one(
    store: &Store,
    ledger: &Ledger,
    effects: &mut impl Effects,
    epoch: &str,
    team: &Team,
    member: &Member,
    seen: Option<&Presence>,
    mark: &AnswerMark,
    now: i64,
) -> Result<()> {
    let (Some(p), Some(session)) = (seen.filter(|p| p.idle()), member.session.as_ref()) else {
        return Ok(());
    };
    let holds = store
        .attempt_holder(&team.id, mark.node)?
        .is_some_and(|h| h.member == member.id && h.generation == mark.generation);
    if !holds {
        return Ok(());
    }
    let binding = AnswerBinding {
        team: &team.id,
        member: &member.id,
        session,
        run: &p.run,
        conversation: &p.conversation,
    };
    let interval = i64::from(team.settings.timers.inbox_interval_seconds) * 1000;
    let Some(AnswerReserved { id, count }) =
        ledger.reserve_answer(&binding, mark, interval, epoch, now)?
    else {
        return Ok(());
    };
    let ring = Ring {
        id: id.clone(),
        gate: Gate::Answer(AnswerBell {
            team: team.id.clone(),
            member: member.id.clone(),
            session: session.clone(),
            node: mark.node,
            generation: mark.generation,
            mark: mark.revision,
        }),
        agent: member.agent.clone(),
        name: p.name.clone(),
        run: p.run.clone(),
        conversation: p.conversation.clone(),
        text: text(count),
    };
    let rung = effects.ring(store, &ring);
    ledger.settle_answer(&id, &rung)
}
/// The service's own words: a count and the command, nothing of an answer.
pub(super) fn text(count: u32) -> String {
    match count {
        1 => "1 answer from your lead waits. Read it with: hgs team current".to_owned(),
        n => format!("{n} answers from your lead wait. Read them with: hgs team current"),
    }
}
