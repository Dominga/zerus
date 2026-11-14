//! The service's own records, written by the actor's real passes, for other
//! components' tests (the plan: a test row of another component's value
//! comes from that component). Only the port outputs are replaced: the
//! team-cancel child's reply and the terminal timers' result.
use super::super::{
    cancel::{Answer, Fact, Pending},
    native::Native,
    ports::{DeliveryResult, Ports},
    Runtime,
};
use super::*;
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
};

struct Failing {
    native: Native,
    cutting_over: String,
    terminal: String,
}
impl Ports for Failing {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
        Ok(())
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        Ok(false)
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
        panic!("No hand-out")
    }
    fn cancels(&mut self, store: &Store, team: &str, after: u32) -> Result<Vec<Pending>> {
        Native::pending_cancels(store, team, after)
    }
    /// The real cancel step and admission; the child answers that the task
    /// is cutting over, with this reason, so no final answer is recorded.
    fn cancel(&mut self, team: &Team, pending: &Pending, original: &str) -> Result<Answer> {
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
            *s.borrow_mut() = Some(
                json!({"schema":1,"team":team.id,"node":pending.node,"state":"cutting_over","error":self.cutting_over,"attempt":pending.attempt,"attempt_id":pending.attempt_id})
                    .to_string()
                    .into_bytes(),
            )
        });
        self.native.run_cancel(team, pending, original)
    }
    fn record_cancel(
        &mut self,
        store: &mut Store,
        team: &Team,
        fact: &Fact,
        now: i64,
    ) -> Result<()> {
        Native::save_cancel(store, team, fact, now)
    }
    fn terminal_timers(
        &mut self,
        _: &mut super::super::auxiliary::State,
        _: &mut Store,
        _: i64,
    ) -> Result<()> {
        Err(zerus_team_plan::Problem::new(
            zerus_team_plan::problem::code::STORAGE_FAILED,
            &self.terminal,
        ))
    }
}

/// `passes` service passes over `state`, an hour apart from `start`, each
/// past every backoff: each pending cancel of a stopped or archived team
/// gets no final answer (its child says `cutting_over`), and each terminal
/// page fails with `terminal`. The actor writes their service items. The
/// team names the computer that runs them. Returns the last pass's time.
pub(crate) fn failing_service_passes(
    state: &Path,
    team: &str,
    cutting_over: &str,
    terminal: &str,
    passes: i64,
    start: i64,
) -> i64 {
    let mut store = Store::open(state).unwrap();
    let machine = store.team(team).unwrap().unwrap().home;
    let native = Native {
        config: crate::config::Config {
            home: String::new(),
            dir: state.join("config"),
            state: state.to_owned(),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        },
        machine: machine.clone(),
        epoch: "fixture".into(),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let mut runtime = Runtime::open(state, &machine).unwrap();
    let mut port = Failing {
        native,
        cutting_over: cutting_over.into(),
        terminal: terminal.into(),
    };
    let mut now = start;
    for pass in 0..passes {
        now = start + pass * 3_600_000;
        runtime
            .page(&mut store, &mut port, &[], now, false)
            .unwrap();
    }
    now
}
