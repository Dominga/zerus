//! The lead's doorbell, the timers and what a stop leaves behind (package
//! 10). The teams actor's tick calls [`Timers::team`] for each live team it
//! visits and [`Timers::terminal`] once per pass. Store facts are recorded
//! only through service operations; each session effect is one original
//! child admitted at its spawn, and an unknown result is never repeated.
#![allow(dead_code)] // The tick of package 8 calls these entries.
mod answers;
mod clear;
mod doorbell;
mod fire;
mod late;
mod ledger;
mod native;
mod ports;
#[cfg(test)]
pub(crate) fn native_doorbell_for_test(config: &crate::config::Config, gate: crate::teams::store::timers::Gate, request: &serde_json::Value) -> serde_json::Value {
    use ports::{Effects, Ring, Rung};
    let text = |key: &str| request[key].as_str().unwrap().to_owned();
    let machine = text("machine");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut native = native::Native { config, machine:&machine, stop:&stop };
    let ring = Ring { id:text("request_id"), gate, agent:"dsh".into(), name:text("name"), run:text("run_id"), conversation:text("conversation_id"), text:text("text") };
    let store = crate::teams::store::Store::open(&config.state).unwrap();
    let program = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/hgs");
    assert!(program.exists(), "Build the private worktree's hgs before the reviewed doorbell test");
    native::Native::fixture_program(&program, || match native.ring(&store,&ring) {
        Rung::Submitted => serde_json::json!({"state":"submitted"}),
        Rung::Refused(reason) => serde_json::json!({"state":"refused","reason":reason}),
        Rung::NotStarted(reason) => serde_json::json!({"state":"not_started","reason":reason}),
        Rung::Uncertain(reason) => serde_json::json!({"state":"uncertain","reason":reason}),
    })
}
#[cfg(test)]
mod tests;
pub(super) use clear::Finished;
pub(super) use ledger::Ledger;
#[allow(unused_imports)] // The tick of package 8 wires the production effects.
pub(super) use native::Native;
#[allow(unused_imports)] // The availability port of package 8 implements Sessions.
pub(super) use ports::{Answer, ClearRequest, Effects, Presence, Ring, Rung, Sessions};

use crate::teams::{
    ops::{self, Actor, Operation, Request},
    store::Store,
};
use ports::Result;
use zerus_team_plan::{
    model::{Member, MemberState, Team, TeamStatus},
    problem::code,
};

pub(super) struct Timers {
    epoch: String,
    memory: fire::Memory,
    keys: clear::Sweep,
    late: late::Sweep,
}
impl Timers {
    /// `epoch` names this service turn; reservations of another are uncertain.
    pub fn new(epoch: &str) -> Self {
        Self {
            epoch: epoch.into(),
            memory: Default::default(),
            keys: Default::default(),
            late: Default::default(),
        }
    }
    /// One live team: its timers, then the lead's doorbell.
    pub fn team(
        &mut self,
        store: &mut Store,
        ledger: &Ledger,
        sessions: &mut impl Sessions,
        effects: &mut impl Effects,
        id: &str,
        now: i64,
    ) -> Result<()> {
        store.refresh()?;
        let team = store.team(id)?.filter(|t| t.status == TeamStatus::Running);
        let Some(team) = team else {
            // A pause or a stop restarts every count the service keeps.
            self.memory.forget(id);
            return Ok(());
        };
        let roster = roster(store, &team)?;
        let mut lead = None;
        let mut seen = Vec::new();
        for m in roster
            .iter()
            .filter(|m| m.session.is_some() && m.state == MemberState::Active)
        {
            let presence = sessions.presence(&team, m);
            if m.id == team.lead {
                lead = presence.clone();
            }
            seen.push((m.id.clone(), presence));
        }
        self.memory.members(&team.id, &seen, now);
        let quiet = self.memory.lead(&team.id, lead.as_ref(), now);
        fire::step(store, &mut self.memory, &team, quiet, now)?;
        let Some(member) = roster.iter().find(|m| m.id == team.lead) else {
            return Ok(());
        };
        doorbell::ring(
            store,
            ledger,
            effects,
            &self.epoch,
            &team,
            member,
            lead.as_ref(),
            now,
        )?;
        Ok(())
    }
    /// One live team's member doorbells for the lead's answers, beside the
    /// lead's doorbell in the same tick.
    pub fn answers(
        &mut self,
        store: &mut Store,
        ledger: &Ledger,
        sessions: &mut impl Sessions,
        effects: &mut impl Effects,
        id: &str,
        now: i64,
    ) -> Result<()> {
        store.refresh()?;
        let team = store.team(id)?.filter(|t| t.status == TeamStatus::Running);
        let Some(team) = team else {
            return Ok(());
        };
        let roster = roster(store, &team)?;
        answers::ring(
            store,
            ledger,
            sessions,
            effects,
            &self.epoch,
            &team,
            &roster,
            now,
        )
    }
    /// Once per pass: one page of a terminal team's task keys and one page
    /// of member launches that completed after a stop or a removal. In the
    /// worst case a pass runs sixteen team-clear children of up to 30
    /// seconds each and four launch reads of up to 15 seconds each.
    pub fn terminal(
        &mut self,
        store: &mut Store,
        ledger: &mut Ledger,
        effects: &mut impl Effects,
        now: i64,
    ) -> Result<()> {
        store.refresh()?;
        // Each page is independent: a fault in one never starves the other.
        let keys = clear::step(store, effects, &mut self.keys, now);
        let late = late::step(store, ledger, effects, &mut self.late, now);
        keys.and(late)
    }
    /// The teams whose key sweep reached its end since the last call.
    pub fn finished_sweeps(&mut self) -> Vec<Finished> {
        self.keys.take_finished()
    }
}
/// The roster within the caps' bound, read in pages.
fn roster(store: &Store, team: &Team) -> Result<Vec<Member>> {
    let mut members = Vec::new();
    let mut cursor = None;
    for _ in 0..2 {
        let page = store.members(&team.id, 200, cursor.as_ref())?;
        members.extend(page.rows);
        cursor = page.next;
        if cursor.is_none() {
            return Ok(members);
        }
    }
    Err(zerus_team_plan::Problem::new(
        code::OVER_CAP,
        "The roster exceeds its bound.",
    ))
}
/// What one service write came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recorded {
    Done,
    /// Proved by the request's saved refusal: the condition passed or the
    /// fact exists. Final for this key.
    Refused,
    /// Another writer came first, or the write's fate is unproved (a storage
    /// failure carries the same code as a refusal): the next pass tries again,
    /// and the uniqueness keys make a repeat harmless.
    Retry,
}
/// Record one service fact with a fresh revision.
fn record(store: &mut Store, team: &Team, operation: Operation, now: i64) -> Result<Recorded> {
    let Some(current) = store.team(&team.id)? else {
        return Ok(Recorded::Retry);
    };
    let actor = Actor::Service {
        person: team.owner.clone(),
        team: team.id.clone(),
    };
    let request = Request {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: team.id.clone(),
        expected_revision: current.revision,
        operation,
    };
    match ops::apply(store, &actor, &request, now) {
        Ok(_) => Ok(Recorded::Done),
        // Nothing was saved: a busy store, or a failed write rolled back.
        Err(p)
            if p.code == code::STALE_REVISION
                || p.code == code::STORE_BUSY
                || p.code == code::STORAGE_FAILED =>
        {
            Ok(Recorded::Retry)
        }
        Err(p) if p.code == code::INVALID_STATE || p.code == code::DUPLICATE => {
            let proved = store.saved_refusal(&team.id, &request.request_id);
            Ok(if proved.unwrap_or(false) {
                Recorded::Refused
            } else {
                Recorded::Retry
            })
        }
        Err(p) => Err(p),
    }
}
