//! A member launch that completed after Stop, Archive or the member's
//! removal (zerus-63rru.11). Its session is recorded as provenance with one
//! owner card that names it, so that the person can end that agent. Zerus
//! never stops it and never binds it. Reading ends at package 7's own launch
//! deadline, or one hour after this service first saw the row without one.
use super::{
    ledger::Ledger,
    ports::{Effects, Result},
    record, Recorded,
};
use crate::desktop::teams::{
    ledger::Row,
    ports::{LaunchRead, ObservedSession, Plan},
};
use crate::teams::{ops::Operation, store::Store};
use std::collections::BTreeMap;
use zerus_team_plan::model::SessionKey;

pub(super) const PAGE: u32 = 4;
pub(super) const WITHOUT_DEADLINE: i64 = 60 * 60_000;

#[derive(Default)]
pub(super) struct Sweep {
    after: (String, String),
    first_seen: BTreeMap<(String, String, String), i64>,
}

pub(super) fn step(
    store: &mut Store,
    ledger: &mut Ledger,
    effects: &mut impl Effects,
    s: &mut Sweep,
    now: i64,
) -> Result<()> {
    let rows = ledger.late(&s.after, PAGE)?;
    let full = rows.len() == PAGE as usize;
    for mut row in rows {
        s.after = (row.team.clone(), row.member.clone());
        visit(store, ledger, effects, s, &mut row, now)?;
    }
    if !full {
        s.after = Default::default();
    }
    Ok(())
}
fn visit(
    store: &mut Store,
    ledger: &mut Ledger,
    effects: &mut impl Effects,
    s: &mut Sweep,
    row: &mut Row,
    now: i64,
) -> Result<()> {
    let Some(plan) = row.plan.clone() else {
        return ledger.finish(row, now);
    };
    if store
        .late_session(&row.team, &row.member, &plan.launch_id)?
        .is_some()
    {
        return ledger.finish(row, now);
    }
    let key = (row.team.clone(), row.member.clone(), plan.launch_id.clone());
    let done = match effects.launch(&plan) {
        Ok(LaunchRead::Session(session)) => provenance(store, row, &plan, &session, now)?,
        Ok(LaunchRead::Ended { .. }) => true,
        Ok(LaunchRead::Absent { .. }) | Err(_) => {
            let deadline = if row.unknown_until > 0 {
                row.unknown_until
            } else {
                s.first_seen
                    .entry(key.clone())
                    .or_insert(now)
                    .saturating_add(WITHOUT_DEADLINE)
            };
            now >= deadline
        }
    };
    if done {
        s.first_seen.remove(&key);
        ledger.finish(row, now)?;
    }
    Ok(())
}
fn provenance(
    store: &mut Store,
    row: &Row,
    plan: &Plan,
    session: &ObservedSession,
    now: i64,
) -> Result<bool> {
    if session.launch_id != plan.launch_id || session.machine != plan.machine {
        return Ok(false);
    }
    let Some(team) = store.team(&row.team)? else {
        return Ok(true);
    };
    // The name is recorded whole. One the native contract would not hold (it
    // never reaches 512 bytes) is named by its launch instead of refused.
    let valid = zerus_client_core::desktop::creation_launch::session_name(&session.name);
    let name = if valid {
        &session.name
    } else {
        &plan.launch_id
    };
    let op = Operation::RecordLateSession {
        member: row.member.clone(),
        launch_id: plan.launch_id.clone(),
        session: SessionKey {
            machine: session.machine.clone(),
            session_id: format!("launch_id:{}", session.launch_id),
        },
        name: name.clone(),
    };
    // A refusal is final: recorded before, or a live member binds normally.
    Ok(record(store, &team, op, now)? != Recorded::Retry)
}
