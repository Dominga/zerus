//! Real replica/task writers, native reply decoder, actor retries and facts.
use super::super::{
    cancel::{Answer, Fact, Pending},
    native::Native,
    ports::{DeliveryResult, Ports},
    Runtime,
};
use super::*;
use crate::{config::Config, desktop::original_child::Ended};

struct Commands<'a> {
    command: &'a mut dyn FnMut() -> serde_json::Value,
    calls: usize,
}
impl Ports for Commands<'_> {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        unreachable!()
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
        unreachable!()
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        Ok(false)
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
        unreachable!()
    }
    fn cancels(&mut self, store: &Store, team: &str, after: u32) -> Result<Vec<Pending>> {
        Native::pending_cancels(store, team, after)
    }
    fn cancel(&mut self, team: &Team, pending: &Pending, _: &str) -> Result<Answer> {
        self.calls += 1;
        let out = (self.command)();
        eprintln!("WP167 real cancel call {}: {out}", self.calls);
        let ended = Ended::Exited {
            code: if out["ok"] == true { 0 } else { 1 },
            output: out["reply"].to_string().into_bytes(),
        };
        super::super::native::cancel_answer(
            ended,
            &json!({"schema":1,"team":team.id,"node":pending.node}),
        )
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
}
fn hold(config: &Config, on: bool) {
    let tasks = zerus_replica::ProjectStore::open(&config.state.join("replica"), "example-project")
        .unwrap();
    let mut project = tasks.project().unwrap();
    project
        .set_cutover_hold(on, &zerus_replica::Actor::Person, 1)
        .unwrap();
    tasks.save_project(&project).unwrap();
}
#[test]
fn a_failed_legacy_cancel_recovers_at_its_next_backoff_without_person_retry() {
    crate::tasks::with_legacy_cancel(|config, store, command| {
        let team = store.team(TEAM).unwrap().unwrap();
        let mut actor = Runtime::open(&config.state, &team.home).unwrap();
        let mut ports = Commands { command, calls: 0 };
        hold(config, true);
        for now in [100, 60_100, 180_100] {
            actor.cancels(store, &mut ports, &team, now).unwrap();
        }
        assert_eq!(ports.calls, 3);
        let cards = store.cards(TEAM, 200, None).unwrap().rows;
        assert_eq!(
            cards
                .iter()
                .filter(|c| c.kind == "team_actor_blocked" && c.state == "open")
                .count(),
            1
        );
        hold(config, false);
        actor.cancels(store, &mut ports, &team, 420_099).unwrap();
        assert_eq!(ports.calls, 3, "retry deadline is preserved");
        actor.cancels(store, &mut ports, &team, 420_100).unwrap();
        assert_eq!(ports.calls, 4);
        assert!(store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
        actor.cancels(store, &mut ports, &team, 420_101).unwrap();
        assert_eq!(ports.calls, 4, "settled cancel is not repeated");
        let cards = store.cards(TEAM, 200, None).unwrap().rows;
        assert!(cards
            .iter()
            .any(|c| c.kind == "team_actor_blocked" && c.state == "closed"));
    });
}
