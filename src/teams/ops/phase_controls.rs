//! S5 controls change phase policy under the same custody as hand-out
//! admission. Scope and pause IDs are evidence, never caller authority.
use super::*;
use std::collections::BTreeSet;
use store::phase_controls as facts;
use zerus_team_plan::phase_control::*;

pub(super) fn prepare(
    store: &Store,
    request: &Request,
) -> Option<Result<facts::task_custody::TaskCustody>> {
    use facts::task_custody::{Selection, TaskCustody};
    let selection = match &request.operation {
        Operation::MovePhase {
            scope, relative_to, ..
        } => Selection::Interval(scope.phase(), relative_to.phase()),
        Operation::SetPhaseAcceptance { scope, .. } => Selection::One(scope.phase()),
        _ => return None,
    };
    let db = store.db.as_ref()?;
    let saved = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM requests WHERE id=?1)",
            [&request.request_id],
            |r| r.get::<_, bool>(0),
        )
        .map_err(storage);
    // Do not open an external resource for a known receipt. Actor, content,
    // stale and native-original verification remain in the common writer.
    match saved {
        Ok(true) => None,
        Ok(false) => Some(TaskCustody::acquire(
            db,
            store.state(),
            &request.team,
            selection,
        )),
        Err(error) => Some(Err(error)),
    }
}

fn bound(ctx: &Context<'_>, scope: &PhaseControlScope) -> Result<Phase> {
    if !scope.valid() {
        return Err(invalid("The exact phase control scope is invalid."));
    }
    if !matches!(
        ctx.team.status,
        TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused
    ) {
        return Err(invalid("Only an active team can change phase controls."));
    }
    if scope.phase() == 0 {
        return Err(invalid("The intake queue has no planned phase controls."));
    }
    let p = facts::phase(ctx.db, &ctx.team.id, scope.phase())?;
    if p.kind != PhaseKind::Plan
        || p.mode != PhaseMode::Standard
        || !matches!(
            p.state,
            PhaseState::Draft
                | PhaseState::Rejected
                | PhaseState::Approved
                | PhaseState::AwaitingAcceptance
        )
    {
        return Err(invalid(
            "Only a nonterminal planned phase can change controls.",
        ));
    }
    if facts::scope(ctx.db, &ctx.team.id, p.n)? != *scope {
        return Err(stale());
    }
    let prerequisites = facts::prerequisites(ctx.db, ctx.team, &p)?;
    if !prerequisites.allowed {
        return Err(invalid(
            prerequisites
                .reason
                .as_deref()
                .unwrap_or("The phase control facts are unavailable."),
        ));
    }
    facts::pause(ctx.db, &ctx.team.id, &p)?;
    Ok(p)
}

fn require_unstarted(ctx: &Context<'_>, p: &Phase) -> Result<()> {
    let prerequisites = facts::prerequisites(ctx.db, ctx.team, p)?;
    if !prerequisites.allowed {
        return Err(invalid(
            prerequisites
                .reason
                .as_deref()
                .unwrap_or("The phase control facts are unavailable."),
        ));
    }
    // Historical store facts and the actual task document must both agree.
    let state = facts::task_custody::availability(ctx.task_custody, ctx.db, ctx.team, p)?;
    if state.allowed {
        Ok(())
    } else {
        Err(invalid(
            state
                .reason
                .as_deref()
                .unwrap_or("The phase is not positively unstarted."),
        ))
    }
}

fn owner(actor: &Actor) -> Result<PauseOwner> {
    match actor {
        Actor::Person { person, .. } => Ok(PauseOwner::Person {
            person: person.clone(),
        }),
        Actor::Session {
            person,
            member,
            session,
            ..
        } => Ok(PauseOwner::Lead {
            person: person.clone(),
            member: member.clone(),
            session: session.clone(),
        }),
        _ => Err(Problem::new(
            code::NOT_ALLOWED,
            "This actor cannot own a phase pause.",
        )),
    }
}

fn pause_binding(ctx: &Context<'_>, p: &Phase, expected: &Option<String>) -> Result<bool> {
    if let Some(id) = expected {
        uuid(id)?;
    }
    let current = facts::pause(ctx.db, &ctx.team.id, p)?;
    let PhasePause::Paused { episode } = current else {
        if expected.is_some() {
            return Err(stale());
        }
        return Ok(false);
    };
    if episode.as_ref().map(|e| &e.id) != expected.as_ref() {
        return Err(stale());
    }
    let who = owner(ctx.actor)?;
    if !is_person(ctx.actor) && episode.as_ref().is_none_or(|e| who != e.owner) {
        return Err(Problem::new(
            code::NOT_ALLOWED,
            "Only the person or the exact owner of this lead pause can remove or replace it.",
        ));
    }
    Ok(true)
}

/// Only authorized S5 control fields are copied to an open proposal. Frozen
/// nodes, original requests, closed history and policy defaults are untouched.
fn save(ctx: &Context<'_>, old: &Phase, new: &Phase) -> Result<()> {
    if let Some(row) = store::rework::open(ctx.db, &ctx.team.id, old.n)? {
        if let Some(mut header) = row.header {
            if !rework::controls(old, &header) {
                return Err(invalid(
                    "The retained correction controls conflict with the phase.",
                ));
            }
            header.barrier = new.barrier;
            header.paused = new.paused;
            ctx.db
                .execute(
                    "UPDATE phase_reworks SET header=?2 WHERE id=?1",
                    params![row.id, encode(&header)?],
                )
                .map_err(storage)?;
        }
        if old.order != new.order || old.acceptance != new.acceptance {
            return Err(invalid(
                "An open correction retains its order and acceptance policy.",
            ));
        }
    }
    ctx.db
        .execute(
            "UPDATE phases SET data=?3 WHERE team=?1 AND n=?2",
            params![ctx.team.id, new.n, encode(new)?],
        )
        .map_err(storage)?;
    Ok(())
}

pub(super) fn write(ctx: &mut Context<'_>) -> Result<()> {
    let scope = match ctx.op {
        Operation::SetPhaseBarrier { scope, .. }
        | Operation::PausePhase { scope, .. }
        | Operation::ResumePhase { scope, .. }
        | Operation::MovePhase { scope, .. }
        | Operation::SetPhaseAcceptance { scope, .. } => scope,
        _ => unreachable!(),
    };
    if !facts::installed(ctx.db)? {
        return Err(storage("The phase controls layout is unavailable."));
    }
    let old = bound(ctx, scope)?;
    let mut new = old.clone();
    match ctx.op {
        Operation::SetPhaseBarrier { barrier, .. } => {
            if old.barrier == *barrier {
                return Err(invalid("The phase already has that barrier setting."));
            }
            new.barrier = *barrier;
            save(ctx, &old, &new)?;
            drafts::revalidate_controls(ctx, &BTreeSet::from([old.n]))?;
        }
        Operation::PausePhase {
            pause, reason: why, ..
        } => {
            reason(why)?;
            text(why, 2048)?;
            if zerus_team_plan::forbidden_char(why).is_some() {
                return Err(invalid(
                    "The phase pause reason contains a forbidden character.",
                ));
            }
            pause_binding(ctx, &old, pause)?;
            let owner = owner(ctx.actor)?;
            if !owner.valid() || encode(&owner)?.len() > 4096 {
                return Err(over("The phase pause owner exceeds its byte bound."));
            }
            ctx.db.execute("UPDATE phase_pause_episodes SET active=0 WHERE team=?1 AND phase=?2 AND active=1",params![ctx.team.id,old.n]).map_err(storage)?;
            ctx.db.execute("INSERT INTO phase_pause_episodes(id,team,phase,owner,reason,created_ms,active) VALUES(?1,?2,?3,?4,?5,?6,1)",params![ctx.request_id,ctx.team.id,old.n,encode(&owner)?,why,ctx.now]).map_err(storage)?;
            new.paused = true;
            save(ctx, &old, &new)?;
        }
        Operation::ResumePhase { pause, .. } => {
            if !pause_binding(ctx, &old, pause)? {
                return Err(invalid("The phase is not paused."));
            }
            ctx.db.execute("UPDATE phase_pause_episodes SET active=0 WHERE team=?1 AND phase=?2 AND active=1",params![ctx.team.id,old.n]).map_err(storage)?;
            new.paused = false;
            save(ctx, &old, &new)?;
        }
        Operation::SetPhaseAcceptance { acceptance, .. } => {
            require_unstarted(ctx, &old)?;
            if *acceptance == Acceptance::Pipeline || old.acceptance == *acceptance {
                return Err(invalid(
                    "Choose a different Person or Lead acceptance policy.",
                ));
            }
            new.acceptance = *acceptance;
            save(ctx, &old, &new)?;
        }
        Operation::MovePhase {
            relative_to,
            placement,
            ..
        } => {
            let target = bound(ctx, relative_to)?;
            require_unstarted(ctx, &target)?;
            if old.n == target.n {
                return Err(invalid("A phase cannot move relative to itself."));
            }
            let mut phases = facts::phases(ctx.db, &ctx.team.id)?;
            phases.retain(|p| p.n != 0);
            phases.sort_by_key(|p| (if p.order == 0 { p.n } else { p.order }, p.n));
            let from = phases
                .iter()
                .position(|p| p.n == old.n)
                .ok_or_else(missing)?;
            let to = phases
                .iter()
                .position(|p| p.n == target.n)
                .ok_or_else(missing)?;
            let insert = match placement {
                PhasePlacement::Before => to,
                PhasePlacement::After => to + 1,
            };
            let insert = if insert > from { insert - 1 } else { insert };
            if insert == from {
                return Err(invalid("The phase is already in that relative position."));
            }
            let lo = from.min(insert);
            let hi = from.max(insert);
            let mut slots = BTreeSet::new();
            for p in &phases {
                if !slots.insert(if p.order == 0 { p.n } else { p.order }) {
                    return Err(invalid("Phase order slots are ambiguous."));
                }
            }
            let slots: Vec<_> = phases[lo..=hi]
                .iter()
                .map(|p| if p.order == 0 { p.n } else { p.order })
                .collect();
            for p in &phases[lo..=hi] {
                require_unstarted(ctx, p)?;
            }
            let moved = phases.remove(from);
            phases.insert(insert, moved);
            let mut changed = BTreeSet::new();
            for (p, slot) in phases[lo..=hi].iter().zip(slots) {
                let mut next = p.clone();
                next.order = slot;
                save(ctx, p, &next)?;
                changed.insert(p.n);
            }
            drafts::revalidate_controls(ctx, &changed)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
