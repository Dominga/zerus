//! Real store pages and facts, with only the original child's result replaced.
use super::super::{
    cancel::{Answer, Fact, Pending},
    native::Native,
    ports::{DeliveryResult, Ports},
    Runtime,
};
use super::*;
use crate::teams::ops::{KeyClearState, Operation, TaskStage};

struct Adapter {
    native: Native,
    cancelled: Vec<u32>,
    cleared: Vec<u32>,
    unknown_fact: bool,
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
        self.cancelled.push(pending.node);
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| *s.borrow_mut() = Some(
            json!({"schema":1,"team":team.id,"node":pending.node,"state":"released","error":"","attempt":pending.attempt,"attempt_id":pending.attempt_id}).to_string().into_bytes()));
        self.native.run_cancel(team, pending, original)
    }
    fn record_cancel(
        &mut self,
        store: &mut Store,
        team: &Team,
        fact: &Fact,
        now: i64,
    ) -> Result<()> {
        Native::save_cancel(store, team, fact, now)?;
        if std::mem::take(&mut self.unknown_fact) {
            Err(zerus_team_plan::Problem::new(
                zerus_team_plan::problem::code::STORAGE_FAILED,
                "fixture lost the committed fact reply",
            ))
        } else {
            Ok(())
        }
    }
    fn terminal_timers(
        &mut self,
        _: &mut super::super::auxiliary::State,
        store: &mut Store,
        now: i64,
    ) -> Result<()> {
        let pending: Vec<u32> = store
            .pending_cancels(TEAM, 200, 0)?
            .into_iter()
            .map(|p| p.node)
            .collect();
        for keyed in store.keyed_tasks(TEAM, 0, 16)? {
            assert!(
                !pending.contains(&keyed.node),
                "key clear preceded its cancel"
            );
            super::super::write(
                store,
                TEAM,
                Operation::RecordKeyClear {
                    node: keyed.node,
                    state: KeyClearState::Cleared,
                    reason: String::new(),
                },
                now,
            )?;
            self.cleared.push(keyed.node);
        }
        Ok(())
    }
}
fn stopped_stages() -> Harness {
    let mut h = Harness::new();
    for (i, stage) in [
        TaskStage::Backlog,
        TaskStage::Todo,
        TaskStage::InProgress,
        TaskStage::InReview,
        TaskStage::Done,
    ]
    .into_iter()
    .enumerate()
    {
        let node = h.add(&person(), i as u32 + 1);
        h.observe(node, stage);
    }
    h.run(&person(), Operation::Stop).unwrap();
    h
}
#[test]
fn a_stop_cancels_each_unfinished_stage_before_any_key_is_cleared() {
    let mut h = stopped_stages();
    let expected: Vec<u32> = h
        .store
        .pending_cancels(TEAM, 200, 0)
        .unwrap()
        .into_iter()
        .filter(|p| p.task.is_some())
        .map(|p| p.node)
        .collect();
    assert!(!expected.is_empty());
    assert!(h
        .store
        .keyed_tasks(TEAM, 0, 200)
        .unwrap()
        .iter()
        .all(|k| !expected.contains(&k.node)));
    let native = super::production::native(&h);
    let mut runtime = Runtime::open(h.temp.path(), &native.machine).unwrap();
    let mut p = Adapter {
        native,
        cancelled: Vec::new(),
        cleared: Vec::new(),
        unknown_fact: false,
    };
    for pass in 0..3 {
        runtime
            .page(&mut h.store, &mut p, &[], 100 + pass * 60_000, false)
            .unwrap();
    }
    assert_eq!(p.cancelled, expected);
    assert!(h.store.pending_cancels(TEAM, 200, 0).unwrap().is_empty());
    assert!(expected.iter().all(|n| p.cleared.contains(n)));
    let calls = p.cancelled.clone();
    runtime
        .page(&mut h.store, &mut p, &[], 240_100, false)
        .unwrap();
    assert_eq!(p.cancelled, calls);
}

#[test]
fn a_stage_without_a_task_records_cancelled_without_invoking_the_native_adapter() {
    let (mut h, _, stage) = super::tasks::wanted();
    assert!(h.node(stage).task.is_none());
    h.run(
        &person(),
        Operation::Cancel {
            node: stage,
            reason: "No stage work remains".into(),
        },
    )
    .unwrap();
    let native = super::production::native(&h);
    let mut runtime = Runtime::open(h.temp.path(), &native.machine).unwrap();
    let mut p = Adapter {
        native,
        cancelled: Vec::new(),
        cleared: Vec::new(),
        unknown_fact: false,
    };
    let team = h.store.team(TEAM).unwrap().unwrap();
    runtime.cancels(&mut h.store, &mut p, &team, 100).unwrap();
    assert_eq!(h.node(stage).state, NodeState::Cancelled);
    assert!(p.cancelled.is_empty());
    assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
}

#[test]
fn storage_failed_replays_the_saved_cancel_fact_request_without_a_second_child() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    h.run(
        &person(),
        Operation::Cancel {
            node,
            reason: "Release the task".into(),
        },
    )
    .unwrap();
    let native = super::production::native(&h);
    let mut runtime = Runtime::open(h.temp.path(), &native.machine).unwrap();
    let mut p = Adapter {
        native,
        cancelled: Vec::new(),
        cleared: Vec::new(),
        unknown_fact: false,
    };
    h.store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER reject_fact BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END").unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    runtime.cancels(&mut h.store, &mut p, &team, 100).unwrap();
    assert_eq!(p.cancelled, vec![node]);
    let pending = h.store.pending_cancels(TEAM, 16, 0).unwrap().remove(0);
    let key = format!("cancel:{}", pending.decision);
    let row = runtime.ledger.get(TEAM, &key).unwrap().unwrap();
    let id = row.data["fact"]["request_id"].as_str().unwrap().to_owned();
    assert!(!h.store.request_refused(TEAM, &id).unwrap());
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_fact")
        .unwrap();
    runtime
        .cancels(&mut h.store, &mut p, &team, 60_100)
        .unwrap();
    assert_eq!(p.cancelled, vec![node]);
    assert_eq!(h.node(node).state, NodeState::Cancelled);
    assert!(runtime.ledger.get(TEAM, &key).unwrap().is_none());
    let saved: bool = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM requests WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        saved,
        "the fact used a new request instead of replaying its original"
    );
}

#[test]
fn a_lost_committed_cancel_fact_reply_replays_its_original_request_after_restart() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    h.run(
        &person(),
        Operation::Cancel {
            node,
            reason: "Release this task".into(),
        },
    )
    .unwrap();
    let native = super::production::native(&h);
    let mut runtime = Runtime::open(h.temp.path(), &native.machine).unwrap();
    let mut p = Adapter {
        native,
        cancelled: Vec::new(),
        cleared: Vec::new(),
        unknown_fact: true,
    };
    let team = h.store.team(TEAM).unwrap().unwrap();
    let decision = h.store.pending_cancels(TEAM, 16, 0).unwrap()[0].decision;
    runtime.cancels(&mut h.store, &mut p, &team, 100).unwrap();
    assert_eq!(p.cancelled, vec![node]);
    assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
    let key = format!("cancel:{decision}");
    let row = runtime.ledger.get(TEAM, &key).unwrap().unwrap();
    let id = row.data["fact"]["request_id"].as_str().unwrap().to_owned();
    assert!(!h.store.request_refused(TEAM, &id).unwrap());
    let saved: bool = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM requests WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        saved,
        "the unknown reply must retain the committed original request ID"
    );
    drop(runtime);
    let mut runtime = Runtime::open(h.temp.path(), &p.native.machine).unwrap();
    runtime
        .cancels(&mut h.store, &mut p, &team, 60_100)
        .unwrap();
    assert_eq!(p.cancelled, vec![node]);
    assert!(
        runtime.ledger.get(TEAM, &key).unwrap().is_none(),
        "the original saved success must replay even though its decision has settled"
    );
}
