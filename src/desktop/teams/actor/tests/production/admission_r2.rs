//! The reviewer's real admission-window probes, now asserted regressions.
use super::*;

fn submitted(id: &str) {
    crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
        *s.borrow_mut() = Some(
            serde_json::json!({"schema":1,"id":id,"state":"submitted","error":""})
                .to_string()
                .into_bytes(),
        )
    });
}

/// The person holds the node after HandoutIntent reserved its effect and
/// before the spawn. The production admission decides.
struct HoldAtDelivery {
    native: native::Native,
    calls: usize,
    mode: &'static str,
    dep: u32,
    admissions: std::rc::Rc<std::cell::Cell<usize>>,
}
impl HoldAtDelivery {
    fn apply_change(&self, t: &Team, n: &Node) {
        if self.mode == "state" {
            let mut store = Store::open(&self.native.config.state).unwrap();
            let old = store.task_observation(&t.id, n.id).unwrap().unwrap();
            super::super::super::write(
                &mut store,
                &t.id,
                Operation::ObserveTask {
                    node: n.id,
                    observation: ops::TaskObservation {
                        revision: "after-intent".into(),
                        previous_revision: old.revision.clone(),
                        stage: ops::TaskStage::InProgress,
                        ..old
                    },
                },
                100,
            )
            .unwrap();
        }
        let operation = self.change(n);
        if let Some(operation) = operation {
            let mut store = Store::open(&self.native.config.state).unwrap();
            let current = store.team(&t.id).unwrap().unwrap();
            let r = ops::apply(
                &mut store,
                &person(),
                &crate::teams::ops::tests::request(current.revision, operation),
                100,
            );
            r.unwrap();
        }
    }

    fn change(&self, n: &Node) -> Option<Operation> {
        match self.mode {
            "hold" => Some(Operation::Hold { node: n.id }),
            "new hard dependency" => {
                let mut op = crate::teams::ops::tests::route(n.id);
                if let Operation::Route { deps, .. } = &mut op {
                    deps.push(Dep {
                        node: self.dep,
                        kind: DepKind::Hard,
                    });
                }
                Some(op)
            }
            "reassign" => Some(Operation::Reassign {
                node: n.id,
                member: "m2".into(),
                reason: "Change the executor".into(),
            }),
            "cancel" => Some(Operation::Cancel {
                node: n.id,
                reason: "Cancel before the spawn".into(),
            }),
            "assignee" => {
                let mut op = crate::teams::ops::tests::route(n.id);
                if let Operation::Route { assignee, .. } = &mut op {
                    *assignee = Some("m2".into());
                }
                Some(op)
            }
            "role" => {
                let mut op = crate::teams::ops::tests::route(n.id);
                if let Operation::Route { role, pipeline, .. } = &mut op {
                    *role = "reviewer".into();
                    *pipeline = "reviewer-work".into();
                }
                Some(op)
            }
            _ => None,
        }
    }
}
impl ports::Ports for HoldAtDelivery {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> ports::Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> ports::Result<()> {
        Ok(())
    }
    fn free(&mut self, _: &Team, m: &Member) -> ports::Result<bool> {
        Ok(m.id == "m1")
    }
    fn deliver(&mut self, t: &Team, n: &Node, m: &Member, id: &str) -> ports::DeliveryResult {
        self.calls += 1;
        if self.mode.starts_with("fleet ") {
            super::fleet::change(&self.native, self.mode);
        }
        self.apply_change(t, n);
        let held = if self.mode == "busy store" && self.calls == 1 {
            let db =
                rusqlite::Connection::open(self.native.config.state.join("teams/teams-v1.sqlite3"))
                    .unwrap();
            db.execute_batch("BEGIN IMMEDIATE").unwrap();
            Some(db)
        } else {
            None
        };
        let count = self.admissions.clone();
        crate::desktop::original_child::AFTER_FINAL_CHECK
            .with(|s| *s.borrow_mut() = Some(Box::new(move || count.set(count.get() + 1))));
        submitted(id);
        let started = std::time::Instant::now();
        let reply = ports::Ports::deliver(&mut self.native, t, n, m, id);
        let elapsed = started.elapsed();
        crate::desktop::original_child::AFTER_FINAL_CHECK.with(|s| s.borrow_mut().take());
        drop(held);
        if self.mode == "busy store" {
            assert!(
                elapsed < std::time::Duration::from_secs(1),
                "busy admission held the spawn turn for {elapsed:?}"
            );
        }
        reply
    }
}
#[test]
fn review08_r2_hold_between_handout_intent_and_spawn() {
    for mode in [
        "control",
        "hold",
        "new hard dependency",
        "reassign",
        "cancel",
        "assignee",
        "role",
        "fleet busy",
        "fleet pending",
        "fleet background",
        "fleet unknown",
        "fleet epoch",
        "fleet run",
    ] {
        window(mode);
    }
}
#[test]
fn a_task_observed_active_after_intent_prevents_the_spawn() {
    window("state");
}
fn window(mode: &'static str) {
    let mut h = Harness::empty();
    let mut create = crate::teams::ops::tests::create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle.pipelines[0].stages.clear();
        let mut reviewer = bundle.pipelines[0].clone();
        reviewer.id = "reviewer-work".into();
        reviewer.implementer = "reviewer".into();
        bundle.pipelines.push(reviewer);
    }
    h.run(&person(), create).unwrap();
    h.bind("m1");
    h.bind("m2");
    h.bind("lead");
    let node = h.add(&person(), 1);
    let dep = h.add(&person(), 2);
    // Only node 1 is offered: node 2 is held from the start.
    h.run(&person(), Operation::Hold { node: dep }).unwrap();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = HoldAtDelivery {
        native: native(&h),
        calls: 0,
        mode,
        dep,
        admissions: Default::default(),
    };
    a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
    let effects = h.store.effects(TEAM, 200, None).unwrap().rows;
    let n = h.store.node(TEAM, node).unwrap().unwrap();
    assert_eq!(p.calls, 1, "{mode}");
    assert_eq!(p.admissions.get(), usize::from(mode == "control"), "{mode}");
    assert_eq!(effects.len(), 1, "{mode}");
    if mode == "control" {
        assert_eq!(effects[0].state, "submitted");
    } else {
        assert_eq!(
            effects[0].outcome.as_ref().unwrap()["status"],
            "refused",
            "{mode}: {:?}",
            effects[0]
        );
    }
    assert!(n.assignee.as_deref() != Some("m1"), "{mode}");
}

#[test]
fn a_busy_admission_retries_after_restart_without_a_refusal_or_a_second_original() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let mut p = HoldAtDelivery {
        native: native(&h),
        calls: 0,
        mode: "busy store",
        dep: 0,
        admissions: Default::default(),
    };
    let machine = p.native.machine.clone();
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
    let effect = h.store.effects(TEAM, 1, None).unwrap().rows.remove(0);
    assert!(
        effect.outcome.is_none(),
        "store busy became a durable refusal"
    );
    assert_eq!(effect.state, "reserved");
    assert!(h.node(node).assignee.is_none());
    drop(a);
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 101, false).unwrap();
    assert_eq!(p.calls, 1);
    assert_eq!(p.admissions.get(), 0);
    a.tick(&mut h.store, &mut p, TEAM, 60_100, false).unwrap();
    assert_eq!(p.calls, 2);
    assert_eq!(p.admissions.get(), 1);
    let effects = h.store.effects(TEAM, 16, None).unwrap().rows;
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].request_id, effect.request_id);
    assert_eq!(effects[0].state, "submitted");
}

#[test]
fn a_deferred_original_lost_after_arming_is_never_started_again() {
    for point in ["retry_armed", "effect", "outcome"] {
        let mut h = Harness::new();
        h.bind("m1");
        h.add(&person(), 1);
        let mut p = HoldAtDelivery {
            native: native(&h),
            calls: 0,
            mode: "busy store",
            dep: 0,
            admissions: Default::default(),
        };
        let machine = p.native.machine.clone();
        let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
        a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
        super::super::super::delivery::CRASH.with(|s| s.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.tick(
                &mut h.store,
                &mut p,
                TEAM,
                60_100,
                false
            )))
            .is_err(),
            "{point}"
        );
        let calls = p.calls;
        let admissions = p.admissions.get();
        drop(a);
        let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
        a.tick(&mut h.store, &mut p, TEAM, 180_100, false).unwrap();
        a.tick(&mut h.store, &mut p, TEAM, 360_100, false).unwrap();
        assert_eq!(p.calls, calls, "{point}");
        assert_eq!(p.admissions.get(), admissions, "{point}");
        assert!(admissions <= 1);
    }
}

#[test]
fn finish_admission_requires_the_exact_ready_acceptance() {
    finish_window("accept");
}
#[test]
fn finish_admission_requires_the_integration_fact() {
    finish_window("integration");
}
#[test]
fn finish_admission_requires_the_frozen_task_binding() {
    finish_window("task");
}
fn change_finish(h: &Harness, frozen: &mut Node, decision: u64, mode: &str) {
    match mode {
        "accept" => {
            h.store
                .db
                .as_ref()
                .unwrap()
                .execute(
                    "UPDATE decisions SET state='awaiting_owner' WHERE seq=?1",
                    [decision],
                )
                .unwrap();
        }
        "integration" => {
            h.store
                .db
                .as_ref()
                .unwrap()
                .execute(
                    "UPDATE nodes SET integrated=0 WHERE team=?1 AND id=?2",
                    rusqlite::params![TEAM, frozen.id],
                )
                .unwrap();
        }
        "task" => {
            frozen.task = Some(crate::teams::ops::tests::task(999));
        }
        _ => unreachable!(),
    }
}
fn finish_window(mode: &str) {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let decision = h.accept_decision(node);
    h.run(
        &service(),
        Operation::RecordIntegration {
            node,
            decision,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let mut frozen = h.node(node);
    let permit = h
        .store
        .pending_accepts(TEAM, 16, None)
        .unwrap()
        .rows
        .remove(0);
    change_finish(&h, &mut frozen, decision, mode);
    let mut n = native(&h);
    let admitted = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = admitted.clone();
    crate::desktop::original_child::AFTER_FINAL_CHECK
        .with(|s| *s.borrow_mut() = Some(Box::new(move || seen.set(true))));
    crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
        *s.borrow_mut() = Some(
            serde_json::json!({"schema":1,"team":TEAM,"node":node,"state":"done","error":""})
                .to_string()
                .into_bytes(),
        )
    });
    let reply = ports::Ports::finish(
        &mut n,
        &team,
        &frozen,
        &permit,
        &uuid::Uuid::new_v4().to_string(),
    );
    crate::desktop::original_child::AFTER_FINAL_CHECK.with(|s| s.borrow_mut().take());
    assert!(reply.is_err(), "{mode}: {reply:?}");
    assert!(!admitted.get(), "{mode}");
}

#[test]
fn stop_and_preview_are_rechecked_after_the_admission_returns() {
    use crate::desktop::teams::child::{self, Ended};
    use std::sync::atomic::Ordering;
    for stop in [true, false] {
        let h = Harness::new();
        let n = native(&h);
        child::FIXTURE_EXIT.with(|s| *s.borrow_mut() = Some(b"never offered".to_vec()));
        let ended = child::run_guarded(
            &n.config,
            &[],
            None,
            None,
            false,
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            &n.stop,
            || {
                if stop {
                    n.stop.store(true, Ordering::Release)
                } else {
                    std::fs::remove_file(n.config.state.join("teams/preview")).unwrap();
                }
                Ok(())
            },
        );
        assert!(matches!(ended, Ended::NotStarted), "stop={stop}");
    }
}
