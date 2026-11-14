use super::super::{
    cancel::{Answer, Attempt, Fact, Pending},
    ports::{Ports, TaskReply},
};
use super::*;
#[derive(Default)]
struct Fake {
    pending: Vec<Pending>,
    calls: Vec<u32>,
    facts: Vec<Fact>,
    reply: Option<TaskReply>,
    terminal: usize,
    reads: usize,
    move_pair: bool,
    answer_pair: Option<Attempt>,
    missing_pair: bool,
    keep_once: bool,
    always_changed: bool,
    keep_observed: bool,
    keep_recorded: bool,
    answered_cancelled: bool,
    fail_observe: bool,
    crashes_before_spawn: u8,
}
impl Ports for Fake {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
        self.reads += 1;
        if self.fail_observe {
            return Err(super::super::problem("fixture observation unavailable"));
        }
        if self.answered_cancelled && !self.keep_observed {
            self.pending.clear();
        }
        if self.move_pair {
            for p in &mut self.pending {
                p.attempt += 1;
                p.attempt_id = format!("observed-{}", p.attempt);
            }
        }
        Ok(())
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        Ok(false)
    }
    fn deliver(
        &mut self,
        _: &Team,
        _: &Node,
        _: &Member,
        _: &str,
    ) -> super::super::ports::DeliveryResult {
        panic!("No delivery")
    }
    fn cancels(&mut self, _: &Store, _: &str, after: u32) -> Result<Vec<Pending>> {
        Ok(self
            .pending
            .iter()
            .filter(|p| p.node > after)
            .take(16)
            .cloned()
            .collect())
    }
    fn cancel(&mut self, _: &Team, p: &Pending, _: &str) -> Result<Answer> {
        if self.crashes_before_spawn > 0 {
            self.crashes_before_spawn -= 1;
            panic!("fixture crash before spawn");
        }
        self.calls.push(p.node);
        self.answered_cancelled = matches!(self.reply, None | Some(TaskReply::Cancelled));
        Ok(Answer {
            reply: self.reply.clone().unwrap_or(TaskReply::Cancelled),
            sentence: String::new(),
            attempt: (!self.missing_pair).then(|| {
                self.answer_pair.clone().unwrap_or(Attempt {
                    number: p.attempt,
                    id: p.attempt_id.clone(),
                })
            }),
        })
    }
    fn cancel_fact_outcome(
        &mut self,
        _: &mut Store,
        _: &Team,
        f: &Fact,
        _: i64,
    ) -> Result<super::super::cancel::FactOutcome> {
        self.facts.push(f.clone());
        if (self.keep_once || self.always_changed) && !f.refused {
            self.keep_once = false;
            self.pending[0].attempt += 1;
            self.pending[0].attempt_id = format!("person-attempt-{}", self.pending[0].attempt);
            return Ok(super::super::cancel::FactOutcome::Changed {
                reason: "The observed execution changed".into(),
            });
        } else if !self.keep_recorded || f.refused {
            self.pending.retain(|p| p.decision != f.pending.decision);
        }
        Ok(super::super::cancel::FactOutcome::Recorded)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn terminal_timers(
        &mut self,
        _: &mut super::super::auxiliary::State,
        _: &mut Store,
        _: i64,
    ) -> Result<()> {
        assert!(
            self.pending.is_empty(),
            "a key was cleared before cancellation settled"
        );
        self.terminal += 1;
        Ok(())
    }
}
fn setup() -> (Harness, super::super::Runtime, Fake) {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let pending = Pending {
        node,
        generation: 1,
        decision: 1,
        task: h.node(node).task,
        attempt: 0,
        attempt_id: String::new(),
        attempt_conflict: Some(false),
    };
    (
        h,
        a,
        Fake {
            pending: vec![pending],
            ..Default::default()
        },
    )
}
#[test]
fn the_tick_consumes_cancels_and_keeps_their_distinct_final_state() {
    for reply in [TaskReply::Cancelled, TaskReply::Released] {
        let (mut h, mut a, mut p) = setup();
        let released = reply == TaskReply::Released;
        p.reply = Some(reply);
        a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
        assert_eq!(p.calls, vec![1]);
        assert_eq!(p.facts.len(), usize::from(released));
        if released {
            assert!(!p.facts[0].refused);
            assert!(p.facts[0].released);
        } else {
            assert_eq!(p.reads, 2);
        }
        a.tick(&mut h.store, &mut p, TEAM, 101, false).unwrap();
        assert_eq!(p.calls.len(), 1);
    }
}
#[test]
fn a_no_task_cancel_records_without_a_child() {
    let (mut h, mut a, mut p) = setup();
    p.pending[0].task = None;
    a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
    assert_eq!(p.facts.len(), 1);
    assert!(p.calls.is_empty());
}
#[test]
fn released_cancel_restarts_before_and_after_each_effect_keep_the_known_answer() {
    for point in ["reserved", "effect", "outcome", "recorded"] {
        let (mut h, mut a, mut p) = setup();
        p.reply = Some(TaskReply::Released);
        super::super::cancel::CRASH.with(|s| s.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.cancels(
                &mut h.store,
                &mut p,
                &h_team(),
                100
            )))
            .is_err(),
            "{point}"
        );
        let machine = a.machine.clone();
        drop(a);
        let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
        a.cancels(&mut h.store, &mut p, &h_team(), 101).unwrap();
        assert!(p.pending.is_empty(), "{point}");
        assert_eq!(
            p.calls.len(),
            if point == "effect" { 2 } else { 1 },
            "{point}"
        );
        assert!(p
            .facts
            .iter()
            .all(|f| f.request_id == p.facts[0].request_id));
    }
}
fn h_team() -> Team {
    fixture().team
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_stop_runs_cancel_before_the_terminal_entry() {
    let (mut h, mut a, mut p) = setup();
    h.run(&person(), crate::teams::ops::Operation::Stop)
        .unwrap();
    a.page(&mut h.store, &mut p, &[], 100, false).unwrap();
    assert_eq!(p.calls, vec![1]);
    assert!(p.pending.is_empty());
    assert_eq!(p.terminal, 1);
}
#[test]
fn a_terminal_cancel_without_an_answer_backs_off_without_sweep_health() {
    let (mut h, mut a, mut p) = setup();
    h.run(&person(), crate::teams::ops::Operation::Stop)
        .unwrap();
    p.reply = Some(TaskReply::CuttingOver {
        reason: "cutting over".into(),
    });
    a.terminal_cancels(&mut h.store, &mut p, 100).unwrap();
    a.terminal_cancels(&mut h.store, &mut p, 101).unwrap();
    assert_eq!(p.calls.len(), 1);
    p.reply = Some(TaskReply::Refused {
        reason: "delivery is unknown".into(),
    });
    a.terminal_cancels(&mut h.store, &mut p, 60_100).unwrap();
    assert_eq!(p.calls.len(), 2);
    assert!(p.pending.is_empty());
    assert!(p.facts[0].refused);
    assert!(crate::desktop::teams::health(h.temp.path()).is_none());
}

#[test]
fn stale_cancel_observes_and_uses_the_new_pair_after_its_backoff_without_a_card() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::Stale);
    p.move_pair = true;
    a.cancels(&mut h.store, &mut p, &h_team(), 100).unwrap();
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.reads, 1);
    assert!(p.facts.is_empty());
    a.cancels(&mut h.store, &mut p, &h_team(), 101).unwrap();
    assert_eq!(p.calls.len(), 1);
    a.cancels(&mut h.store, &mut p, &h_team(), 60_100).unwrap();
    assert_eq!(p.calls.len(), 2);
    assert_eq!(p.reads, 2);
    a.cancels(&mut h.store, &mut p, &h_team(), 120_100).unwrap();
    assert_eq!(p.calls.len(), 2);
    assert!(p.facts.is_empty());
    assert!(h.store.cards(TEAM, 16, None).unwrap().rows.is_empty());
}
#[test]
fn a_cancel_fact_uses_the_reply_pair_instead_of_the_pending_pair() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::Released);
    p.answer_pair = Some(Attempt {
        number: 7,
        id: "actual-reply".into(),
    });
    a.cancels(&mut h.store, &mut p, &h_team(), 100).unwrap();
    assert_eq!(p.facts.len(), 1);
    assert_eq!(p.facts[0].attempt, 7);
    assert_eq!(p.facts[0].attempt_id, "actual-reply");
    assert_eq!(p.facts[0].pending.attempt, 0);
}

#[test]
fn a_saved_answer_whose_attempt_moved_ends_the_visit_without_a_second_child() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::Released);
    super::super::cancel::CRASH.with(|s| s.set(Some("outcome")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.cancels(
            &mut h.store,
            &mut p,
            &h_team(),
            100
        )))
        .is_err()
    );
    p.keep_once = true;
    let machine = a.machine.clone();
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    a.cancels(&mut h.store, &mut p, &h_team(), 101).unwrap();
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.facts.len(), 1);
    assert_eq!(p.pending.len(), 1);
    a.cancels(&mut h.store, &mut p, &h_team(), 102).unwrap();
    assert_eq!(p.calls.len(), 2);
    assert!(p.pending.is_empty());
}
#[test]
fn a_stale_cancel_keeps_its_backoff_across_a_restart() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::Stale);
    a.cancels(&mut h.store, &mut p, &h_team(), 100).unwrap();
    let machine = a.machine.clone();
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    a.cancels(&mut h.store, &mut p, &h_team(), 101).unwrap();
    assert_eq!(p.calls.len(), 1);
    p.reply = Some(TaskReply::Released);
    a.cancels(&mut h.store, &mut p, &h_team(), 60_100).unwrap();
    assert_eq!(p.calls.len(), 2);
    assert_eq!(p.facts.len(), 1);
    assert!(h.store.cards(TEAM, 16, None).unwrap().rows.is_empty());
}
#[test]
fn a_cancel_success_without_attempt_evidence_never_records_a_fact() {
    let (mut h, mut a, mut p) = setup();
    p.missing_pair = true;
    for now in [100, 101] {
        a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
    }
    assert_eq!(p.calls.len(), 1);
    assert!(p.facts.is_empty());
    assert!(h.store.cards(TEAM, 16, None).unwrap().rows.is_empty());
    for now in [60_100, 180_100] {
        a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
    }
    assert_eq!(p.calls.len(), 3);
    assert!(p.facts.is_empty());
    assert_eq!(h.store.cards(TEAM, 16, None).unwrap().rows.len(), 1);
}
#[test]
fn a_legacy_saved_cancel_without_a_pair_is_settled_by_observation_only() {
    let (mut h, mut a, mut p) = setup();
    p.answered_cancelled = true;
    let key = format!("cancel:{}", p.pending[0].decision);
    a.put(
        TEAM,
        &key,
        &json!({"pending":p.pending[0],"reply":"Cancelled",
        "fact":null,"failures":0,"next_ms":0,"reason":""}),
    )
    .unwrap();
    let machine = a.machine.clone();
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    for now in [100, 101, 60_100] {
        a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
    }
    assert_eq!(p.reads, 1);
    assert!(p.calls.is_empty());
    assert!(p.facts.is_empty());
    assert!(a.ledger.get(TEAM, &key).unwrap().is_none());
}

#[test]
fn a_legacy_reserved_live_cancel_fact_is_retired_without_reusing_its_request() {
    let (mut h, mut a, mut p) = setup();
    p.answered_cancelled = true;
    let key = format!("cancel:{}", p.pending[0].decision);
    let id = uuid::Uuid::new_v4().to_string();
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    a.put(
        TEAM,
        &key,
        &json!({"pending":p.pending[0],"reply":"Released",
        "attempt":{"number":0,"id":""},"fact":{"request_id":id,"revision":revision,
        "pending":p.pending[0],"refused":false,"reason":"","attempt":0,"attempt_id":""},
        "failures":0,"next_ms":0,"reason":""}),
    )
    .unwrap();
    a.cancels(&mut h.store, &mut p, &h_team(), 100).unwrap();
    assert!(p.calls.is_empty());
    assert!(
        p.facts.is_empty(),
        "a legacy fact is not rewritten under its old or a new ID"
    );
    assert_eq!(p.reads, 1);
    assert!(a.ledger.get(TEAM, &key).unwrap().is_none());
}

#[test]
fn live_cancel_restarts_before_and_after_the_observation_never_record_an_end_fact() {
    for point in ["reserved", "effect", "before_observe", "observed"] {
        let (mut h, mut a, mut p) = setup();
        super::super::cancel::CRASH.with(|s| s.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.cancels(
                &mut h.store,
                &mut p,
                &h_team(),
                100
            )))
            .is_err(),
            "{point}"
        );
        let machine = a.machine.clone();
        drop(a);
        let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
        a.cancels(&mut h.store, &mut p, &h_team(), 101).unwrap();
        assert!(p.pending.is_empty(), "{point}");
        assert_eq!(
            p.calls.len(),
            if point == "effect" { 2 } else { 1 },
            "{point}"
        );
        assert_eq!(p.reads, 1, "{point}");
        assert!(p.facts.is_empty(), "{point}");
    }
}
#[test]
fn a_failed_live_cancel_observation_backs_off_and_retries_observation_without_a_child() {
    let (mut h, mut a, mut p) = setup();
    p.fail_observe = true;
    a.cancels(&mut h.store, &mut p, &h_team(), 100).unwrap();
    a.cancels(&mut h.store, &mut p, &h_team(), 101).unwrap();
    assert_eq!(p.calls.len(), 1);
    assert!(p.facts.is_empty());
    p.fail_observe = false;
    a.cancels(&mut h.store, &mut p, &h_team(), 60_100).unwrap();
    assert_eq!(p.calls.len(), 1);
    assert!(p.pending.is_empty());
    assert!(p.facts.is_empty());
}

#[test]
fn a_cached_release_without_a_tuple_is_observed_without_recording_a_default_pair() {
    let (mut h, mut a, mut p) = setup();
    p.answered_cancelled = true;
    let key = format!("cancel:{}", p.pending[0].decision);
    a.put(
        TEAM,
        &key,
        &json!({"pending":p.pending[0],"reply":"Released","fact":null,
        "failures":0,"next_ms":0,"reason":""}),
    )
    .unwrap();
    for now in [100, 101, 60_100] {
        a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
    }
    assert_eq!(p.reads, 1);
    assert!(p.calls.is_empty());
    assert!(p.facts.is_empty());
    assert!(a.ledger.get(TEAM, &key).unwrap().is_none());
}
#[test]
fn an_invalid_terminal_stale_reply_backs_off_without_observing_or_recording_a_refusal() {
    let (mut h, mut a, mut p) = setup();
    h.run(&person(), crate::teams::ops::Operation::Stop)
        .unwrap();
    p.reply = Some(TaskReply::Stale);
    a.terminal_cancels(&mut h.store, &mut p, 100).unwrap();
    a.terminal_cancels(&mut h.store, &mut p, 101).unwrap();
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.reads, 0);
    assert!(p.facts.is_empty());
    p.reply = Some(TaskReply::Refused {
        reason: "attempt_changed: the person resumed it".into(),
    });
    a.terminal_cancels(&mut h.store, &mut p, 60_100).unwrap();
    assert_eq!(p.calls.len(), 2);
    assert_eq!(p.reads, 0);
    assert!(p.facts[0].refused);
    assert_eq!(p.facts[0].reason, "attempt_changed: the person resumed it");
}

#[test]
fn three_live_stale_answers_with_unchanged_observations_have_no_fourth_child_in_a_hundred_passes() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::Stale);
    for pass in 0..100 {
        let machine = a.machine.clone();
        if pass > 0 {
            drop(a);
            a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
        }
        a.cancels(&mut h.store, &mut p, &h_team(), 100 + pass * 900_000)
            .unwrap();
    }
    assert_eq!(p.calls.len(), 3);
    assert!(p.pending.is_empty());
    assert_eq!(p.facts.len(), 1);
    assert!(p.facts[0].refused);
    assert!(p.facts[0].reason.contains("stale"));
}

#[test]
fn a_cancel_pass_spends_at_most_four_effects_and_resumes_the_rest() {
    let (mut h, mut a, mut p) = setup();
    let template = p.pending[0].clone();
    p.pending = (1..=9)
        .map(|node| Pending {
            node,
            decision: 100 + u64::from(node),
            task: Some(task(node)),
            ..template.clone()
        })
        .collect();
    p.reply = Some(TaskReply::Released);
    a.cancels(&mut h.store, &mut p, &h_team(), 100).unwrap();
    assert_eq!(p.calls, vec![1, 2, 3, 4]);
    assert_eq!(p.pending.len(), 5);
    a.cancels(&mut h.store, &mut p, &h_team(), 101).unwrap();
    assert_eq!(p.calls.len(), 8);
    a.cancels(&mut h.store, &mut p, &h_team(), 102).unwrap();
    assert_eq!(p.calls, (1..=9).collect::<Vec<_>>());
    assert!(p.pending.is_empty());
}

#[test]
fn a_saved_complete_answer_retries_only_its_record_or_observation_while_a_decision_stays_pending() {
    for observed in [false, true] {
        let (mut h, mut a, mut p) = setup();
        p.reply = Some(if observed {
            TaskReply::Cancelled
        } else {
            TaskReply::Released
        });
        p.keep_observed = observed;
        p.keep_recorded = !observed;
        for pass in 0..100 {
            let machine = a.machine.clone();
            drop(a);
            a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
            a.cancels(&mut h.store, &mut p, &h_team(), 100 + pass * 900_000)
                .unwrap();
        }
        assert_eq!(p.calls.len(), 1);
        assert!(p.facts.iter().all(|f| !f.refused));
        let ids: std::collections::BTreeSet<_> = p.facts.iter().map(|f| &f.request_id).collect();
        assert!(ids.len() <= 1);
        assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
    }
}
#[test]
fn a_crash_after_a_stale_answer_before_its_save_can_cost_one_extra_child() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::Stale);
    for now in [100, 60_100] {
        a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
    }
    super::super::cancel::CRASH.with(|s| s.set(Some("effect")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.cancels(
            &mut h.store,
            &mut p,
            &h_team(),
            180_100
        )))
        .is_err()
    );
    let machine = a.machine.clone();
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    for pass in 0..100 {
        a.cancels(&mut h.store, &mut p, &h_team(), 180_101 + pass * 900_000)
            .unwrap();
    }
    assert_eq!(p.calls.len(), 4);
    assert!(p.pending.is_empty());
    assert_eq!(p.facts.len(), 1);
    assert!(p.facts[0].refused);
}

#[test]
fn crashes_before_spawn_do_not_count_as_answers_even_after_each_restart() {
    let (mut h, mut a, mut p) = setup();
    p.crashes_before_spawn = 5;
    for pass in 0..5 {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.cancels(
                &mut h.store,
                &mut p,
                &h_team(),
                100 + pass
            )))
            .is_err()
        );
        let machine = a.machine.clone();
        drop(a);
        a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    }
    assert!(p.calls.is_empty());
    p.reply = Some(TaskReply::Released);
    a.cancels(&mut h.store, &mut p, &h_team(), 106).unwrap();
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.facts.len(), 1);
    assert!(!p.facts[0].refused);
}

#[test]
fn a_cached_stale_answer_counts_once_across_restarts_before_and_after_its_durable_count() {
    for point in ["outcome", "before_stale_count_save", "stale_counted"] {
        let (mut h, mut a, mut p) = setup();
        p.reply = Some(TaskReply::Stale);
        super::super::cancel::CRASH.with(|s| s.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.cancels(
                &mut h.store,
                &mut p,
                &h_team(),
                100
            )))
            .is_err()
        );
        let machine = a.machine.clone();
        drop(a);
        let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
        for now in [101, 60_101, 180_101] {
            a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
        }
        assert_eq!(p.calls.len(), 3, "{point}");
        assert!(p.pending.is_empty());
        assert_eq!(p.facts.len(), 1);
        assert!(p.facts[0].refused);
    }
}

#[test]
fn progress_observed_before_a_crash_is_not_lost_when_the_cached_stale_answer_resumes() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::Stale);
    for now in [100, 60_100] {
        a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
    }
    p.move_pair = true;
    super::super::cancel::CRASH.with(|s| s.set(Some("before_stale_count_save")));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.cancels(
            &mut h.store,
            &mut p,
            &h_team(),
            180_100
        )))
        .is_err()
    );
    p.move_pair = false;
    let machine = a.machine.clone();
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    a.cancels(&mut h.store, &mut p, &h_team(), 180_101).unwrap();
    assert!(p.facts.is_empty(), "the observation already made progress");
    assert!(!p.pending.is_empty());
    let row = a.ledger.prefix_page(TEAM, "cancel:", "").unwrap().remove(0);
    assert_eq!(row.data["unchanged_answers"], 0);
    p.reply = Some(TaskReply::Released);
    a.cancels(&mut h.store, &mut p, &h_team(), 420_101).unwrap();
    assert!(p.pending.is_empty());
    assert_eq!(p.facts.len(), 1);
    assert!(!p.facts[0].refused);
}

#[test]
fn a_cached_stale_baseline_stays_small_with_the_largest_observed_title() {
    let (mut h, mut a, mut p) = setup();
    let mut observation = h.observation(1, crate::teams::ops::TaskStage::Todo);
    observation.title = "\\".repeat(1024);
    h.run(
        &service(),
        crate::teams::ops::Operation::ObserveTask {
            node: 1,
            observation,
        },
    )
    .unwrap();
    p.reply = Some(TaskReply::Stale);
    p.fail_observe = true;
    a.cancels(&mut h.store, &mut p, &h_team(), 100).unwrap();
    let row = a.ledger.prefix_page(TEAM, "cancel:", "").unwrap().remove(0);
    assert_eq!(
        row.data["stale_before"]["observation"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert!(serde_json::to_vec(&row.data).unwrap().len() < 2048);
}

#[test]
fn a_new_observation_is_progress_for_a_cached_stale_answer_even_when_the_pair_matches() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::Stale);
    for now in [100, 60_100] {
        a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
    }
    p.fail_observe = true;
    a.cancels(&mut h.store, &mut p, &h_team(), 180_100).unwrap();
    h.observe(1, crate::teams::ops::TaskStage::InProgress);
    p.fail_observe = false;
    a.cancels(&mut h.store, &mut p, &h_team(), 240_100).unwrap();
    assert!(p.facts.is_empty(), "the store learned the current stage");
    assert_eq!(p.calls.len(), 3);
    let row = a.ledger.prefix_page(TEAM, "cancel:", "").unwrap().remove(0);
    assert_eq!(row.data["unchanged_answers"], 0);
    p.reply = Some(TaskReply::Released);
    a.cancels(&mut h.store, &mut p, &h_team(), 480_100).unwrap();
    assert!(p.pending.is_empty());
    assert_eq!(p.facts.len(), 1);
    assert!(!p.facts[0].refused);
}

/// The service items of the team, as `hgs team status` reads them.
fn service_items(h: &Harness, now: i64) -> Vec<crate::desktop::teams::ServiceItem> {
    crate::desktop::teams::service_items(h.temp.path(), TEAM, Some(true), now)
        .expect("the actor ledger writes service items")
        .items
}
/// A stopped team's cancel that keeps failing is a service item, written
/// with its failing row and cleared when the row ends; a live team's has
/// its card instead and no item.
#[test]
fn a_stopped_teams_failing_cancel_is_a_service_item_until_its_row_ends() {
    let (mut h, mut a, mut p) = setup();
    h.run(&person(), crate::teams::ops::Operation::Stop)
        .unwrap();
    p.reply = Some(TaskReply::CuttingOver {
        reason: "cutting\nover".into(),
    });
    a.terminal_cancels(&mut h.store, &mut p, 100).unwrap();
    let items = service_items(&h, 100);
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(
        (
            item.kind.as_str(),
            item.subject.as_str(),
            item.node,
            item.failures
        ),
        ("cancel_retry", "1", Some(1), 1)
    );
    assert_eq!(item.next_ms, Some(60_100));
    assert!(item.current);
    assert!(!item.reason.contains('\n'), "{}", item.reason);
    let row = a.ledger.get(TEAM, "cancel:1").unwrap().unwrap();
    assert_eq!(row.data["failures"], 1, "the item mirrors its row");
    p.reply = Some(TaskReply::Refused {
        reason: "delivery is unknown".into(),
    });
    a.terminal_cancels(&mut h.store, &mut p, 60_100).unwrap();
    assert!(p.pending.is_empty());
    assert!(a.ledger.get(TEAM, "cancel:1").unwrap().is_none());
    assert!(service_items(&h, 60_100).is_empty(), "cleared with its row");
}
/// A live team's failing cancel has its card, never an item.
#[test]
fn a_live_teams_failing_cancel_writes_no_service_item() {
    let (mut h, mut a, mut p) = setup();
    p.reply = Some(TaskReply::CuttingOver {
        reason: "cutting over".into(),
    });
    for now in [100, 60_100, 180_100] {
        a.cancels(&mut h.store, &mut p, &h_team(), now).unwrap();
    }
    assert!(a.ledger.get(TEAM, "cancel:1").unwrap().is_some());
    assert!(service_items(&h, 180_100).is_empty());
}
