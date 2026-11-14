//! zerus-63rru.102 round 3: a stopped or archived team records a durable
//! hosted outcome as a fact only. It never hands the task back to the lead:
//! no decision moves (the Stop's own Cancel included), the node is not held
//! again, and no lead inbox item or card is written. A running team keeps the
//! hand-back it had before.
//!
//! Every store row comes from the product's own writers: the real Create,
//! bind and Add, the actor's real send up to a delivery crash boundary, the
//! hosted child's own `Ledger::reserve_input` carrying its reply in the exact
//! `Input::reply` shape (`src/tasks/team_deliver.rs:42-51`; `final_reply`
//! adds `final`), the person's real ArchiveMember, Stop and Archive. Raw SQL
//! is never used: `rows::handed_back` and the store pages are product reads.
use super::*;
use crate::teams::ops::{CancelState, HandoutOutcome, KeyClearState, Operation};
use serde_json::{json, Value};

fn dsh_team() -> Operation {
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle
            .roles
            .iter_mut()
            .find(|r| r.id == "developer")
            .unwrap()
            .agent = "dsh".into();
    }
    create
}

/// One delivery taken from the real send to a crash boundary.
struct Boot {
    h: Harness,
    home: String,
    id: String,
}

impl Boot {
    /// The real Create/bind/Add, then one actor tick that stops at `point`
    /// (`effect`: the child answered, the row has no outcome; `outcome`: the
    /// row's outcome is persisted, the store has not recorded it).
    fn new(point: &'static str, delivered: HandoutOutcome) -> Self {
        let mut h = Harness::empty();
        h.run(&person(), dsh_team()).unwrap();
        h.bind("m1");
        h.add(&person(), 1);
        let home = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
        let mut p = super::core::Fake::new();
        p.reply = delivered;
        super::super::delivery::CRASH.with(|c| c.set(Some(point)));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            a.begin_pass();
            a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap()
        }))
        .is_err());
        let row = a
            .ledger
            .prefix_page(TEAM, "delivery:", "")
            .unwrap()
            .remove(0);
        let id = row.data["id"].as_str().unwrap().to_string();
        assert_eq!(
            !row.data["outcome"].is_null(),
            point == "outcome",
            "the crash boundary did not leave the row it promises"
        );
        drop(a);
        Self { h, home, id }
    }
    /// The hosted child's own writer of its retained input, with its reply in
    /// the child's exact shape. The terminal read touches only `reply`.
    fn retain(&self, reply: Value) {
        super::super::ledger::Ledger::open(self.h.temp.path())
            .unwrap()
            .reserve_input(
                TEAM,
                &format!("hosted-input:{}", self.id),
                json!({
                    "input": {"text": "retained task text"},
                    "to": null,
                    "body": {"schema": 1, "id": self.id, "team": TEAM, "node": 1, "member": "m1"},
                    "reply": reply,
                }),
            )
            .unwrap();
    }
    fn stop(&mut self) {
        self.h.run(&person(), Operation::Stop).unwrap();
    }
    fn pause(&mut self) {
        self.h.run(&person(), Operation::Pause).unwrap();
    }
    fn archive(&mut self) {
        self.h.run(&person(), Operation::Archive).unwrap();
    }
    fn remove_member(&mut self) {
        self.h
            .run(
                &person(),
                Operation::ArchiveMember {
                    member: "m1".into(),
                },
            )
            .unwrap();
    }
    /// One real actor tick under the team's current status.
    fn tick(&mut self, now: i64) {
        let home = self.home.clone();
        let mut a = super::super::Runtime::open(self.h.temp.path(), &home).unwrap();
        let mut p = super::core::Fake::new();
        a.begin_pass();
        a.tick(&mut self.h.store, &mut p, TEAM, now, false).unwrap();
    }
    /// The real terminal pass, once per call.
    fn terminal(&mut self, passes: u32) {
        let home = self.home.clone();
        let mut a = super::super::Runtime::open(self.h.temp.path(), &home).unwrap();
        let team = self.h.store.team(TEAM).unwrap().unwrap();
        for pass in 0..i64::from(passes) {
            a.terminal_deliveries(&mut self.h.store, &team, 5_000_000 + pass)
                .unwrap();
        }
    }
}

/// One read of every fact a terminal settle must or must not change.
#[derive(Debug)]
struct Snapshot {
    row: bool,
    input: bool,
    reserved: bool,
    effect: String,
    outcome: Value,
    row_outcome: Value,
    cancels: usize,
    hand_back: bool,
    inbox: Vec<String>,
    open_cards: usize,
    gates: Vec<String>,
    team: String,
}

fn snapshot(h: &Harness, id: &str) -> Snapshot {
    let ledger = super::super::ledger::Ledger::open(h.temp.path()).unwrap();
    let stored = ledger.get(TEAM, &format!("delivery:{id}")).unwrap();
    let row_outcome = stored
        .as_ref()
        .map_or(Value::Null, |r| r.data["outcome"].clone());
    let row = stored.is_some();
    let input = ledger
        .input(TEAM, &format!("hosted-input:{id}"))
        .unwrap()
        .is_some();
    let gates = ledger
        .page(TEAM, "")
        .unwrap()
        .into_iter()
        .filter(|r| r.key.starts_with("gate:"))
        .map(|r| r.key)
        .collect();
    drop(ledger);
    let effects = h.store.effects(TEAM, 200, None).unwrap().rows;
    let effect = effects.iter().find(|e| e.request_id == id);
    let (reserved, state, outcome) = match effect {
        Some(e) => (
            h.store.effect_reserved(TEAM, id).unwrap(),
            e.state.clone(),
            e.outcome.clone().unwrap_or(Value::Null),
        ),
        None => (false, String::new(), Value::Null),
    };
    Snapshot {
        row,
        input,
        reserved,
        effect: state,
        outcome,
        row_outcome,
        cancels: h.store.pending_cancels(TEAM, 16, 0).unwrap().len(),
        hand_back: crate::teams::store::rows::handed_back(
            h.store.db.as_ref().unwrap(),
            TEAM,
            1,
        )
        .unwrap(),
        inbox: h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .map(|i| i.kind)
            .collect(),
        open_cards: h
            .store
            .cards(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .filter(|c| c.state == "open")
            .count(),
        gates,
        team: format!("{:?}", h.store.team(TEAM).unwrap().unwrap().status),
    }
}

/// The state the outcome writer stores and the shape it records.
fn shape(outcome: &HandoutOutcome) -> (&'static str, &'static str) {
    match outcome {
        HandoutOutcome::Submitted { .. } => ("submitted", "submitted"),
        HandoutOutcome::Refused { .. } => ("refused", "refused"),
        HandoutOutcome::CuttingOver { .. } => ("cutting_over", "cutting_over"),
        HandoutOutcome::Uncertain { .. } => ("uncertain", "uncertain"),
        HandoutOutcome::HandedBack { .. } => ("refused", "handed_back"),
    }
}

/// A reply in the child's exact shape. `extra` carries the child's own
/// `receipt`, `error`, `cause` or `final`.
fn child_reply(id: &str, state: &str, extra: Value) -> Value {
    let mut reply = json!({
        "schema": 1, "id": id, "team": TEAM, "node": 1, "member": "m1", "state": state
    });
    reply
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    reply
}

fn submitted_reply(id: &str) -> Value {
    child_reply(id, "submitted", json!({"receipt": {"request_id": id}}))
}

fn refused_reply(id: &str) -> Value {
    child_reply(
        id,
        "refused",
        json!({
            "error": "DeepSeek refused the task input: fixture",
            "cause": "dsh_not_offered"
        }),
    )
}

fn final_reply(id: &str) -> Value {
    child_reply(
        id,
        "refused",
        json!({
            "error": "DeepSeek refused the task text because it contains a native session mention. The task waits for the lead; this input is never retried.",
            "final": true,
            "cause": "dsh_not_offered"
        }),
    )
}

fn cutting_over_reply(id: &str) -> Value {
    child_reply(
        id,
        "cutting_over",
        json!({"error": "The member is cutting over to a new conversation."}),
    )
}

fn uncertain_reply(id: &str) -> Value {
    child_reply(
        id,
        "uncertain",
        json!({"error": "The DeepSeek task input is uncertain (fixture); it is never sent again."}),
    )
}

fn pending_reply(id: &str) -> Value {
    child_reply(id, "pending", json!({"error": "still reading"}))
}

/// The reply a round-1 build wrote: no `schema`, no `id`. The terminal read
/// cannot settle from it.
fn round1_reply(id: &str) -> Value {
    json!({"state": "submitted", "receipt": {"request_id": id}})
}

/// Every fact hand_to_lead would change. A terminal outcome must leave all
/// of them as the Stop left them. `assert_only_a_fact` also requires that the
/// settle opened no failure gate; the one test that starts with a real gate
/// checks that separately.
fn assert_no_hand_back(s: &Snapshot, before: &Snapshot, case: &str) {
    assert!(!s.row, "{case}: the delivery row stayed");
    assert!(!s.reserved, "{case}: the effect stayed reserved");
    assert_eq!(
        s.cancels, before.cancels,
        "{case}: the Stop's cancel decision changed"
    );
    assert!(!s.hand_back, "{case}: the node was held for the lead again");
    assert!(
        !s.inbox.iter().any(|k| k.starts_with("handout_handed_back:")),
        "{case}: a hand-back inbox item was written: {:?}",
        s.inbox
    );
    assert_eq!(s.open_cards, 0, "{case}: a card was opened");
    assert!(
        s.team == "Stopped" || s.team == "Archived",
        "{case}: the team status changed: {}",
        s.team
    );
}

fn assert_only_a_fact(s: &Snapshot, before: &Snapshot, case: &str) {
    assert_no_hand_back(s, before, case);
    assert!(
        s.gates.is_empty(),
        "{case}: a failure gate was opened: {:?}",
        s.gates
    );
    // The terminal fixture may already carry an item from an earlier writer
    // (ArchiveMember's member_removed): the rule is that the terminal pass
    // adds none, so a Stopped or Archived team writes no new lead item at all
    // (zerus-63rru.102 round 4, D1).
    let new: Vec<&String> = s
        .inbox
        .iter()
        .filter(|kind| !before.inbox.contains(kind))
        .collect();
    assert!(
        new.is_empty(),
        "{case}: a new lead item was written on a terminal team: {new:?}"
    );
}

/// P2-1: the four reviewer table rows settle from the durable outcome they
/// hold (the child's retained submitted reply), record it, and leave no row,
/// input, gate or decision change.
#[test]
fn the_terminal_team_settles_each_probe_row_from_its_durable_outcome() {
    for point in ["effect", "outcome"] {
        for remove_member in [false, true] {
            for archive in [false, true] {
                let mut b = Boot::new(
                    point,
                    HandoutOutcome::Submitted {
                        receipt: "submitted".into(),
                    },
                );
                b.retain(submitted_reply(&b.id));
                if remove_member {
                    b.remove_member();
                }
                b.stop();
                if archive {
                    b.archive();
                }
                let case = format!("{point}/{remove_member}/{archive}");
                let before = snapshot(&b.h, &b.id);
                assert!(before.row, "{case}: the fixture did not reserve a delivery");
                assert_eq!(
                    before.row_outcome.is_null(),
                    point == "effect",
                    "{case}: the crash boundary did not leave the promised row"
                );
                b.terminal(2);
                let after = snapshot(&b.h, &b.id);
                assert_only_a_fact(&after, &before, &case);
                assert!(!after.input, "{case}: the retained input stayed");
                assert!(
                    after.effect == "submitted" || after.effect == "superseded",
                    "{case}: the effect recorded {}",
                    after.effect
                );
                assert_eq!(
                    after.outcome["status"], "submitted",
                    "{case}: the outcome was not recorded"
                );
            }
        }
    }
}

/// P2-1: every persisted outcome shape on a stopped and on an archived team
/// is recorded as a fact. No retained input exists, so only the row's own
/// outcome can settle the delivery (this kills M7a, the "persisted outcome
/// ignored" mutant).
#[test]
fn the_terminal_team_records_every_persisted_outcome_without_handing_back() {
    for outcome in [
        HandoutOutcome::Submitted {
            receipt: "submitted".into(),
        },
        HandoutOutcome::Refused {
            reason: "DeepSeek refused the task input: fixture".into(),
        },
        HandoutOutcome::CuttingOver {
            reason: "The member is cutting over.".into(),
        },
        HandoutOutcome::Uncertain {
            reason: "The DeepSeek task input is uncertain.".into(),
        },
        HandoutOutcome::HandedBack {
            reason: "DeepSeek refused the task text finally.".into(),
        },
    ] {
        for archive in [false, true] {
            let (state, status) = shape(&outcome);
            let mut b = Boot::new("outcome", outcome.clone());
            b.stop();
            if archive {
                b.archive();
            }
            let case = format!("{status}/archive={archive}");
            let before = snapshot(&b.h, &b.id);
            assert!(before.reserved, "{case}: the effect was not open");
            assert_eq!(
                before.row_outcome["status"], status,
                "{case}: the persisted outcome differs"
            );
            b.terminal(2);
            let after = snapshot(&b.h, &b.id);
            assert_only_a_fact(&after, &before, &case);
            assert_eq!(after.effect, state, "{case}: effect state");
            assert_eq!(
                after.outcome["status"], status,
                "{case}: the persisted outcome was not the fact"
            );
        }
    }
}

/// P2-1 and P3: every retained reply shape on a stopped and on an archived
/// team. Submitted, refused, cutting-over and the child's final refusal
/// settle; an uncertain, pending or round-1 reply stays unknown with its
/// input, never replayed. The final refusal (N6) and the uncertain reply
/// (N1) are each a red run under their mutants.
#[test]
fn the_terminal_team_records_every_retained_reply_shape_without_replaying() {
    type Builder = fn(&str) -> Value;
    type Case = (&'static str, Builder, Option<(&'static str, &'static str)>);
    let cases: [Case; 7] = [
        ("submitted", submitted_reply, Some(("submitted", "submitted"))),
        ("refused", refused_reply, Some(("refused", "refused"))),
        ("final", final_reply, Some(("refused", "handed_back"))),
        (
            "cutting_over",
            cutting_over_reply,
            Some(("cutting_over", "cutting_over")),
        ),
        ("uncertain", uncertain_reply, None),
        ("pending", pending_reply, None),
        ("round-1 shape", round1_reply, None),
    ];
    for archive in [false, true] {
        for (label, build, settled) in cases {
            let mut b = Boot::new(
                "effect",
                HandoutOutcome::Submitted {
                    receipt: "submitted".into(),
                },
            );
            b.retain(build(&b.id));
            b.stop();
            if archive {
                b.archive();
            }
            let case = format!("{label}/archive={archive}");
            let before = snapshot(&b.h, &b.id);
            assert!(before.reserved, "{case}: the effect was not open");
            b.terminal(2);
            let after = snapshot(&b.h, &b.id);
            match settled {
                Some((state, status)) => {
                    assert_only_a_fact(&after, &before, &case);
                    assert!(!after.input, "{case}: the retained input stayed");
                    assert_eq!(after.effect, state, "{case}: effect state");
                    assert_eq!(after.outcome["status"], status, "{case}: outcome shape");
                }
                None => {
                    assert!(after.row, "{case}: the unknown row was removed");
                    assert!(after.input, "{case}: the unknown input was removed");
                    assert!(after.reserved, "{case}: the unknown effect was settled");
                    assert_eq!(
                        after.cancels, before.cancels,
                        "{case}: the Stop's cancel decision changed"
                    );
                    assert!(!after.hand_back, "{case}: the node was held");
                    assert_eq!(after.open_cards, 0, "{case}: a card was opened");
                    assert!(after.gates.is_empty(), "{case}: {:?}", after.gates);
                    assert!(
                        after.inbox.is_empty(),
                        "{case}: an unknown delivery wrote a lead item: {:?}",
                        after.inbox
                    );
                    assert_eq!(after.team, before.team, "{case}: the team status changed");
                }
            }
        }
    }
}

/// M7a, the stronger shape: the row's persisted outcome is the fact even
/// when the retained reply names a different answer. The child's final
/// refusal is persisted; the retained reply says submitted. Head records
/// `handed_back`; the mutant reads the retained reply instead.
#[test]
fn the_persisted_outcome_is_the_fact_even_when_the_retained_reply_differs() {
    let mut b = Boot::new(
        "outcome",
        HandoutOutcome::HandedBack {
            reason: "DeepSeek refused the task text finally.".into(),
        },
    );
    b.retain(submitted_reply(&b.id));
    b.stop();
    let before = snapshot(&b.h, &b.id);
    b.terminal(2);
    let after = snapshot(&b.h, &b.id);
    assert_only_a_fact(
        &after,
        &before,
        "persisted final with retained submitted",
    );
    assert_eq!(after.effect, "refused");
    assert_eq!(
        after.outcome["status"], "handed_back",
        "the retained reply replaced the persisted outcome"
    );
}

/// N4: the terminal pass records the outcome and its gate durably before it
/// applies them. A crash after the write leaves the fact for the next pass;
/// dropping the write leaves a row that only looks unknown.
#[test]
fn the_terminal_settlement_records_the_fact_durably_before_applying_it() {
    let mut b = Boot::new(
        "effect",
        HandoutOutcome::Submitted {
            receipt: "submitted".into(),
        },
    );
    b.retain(refused_reply(&b.id));
    b.stop();
    let before = snapshot(&b.h, &b.id);
    let home = b.home.clone();
    let team = b.h.store.team(TEAM).unwrap().unwrap();
    let mut a = super::super::Runtime::open(b.h.temp.path(), &home).unwrap();
    super::super::delivery::CRASH.with(|c| c.set(Some("terminal_outcome")));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.terminal_deliveries(&mut b.h.store, &team, 5_000_000)
            .unwrap()
    }))
    .is_err());
    let row = a
        .ledger
        .get(TEAM, &format!("delivery:{}", b.id))
        .unwrap()
        .expect("the delivery row is still here after the crash");
    assert!(
        !row.data["outcome"].is_null(),
        "the outcome was not durable before the settle"
    );
    assert_eq!(
        row.data["gated"].as_bool(),
        Some(true),
        "the gate flag was not durable before the settle"
    );
    let team = b.h.store.team(TEAM).unwrap().unwrap();
    for pass in 0..2i64 {
        a.terminal_deliveries(&mut b.h.store, &team, 5_000_001 + pass)
            .unwrap();
    }
    drop(a);
    let after = snapshot(&b.h, &b.id);
    assert_only_a_fact(&after, &before, "resumed terminal settle");
    assert!(!after.input, "the retained input stayed");
    assert_eq!(after.outcome["status"], "refused");
}

/// N7 and the gate assertion: a real deferred `gate:delivery:<id>` (written
/// by `defer`, `retry.rs:116-128`, after an unreadable running hosted read)
/// is not recovered by a terminal settle. The terminal prune reclaims it once
/// the team's cancel frontier is done, as the maintenance rule says.
#[test]
fn a_terminal_team_recovers_no_gate_and_the_prune_reclaims_it() {
    let mut b = Boot::new(
        "effect",
        HandoutOutcome::Submitted {
            receipt: "submitted".into(),
        },
    );
    b.retain(submitted_reply(&b.id));
    {
        let home = b.home.clone();
        let mut a = super::super::Runtime::open(b.h.temp.path(), &home).unwrap();
        let mut p = super::core::Fake::new();
        p.recovery_unreadable = true;
        a.begin_pass();
        a.tick(&mut b.h.store, &mut p, TEAM, 200, false).unwrap();
    }
    let gate = format!("gate:delivery:{}", b.id);
    b.stop();
    let before = snapshot(&b.h, &b.id);
    assert!(
        before.gates.contains(&gate),
        "the real deferred gate was not written: {:?}",
        before.gates
    );
    b.terminal(1);
    let settled = snapshot(&b.h, &b.id);
    assert_no_hand_back(&settled, &before, "terminal settle with a gate");
    assert!(!settled.input, "the retained input stayed");
    assert!(
        settled.gates.contains(&gate),
        "the terminal settle recovered a gate it must leave for the prune: {:?}",
        settled.gates
    );
    // The service records the Stop's cancel through its own writer; then the
    // terminal frontier is done and the maintenance prune reclaims the gate.
    let pending = b.h.store.pending_cancels(TEAM, 16, 0).unwrap().remove(0);
    b.h.run(
        &service(),
        Operation::RecordCancel {
            node: pending.node,
            decision: pending.decision,
            state: CancelState::Cancelled,
            reason: String::new(),
            attempt: pending.attempt,
            attempt_id: pending.attempt_id,
        },
    )
    .unwrap();
    b.h.run(
        &service(),
        Operation::RecordKeyClear {
            node: pending.node,
            state: KeyClearState::Cleared,
            reason: String::new(),
        },
    )
    .unwrap();
    {
        let home = b.home.clone();
        let mut a = super::super::Runtime::open(b.h.temp.path(), &home).unwrap();
        for _ in 0..4 {
            a.maintain(&b.h.store).unwrap();
        }
    }
    let pruned = snapshot(&b.h, &b.id);
    assert!(
        !pruned.gates.contains(&gate),
        "the terminal prune did not reclaim the gate: {:?}",
        pruned.gates
    );
}

/// P2-1: with neither a persisted outcome nor a settled retained reply, the
/// delivery stays unknown: keep the row and the input, never replay, and the
/// open effect keeps the terminal prune blocked for that team.
#[test]
fn an_unknown_delivery_without_a_durable_outcome_stays_unknown() {
    for archive in [false, true] {
        let mut b = Boot::new(
            "effect",
            HandoutOutcome::Submitted {
                receipt: "submitted".into(),
            },
        );
        b.retain(uncertain_reply(&b.id));
        b.stop();
        if archive {
            b.archive();
        }
        b.terminal(2);
        let after = snapshot(&b.h, &b.id);
        assert!(after.row, "archive={archive}: the unknown row was removed");
        assert!(
            after.input,
            "archive={archive}: the unknown input was removed"
        );
        assert!(
            after.reserved,
            "archive={archive}: the unknown effect was settled"
        );
    }
}

/// D2: Paused is moving. A paused team still writes the informational
/// delivery_uncertain item and still runs its hand-back path, so the round-4
/// item gate did not narrow the live team.
#[test]
fn a_paused_team_still_writes_the_uncertain_item_and_hands_back() {
    let mut uncertain = Boot::new(
        "outcome",
        HandoutOutcome::Uncertain {
            reason: "The DeepSeek task input is uncertain.".into(),
        },
    );
    uncertain.pause();
    uncertain.tick(1_000);
    let after = snapshot(&uncertain.h, &uncertain.id);
    assert_eq!(after.team, "Paused");
    assert!(!after.row, "the paused delivery row stayed");
    assert!(
        after.inbox.iter().any(|k| k == "delivery_uncertain"),
        "the paused team wrote no delivery_uncertain item: {:?}",
        after.inbox
    );
    assert!(!after.hand_back, "the paused uncertain held the node");

    let mut final_refusal = Boot::new(
        "outcome",
        HandoutOutcome::HandedBack {
            reason: "DeepSeek refused the task text finally.".into(),
        },
    );
    final_refusal.pause();
    final_refusal.tick(2_000);
    let after = snapshot(&final_refusal.h, &final_refusal.id);
    assert_eq!(after.team, "Paused");
    assert!(!after.row, "the paused hand-back row stayed");
    assert!(after.hand_back, "the paused hand-back did not hold the node");
    assert!(
        after
            .inbox
            .iter()
            .any(|k| k.starts_with("handout_handed_back:")),
        "the paused hand-back told no lead: {:?}",
        after.inbox
    );
}

/// The running team's final refusal still moves the decision and tells the
/// lead: the terminal fact-only rule changed no running behaviour.
#[test]
fn a_running_team_still_hands_a_final_refusal_to_the_lead() {
    let mut b = Boot::new(
        "effect",
        HandoutOutcome::Submitted {
            receipt: "submitted".into(),
        },
    );
    let home = b.home.clone();
    let mut a = super::super::Runtime::open(b.h.temp.path(), &home).unwrap();
    let mut p = super::core::Fake::new();
    p.reply = HandoutOutcome::HandedBack {
        reason: "DeepSeek refused the task text finally.".into(),
    };
    a.begin_pass();
    a.tick(&mut b.h.store, &mut p, TEAM, 1_000, false).unwrap();
    drop(a);
    let after = snapshot(&b.h, &b.id);
    assert!(after.hand_back, "the running hand-back did not hold the node");
    assert!(
        after
            .inbox
            .iter()
            .any(|k| k.starts_with("handout_handed_back:")),
        "the running hand-back told no lead: {:?}",
        after.inbox
    );
    assert!(!after.row, "the running delivery row stayed");
    assert_eq!(after.team, "Running");
}

/// P3-4: the running path's not-reserved removal in recover_delivery is
/// pinned: a running team's delivery whose effect the store superseded
/// (ArchiveMember) leaves no row and no input on the next tick.
#[test]
fn the_running_teams_not_reserved_delivery_removes_the_input() {
    let mut b = Boot::new(
        "effect",
        HandoutOutcome::Submitted {
            receipt: "submitted".into(),
        },
    );
    b.retain(Value::Null);
    b.remove_member();
    let home = b.home.clone();
    let mut a = super::super::Runtime::open(b.h.temp.path(), &home).unwrap();
    let mut p = super::core::Fake::new();
    a.begin_pass();
    a.tick(&mut b.h.store, &mut p, TEAM, 1_000, false).unwrap();
    drop(a);
    let after = snapshot(&b.h, &b.id);
    assert!(!after.row, "the not-reserved delivery row stayed");
    assert!(!after.input, "the not-reserved input stayed");
}
