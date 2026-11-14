use super::super::ports::{Ports, TaskReply};
use super::*;
use std::collections::BTreeSet;

pub(super) fn wanted() -> (Harness, u32, u32) {
    let mut h = Harness::empty();
    h.run(&person(), create()).unwrap();
    h.bind("m1");
    h.bind("m3");
    let target = h.add(&person(), 1);
    h.submit(target, "m1");
    h.report(target, "m1");
    let stage = h.store.stage_tasks(TEAM, 1, None).unwrap().rows[0].node;
    (h, target, stage)
}

#[derive(Default)]
struct Fake {
    calls: Vec<String>,
    originals: Vec<String>,
    documents: BTreeSet<String>,
    reads: usize,
    lose_first: bool,
    clears: Vec<u32>,
    timer_teams: Vec<String>,
    answer_teams: Vec<String>,
    fail_answers: bool,
    terminals: usize,
    fail_timers: bool,
    release: Option<(std::path::PathBuf, u32)>,
    clear_reply: Option<TaskReply>,
    stage_reply: Option<TaskReply>,
    lose_clear: bool,
}
impl Ports for Fake {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn team_timers(
        &mut self,
        _: &mut super::super::auxiliary::State,
        _: &mut Store,
        t: &Team,
        _: i64,
    ) -> Result<()> {
        self.timer_teams.push(t.id.clone());
        if self.fail_timers {
            Err(super::super::problem("fixture team timer fault"))
        } else {
            Ok(())
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn answer_timers(
        &mut self,
        _: &mut super::super::auxiliary::State,
        _: &mut Store,
        t: &Team,
        _: i64,
    ) -> Result<()> {
        self.answer_teams.push(t.id.clone());
        if self.fail_answers {
            Err(super::super::problem("fixture answer timer fault"))
        } else {
            Ok(())
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn terminal_timers(
        &mut self,
        _: &mut super::super::auxiliary::State,
        _: &mut Store,
        _: i64,
    ) -> Result<()> {
        self.terminals += 1;
        if self.fail_timers {
            Err(super::super::problem("fixture terminal fault"))
        } else {
            Ok(())
        }
    }

    fn changes(&mut self, _: &str, g: &str, _: u64) -> Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: if g.is_empty() {
                None
            } else {
                Some(Default::default())
            },
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
        self.reads += 1;
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
        panic!("no native deliveries in this fixture")
    }
    fn stage(
        &mut self,
        _: &Team,
        e: &crate::teams::store::StageTaskEffect,
        original: &str,
    ) -> Result<TaskReply> {
        assert!(uuid::Uuid::parse_str(original).is_ok_and(|id| !id.is_nil()));
        self.originals.push(original.into());
        self.calls.push(e.request_id.clone());
        self.documents.insert(e.request_id.clone());
        if let Some((state, target)) = self.release.take() {
            let mut store = Store::open(&state).unwrap();
            let revision = store.team(&e.team).unwrap().unwrap().revision;
            crate::teams::ops::apply(
                &mut store,
                &person(),
                &crate::teams::ops::Request {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    team: e.team.clone(),
                    expected_revision: revision,
                    operation: crate::teams::ops::Operation::Return {
                        node: target,
                        reason: "Revise during task creation".into(),
                    },
                },
                101,
            )
            .unwrap();
        }
        if std::mem::take(&mut self.lose_first) {
            return Err(super::super::problem("child ended after saving its task"));
        }
        Ok(self.stage_reply.clone().unwrap_or(TaskReply::Created))
    }
    fn clear_stage(
        &mut self,
        _: &Team,
        keyed: &crate::teams::store::timers::KeyedTask,
        _: &str,
    ) -> Result<TaskReply> {
        self.clears.push(keyed.node);
        if std::mem::take(&mut self.lose_clear) {
            return Err(super::super::problem("Clear child has no answer"));
        }
        Ok(self.clear_reply.clone().unwrap_or(TaskReply::Cleared))
    }
}
fn tick(h: &mut Harness, a: &mut super::super::Runtime, p: &mut Fake, now: i64) {
    a.begin_pass();
    a.tick(&mut h.store, p, TEAM, now, false).unwrap();
}

#[test]
fn the_stage_slot_creates_one_task_and_an_unchanged_pass_reads_no_document() {
    let (mut h, _, stage) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake::default();
    tick(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.calls.len(), 1);
    assert_eq!(h.node(stage).task.as_ref().unwrap().uuid, p.calls[0]);
    let reads = p.reads;
    tick(&mut h, &mut a, &mut p, 101);
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.reads, reads);
}

#[test]
fn a_child_lost_after_save_repeats_only_the_recorded_creation_identity() {
    let (mut h, _, stage) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake {
        lose_first: true,
        ..Default::default()
    };
    tick(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.calls.len(), 1);
    assert!(h.node(stage).task.is_none());
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    tick(&mut h, &mut a, &mut p, 101);
    assert_eq!(p.calls.len(), 1, "backoff was lost at restart");
    tick(&mut h, &mut a, &mut p, 60_100);
    assert_eq!(p.calls.len(), 2);
    assert_eq!(p.calls[0], p.calls[1]);
    assert_ne!(
        p.originals[0], p.originals[1],
        "a retry continued the old original"
    );
    assert_eq!(p.documents.len(), 1);
    assert_eq!(h.node(stage).task.as_ref().unwrap().uuid, p.calls[0]);
}

#[test]
fn stage_restarts_before_and_after_each_effect_keep_the_same_reserved_task() {
    for point in [
        "before_intent",
        "intent",
        "reserved",
        "effect",
        "outcome",
        "recorded",
    ] {
        let (mut h, _, stage) = wanted();
        let machine = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
        let mut p = Fake::default();
        super::super::tasks::CRASH.with(|s| s.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(
                &mut h, &mut a, &mut p, 100
            )))
            .is_err(),
            "{point}"
        );
        let reserved = h
            .store
            .stage_tasks(TEAM, 16, None)
            .unwrap()
            .rows
            .into_iter()
            .find(|s| s.node == stage)
            .and_then(|s| s.request_id);
        drop(a);
        let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
        for now in [101, 60_101] {
            tick(&mut h, &mut a, &mut p, now);
        }
        let task = h.node(stage).task.unwrap();
        assert_eq!(p.documents.len(), 1, "{point}");
        if let Some(id) = reserved {
            assert_eq!(task.uuid, id, "{point}");
        }
        if matches!(point, "outcome" | "recorded") {
            assert_eq!(p.calls.len(), 1, "{point}");
        }
    }
}

#[test]
fn a_released_stage_is_cleared_once_and_the_store_removes_it_from_the_outbox() {
    let (mut h, target, stage) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake::default();
    tick(&mut h, &mut a, &mut p, 100);
    h.run(
        &person(),
        crate::teams::ops::Operation::Return {
            node: target,
            reason: "Please revise".into(),
        },
    )
    .unwrap();
    h.release_retired(stage);
    assert_eq!(h.store.released_stages(TEAM, 16, 0).unwrap()[0].node, stage);
    tick(&mut h, &mut a, &mut p, 101);
    assert_eq!(p.clears, vec![stage]);
    assert!(h.store.released_stages(TEAM, 16, 0).unwrap().is_empty());
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    tick(&mut h, &mut a, &mut p, 102);
    assert_eq!(p.clears, vec![stage]);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn the_tick_calls_each_live_team_entry_and_one_terminal_entry_per_pass() {
    let mut h = Harness::new();
    let mut other = create();
    if let crate::teams::ops::Operation::Create {
        integration_branch, ..
    } = &mut other
    {
        *integration_branch = "teams/other-team".into();
    }
    h.run_for(TEAM_B, &person(), other).unwrap();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake::default();
    let teams = h.store.live_teams(16, None).unwrap().rows;
    a.page(&mut h.store, &mut p, &teams, 100, false).unwrap();
    assert_eq!(p.timer_teams.len(), 2);
    assert!(p.timer_teams.contains(&TEAM.into()) && p.timer_teams.contains(&TEAM_B.into()));
    assert_eq!(p.answer_teams.len(), 2);
    assert!(p.answer_teams.contains(&TEAM.into()) && p.answer_teams.contains(&TEAM_B.into()));
    assert_eq!(p.terminals, 1);
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn timer_entry_faults_back_off_as_rows_and_do_not_set_sweep_health() {
    let mut h = Harness::new();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake {
        fail_timers: true,
        ..Default::default()
    };
    let teams = h.store.live_teams(16, None).unwrap().rows;
    a.page(&mut h.store, &mut p, &teams, 100, false).unwrap();
    assert_eq!(p.terminals, 1);
    assert_eq!(a.ledger.terminal_retry().unwrap().unwrap().failures, 1);
    a.page(&mut h.store, &mut p, &teams, 101, false).unwrap();
    assert_eq!(p.terminals, 1);
    assert_eq!(p.timer_teams.len(), 1);
    assert_eq!(
        p.answer_teams.len(),
        2,
        "lead failure cannot suppress answers"
    );
    assert!(crate::desktop::teams::health(h.temp.path()).is_none());
    p.fail_timers = false;
    a.page(&mut h.store, &mut p, &teams, 60_100, false).unwrap();
    assert_eq!(p.terminals, 2);
    assert!(a.ledger.terminal_retry().unwrap().is_none());
    assert!(crate::desktop::teams::health(h.temp.path()).is_none());
}

#[test]
fn a_stage_released_during_creation_records_provenance_then_clears_its_task() {
    let (mut h, target, stage) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake {
        release: Some((h.temp.path().into(), target)),
        ..Default::default()
    };
    tick(&mut h, &mut a, &mut p, 100);
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.clears, vec![stage]);
    assert!(h.store.released_stages(TEAM, 16, 0).unwrap().is_empty());
    tick(&mut h, &mut a, &mut p, 102);
    assert_eq!(p.calls.len(), 1);
    assert_eq!(p.clears, vec![stage]);
}

#[test]
fn clearing_restarts_before_and_after_each_effect_keep_the_final_receipt() {
    for point in ["reserved", "effect", "outcome", "recorded"] {
        let (mut h, target, stage) = wanted();
        let machine = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
        let mut p = Fake::default();
        tick(&mut h, &mut a, &mut p, 100);
        h.run(
            &person(),
            crate::teams::ops::Operation::Return {
                node: target,
                reason: "Revise".into(),
            },
        )
        .unwrap();
        h.release_retired(stage);
        super::super::tasks::CRASH.with(|s| s.set(Some(point)));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(
                &mut h, &mut a, &mut p, 101
            )))
            .is_err(),
            "{point}"
        );
        drop(a);
        let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
        tick(&mut h, &mut a, &mut p, 102);
        assert!(
            h.store.released_stages(TEAM, 16, 0).unwrap().is_empty(),
            "{point}"
        );
        let expected = if point == "effect" { 2 } else { 1 };
        assert_eq!(p.clears, vec![stage; expected], "{point}");
    }
}

#[test]
fn clearing_no_answer_backs_off_and_a_refusal_is_final() {
    let (mut h, target, stage) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake {
        lose_clear: true,
        clear_reply: Some(TaskReply::Refused {
            reason: "Task no longer belongs to this stage".into(),
        }),
        ..Default::default()
    };
    tick(&mut h, &mut a, &mut p, 100);
    h.run(
        &person(),
        crate::teams::ops::Operation::Return {
            node: target,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    h.release_retired(stage);
    tick(&mut h, &mut a, &mut p, 101);
    tick(&mut h, &mut a, &mut p, 102);
    assert_eq!(p.clears, vec![stage]);
    tick(&mut h, &mut a, &mut p, 60_101);
    assert_eq!(p.clears, vec![stage, stage]);
    assert!(h.store.released_stages(TEAM, 16, 0).unwrap().is_empty());
    tick(&mut h, &mut a, &mut p, 120_101);
    assert_eq!(p.clears.len(), 2);
}

#[test]
fn a_stage_cutting_over_retries_the_same_task_after_backoff() {
    let (mut h, _, stage) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake {
        stage_reply: Some(TaskReply::CuttingOver {
            reason: "Project is cutting over".into(),
        }),
        ..Default::default()
    };
    tick(&mut h, &mut a, &mut p, 100);
    assert!(h.node(stage).task.is_none());
    tick(&mut h, &mut a, &mut p, 101);
    assert_eq!(p.calls.len(), 1);
    p.stage_reply = None;
    tick(&mut h, &mut a, &mut p, 60_100);
    assert_eq!(p.calls.len(), 2);
    assert_eq!(p.calls[0], p.calls[1]);
    assert_eq!(h.node(stage).task.unwrap().uuid, p.calls[0]);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn answer_entry_faults_have_their_own_durable_backoff_and_no_sweep_health() {
    let mut h = Harness::new();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let teams = h.store.live_teams(16, None).unwrap().rows;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake {
        fail_answers: true,
        ..Default::default()
    };
    a.page(&mut h.store, &mut p, &teams, 100, false).unwrap();
    assert_eq!(
        (p.timer_teams.len(), p.answer_teams.len(), p.terminals),
        (1, 1, 1)
    );
    assert!(crate::desktop::teams::health(h.temp.path()).is_none());
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    a.page(&mut h.store, &mut p, &teams, 101, false).unwrap();
    assert_eq!(
        (p.timer_teams.len(), p.answer_teams.len(), p.terminals),
        (2, 1, 2)
    );
    p.fail_answers = false;
    a.page(&mut h.store, &mut p, &teams, 60_100, false).unwrap();
    assert_eq!(
        (p.timer_teams.len(), p.answer_teams.len(), p.terminals),
        (3, 2, 3)
    );
    assert!(crate::desktop::teams::health(h.temp.path()).is_none());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_healthy_answer_step_does_not_reclassify_a_failed_team_card_as_sweep_health() {
    let mut h = Harness::new();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let teams = h.store.live_teams(16, None).unwrap().rows;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake {
        fail_timers: true,
        ..Default::default()
    };
    for now in [100, 60_100] {
        a.page(&mut h.store, &mut p, &teams, now, false).unwrap();
    }
    // The third failure opens a card. Fail that row's storage write while
    // the independent answer step succeeds; this is still a row failure.
    let db = rusqlite::Connection::open(h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fixture_card_write BEFORE INSERT ON cards BEGIN SELECT RAISE(ABORT,'fixture card storage failed'); END;").unwrap();
    a.page(&mut h.store, &mut p, &teams, 180_100, false)
        .unwrap();
    assert_eq!(p.timer_teams.len(), 3);
    assert_eq!(p.answer_teams.len(), 3);
    assert!(h.store.cards(TEAM, 16, None).unwrap().rows.is_empty());
    let gate = a.ledger.get(TEAM, "gate:timers:team").unwrap().unwrap();
    assert_eq!(gate.data["failures"], 3);
    assert!(gate.data["notice"].is_object());
    assert!(crate::desktop::teams::health(h.temp.path()).is_none());
}

mod budget;
mod receipts;
mod vanished;

/// A stage command's cutting-over sentence reaches the retry card cleaned.
#[test]
fn a_stage_commands_cutting_over_sentence_reaches_its_card_clean() {
    let (mut h, _, _) = wanted();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Fake {
        stage_reply: Some(TaskReply::CuttingOver {
            reason: super::hostile_sentence(),
        }),
        ..Default::default()
    };
    for now in [100, 60_100, 180_100, 420_100] {
        tick(&mut h, &mut a, &mut p, now);
    }
    super::assert_clean(&super::blocked_cards(&h), "stage");
}
