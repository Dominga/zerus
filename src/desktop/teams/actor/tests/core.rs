use super::*;
use crate::teams::ops::{self, HandoutOutcome, Operation, TaskStage};
use std::collections::BTreeSet;
pub(super) struct Fake {
    reads: usize,
    journals: usize,
    sends: usize,
    free: BTreeSet<String>,
    pub(super) reply: HandoutOutcome,
    stale: bool,
    driven: bool,
    busy_on_recheck: bool,
    free_reads: usize,
    documents: BTreeSet<String>,
    failed_task: Option<String>,
    fail_observation_after_send: bool,
    lock_after_send: Option<std::path::PathBuf>,
    held: Option<rusqlite::Connection>,
    pause_on_recheck: Option<std::path::PathBuf>,
    recovery_reads: usize,
    pub(super) recovery_unreadable: bool,
}
impl Fake {
    pub(super) fn new() -> Self {
        Self {
            reads: 0,
            journals: 0,
            sends: 0,
            free: ["m1".into(), "m2".into()].into(),
            reply: HandoutOutcome::Submitted {
                receipt: "submitted".into(),
            },
            stale: false,
            driven: false,
            busy_on_recheck: false,
            free_reads: 0,
            documents: BTreeSet::new(),
            failed_task: None,
            fail_observation_after_send: false,
            lock_after_send: None,
            held: None,
            pause_on_recheck: None,
            recovery_reads: 0,
            recovery_unreadable: false,
        }
    }
}
impl super::super::ports::Ports for Fake {
    fn recover_delivery(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> Result<Option<HandoutOutcome>> {
        self.recovery_reads += 1;
        if self.recovery_unreadable { Ok(None) } else { Ok(Some(self.reply.clone())) }
    }
    fn changes(&mut self, _: &str, g: &str, _: u64) -> Result<zerus_replica::Changes> {
        self.journals += 1;
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: if g != "journal" {
                None
            } else {
                Some(std::mem::take(&mut self.documents))
            },
        })
    }
    fn observe(&mut self, store: &mut Store, task: &TaskRef) -> Result<()> {
        self.reads += 1;
        if self.failed_task.as_deref() == Some(&task.uuid) {
            return Err(super::super::problem("Fixture observation unavailable"));
        }
        if self.driven && self.reads > 1 {
            for node in store.queue(TEAM, 200, None)?.rows {
                if node.task.as_ref() == Some(task) {
                    let old = store.task_observation(TEAM, node.id)?.unwrap();
                    let o = ops::TaskObservation {
                        revision: format!("external-{}", self.reads),
                        previous_revision: old.revision.clone(),
                        stage: TaskStage::InProgress,
                        ..old
                    };
                    super::super::write(
                        store,
                        TEAM,
                        Operation::ObserveTask {
                            node: node.id,
                            observation: o,
                        },
                        100,
                    )?;
                }
            }
        }
        Ok(())
    }
    fn free(&mut self, _: &Team, m: &Member) -> Result<bool> {
        self.free_reads += 1;
        if self.free_reads > 2 {
            if let Some(state) = self.pause_on_recheck.take() {
                let mut store = Store::open(&state)?;
                let revision = store.team(TEAM)?.unwrap().revision;
                ops::apply(
                    &mut store,
                    &person(),
                    &request(
                        revision,
                        Operation::PauseMember {
                            member: m.id.clone(),
                        },
                    ),
                    100,
                )?;
            }
        }
        Ok(self.free.contains(&m.id) && !(self.busy_on_recheck && self.free_reads > 2))
    }
    fn deliver(
        &mut self,
        _: &Team,
        node: &Node,
        _: &Member,
        _: &str,
    ) -> super::super::ports::DeliveryResult {
        self.sends += 1;
        if self.fail_observation_after_send {
            self.failed_task = node.task.as_ref().map(|t| t.uuid.clone());
        }
        if let Some(path) = self.lock_after_send.take() {
            let db = rusqlite::Connection::open(path).unwrap();
            db.execute_batch("BEGIN IMMEDIATE").unwrap();
            self.held = Some(db);
        }
        if self.stale {
            super::super::ports::DeliveryResult::Stale
        } else {
            self.reply.clone().into()
        }
    }
}
fn setup(n: u32) -> (Harness, super::super::Runtime, Fake) {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    for i in 1..=n {
        h.add(&person(), i);
    }
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    (h, a, Fake::new())
}
fn tick(h: &mut Harness, a: &mut super::super::Runtime, p: &mut Fake, now: i64, safety: bool) {
    a.begin_pass();
    a.tick(&mut h.store, p, TEAM, now, safety).unwrap();
}
#[test]
fn two_free_members_receive_only_two_of_three_ready_nodes() {
    let (mut h, mut a, mut p) = setup(3);
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.sends, 2);
    assert_eq!(h.store.open_effects(TEAM, 200, None).unwrap().rows.len(), 2);
}
#[test]
fn a_part_a_retry_row_reopens_without_the_new_failure_event_field() {
    let (mut h, mut a, _) = setup(0);
    a.put(
        TEAM,
        "gate:report:1",
        &json!({"revision":"old","failures":2,"at":60_100,"card":null,"notice":null}),
    )
    .unwrap();
    drop(a);
    let t = h.store.team(TEAM).unwrap().unwrap();
    let mut a = super::super::Runtime::open(h.temp.path(), &t.home).unwrap();
    assert!(!a.due(&mut h.store, &t, "report:1", "old", 100).unwrap());
    a.failure(
        &mut h.store,
        &t,
        "report:1",
        "old",
        &super::super::problem("Still unavailable"),
        60_100,
    )
    .unwrap();
    assert_eq!(
        a.ledger.get(TEAM, "gate:report:1").unwrap().unwrap().data["failures"],
        3
    );
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
}
#[test]
fn unknown_delivery_creates_one_inbox_and_never_sends_again_after_restart() {
    let (mut h, mut a, mut p) = setup(1);
    p.reply = HandoutOutcome::Uncertain {
        reason: "Lost writer receipt".into(),
    };
    tick(&mut h, &mut a, &mut p, 100, false);
    drop(a);
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    for i in 0..3 {
        tick(&mut h, &mut a, &mut p, 200 + i, false);
    }
    assert_eq!(p.sends, 1);
    assert_eq!(
        h.store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .filter(|r| r.kind == "delivery_uncertain")
            .count(),
        1
    );
}
#[test]
fn refusal_leaves_no_assignee_and_obeys_retry_gate() {
    let (mut h, mut a, mut p) = setup(1);
    p.reply = HandoutOutcome::Refused {
        reason: "Busy elsewhere".into(),
    };
    tick(&mut h, &mut a, &mut p, 100, false);
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 1);
    assert!(h.node(1).assignee.is_none());
    tick(&mut h, &mut a, &mut p, 60_101, false);
    assert_eq!(p.sends, 2);
}
#[test]
fn task_driven_elsewhere_and_member_busy_races_do_not_send() {
    for driven in [true, false] {
        let (mut h, mut a, mut p) = setup(1);
        p.driven = driven;
        p.busy_on_recheck = !driven;
        tick(&mut h, &mut a, &mut p, 100, false);
        assert_eq!(p.sends, 0);
    }
}
#[test]
fn second_unchanged_woken_pass_reads_no_task_document_or_effect() {
    let (mut h, mut a, mut p) = setup(1);
    p.free.clear();
    tick(&mut h, &mut a, &mut p, 100, false);
    let reads = p.reads;
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.reads, reads);
    assert_eq!(p.sends, 0);
    tick(&mut h, &mut a, &mut p, 60_101, true);
    assert_eq!(p.reads, reads + 1);
}
#[test]
fn restart_at_each_delivery_boundary_never_continues_an_original() {
    for point in ["reserved", "intent", "effect", "outcome", "recorded"] {
        let (mut h, mut a, mut p) = setup(1);
        super::super::delivery::CRASH.with(|c| c.set(Some(point)));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tick(&mut h, &mut a, &mut p, 100, false)
        }));
        assert!(result.is_err(), "boundary {point} was not exercised");
        let expected = usize::from(matches!(point, "effect" | "outcome" | "recorded"));
        assert_eq!(p.sends, expected);
        drop(a);
        let home = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
        for i in 0..3 {
            tick(&mut h, &mut a, &mut p, 200 + i, false);
        }
        assert_eq!(p.sends, expected, "restart continued {point}");
        let inbox = h.store.inbox(TEAM, 200, None).unwrap();
        assert_eq!(
            inbox
                .rows
                .iter()
                .filter(|r| r.kind == "delivery_uncertain")
                .count(),
            usize::from(matches!(point, "reserved" | "intent" | "effect"))
        );
    }
}

#[test]
fn a_lost_hosted_answer_reads_boundedly_then_settles_without_another_original() {
    for unreadable in [false, true] {
        let mut h = Harness::empty();
        let mut create = create();
        if let Operation::Create { bundle, .. } = &mut create {
            bundle.roles.iter_mut().find(|r| r.id == "developer").unwrap().agent = "dsh".into();
        }
        h.run(&person(), create).unwrap();
        h.bind("m1");
        h.add(&person(), 1);
        let home = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
        let mut p = Fake::new();
        p.recovery_unreadable = unreadable;
        super::super::delivery::CRASH.with(|c| c.set(Some("effect")));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(&mut h,&mut a,&mut p,100,false))).is_err());
        drop(a);
        let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
        for pass in 1..=6 { tick(&mut h,&mut a,&mut p, pass * 900_000, false); }
        assert_eq!(p.sends, 1);
        assert_eq!(p.recovery_reads, if unreadable { 3 } else { 1 });
        let inbox = h.store.inbox(TEAM, 200, None).unwrap();
        assert_eq!(inbox.rows.iter().any(|item| item.kind == "delivery_uncertain"), unreadable);
    }
}
#[test]
fn unknown_fleet_or_external_task_occupancy_never_qualifies_a_member() {
    let (mut h, mut a, mut p) = setup(2);
    p.free.clear();
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.sends, 0);
    assert!(h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
}
#[test]
fn new_journal_generation_observes_all_live_nodes_once() {
    let (mut h, mut a, mut p) = setup(3);
    p.free.clear();
    tick(&mut h, &mut a, &mut p, 100, false);
    let before = p.reads;
    let old = super::super::observe::position_fixture("other-generation");
    a.put(TEAM, "journal", &old).unwrap();
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.reads, before + 3);
    tick(&mut h, &mut a, &mut p, 102, false);
    assert_eq!(p.reads, before + 3);
}
#[test]
fn a_return_reserves_and_delivers_a_new_generation_once() {
    let (mut h, mut a, mut p) = setup(1);
    tick(&mut h, &mut a, &mut p, 100, false);
    h.observe(1, TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node: 1,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    let lead = h.bind("lead");
    h.run(
        &lead,
        Operation::Return {
            node: 1,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 2);
    assert_eq!(h.store.generation(TEAM, 1).unwrap(), Some(2));
    tick(&mut h, &mut a, &mut p, 102, false);
    assert_eq!(p.sends, 2);
}
#[test]
fn journal_hint_observes_only_the_changed_live_task() {
    let (mut h, mut a, mut p) = setup(3);
    p.free.clear();
    tick(&mut h, &mut a, &mut p, 100, false);
    let before = p.reads;
    p.documents.insert(h.node(2).task.unwrap().uuid);
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.reads, before + 1);
    tick(&mut h, &mut a, &mut p, 102, false);
    assert_eq!(p.reads, before + 1);
}
#[test]
fn failed_observation_continues_other_rows_and_obeys_backoff() {
    let (mut h, mut a, mut p) = setup(3);
    p.free.clear();
    p.failed_task = Some(h.node(1).task.unwrap().uuid);
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.reads, 3);
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(
        p.reads, 5,
        "failed row must stay backed off while other rows progress"
    );
}
#[test]
fn one_journal_read_serves_two_teams_in_the_same_project() {
    let (mut h, mut a, mut p) = setup(1);
    p.free.clear();
    let mut op = create();
    if let Operation::Create {
        integration_branch, ..
    } = &mut op
    {
        *integration_branch = "teams/other".into();
    }
    h.run_for(TEAM_B, &person(), op).unwrap();
    let teams = h.store.live_teams(16, None).unwrap().rows;
    a.prepare(&teams).unwrap();
    for t in teams {
        a.tick(&mut h.store, &mut p, &t.id, 100, false).unwrap();
    }
    assert_eq!(p.journals, 1);
}
#[test]
fn retry_decision_runs_the_row_early_and_its_next_failure_opens_one_new_card() {
    let (mut h, mut a, mut p) = setup(1);
    p.free.clear();
    p.failed_task = Some(h.node(1).task.unwrap().uuid);
    for now in [100, 60_100, 180_100] {
        tick(&mut h, &mut a, &mut p, now, false);
    }
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    assert_eq!(cards.len(), 1);
    assert_eq!(
        cards[0].decisions,
        ["retry", "acknowledged"],
        "the decision the gate reads, then the one that only closes the card"
    );
    let first = cards[0].seq;
    h.run(
        &person(),
        Operation::DecideCard {
            card: first,
            decision: "retry".into(),
        },
    )
    .unwrap();
    tick(&mut h, &mut a, &mut p, 180_101, false);
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    assert_eq!(cards.len(), 2);
    assert_eq!(cards.iter().filter(|c| c.state == "open").count(), 1);
    p.failed_task = None;
    tick(&mut h, &mut a, &mut p, 660_102, false);
    assert!(h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .iter()
        .all(|c| c.state != "open"));
}
/// The gate's card lists retry, then acknowledged. Acknowledged closes it:
/// the gate keeps the closed card, so no card opens again while the gate
/// stays, and the automatic checks go on by their backoff. After the gate
/// recovers, a later failure opens a new card whose Retry is read as before.
#[test]
fn an_acknowledged_gate_card_stays_closed_while_the_gate_backs_off() {
    let (mut h, mut a, mut p) = setup(1);
    p.free.clear();
    p.failed_task = Some(h.node(1).task.unwrap().uuid);
    for now in [100, 60_100, 180_100] {
        tick(&mut h, &mut a, &mut p, now, false);
    }
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    assert_eq!(cards.len(), 1);
    let first = cards[0].seq;
    h.run(
        &person(),
        Operation::DecideCard {
            card: first,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    let card = h.store.card(TEAM, first).unwrap().unwrap();
    assert_eq!(
        (card.state.as_str(), card.decision.as_deref()),
        ("closed", Some("acknowledged"))
    );
    // Not run early: the third failure backs off four minutes.
    let reads = p.reads;
    tick(&mut h, &mut a, &mut p, 180_101, false);
    tick(&mut h, &mut a, &mut p, 420_099, false);
    assert_eq!(p.reads, reads, "acknowledged is not a retry");
    // The automatic checks go on, and no card opens again.
    tick(&mut h, &mut a, &mut p, 420_100, false);
    assert_eq!(p.reads, reads + 1, "the backoff's check");
    tick(&mut h, &mut a, &mut p, 900_100, false);
    assert_eq!(p.reads, reads + 2, "the next check");
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    assert_eq!(cards.len(), 1, "no card opens again while the gate stays");
    assert_eq!(cards[0].decision.as_deref(), Some("acknowledged"));
    // The gate recovers; the acknowledged card stays as it was.
    p.failed_task = None;
    tick(&mut h, &mut a, &mut p, 1_800_100, false);
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].decision.as_deref(), Some("acknowledged"));
    // A later failure opens a new card, and its Retry is read as before.
    p.failed_task = Some(h.node(1).task.unwrap().uuid);
    for now in [1_800_101, 1_860_101, 1_980_101] {
        p.documents.insert(h.node(1).task.unwrap().uuid);
        tick(&mut h, &mut a, &mut p, now, false);
    }
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    assert_eq!(cards.len(), 2, "a later failure opens a new card");
    let second = cards.iter().find(|c| c.state == "open").unwrap().seq;
    h.run(
        &person(),
        Operation::DecideCard {
            card: second,
            decision: "retry".into(),
        },
    )
    .unwrap();
    let reads = p.reads;
    tick(&mut h, &mut a, &mut p, 1_980_102, false);
    assert_eq!(p.reads, reads + 1, "the retry runs the row early");
}
#[test]
fn an_uncertain_reopen_is_not_sent_twice_and_stays_in_its_new_generation() {
    let (mut h, mut a, mut p) = setup(1);
    tick(&mut h, &mut a, &mut p, 100, false);
    h.observe(1, TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node: 1,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    let lead = h.bind("lead");
    h.run(
        &lead,
        Operation::Return {
            node: 1,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    p.reply = HandoutOutcome::Uncertain {
        reason: "Lost receipt".into(),
    };
    tick(&mut h, &mut a, &mut p, 101, false);
    drop(a);
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    for now in [102, 60_101, 600_101] {
        tick(&mut h, &mut a, &mut p, now, false);
    }
    assert_eq!(p.sends, 2);
    assert_eq!(h.store.generation(TEAM, 1).unwrap(), Some(2));
    assert_eq!(
        h.store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .filter(|r| r.kind == "delivery_uncertain")
            .count(),
        1
    );
}
#[test]
fn failed_durable_outcome_save_never_repeats_the_original_delivery() {
    let (mut h, mut a, mut p) = setup(1);
    p.lock_after_send = Some(h.temp.path().join("teams/actor-v1.sqlite3"));
    a.begin_pass();
    assert!(a.tick(&mut h.store, &mut p, TEAM, 100, false).is_err());
    assert_eq!(p.sends, 1);
    drop(p.held.take());
    drop(a);
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    tick(&mut h, &mut a, &mut p, 101, false);
    tick(&mut h, &mut a, &mut p, 102, false);
    assert_eq!(p.sends, 1);
    assert_eq!(
        h.store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .filter(|r| r.kind == "delivery_uncertain")
            .count(),
        1
    );
}
#[test]
fn saved_intent_refusal_removes_its_unsent_reservation_and_allows_a_later_choice() {
    let (mut h, mut a, mut p) = setup(1);
    p.pause_on_recheck = Some(h.temp.path().to_owned());
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.sends, 0);
    assert!(
        a.ledger
            .page(TEAM, "")
            .unwrap()
            .iter()
            .all(|r| !r.key.starts_with("delivery:")),
        "the saved refusal must remove the unsent original"
    );
    h.run(
        &person(),
        Operation::ResumeMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 0);
    tick(&mut h, &mut a, &mut p, 60_101, false);
    assert_eq!(p.sends, 1);
}
#[test]
fn a_fault_of_a_known_row_does_not_become_a_worker_health_fault() {
    let (mut h, mut a, mut p) = setup(1);
    p.free.clear();
    a.put(
        TEAM,
        "gate:observe:1",
        &serde_json::json!({"unreadable":"fixture"}),
    )
    .unwrap();
    let teams = h.store.live_teams(16, None).unwrap().rows;
    assert!(a.page(&mut h.store, &mut p, &teams, 100, false).is_ok());
    assert_eq!(p.sends, 0);
}
#[test]
fn cooling_refused_tasks_do_not_claim_members_from_other_ready_tasks() {
    let (mut h, mut a, mut p) = setup(2);
    p.reply = HandoutOutcome::Refused {
        reason: "Task writer refused".into(),
    };
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.sends, 2);
    let node = h.add(&person(), 3);
    p.reply = HandoutOutcome::Submitted {
        receipt: "new task".into(),
    };
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 3);
    assert!(h.node(node).implementer.is_some());
}
#[test]
fn a_full_actor_ledger_is_a_visible_sweep_fault_before_any_new_delivery() {
    let (mut h, mut a, mut p) = setup(1);
    p.free.clear();
    let teams = h.store.live_teams(16, None).unwrap().rows;
    a.page(&mut h.store, &mut p, &teams, 100, false).unwrap();
    let db = rusqlite::Connection::open(h.temp.path().join("teams/actor-v1.sqlite3")).unwrap();
    db.execute("UPDATE actor_budget SET rows=8192 WHERE id=1", [])
        .unwrap();
    p.free.insert("m1".into());
    let error = a
        .page(&mut h.store, &mut p, &teams, 101, false)
        .unwrap_err();
    assert_eq!(error.code, super::super::ledger::CAPACITY);
    assert_eq!(p.sends, 0);
}

#[test]
fn delivered_reopen_closes_its_old_retry_card_and_gate() {
    let (mut h, mut a, mut p) = setup(2);
    p.free = ["m1".to_string()].into();
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.sends, 1, "node 1 to m1");
    h.observe(1, TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node: 1,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 2, "node 2 to m1, which is now busy");
    let lead = h.bind("lead");
    h.run(
        &lead,
        Operation::Return {
            node: 1,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    let seq = h
        .store
        .pending_reopens(TEAM, 200, None)
        .unwrap()
        .rows
        .first()
        .unwrap()
        .seq;
    // m1 is busy on node 2: the store refuses the reopen intent three times.
    for now in [102, 60_102, 180_102, 420_102] {
        tick(&mut h, &mut a, &mut p, now, false);
    }
    let open_before: Vec<String> = h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|c| c.state == "open")
        .map(|c| c.text)
        .collect();
    // Node 2 is reported: m1 is no longer busy, and the reopen is delivered.
    h.observe(2, TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node: 2,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    for now in [2_000_000, 2_060_000, 2_200_000] {
        tick(&mut h, &mut a, &mut p, now, false);
    }
    let open_after: Vec<String> = h
        .store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|c| c.state == "open")
        .map(|c| c.text)
        .collect();
    let gate = a.ledger.get(TEAM, &format!("gate:reopen:{seq}")).unwrap();
    assert!(!open_before.is_empty());
    assert!(
        open_after.is_empty(),
        "recovered reopen still shows cards: {open_after:?}"
    );
    assert!(gate.is_none(), "settled reopen kept its retry gate");
    assert_eq!(p.sends, 3, "the reopen was delivered once");
}

#[test]
fn a_new_worker_epoch_observes_all_live_tasks_without_a_journal_change() {
    let (mut h, mut a, mut p) = setup(3);
    p.free.clear();
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.reads, 3);
    drop(a);
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.reads, 6);
    tick(&mut h, &mut a, &mut p, 102, false);
    assert_eq!(p.reads, 6);
}
#[test]
fn recovery_adopts_an_open_store_effect_without_a_local_ledger_row() {
    let (mut h, mut a, mut p) = setup(1);
    h.intent(1, "m1");
    tick(&mut h, &mut a, &mut p, 100, false);
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 0);
    let e = h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .remove(0);
    assert!(matches!(
        serde_json::from_value::<HandoutOutcome>(e.outcome.unwrap()).unwrap(),
        HandoutOutcome::Uncertain { .. }
    ));
    assert_eq!(
        h.store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .iter()
            .filter(|i| i.kind == "delivery_uncertain")
            .count(),
        1
    );
}
#[test]
fn an_unsaved_intent_error_preserves_the_original_reservation() {
    let (mut h, mut a, mut p) = setup(1);
    let db = rusqlite::Connection::open(h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let n = h.store.node(TEAM, 1).unwrap().unwrap();
    let m = h.store.member(TEAM, "m1").unwrap().unwrap();
    assert!(a
        .send(&mut h.store, &mut p, &t, (&n, &m), "handout", false, 100)
        .is_err());
    assert_eq!(p.sends, 0);
    assert!(
        a.ledger.pending_node(TEAM, 1).unwrap(),
        "an unread refusal was taken as saved"
    );
    db.execute_batch("ROLLBACK").unwrap();
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 0);
}

#[test]
fn a_reopen_waits_while_its_holder_is_busy_outside_the_team() {
    let (mut h, mut a, mut p) = setup(1);
    tick(&mut h, &mut a, &mut p, 100, false);
    h.observe(1, TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node: 1,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    let lead = h.bind("lead");
    h.run(
        &lead,
        Operation::Return {
            node: 1,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    p.free.clear();
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 1, "a busy holder was sent the reopen");
}

#[test]
fn stale_delivery_observes_then_backs_off_across_restart_without_a_refusal_or_card() {
    let (mut h, mut a, mut p) = setup(1);
    p.stale = true;
    tick(&mut h, &mut a, &mut p, 100, false);
    let reads = p.reads;
    assert_eq!(p.sends, 1);
    assert!(h.store.effects(TEAM, 16, None).unwrap().rows[0]
        .outcome
        .is_none());
    assert!(h.store.cards(TEAM, 16, None).unwrap().rows.is_empty());
    let machine = a.machine.clone();
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(p.sends, 1);
    let after_epoch_observation = p.reads;
    assert!(after_epoch_observation >= reads);
    tick(&mut h, &mut a, &mut p, 60_100, false);
    assert_eq!(p.sends, 2);
    assert!(
        p.reads > after_epoch_observation,
        "stale answer did not observe its task"
    );
    tick(&mut h, &mut a, &mut p, 120_100, false);
    assert_eq!(
        p.sends, 2,
        "the second stale answer did not increase its backoff"
    );
    assert!(h.store.cards(TEAM, 16, None).unwrap().rows.is_empty());
    assert!(h.store.effects(TEAM, 16, None).unwrap().rows[0]
        .outcome
        .is_none());
}

#[test]
fn a_failed_observation_after_stale_delivery_still_keeps_the_delivery_backoff() {
    let (mut h, mut a, mut p) = setup(1);
    p.stale = true;
    p.fail_observation_after_send = true;
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.sends, 1);
    assert!(h.store.effects(TEAM, 16, None).unwrap().rows[0]
        .outcome
        .is_none());
    tick(&mut h, &mut a, &mut p, 101, false);
    assert_eq!(
        p.sends, 1,
        "failed observation bypassed the delivery's backoff"
    );
    p.failed_task = None;
    p.fail_observation_after_send = false;
    tick(&mut h, &mut a, &mut p, 60_100, false);
    assert_eq!(p.sends, 2);
}

#[test]
fn person_override_closing_an_interrupted_effect_never_resends_or_binds_the_old_outcome() {
    for point in ["effect", "outcome"] {
        let (mut h, mut a, mut p) = setup(1);
        super::super::delivery::CRASH.with(|s| s.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(
                &mut h, &mut a, &mut p, 100, false
            )))
            .is_err()
        );
        assert_eq!(p.sends, 1);
        let mut observation = h.observation(1, TaskStage::InProgress);
        observation.attempt = 2;
        observation.attempt_id = "person-started-elsewhere".into();
        h.run(
            &service(),
            Operation::ObserveTask {
                node: 1,
                observation,
            },
        )
        .unwrap();
        assert_eq!(h.store.generation(TEAM, 1).unwrap(), Some(2));
        assert!(h
            .store
            .open_effects(TEAM, 200, None)
            .unwrap()
            .rows
            .is_empty());
        let home = a.machine.clone();
        drop(a);
        let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
        let team = h.store.team(TEAM).unwrap().unwrap();
        for now in [101, 60_100] {
            a.recover(&mut h.store, &mut p, &team, now).unwrap();
        }
        assert_eq!(p.sends, 1);
        assert_eq!(h.store.generation(TEAM, 1).unwrap(), Some(2));
        assert!(h.node(1).assignee.is_none());
        assert!(!a
            .ledger
            .page(TEAM, "")
            .unwrap()
            .iter()
            .any(|r| r.key.starts_with("delivery:")));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_terminal_settle_error_after_the_live_page_is_a_row_fault_not_sweep_health() {
    let (mut h, mut a, mut p) = setup(1);
    p.stale = true;
    tick(&mut h, &mut a, &mut p, 100, false);
    let page = h.store.live_teams(16, None).unwrap().rows;
    assert!(
        a.ledger.prefix_page(TEAM, "delivery:", "").unwrap()[0].data["not_started"]
            .as_bool()
            .unwrap()
    );
    h.run(&person(), Operation::Stop).unwrap();
    h.store.db.as_ref().unwrap().execute_batch("CREATE TRIGGER fail_settle BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'fixture settle storage failed'); END;").unwrap();
    a.page(&mut h.store, &mut p, &page, 101, false).unwrap();
    assert_eq!(p.sends, 1);
    assert!(a.ledger.terminal_retry().unwrap().is_some());
    assert!(crate::desktop::teams::health(h.temp.path()).is_none());
}

/// What the actor leaves once it settled a reopen delivery's outcome
/// (zerus-63rru.64): the failure gate of its node, the sends, the effect's
/// state and the inbox, after later passes too.
#[derive(Debug)]
pub(super) struct SettledReopen {
    pub(super) gate: Option<serde_json::Value>,
    pub(super) sends: usize,
    pub(super) effect: String,
    pub(super) inbox: Vec<String>,
}

/// The actor delivers a reopen of node 1 to m1 that answers `outcome`, then
/// makes two later passes, past the first retry delay.
pub(super) fn settle_reopen(outcome: HandoutOutcome) -> SettledReopen {
    let (mut h, mut a, mut p) = setup(1);
    tick(&mut h, &mut a, &mut p, 100, false);
    assert_eq!(p.sends, 1, "the hand-out of node 1 to m1");
    h.observe(1, TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node: 1,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: COMMIT.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
    let lead = h.bind("lead");
    h.run(
        &lead,
        Operation::Return {
            node: 1,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    p.reply = outcome;
    for now in [101, 60_101, 600_101] {
        tick(&mut h, &mut a, &mut p, now, false);
    }
    let effect = h
        .store
        .effects(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .rev()
        .find(|e| e.kind == "reopen")
        .map(|e| e.state)
        .expect("the reopen effect");
    SettledReopen {
        gate: a
            .ledger
            .get(TEAM, "gate:handout:1")
            .unwrap()
            .map(|row| row.data),
        sends: p.sends,
        effect,
        inbox: h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .map(|item| item.kind)
            .collect(),
    }
}

/// The helper's own control: a reopen the member refuses opens the node's
/// failure gate, which `settle_reopen` reads.
#[test]
fn a_refused_reopen_opens_the_failure_gate_the_hand_back_witness_reads() {
    let settled = settle_reopen(HandoutOutcome::Refused {
        reason: "Busy elsewhere".into(),
    });
    assert!(settled.gate.is_some(), "{settled:?}");
}
