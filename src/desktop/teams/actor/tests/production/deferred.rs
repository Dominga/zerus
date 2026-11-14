use super::*;
use std::{cell::Cell, rc::Rc};

fn submitted(id: &str) {
    crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
        *s.borrow_mut() = Some(
            serde_json::json!({"schema":1,"id":id,"state":"submitted","error":""})
                .to_string()
                .into_bytes(),
        )
    });
}
fn as_person(state: &Path, op: Operation) -> String {
    let mut store = Store::open(state).unwrap();
    let rev = store.team(TEAM).unwrap().unwrap().revision;
    match ops::apply(
        &mut store,
        &person(),
        &crate::teams::ops::tests::request(rev, op),
        100,
    ) {
        Ok(r) => format!("ok rev {}", r.revision),
        Err(e) => format!("refused {}: {}", e.code, e.message),
    }
}
type Change = Box<dyn FnMut(usize, &native::Native, &Team, &Node) -> String>;
struct W {
    native: native::Native,
    calls: usize,
    free_member: &'static str,
    admissions: Rc<Cell<usize>>,
    busy_on: Vec<usize>,
    change: Change,
    log: Vec<String>,
}
impl ports::Ports for W {
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
        Ok(m.id == self.free_member)
    }
    fn deliver(&mut self, t: &Team, n: &Node, m: &Member, id: &str) -> ports::DeliveryResult {
        self.calls += 1;
        let note = (self.change)(self.calls, &self.native, t, n);
        let held = if self.busy_on.contains(&self.calls) {
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
        crate::desktop::original_child::AFTER_FINAL_CHECK.with(|s| s.borrow_mut().take());
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| s.borrow_mut().take());
        drop(held);
        self.log.push(format!(
            "call {} node {} member {} change [{note}] -> {:?} in {:?}",
            self.calls,
            n.id,
            m.id,
            reply,
            started.elapsed()
        ));
        reply
    }
}
fn w(h: &Harness, busy_on: Vec<usize>, change: Change) -> W {
    W {
        native: native(h),
        calls: 0,
        free_member: "m1",
        admissions: Default::default(),
        busy_on,
        change,
        log: vec![],
    }
}
fn none() -> Change {
    Box::new(|_, _, _, _| String::new())
}

#[test]
fn a_busy_not_started_delivery_is_refused_before_terminal_cancellation() {
    for archived in [false, true] {
        let mut h = Harness::new();
        h.bind("m1");
        let node = h.add(&person(), 1);
        let machine = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
        let mut p = w(&h, vec![1], none());
        a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
        assert_eq!(p.calls, 1);
        let row = a
            .ledger
            .prefix_page(TEAM, "delivery:", "")
            .unwrap()
            .remove(0);
        assert_eq!(row.data["not_started"], true);
        assert_eq!(h.store.open_effects(TEAM, 200, None).unwrap().rows.len(), 1);
        h.run(&person(), Operation::Stop).unwrap();
        if archived {
            h.run(&person(), Operation::Archive).unwrap();
        }
        drop(a);
        let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
        // The real terminal page must settle the proof before asking for
        // cancel work. No live-team tick is needed after Stop.
        a.terminal_cancels(&mut h.store, &mut p, 101).unwrap();
        assert_eq!(p.calls, 1);
        assert!(h
            .store
            .open_effects(TEAM, 200, None)
            .unwrap()
            .rows
            .is_empty());
        assert!(a
            .ledger
            .prefix_page(TEAM, "delivery:", "")
            .unwrap()
            .is_empty());
        let effect = h.store.effects(TEAM, 200, None).unwrap().rows.remove(0);
        assert_eq!(effect.state, "refused");
        assert!(h.node(node).assignee.is_none());
        a.tick(&mut h.store, &mut p, TEAM, 60_100, false).unwrap();
        assert_eq!(p.calls, 1);
    }
}

#[test]
fn unknown_occupancy_releases_its_member_after_five_proven_nonstarts() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = w(&h, vec![], none());
    native::TEST_INVENTORY.with(|s| {
        *s.borrow_mut() = Some(serde_json::json!({
            "node_id":machine,"organization":{"projects":[{"id":"project-1"}]}
        }))
    });
    let mut at = Vec::new();
    for now in (100..=900_100).step_by(60_000) {
        let before = p.calls;
        a.tick(&mut h.store, &mut p, TEAM, now, false).unwrap();
        if p.calls != before {
            at.push(now);
        }
    }
    native::TEST_INVENTORY.with(|s| *s.borrow_mut() = None);
    assert_eq!(at, [100, 60_100, 180_100, 420_100, 900_100]);
    assert_eq!(p.calls, 5);
    assert!(
        p.log.iter().all(|line| line.contains("NotStarted")),
        "the actual delivery command bypassed occupancy: {:?}",
        p.log
    );
    assert!(h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    assert_eq!(
        h.store.effects(TEAM, 200, None).unwrap().rows[0].state,
        "refused"
    );
    assert!(h.node(node).assignee.is_none());
    p.free_member = "m2";
    a.tick(&mut h.store, &mut p, TEAM, 960_100, false).unwrap();
    let effects = h.store.effects(TEAM, 200, None).unwrap().rows;
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[1].member, "m2");
    assert_eq!(effects[1].state, "submitted");
    assert_eq!(
        p.admissions.get(),
        6,
        "five occupancy readers and one delivery"
    );
}

#[test]
fn production_reopen_rechecks_member_and_launch_identity_and_admits_the_control() {
    for mode in ["control", "pause member", "stop member", "fleet launch"] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(
            &lead,
            Operation::Return {
                node,
                reason: "Revise".into(),
            },
        )
        .unwrap();
        let machine = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
        let state = h.temp.path().to_owned();
        let change: Change = Box::new(move |_, native, _, _| {
            let op = match mode {
                "pause member" => Some(Operation::PauseMember {
                    member: "m1".into(),
                }),
                "stop member" => Some(Operation::StopMember {
                    member: "m1".into(),
                }),
                "fleet launch" => {
                    fleet::change(native, mode);
                    None
                }
                _ => None,
            };
            if let Some(op) = op {
                let result = as_person(&state, op);
                assert!(result.starts_with("ok"), "{result}");
            }
            String::new()
        });
        let mut p = w(&h, vec![], change);
        a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
        assert_eq!(p.calls, 1, "{mode}: {:?}", p.log);
        let effect = h
            .store
            .effects(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|e| e.kind == "reopen")
            .unwrap();
        assert_eq!(
            effect.state,
            if mode == "control" {
                "submitted"
            } else {
                "refused"
            },
            "{mode}"
        );
        assert_eq!(p.admissions.get(), usize::from(mode == "control"), "{mode}");
    }
}

#[test]
fn a_busy_delivery_waits_through_pause_and_resumes_only_its_original_effect() {
    let mut h = Harness::new();
    h.bind("m1");
    h.add(&person(), 1);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = w(&h, vec![1], none());
    a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
    let id = h.store.open_effects(TEAM, 200, None).unwrap().rows[0]
        .request_id
        .clone();
    h.run(&person(), Operation::Pause).unwrap();
    for now in [60_100, 600_100] {
        a.tick(&mut h.store, &mut p, TEAM, now, false).unwrap();
        assert_eq!(p.calls, 1);
        assert_eq!(
            h.store.open_effects(TEAM, 200, None).unwrap().rows[0].request_id,
            id
        );
    }
    h.run(&person(), Operation::Resume).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 600_200, false).unwrap();
    let effects = h.store.effects(TEAM, 200, None).unwrap().rows;
    assert_eq!(p.calls, 2);
    assert_eq!(
        effects.len(),
        1,
        "Pause must not replace the reserved hand-out"
    );
    assert_eq!(effects[0].request_id, id);
    assert_eq!(effects[0].state, "submitted");
    assert_eq!(p.admissions.get(), 1);
}
#[test]
fn the_production_merge_clock_uses_wall_time_instead_of_the_page_clock() {
    let h = Harness::new();
    let n = native(&h);
    let before = chrono::Utc::now().timestamp_millis();
    let value = ports::Ports::clock_ms(&n, 5);
    let after = chrono::Utc::now().timestamp_millis();
    assert!(
        (before..=after).contains(&value),
        "{value} is a stale page clock"
    );
}

#[test]
fn a_bad_terminal_delivery_row_does_not_stop_the_next_teams_settlement() {
    let mut h = Harness::new();
    h.bind("m1");
    h.add(&person(), 1);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = w(&h, vec![1], none());
    a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    // TEAM_B sorts before TEAM. Its malformed row is a fixture fault, not
    // evidence that any original ran or that the next team's work may stop.
    h.run_for(TEAM_B, &person(), create()).unwrap();
    h.run_for(
        TEAM_B,
        &person(),
        Operation::AddTask {
            task: task(2),
            role: None,
            pipeline: None,
        },
    )
    .unwrap();
    h.run_for(TEAM_B, &person(), Operation::Stop).unwrap();
    a.ledger
        .reserve(TEAM_B, "delivery:malformed", serde_json::json!({}))
        .unwrap();
    assert_eq!(h.store.keyed_teams("", 16).unwrap(), [TEAM_B, TEAM]);
    assert!(a.terminal_cancels(&mut h.store, &mut p, 101).is_err());
    assert_eq!(p.calls, 1);
    assert!(h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    assert_eq!(
        h.store.effects(TEAM, 200, None).unwrap().rows[0].state,
        "refused"
    );
}

fn conflict_at_reopen(state: &Path, node: u32) {
    let mut store = Store::open(state).unwrap();
    let mut o = store.task_observation(TEAM, node).unwrap().unwrap();
    o.previous_revision = o.revision.clone();
    o.revision = uuid::Uuid::new_v4().to_string();
    o.attempt = o.attempt.max(2);
    o.attempt_id.clear();
    o.attempt_conflict = true;
    let revision = store.team(TEAM).unwrap().unwrap().revision;
    ops::apply(
        &mut store,
        &service(),
        &crate::teams::ops::tests::request(
            revision,
            Operation::ObserveTask {
                node,
                observation: o,
            },
        ),
        100,
    )
    .unwrap();
}
#[test]
fn reopens_wait_for_the_reviewed_attempt_before_reserving_an_effect() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    conflict_at_reopen(h.temp.path(), node);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = w(&h, vec![], none());
    let team = h.store.team(TEAM).unwrap().unwrap();
    a.reopens(&mut h.store, &mut p, &team, 100).unwrap();
    assert_eq!(p.calls, 0);
    assert!(!h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .any(|e| e.kind == "reopen"));
}
#[test]
fn a_reopen_rechecks_the_reviewed_attempt_at_the_spawn() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let state = h.temp.path().to_owned();
    let mut p = w(
        &h,
        vec![],
        Box::new(move |_, _, _, n| {
            conflict_at_reopen(&state, n.id);
            String::new()
        }),
    );
    a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
    assert_eq!(p.calls, 1);
    assert_eq!(p.admissions.get(), 0);
    let e = h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|e| e.kind == "reopen")
        .unwrap();
    assert_eq!(e.state, "refused");
}
