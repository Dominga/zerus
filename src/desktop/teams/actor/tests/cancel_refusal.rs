//! A proved refusal of an unchanged cancel visit has one durable exit.
use super::super::{
    cancel::{Answer, Attempt, Fact, Pending},
    native::Native,
    ports::{DeliveryResult, Ports},
    Runtime,
};
use super::*;
use crate::teams::ops::{HandoutOutcome, KeyClearState, Operation};

struct Adapter {
    native: Native,
    children: usize,
    facts: Vec<Fact>,
    pair: Attempt,
    reject_exit: bool,
    /// The next team-cancel child replies with output that is no answer.
    garbled: bool,
    cleared: Vec<u32>,
}
impl Ports for Adapter {
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
    fn cancels(&mut self, s: &Store, t: &str, after: u32) -> Result<Vec<Pending>> {
        Native::pending_cancels(s, t, after)
    }
    fn cancel(&mut self, t: &Team, pending: &Pending, original: &str) -> Result<Answer> {
        self.children += 1;
        let reply = if std::mem::take(&mut self.garbled) {
            b"not a reply".to_vec()
        } else {
            json!({"schema":1,"team":t.id,"node":pending.node,"state":"cancelled","error":"","attempt":self.pair.number,"attempt_id":self.pair.id}).to_string().into_bytes()
        };
        crate::desktop::teams::child::FIXTURE_EXIT.with(|slot| *slot.borrow_mut() = Some(reply));
        self.native.run_cancel(t, pending, original)
    }
    fn record_cancel(&mut self, s: &mut Store, t: &Team, fact: &Fact, now: i64) -> Result<()> {
        self.facts.push(fact.clone());
        let mut attempted = fact.clone();
        if fact.refused && self.reject_exit {
            // Model a future rule refusal, with a real saved refusal receipt.
            attempted.reason.clear();
        }
        Native::save_cancel(s, t, &attempted, now)
    }
    fn terminal_timers(
        &mut self,
        _: &mut super::super::auxiliary::State,
        s: &mut Store,
        now: i64,
    ) -> Result<()> {
        for k in s.keyed_tasks(TEAM, 0, 16)? {
            assert!(s
                .pending_cancels(TEAM, 16, 0)?
                .iter()
                .all(|p| p.node != k.node));
            super::super::write(
                s,
                TEAM,
                Operation::RecordKeyClear {
                    node: k.node,
                    state: KeyClearState::Cleared,
                    reason: String::new(),
                },
                now,
            )?;
            self.cleared.push(k.node);
        }
        Ok(())
    }
}
fn terminal(archived: bool) -> (Harness, Runtime, Adapter) {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let effect = h.intent(node, "m1");
    h.outcome(
        &effect,
        HandoutOutcome::Submitted {
            receipt: "delivered before observation".into(),
        },
    );
    assert_eq!(
        h.store
            .task_observation(TEAM, node)
            .unwrap()
            .unwrap()
            .attempt,
        0
    );
    h.run(&person(), Operation::Stop).unwrap();
    if archived {
        h.run(&person(), Operation::Archive).unwrap();
    }
    let native = super::production::native(&h);
    let runtime = Runtime::open(h.temp.path(), &native.machine).unwrap();
    let p = Adapter {
        native,
        children: 0,
        facts: vec![],
        pair: Attempt {
            number: 1,
            id: effect,
        },
        reject_exit: false,
        garbled: false,
        cleared: vec![],
    };
    (h, runtime, p)
}
fn refused(archived: bool) -> (Harness, Runtime, Adapter) {
    let (h, runtime, mut p) = terminal(archived);
    p.pair.id = "not-an-effect-of-this-node".into();
    (h, runtime, p)
}
fn passes(h: &mut Harness, runtime: &mut Runtime, p: &mut Adapter) {
    for pass in 0..20 {
        runtime
            .page(&mut h.store, p, &[], 100 + pass * 900_000, false)
            .unwrap();
    }
}
#[test]
fn an_unrecordable_terminal_answer_exits_a_saved_refusal_and_cleanup_continues() {
    for archived in [false, true] {
        let (mut h, mut runtime, mut p) = refused(archived);
        passes(&mut h, &mut runtime, &mut p);
        assert_eq!(p.children, 1);
        assert_eq!(p.facts.len(), 2);
        assert!(!p.facts[0].refused && p.facts[1].refused);
        assert_ne!(p.facts[0].request_id, p.facts[1].request_id);
        assert!(p.facts[1].reason.contains("cancelled"));
        assert!(p.facts[1].reason.contains("another attempt"));
        assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
        assert_eq!(p.cleared, vec![1]);
        assert!(runtime
            .ledger
            .get(TEAM, &format!("cancel:{}", p.facts[0].pending.decision))
            .unwrap()
            .is_none());
    }
}
#[test]
fn cancel_refusal_exit_survives_a_restart_on_both_sides_of_its_reservation() {
    for point in ["fact_refused", "refusal_exit"] {
        let (mut h, mut runtime, mut p) = refused(false);
        super::super::cancel::CRASH.with(|s| s.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.page(
                &mut h.store,
                &mut p,
                &[],
                100,
                false
            )))
            .is_err(),
            "{point}"
        );
        let before = p.facts.len();
        drop(runtime);
        let mut runtime = Runtime::open(h.temp.path(), &p.native.machine).unwrap();
        passes(&mut h, &mut runtime, &mut p);
        assert_eq!(p.children, 1, "{point}");
        assert!(p.facts.last().unwrap().refused, "{point}");
        if point == "refusal_exit" {
            assert!(p.facts[before..].iter().all(|f| f.refused));
        }
        assert!(
            h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty(),
            "{point}"
        );
    }
}
#[test]
fn a_second_proved_refusal_ends_the_visit_durably_without_replaying_either_request() {
    let (mut h, mut runtime, mut p) = refused(false);
    p.reject_exit = true;
    runtime.page(&mut h.store, &mut p, &[], 100, false).unwrap();
    assert_eq!(p.facts.len(), 2);
    assert!(h
        .store
        .request_refused(TEAM, &p.facts[1].request_id)
        .unwrap());
    let key = format!("cancel:{}", p.facts[0].pending.decision);
    let row = runtime.ledger.get(TEAM, &key).unwrap().unwrap();
    assert_eq!(row.data["halted"], true);
    assert!(runtime.ledger.terminal_retry().unwrap().is_none());
    drop(runtime);
    let mut runtime = Runtime::open(h.temp.path(), &p.native.machine).unwrap();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute("UPDATE task_observations SET data=json_set(data,'$.attempt_conflict',json('true')),attempt_conflict=1", [])
        .unwrap();
    assert_eq!(
        Native::pending_cancels(&h.store, TEAM, 0).unwrap()[0].attempt_conflict,
        Some(true)
    );
    passes(&mut h, &mut runtime, &mut p);
    assert_eq!(p.facts.len(), 2);
    assert_eq!(p.children, 1);
    assert!(p.cleared.is_empty());
    assert!(runtime.ledger.get(TEAM, &key).unwrap().is_some());
}

#[test]
fn a_terminal_saved_refusal_takes_the_exit_even_when_its_visit_changed() {
    use super::super::cancel::FactOutcome;
    for legacy in [false, true] {
        let (mut h, _, mut p) = refused(false);
        let t = h.store.team(TEAM).unwrap().unwrap();
        let mut pending = Native::pending_cancels(&h.store, TEAM, 0)
            .unwrap()
            .remove(0);
        if legacy {
            pending.attempt_conflict = None;
        } else {
            pending.attempt_id = "older-visit".into();
        }
        let fact = Fact {
            request_id: uuid::Uuid::new_v4().to_string(),
            revision: t.revision,
            pending,
            refused: false,
            released: false,
            reason: String::new(),
            attempt: p.pair.number,
            attempt_id: p.pair.id,
        };
        assert!(matches!(
            super::super::ports::Ports::cancel_fact_outcome(
                &mut p.native,
                &mut h.store,
                &t,
                &fact,
                100
            )
            .unwrap(),
            FactOutcome::Refused { .. }
        ));
        assert!(h.store.request_refused(TEAM, &fact.request_id).unwrap());
    }
}

#[test]
fn terminal_own_delivery_unobserved_is_recorded_as_cancelled_without_the_exit() {
    for archived in [false, true] {
        let (mut h, mut runtime, mut p) = terminal(archived);
        passes(&mut h, &mut runtime, &mut p);
        assert_eq!(p.children, 1);
        assert_eq!(p.facts.len(), 1);
        assert!(!p.facts[0].refused && !p.facts[0].released);
        assert!(!h
            .store
            .request_refused(TEAM, &p.facts[0].request_id)
            .unwrap());
        assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
        assert_eq!(p.cleared, vec![1]);
    }
}

/// A stopped team's cancel that halts is its own service item: it replaces
/// the cancel's earlier retry item, and goes with its row once the decision
/// no longer waits.
#[test]
fn a_halted_stopped_cancel_is_its_own_service_item_until_its_row_ends() {
    let (mut h, mut runtime, mut p) = refused(false);
    let row = h.store.pending_cancels(TEAM, 16, 0).unwrap().remove(0);
    let subject = row.decision.to_string();
    let read = |h: &Harness, now: i64| {
        crate::desktop::teams::service_items(h.temp.path(), TEAM, Some(true), now)
            .unwrap()
            .items
    };
    // An earlier pass whose child gave no answer: the cancel's retry item.
    p.garbled = true;
    runtime.page(&mut h.store, &mut p, &[], 50, false).unwrap();
    let earlier = read(&h, 50);
    let kinds: Vec<(&str, &str, Option<u32>)> = earlier
        .iter()
        .map(|i| (i.kind.as_str(), i.subject.as_str(), i.node))
        .collect();
    assert_eq!(kinds, [("cancel_retry", subject.as_str(), Some(row.node))]);
    // After its backoff the cancel is refused and its fact refused too.
    p.reject_exit = true;
    runtime
        .page(&mut h.store, &mut p, &[], 60_100, false)
        .unwrap();
    let key = format!("cancel:{}", row.decision);
    assert_eq!(
        runtime.ledger.get(TEAM, &key).unwrap().unwrap().data["halted"],
        true
    );
    let items = read(&h, 60_100);
    let kinds: Vec<(&str, &str)> = items
        .iter()
        .map(|i| (i.kind.as_str(), i.subject.as_str()))
        .collect();
    assert_eq!(kinds, [("cancel_halted", subject.as_str())]);
    assert!(
        items[0].reason.contains("refused too"),
        "{}",
        items[0].reason
    );
    // The decision stops waiting through the store: a later service records
    // the refusal with its reason.
    h.run(
        &service(),
        Operation::RecordCancel {
            node: row.node,
            decision: row.decision,
            state: crate::teams::ops::CancelState::Refused,
            reason: "Refused under a later rule".into(),
            attempt: row.attempt,
            attempt_id: row.attempt_id.clone(),
        },
    )
    .unwrap();
    assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
    // The cancel step alone, without the cleanup pass that would also
    // remove an orphaned item: only the row's own end clears it here.
    for pass in 0..20 {
        runtime
            .terminal_cancels(&mut h.store, &mut p, 60_100 + pass * 900_000)
            .unwrap();
    }
    assert!(runtime.ledger.get(TEAM, &key).unwrap().is_none());
    assert!(read(&h, 60_100).is_empty());
}
