//! A retired stage's own cancel, the whole way through the service: the
//! store's pending cancel, the actor's cancel step with its exact-row
//! admission and the team-cancel child, the released answer recorded as the
//! stage's end, then the released stage's task key cleared once by the
//! actor's clear step with its own admission and team-clear child. Only the
//! children's output is replaced, as the stop test (native_cancel_tick) does.
use super::super::{
    cancel::{Answer, Fact, Pending},
    native::Native,
    ports::{DeliveryResult, Ports, TaskReply},
    Runtime,
};
use super::*;
use crate::teams::{
    ops::{Actor, Operation, TaskStage},
    store::timers::KeyedTask,
};

struct Adapter {
    native: Native,
    calls: Vec<(&'static str, u32)>,
}
impl Ports for Adapter {
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
    fn cancel(&mut self, team: &Team, pending: &Pending, original: &str) -> Result<Answer> {
        self.calls.push(("cancel", pending.node));
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
            *s.borrow_mut() = Some(
                json!({"schema":1,"team":team.id,"node":pending.node,"state":"released","error":"","attempt":pending.attempt,"attempt_id":pending.attempt_id})
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
    fn clear_stage(&mut self, team: &Team, keyed: &KeyedTask, original: &str) -> Result<TaskReply> {
        self.calls.push(("clear", keyed.node));
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
            *s.borrow_mut() = Some(
                json!({"schema":1,"team":team.id,"node":keyed.node,"state":"cleared","error":""})
                    .to_string()
                    .into_bytes(),
            )
        });
        self.native.clear_stage(team, keyed, original)
    }
}

fn count(h: &Harness, sql: &str, node: u32) -> u32 {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(sql, rusqlite::params![TEAM, node], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_retired_stages_own_cancel_is_carried_out_by_the_service_before_its_key_is_cleared_once() {
    let mut h = Harness::empty();
    h.run(&person(), create()).unwrap();
    for member in ["lead", "m1", "m2", "m3"] {
        h.bind(member);
    }
    let parent = h.add(&person(), 1);
    h.submit(parent, "m1");
    h.report(parent, "m1");
    let stage = h.store.stage_tasks(TEAM, 1, None).unwrap().rows[0].node;
    let id = uuid::Uuid::new_v4().to_string();
    h.run(
        &service(),
        Operation::StageTaskIntent {
            node: stage,
            effect_request_id: id.clone(),
        },
    )
    .unwrap();
    h.run(
        &service(),
        Operation::StageTaskOutcome {
            effect_request_id: id.clone(),
            outcome: crate::teams::ops::StageTaskOutcome::Created {
                task: task(1000 + stage),
            },
        },
    )
    .unwrap();
    h.observe(stage, TaskStage::Todo);
    let lead = Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: h
            .store
            .member(TEAM, "lead")
            .unwrap()
            .unwrap()
            .session
            .unwrap(),
    };
    h.run(
        &lead,
        Operation::Return {
            node: parent,
            reason: "Fix it".into(),
        },
    )
    .unwrap();
    let waiting: Vec<_> = h
        .store
        .pending_cancels(TEAM, 200, 0)
        .unwrap()
        .into_iter()
        .filter(|p| p.node == stage)
        .collect();
    assert_eq!(waiting.len(), 1, "the retirement's own cancel waits");
    assert_eq!(waiting[0].task, Some(task(1000 + stage)));
    let decision = waiting[0].decision;
    assert!(
        h.store.released_stages(TEAM, 200, 0).unwrap().is_empty(),
        "the key waits for the cancel's answer"
    );
    let archive = "SELECT count(*) FROM node_archive WHERE team=?1 AND id=?2";
    assert_eq!(count(&h, archive, stage), 0);

    let native = super::production::native(&h);
    let machine = native.machine.clone();
    let mut runtime = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Adapter {
        native,
        calls: Vec::new(),
    };
    let rows = h.store.teams(16, None).unwrap().rows;
    for pass in 0..3 {
        runtime
            .page(&mut h.store, &mut p, &rows, 100 + pass * 60_000, false)
            .unwrap();
    }
    assert_eq!(
        p.calls,
        vec![("cancel", stage), ("clear", stage)],
        "one cancel by the actor, then one clear"
    );
    assert!(h
        .store
        .pending_cancels(TEAM, 200, 0)
        .unwrap()
        .iter()
        .all(|p| p.node != stage));
    let (recorded, state): (u64, String) = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT decision,state FROM cancel_outcomes WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, stage],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (recorded, state.as_str()),
        (decision, "released"),
        "the released answer is the retirement's own cancel's outcome"
    );
    assert_eq!(
        count(&h, archive, stage),
        1,
        "the stage ended with its answer"
    );
    assert_eq!(
        count(
            &h,
            "SELECT count(*) FROM key_clears WHERE team=?1 AND node=?2 AND state='cleared'",
            stage
        ),
        1,
        "the key clear is recorded"
    );
    assert!(h.store.released_stages(TEAM, 200, 0).unwrap().is_empty());

    drop(runtime);
    let mut runtime = Runtime::open(h.temp.path(), &machine).unwrap();
    let rows = h.store.teams(16, None).unwrap().rows;
    runtime
        .page(&mut h.store, &mut p, &rows, 400_000, false)
        .unwrap();
    assert_eq!(p.calls.len(), 2, "the key is cleared once");
}
