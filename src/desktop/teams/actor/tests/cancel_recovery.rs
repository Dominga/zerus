//! Cancel receipts survive revision changes; one halted row owns only its retry.
use super::super::{
    cancel::{Answer, Attempt, Fact, Pending},
    native::Native,
    ports::{DeliveryResult, Ports, Result},
    Runtime,
};
use super::*;
use crate::teams::ops::{CancelState, HandoutOutcome, KeyClearState, Operation, TaskStage};
struct Recovery {
    native: Native,
    live: bool,
    pair: Attempt,
    loss: Option<bool>,
    only: Option<u32>,
    calls: Vec<(String, u32)>,
    facts: Vec<Fact>,
    reject_exit: bool,
    cleared: Vec<(String, u32)>,
    hold: bool,
    stale: bool,
    fail_observe: bool,
    settle_observation: bool,
    reads: usize,
    malformed: bool,
}
impl Ports for Recovery {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        unreachable!()
    }
    fn observe(&mut self, store: &mut Store, task: &TaskRef) -> Result<()> {
        self.reads += 1;
        if self.fail_observe {
            return Err(super::super::problem("The task cannot be observed yet."));
        }
        if self.settle_observation {
            assert_eq!(store.node(TEAM, 1)?.unwrap().task.as_ref(), Some(task));
            let mut o = store.task_observation(TEAM, 1)?.unwrap();
            o.previous_revision = o.revision.clone();
            o.revision = uuid::Uuid::new_v4().to_string();
            o.cancelled = true;
            super::super::write(
                store,
                TEAM,
                Operation::ObserveTask {
                    node: 1,
                    observation: o,
                },
                100,
            )?;
        }
        Ok(())
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        Ok(false)
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
        unreachable!()
    }
    fn cancels(&mut self, store: &Store, team: &str, after: u32) -> Result<Vec<Pending>> {
        Ok(Native::pending_cancels(store, team, after)?
            .into_iter()
            .filter(|p| self.only.is_none_or(|n| n == p.node))
            .collect())
    }
    fn cancel(&mut self, team: &Team, pending: &Pending, original: &str) -> Result<Answer> {
        self.calls.push((team.id.clone(), pending.node));
        let pair = if team.id == TEAM && pending.node == 1 {
            self.pair.clone()
        } else {
            Attempt {
                number: pending.attempt,
                id: pending.attempt_id.clone(),
            }
        };
        let state = if self.hold {
            "cutting_over"
        } else if self.stale {
            "stale"
        } else if self.live && !self.settle_observation {
            "released"
        } else {
            "cancelled"
        };
        crate::desktop::teams::child::FIXTURE_EXIT.with(|s| {
            *s.borrow_mut() = Some(
                json!({"schema":1,"team":team.id,"node":pending.node,"state":state,"error":if self.hold { "The project is held for cutover." } else if self.stale { "Observe this task before trying again." } else { "" },
                "attempt":pair.number,"attempt_id":pair.id})
                .to_string()
                .into_bytes(),
            )
        });
        if self.malformed {
            crate::desktop::teams::child::FIXTURE_EXIT
                .with(|s| *s.borrow_mut() = Some(b"{".to_vec()));
        }
        self.native.run_cancel(team, pending, original)
    }
    fn record_cancel(
        &mut self,
        store: &mut Store,
        team: &Team,
        fact: &Fact,
        now: i64,
    ) -> Result<()> {
        self.facts.push(fact.clone());
        let lose = self.loss == Some(fact.refused);
        if lose {
            self.loss = None;
            store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER lose_cancel_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'fixture lost receipt'); END").unwrap();
        }
        let mut attempted = fact.clone();
        if self.reject_exit && team.id == TEAM && fact.pending.node == 1 && fact.refused {
            attempted.reason.clear();
        }
        let result = Native::save_cancel(store, team, &attempted, now);
        if lose {
            store
                .db
                .as_ref()
                .unwrap()
                .execute_batch("DROP TRIGGER lose_cancel_receipt")
                .unwrap();
            assert_eq!(
                result.as_ref().unwrap_err().code,
                zerus_team_plan::problem::code::STORAGE_FAILED
            );
        }
        result
    }
    fn terminal_timers(
        &mut self,
        _: &mut super::super::auxiliary::State,
        store: &mut Store,
        now: i64,
    ) -> Result<()> {
        for id in store.keyed_teams("", 16)? {
            for key in store.keyed_tasks(&id, 0, 200)? {
                super::super::write(
                    store,
                    &id,
                    Operation::RecordKeyClear {
                        node: key.node,
                        state: KeyClearState::Cleared,
                        reason: String::new(),
                    },
                    now,
                )?;
                self.cleared.push((id.clone(), key.node));
            }
        }
        Ok(())
    }
}
fn case(status: &str, exit: bool) -> (Harness, Runtime, Recovery) {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.add(&person(), 2);
    let id = if status == "live" {
        String::new()
    } else {
        let id = h.intent(node, "m1");
        h.outcome(
            &id,
            HandoutOutcome::Submitted {
                receipt: "submitted".into(),
            },
        );
        id
    };
    if status == "live" {
        h.run(
            &person(),
            Operation::Cancel {
                node,
                reason: "Cancel".into(),
            },
        )
        .unwrap();
    } else {
        h.run(&person(), Operation::Stop).unwrap();
        if status == "archived" {
            h.run(&person(), Operation::Archive).unwrap();
        }
    }
    let native = super::production::native(&h);
    let runtime = Runtime::open(h.temp.path(), &native.machine).unwrap();
    let p = Recovery {
        native,
        live: status == "live",
        pair: if exit {
            Attempt {
                number: 9,
                id: "not-the-node".into(),
            }
        } else {
            Attempt {
                number: u64::from(status != "live"),
                id,
            }
        },
        loss: Some(exit),
        only: Some(node),
        calls: vec![],
        facts: vec![],
        reject_exit: false,
        cleared: vec![],
        hold: false,
        stale: false,
        fail_observe: false,
        settle_observation: false,
        reads: 0,
        malformed: false,
    };
    (h, runtime, p)
}
fn team_write(h: &mut Harness, live: bool) {
    if live {
        h.observe(2, TaskStage::Todo);
    } else {
        let c = h
            .store
            .pending_cancels(TEAM, 16, 0)
            .unwrap()
            .into_iter()
            .find(|c| c.node == 2)
            .unwrap();
        h.run(
            &service(),
            Operation::RecordCancel {
                node: c.node,
                decision: c.decision,
                state: CancelState::Refused,
                reason: "Sibling progress".into(),
                attempt: c.attempt,
                attempt_id: c.attempt_id,
            },
        )
        .unwrap();
    }
}
fn check_stale(status: &str, exit: bool) {
    let (mut h, mut a, mut p) = case(status, exit);
    let team = h.store.team(TEAM).unwrap().unwrap();
    a.cancels(&mut h.store, &mut p, &team, 100).unwrap();
    assert!(p.loss.is_none());
    let lost = p.facts.last().unwrap().request_id.clone();
    assert!(!h.store.request_refused(TEAM, &lost).unwrap());
    team_write(&mut h, p.live);
    for pass in 1..=5 {
        let machine = a.machine.clone();
        drop(a);
        a = Runtime::open(h.temp.path(), &machine).unwrap();
        let t = h.store.team(TEAM).unwrap().unwrap();
        a.cancels(&mut h.store, &mut p, &t, 100 + pass * 60_000)
            .unwrap();
    }
    assert_eq!(p.calls.len(), 1, "{status}, exit={exit}");
    assert!(h
        .store
        .pending_cancels(TEAM, 16, 0)
        .unwrap()
        .iter()
        .all(|c| c.node != 1));
    assert!(
        h.store.request_refused(TEAM, &lost).unwrap(),
        "stale request is spent"
    );
    let repeats: Vec<_> = p.facts.iter().filter(|f| f.request_id == lost).collect();
    assert_eq!(repeats.len(), 2, "lost reply must first retry its UUID");
    let saved = p.facts.last().unwrap();
    assert_ne!(saved.request_id, lost);
    assert_eq!(saved.refused, exit, "{status}");
    assert_eq!(saved.released, p.live && !exit);
    assert!(!h.store.request_refused(TEAM, &saved.request_id).unwrap());
    assert!(a
        .ledger
        .prefix_page(TEAM, "cancel:", "")
        .unwrap()
        .is_empty());
}
#[test]
fn a_lost_original_cancel_fact_replays_stale_then_records_the_answer_with_a_fresh_uuid() {
    for status in ["stopped", "archived", "live"] {
        check_stale(status, false);
    }
}
#[test]
fn a_lost_exit_fact_replays_stale_without_becoming_a_second_rule_refusal() {
    for status in ["stopped", "archived", "live"] {
        check_stale(status, true);
    }
}

fn stop_other_team(h: &mut Harness) -> u32 {
    let mut op = create();
    if let Operation::Create {
        integration_branch, ..
    } = &mut op
    {
        *integration_branch = "teams/other-team".into();
    }
    h.run_for(TEAM_B, &person(), op).unwrap();
    let node = h
        .run_for(
            TEAM_B,
            &person(),
            Operation::AddTask {
                task: task(5),
                role: None,
                pipeline: None,
            },
        )
        .unwrap()
        .node
        .unwrap();
    let mut o = h.observation(2, TaskStage::Todo);
    o.previous_revision = String::new();
    let service = crate::teams::ops::Actor::Service {
        person: "local".into(),
        team: TEAM_B.into(),
    };
    h.run_for(
        TEAM_B,
        &service,
        Operation::ObserveTask {
            node,
            observation: o,
        },
    )
    .unwrap();
    h.run_for(TEAM_B, &person(), Operation::Stop).unwrap();
    node
}
#[test]
fn a_halted_cancel_keeps_its_own_backoff_without_delaying_siblings_or_another_team() {
    let (mut h, mut a, mut p) = case("stopped", true);
    p.loss = None;
    p.only = None;
    p.reject_exit = true;
    a.page(&mut h.store, &mut p, &[], 100, false).unwrap();
    assert_eq!(p.calls, vec![(TEAM.into(), 1), (TEAM.into(), 2)]);
    assert!(p.cleared.contains(&(TEAM.into(), 2)));
    assert!(a.ledger.terminal_retry().unwrap().is_none());
    let other = stop_other_team(&mut h);
    for minute in 1..240 {
        a.page(&mut h.store, &mut p, &[], 100 + minute * 60_000, false)
            .unwrap();
        if minute == 1 {
            assert!(p.calls.contains(&(TEAM_B.into(), other)));
            assert!(p.cleared.contains(&(TEAM_B.into(), other)));
        }
    }
    assert_eq!(p.calls.len(), 3);
    assert!(a.ledger.terminal_retry().unwrap().is_none());
    assert_eq!(h.store.pending_cancels(TEAM, 16, 0).unwrap().len(), 1);
}
#[test]
fn many_halted_cancel_rows_do_not_spend_the_page_budget_before_a_later_node() {
    let mut h = Harness::new();
    for n in 1..=13 {
        h.add(&person(), n);
    }
    h.run(&person(), Operation::Stop).unwrap();
    let native = super::production::native(&h);
    let mut a = Runtime::open(h.temp.path(), &native.machine).unwrap();
    for pending in Native::pending_cancels(&h.store, TEAM, 0)
        .unwrap()
        .into_iter()
        .take(12)
    {
        a.ledger.reserve(TEAM, &format!("cancel:{}", pending.decision), json!({
            "pending": pending, "reply": null, "fact": null, "halted": true,
            "refusal_exit":true,"failures":1,"next_ms":0,"reason":"The outcome was refused twice."
        })).unwrap();
    }
    let mut p = Recovery {
        native,
        live: false,
        pair: Attempt::default(),
        loss: None,
        only: None,
        calls: vec![],
        facts: vec![],
        reject_exit: false,
        cleared: vec![],
        hold: false,
        stale: false,
        fail_observe: false,
        settle_observation: false,
        reads: 0,
        malformed: false,
    };
    a.page(&mut h.store, &mut p, &[], 100, false).unwrap();
    assert_eq!(p.calls, vec![(TEAM.into(), 13)]);
    assert_eq!(p.cleared, vec![(TEAM.into(), 13)]);
    assert!(a.ledger.terminal_retry().unwrap().is_none());
    for row in a.ledger.prefix_page(TEAM, "cancel:", "").unwrap() {
        assert!(row.data["next_ms"].as_i64().unwrap() > 100);
        assert_eq!(row.data["halted"], true);
    }
}

#[test]
fn a_thirty_minute_cutover_hold_backs_off_without_refusing_and_runs_when_released() {
    for status in ["live", "stopped", "archived"] {
        let (mut h, mut a, mut p) = case(status, false);
        p.loss = None;
        p.hold = true;
        for minute in 0..30 {
            let t = h.store.team(TEAM).unwrap().unwrap();
            a.cancels(&mut h.store, &mut p, &t, 100 + minute * 60_000)
                .unwrap();
        }
        assert_eq!(p.calls.len(), 5, "{status}");
        assert!(p.facts.is_empty());
        assert_eq!(
            h.store.cards(TEAM, 200, None).unwrap().rows.len(),
            usize::from(p.live)
        );
        let row = a.ledger.prefix_page(TEAM, "cancel:", "").unwrap().remove(0);
        assert!(row.data["reason"].as_str().unwrap().contains("cutover"));
        assert_eq!(row.data["next_ms"], 1_800_100);
        p.hold = false;
        let t = h.store.team(TEAM).unwrap().unwrap();
        a.cancels(&mut h.store, &mut p, &t, 1_800_100).unwrap();
        assert_eq!(p.calls.len(), 6);
        assert_eq!(p.facts.len(), 1);
        assert!(!p.facts[0].refused);
        assert!(a
            .ledger
            .prefix_page(TEAM, "cancel:", "")
            .unwrap()
            .is_empty());
        assert!(h
            .store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .all(|c| c.state != "open"));
        assert!(a.ledger.terminal_retry().unwrap().is_none());
    }
}
#[test]
fn proven_nonstarts_have_their_own_backoff_and_never_consume_stale_answer_slots() {
    for status in ["live", "stopped"] {
        let (mut h, mut a, mut p) = case(status, false);
        p.loss = None;
        p.native
            .stop
            .store(true, std::sync::atomic::Ordering::Release);
        for minute in 0..30 {
            let t = h.store.team(TEAM).unwrap().unwrap();
            a.cancels(&mut h.store, &mut p, &t, 100 + minute * 60_000)
                .unwrap();
        }
        assert_eq!(p.calls.len(), 5);
        assert!(p.facts.is_empty());
        let row = a.ledger.prefix_page(TEAM, "cancel:", "").unwrap().remove(0);
        assert_eq!(row.data["unchanged_answers"], 0);
        assert_eq!(
            h.store.cards(TEAM, 200, None).unwrap().rows.len(),
            usize::from(p.live)
        );
        p.native
            .stop
            .store(false, std::sync::atomic::Ordering::Release);
        let t = h.store.team(TEAM).unwrap().unwrap();
        a.cancels(&mut h.store, &mut p, &t, 1_800_100).unwrap();
        assert_eq!(p.facts.len(), 1);
        assert!(!p.facts[0].refused);
    }
}
#[test]
fn a_cancelled_answer_is_saved_while_observation_retries_without_another_child() {
    let (mut h, mut a, mut p) = case("live", false);
    p.loss = None;
    p.settle_observation = true;
    p.fail_observe = true;
    for minute in 0..30 {
        let machine = a.machine.clone();
        drop(a);
        a = Runtime::open(h.temp.path(), &machine).unwrap();
        let t = h.store.team(TEAM).unwrap().unwrap();
        a.cancels(&mut h.store, &mut p, &t, 100 + minute * 60_000)
            .unwrap();
    }
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.reads, 5);
    assert!(p.facts.is_empty());
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
    p.fail_observe = false;
    let t = h.store.team(TEAM).unwrap().unwrap();
    a.cancels(&mut h.store, &mut p, &t, 1_800_100).unwrap();
    assert_eq!(p.calls.len(), 1);
    assert!(p.facts.is_empty());
    assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
    assert_eq!(h.node(1).state, NodeState::Cancelled);
}
#[test]
fn three_unchanged_live_stale_answers_record_one_refusal_and_no_fourth_child() {
    let (mut h, mut a, mut p) = case("live", false);
    p.loss = None;
    p.stale = true;
    for pass in 0..100 {
        let t = h.store.team(TEAM).unwrap().unwrap();
        a.cancels(&mut h.store, &mut p, &t, 100 + pass * 60_000)
            .unwrap();
    }
    assert_eq!(p.calls.len(), 3);
    assert_eq!(p.facts.len(), 1);
    assert!(p.facts[0].refused);
    assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
    assert_eq!(h.store.inbox(TEAM, 200, None).unwrap().rows.len(), 1);
}

#[test]
fn unreadable_answers_fail_with_backoff_and_recover_without_a_bound_refusal() {
    for status in ["live", "stopped"] {
        let (mut h, mut a, mut p) = case(status, false);
        p.loss = None;
        p.malformed = true;
        for minute in 0..30 {
            let t = h.store.team(TEAM).unwrap().unwrap();
            a.cancels(&mut h.store, &mut p, &t, 100 + minute * 60_000)
                .unwrap();
        }
        assert_eq!(p.calls.len(), 5);
        assert!(p.facts.is_empty());
        let row = a.ledger.prefix_page(TEAM, "cancel:", "").unwrap().remove(0);
        assert_eq!(row.data["unchanged_answers"], 0);
        assert!(row.data["reason"].as_str().unwrap().contains("malformed"));
        assert_eq!(
            h.store.cards(TEAM, 200, None).unwrap().rows.len(),
            usize::from(p.live)
        );
        p.malformed = false;
        let t = h.store.team(TEAM).unwrap().unwrap();
        a.cancels(&mut h.store, &mut p, &t, 1_800_100).unwrap();
        assert_eq!(p.facts.len(), 1);
        assert!(!p.facts[0].refused);
    }
}

#[test]
fn cancel_exits_name_the_actual_answer_and_the_store_or_retry_cause() {
    for released in [false, true] {
        let (mut h, mut a, mut p) = case("stopped", true);
        p.loss = None;
        p.live = released;
        let t = h.store.team(TEAM).unwrap().unwrap();
        a.cancels(&mut h.store, &mut p, &t, 100).unwrap();
        let reason = &p.facts.last().unwrap().reason;
        assert!(reason.starts_with(if released {
            "The cancel command answered released."
        } else {
            "The cancel command answered cancelled."
        }));
        assert!(reason.contains("The team store refused this outcome:"));
        assert!(reason.contains("another attempt"));
        assert!(!reason.contains("invalid_state"));
        assert!(reason.len() <= 2048);
    }
    let (mut h, mut a, mut p) = case("live", false);
    p.loss = None;
    p.stale = true;
    for now in [100, 60_100, 180_100] {
        let t = h.store.team(TEAM).unwrap().unwrap();
        a.cancels(&mut h.store, &mut p, &t, now).unwrap();
    }
    let reason = &p.facts[0].reason;
    assert!(reason
        .starts_with("The cancel command answered stale: Observe this task before trying again."));
    assert!(reason.contains("The service stopped retrying after three stale answers"));
    assert!(!reason.contains("team store") && !reason.contains("no recordable answer"));
    assert!(reason.len() <= 2048);
}
