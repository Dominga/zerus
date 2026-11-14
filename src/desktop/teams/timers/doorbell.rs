//! The lead's doorbell: one guarded send that says how many items wait and
//! names the command that reads them. It carries no text of an item; the
//! inbox is the durable state and the doorbell only points at it.
use super::{
    ledger::{Binding, Ledger},
    ports::{Effects, Presence, Result, Ring, Rung},
};
use crate::teams::store::{
    timers::{Bell, Gate},
    Store,
};
use zerus_team_plan::model::{Member, MemberState, Team, TeamStatus};

#[allow(clippy::too_many_arguments)]
pub(super) fn ring(
    store: &Store,
    ledger: &Ledger,
    effects: &mut impl Effects,
    epoch: &str,
    team: &Team,
    lead: &Member,
    seen: Option<&Presence>,
    now: i64,
) -> Result<Option<Rung>> {
    ledger.interrupted(&team.id)?;
    let (Some(p), Some(session)) = (seen.filter(|p| p.idle()), lead.session.as_ref()) else {
        return Ok(None);
    };
    if team.status != TeamStatus::Running || lead.state != MemberState::Active {
        return Ok(None);
    }
    let Some(view) = store.inbox_view(&team.id)? else {
        return Ok(None);
    };
    let binding = Binding {
        team: &team.id,
        member: &lead.id,
        session,
        run: &p.run,
        conversation: &p.conversation,
    };
    let interval = i64::from(team.settings.timers.inbox_interval_seconds) * 1000;
    let Some(reserved) = ledger.reserve(&binding, view.newest, view.open, interval, epoch, now)?
    else {
        return Ok(None);
    };
    let ring = Ring {
        id: reserved.id.clone(),
        gate: Gate::Lead(Bell {
            team: team.id.clone(),
            member: lead.id.clone(),
            session: session.clone(),
            after: reserved.after,
        }),
        agent: lead.agent.clone(),
        name: p.name.clone(),
        run: p.run.clone(),
        conversation: p.conversation.clone(),
        text: text(view.open, &team.id),
    };
    let rung = effects.ring(store, &ring);
    ledger.settle(&reserved.id, &rung)?;
    ledger.prune(&team.id, &p.run, now)?;
    Ok(Some(rung))
}
pub(super) fn text(open: u32, team: &str) -> String {
    let count = match open {
        1 => "1 item waits".to_owned(),
        n if n > 200 => "More than 200 items wait".to_owned(),
        n => format!("{n} items wait"),
    };
    format!("{count} in your team inbox. Read them with: hgs team inbox --team {team}")
}
