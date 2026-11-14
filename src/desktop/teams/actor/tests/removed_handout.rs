//! zerus-63rru.77 through the production Native and the real pass: a member
//! removed while it holds a hand-out of a task that has not started. The
//! removal settles the effect and tells the lead once; the actor ends the
//! delivery's ledger row without delivering, whatever state it was left in.
use super::real_rows::Production;
use super::rows::seed_row;
use crate::desktop::teams::actor::ports::{DeliveryResult, Ports};
use crate::teams::ops::tests::*;
use crate::teams::ops::{Actor, HandoutOutcome, Operation};
use serde_json::{json, Value};
use zerus_team_plan::model::{Member, Node, TaskRef, Team};

const MINUTE: i64 = 60_000;
const T0: i64 = 10_000_000;

/// The member's committed fleet row, as its producer writes it.
fn row(p: &Production, h: &Harness, member: &str, changes: &[(&str, Value)]) {
    let m = h.store.member(TEAM, member).unwrap().unwrap();
    let launch = m.session.as_ref().unwrap().session_id["launch_id:".len()..].to_owned();
    let run = format!("fixture-{member}");
    let mut all = vec![
        ("name", json!(format!("{}/fixture/{member}", m.agent))),
        ("run_id", json!(run)),
        ("launch_id", json!(launch)),
        ("conversation_id", json!(format!("conversation-{member}"))),
    ];
    all.extend(changes.iter().cloned());
    let row = seed_row(&m.agent, &all).to_string();
    rusqlite::Connection::open(p.native.config.state.join("desktop/state.sqlite3"))
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO desktop_catalog_rows(generation,key,json,bytes) VALUES(1,?1,?2,?3)",
            rusqlite::params![format!("run:{run}"), row, row.len()],
        )
        .unwrap();
}
/// A running team whose task 1 can go to m1 only (m2 is busy), the lead
/// bound; the first pass has not run.
fn team() -> (Harness, super::super::Runtime, Production, Actor) {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let native = super::production::native(&h);
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let a = super::super::Runtime::open(h.temp.path(), &machine).unwrap();
    let p = Production {
        native,
        deliveries: vec![],
    };
    row(&p, &h, "m2", &[("activity", json!("busy"))]);
    (h, a, p, lead)
}
fn removed(h: &mut Harness) {
    h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m1".into(),
        },
    )
    .unwrap();
}
fn effects(h: &Harness) -> Vec<(String, String, u32)> {
    let rows = h.store.effects(TEAM, 200, None).unwrap().rows;
    rows.into_iter()
        .map(|e| (e.state, e.member, e.node))
        .collect()
}
fn effect_id(h: &Harness) -> String {
    let rows = h.store.effects(TEAM, 200, None).unwrap().rows;
    rows.into_iter()
        .find(|e| e.member == "m1")
        .unwrap()
        .request_id
}
fn items(h: &Harness) -> Vec<(String, String)> {
    let rows = h.store.inbox(TEAM, 200, None).unwrap().rows;
    rows.into_iter().map(|i| (i.kind, i.text)).collect()
}
fn cards(h: &Harness) -> usize {
    h.store.cards(TEAM, 200, None).unwrap().rows.len()
}
/// No delivery row and no retry gate of a hand-out or a delivery remain.
fn no_rows(a: &super::super::Runtime) {
    for prefix in ["delivery:", "gate:handout:", "gate:delivery:"] {
        let rows = a.ledger.prefix_page(TEAM, prefix, "").unwrap();
        assert!(
            rows.is_empty(),
            "{prefix}: {:?}",
            rows.iter().map(|r| &r.key).collect::<Vec<_>>()
        );
    }
}
fn told(id: &str) -> (String, String) {
    (
        format!("member_removed:{id}"),
        "Member m1 was removed before task 1 started; the delivery's result is not known: the prompt may have reached a session that is now gone. The task waits for the lead.".into(),
    )
}
fn passes(
    h: &mut Harness,
    a: &mut super::super::Runtime,
    p: &mut impl Ports,
    from: i64,
    count: i64,
) {
    for minute in 0..count {
        a.tick(&mut h.store, p, TEAM, from + minute * MINUTE, false)
            .unwrap();
    }
}

/// The review's probe (teams-review71, item 2): the hand-out was submitted,
/// then its member was removed. Twenty passes later the task is with the
/// lead, told once, and nobody else was sent anything.
#[test]
fn a_handed_task_whose_member_is_removed_goes_back_to_the_lead_once() {
    let (mut h, mut a, mut p, lead) = team();
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    assert_eq!(p.deliveries.len(), 1, "{:?}", p.deliveries);
    assert_eq!(effects(&h), [("submitted".into(), "m1".into(), 1)]);
    let id = effect_id(&h);
    // m2 is free from now on: a held task must still not go to it.
    row(&p, &h, "m2", &[]);
    removed(&mut h);
    passes(&mut h, &mut a, &mut p, T0 + MINUTE, 20);

    assert_eq!(effects(&h), [("superseded".into(), "m1".into(), 1)]);
    assert!(h.node(1).hold);
    assert_eq!(items(&h), [told(&id)]);
    assert_eq!(cards(&h), 0);
    no_rows(&a);
    assert_eq!(p.deliveries.len(), 1, "nothing else was sent");
    let again = h
        .run(
            &person(),
            Operation::ArchiveMember {
                member: "m1".into(),
            },
        )
        .unwrap_err();
    assert_eq!(again.message, "The member cannot be removed.");
    passes(&mut h, &mut a, &mut p, T0 + 30 * MINUTE, 3);
    assert_eq!(items(&h).len(), 1, "no second item");

    // The lead decides from the held state: a Reassign reaches m2 through
    // the real pass.
    h.run(
        &lead,
        Operation::Reassign {
            node: 1,
            member: "m2".into(),
            reason: "Another member takes it".into(),
        },
    )
    .unwrap();
    passes(&mut h, &mut a, &mut p, T0 + 40 * MINUTE, 3);
    assert_eq!(p.deliveries.len(), 2, "{:?}", p.deliveries);
    assert!(
        p.deliveries[1].starts_with("m2 -> Outcome(Submitted"),
        "{:?}",
        p.deliveries
    );
    assert_eq!(
        effects(&h),
        [
            ("superseded".into(), "m1".into(), 1),
            ("submitted".into(), "m2".into(), 1)
        ]
    );
    assert_eq!(cards(&h), 0);
}

/// The actor crashed after the hand-out's intent and before its delivery;
/// the member was removed before the service came back. The restarted pass
/// finds the row of an effect that is no longer reserved and ends it: no
/// delivery, no recorded outcome, no failure. A crash before the intent
/// leaves no effect: the removal says nothing and the intent is refused.
#[test]
fn a_crash_between_the_removal_and_the_next_pass_ends_the_row_without_a_send() {
    for point in ["reserved", "intent"] {
        let (mut h, mut a, mut p, _) = team();
        super::super::delivery::CRASH.with(|c| c.set(Some(point)));
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            a.tick(&mut h.store, &mut p, TEAM, T0, false)
        }));
        assert!(crashed.is_err(), "boundary {point} was not exercised");
        assert!(p.deliveries.is_empty(), "{point}");
        drop(a);
        removed(&mut h);
        let home = h.store.team(TEAM).unwrap().unwrap().home;
        let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
        passes(&mut h, &mut a, &mut p, T0 + MINUTE, 10);

        assert!(p.deliveries.is_empty(), "{point}: {:?}", p.deliveries);
        no_rows(&a);
        assert_eq!(cards(&h), 0, "{point}");
        if point == "intent" {
            let e = h.store.effects(TEAM, 200, None).unwrap().rows.remove(0);
            assert_eq!(
                (e.state.as_str(), e.outcome),
                ("superseded", None),
                "{point}"
            );
            assert_eq!(items(&h), [told(&e.request_id)], "{point}");
            assert!(h.node(1).hold, "{point}");
        } else {
            assert!(effects(&h).is_empty(), "{point}: no effect was reserved");
            assert!(items(&h).is_empty(), "{point}");
        }
    }
}

/// A delivery whose child proved nothing started waits in its row for the
/// next replay; the member is removed meanwhile. The next pass ends the row
/// instead of replaying it: one attempt in all, no gate left behind.
#[test]
fn a_deferred_delivery_of_a_removed_member_is_never_replayed() {
    struct Once {
        inner: Production,
        calls: usize,
    }
    impl Ports for Once {
        fn changes(
            &mut self,
            project: &str,
            generation: &str,
            offset: u64,
        ) -> crate::desktop::teams::actor::ports::Result<zerus_replica::Changes> {
            Ports::changes(&mut self.inner, project, generation, offset)
        }
        fn observe(
            &mut self,
            store: &mut crate::teams::store::Store,
            task: &TaskRef,
        ) -> crate::desktop::teams::actor::ports::Result<()> {
            Ports::observe(&mut self.inner, store, task)
        }
        fn free(
            &mut self,
            team: &Team,
            member: &Member,
        ) -> crate::desktop::teams::actor::ports::Result<bool> {
            Ports::free(&mut self.inner, team, member)
        }
        fn availability(
            &mut self,
            team: &Team,
            member: &Member,
        ) -> crate::desktop::teams::actor::ports::Result<
            crate::desktop::teams::actor::ports::Availability,
        > {
            Ports::availability(&mut self.inner, team, member)
        }
        fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
            self.calls += 1;
            DeliveryResult::NotStarted {
                reason: "The task store is busy; nothing was sent.".into(),
            }
        }
    }
    let (mut h, mut a, p, _) = team();
    let mut once = Once { inner: p, calls: 0 };
    a.tick(&mut h.store, &mut once, TEAM, T0, false).unwrap();
    assert_eq!(once.calls, 1);
    assert_eq!(effects(&h), [("reserved".into(), "m1".into(), 1)]);
    let id = effect_id(&h);
    assert!(a
        .ledger
        .get(TEAM, &format!("delivery:{id}"))
        .unwrap()
        .is_some());
    removed(&mut h);
    passes(&mut h, &mut a, &mut once, T0 + MINUTE, 20);

    assert_eq!(once.calls, 1, "the deferred delivery was replayed");
    no_rows(&a);
    assert_eq!(cards(&h), 0);
    assert_eq!(items(&h), [told(&id)]);
}

/// The removal lands while a delivery child is already running: after the
/// spawn admission's final check, when its guard is gone. Whatever the
/// child answers is recorded as a fact on the settled effect and ends the
/// row; the lead keeps the removal's one item, and no failure or card
/// follows. (What the child's own check answers, and whether it can still
/// type, is pinned with the real task command in team_deliver's tests.)
#[test]
fn a_removal_that_lands_while_the_delivery_child_runs_is_recorded_once() {
    /// The production delivery whose child replies `state` with `error`.
    struct Reply {
        inner: Production,
        state: &'static str,
        error: &'static str,
    }
    impl Ports for Reply {
        fn changes(
            &mut self,
            project: &str,
            generation: &str,
            offset: u64,
        ) -> crate::desktop::teams::actor::ports::Result<zerus_replica::Changes> {
            Ports::changes(&mut self.inner, project, generation, offset)
        }
        fn observe(
            &mut self,
            store: &mut crate::teams::store::Store,
            task: &TaskRef,
        ) -> crate::desktop::teams::actor::ports::Result<()> {
            Ports::observe(&mut self.inner, store, task)
        }
        fn free(
            &mut self,
            team: &Team,
            member: &Member,
        ) -> crate::desktop::teams::actor::ports::Result<bool> {
            Ports::free(&mut self.inner, team, member)
        }
        fn availability(
            &mut self,
            team: &Team,
            member: &Member,
        ) -> crate::desktop::teams::actor::ports::Result<
            crate::desktop::teams::actor::ports::Availability,
        > {
            Ports::availability(&mut self.inner, team, member)
        }
        fn deliver(&mut self, t: &Team, n: &Node, m: &Member, id: &str) -> DeliveryResult {
            let reply = json!({"schema":1,"id":id,"state":self.state,"error":self.error});
            crate::desktop::teams::child::FIXTURE_EXIT
                .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
            let r = Ports::deliver(&mut self.inner.native, t, n, m, id);
            crate::desktop::teams::child::FIXTURE_EXIT.with(|s| s.borrow_mut().take());
            self.inner.deliveries.push(format!("{} -> {r:?}", m.id));
            r
        }
    }
    for (reply, error) in [
        ("submitted", ""),
        // The child's own words when the removal reached its check first.
        (
            "refused",
            "Effect is closed or belongs to another generation.",
        ),
    ] {
        let (mut h, mut a, p, _) = team();
        let mut p = Reply {
            inner: p,
            state: reply,
            error,
        };
        let state = h.temp.path().to_owned();
        crate::desktop::original_child::AFTER_SPAWN.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                let mut store = crate::teams::store::Store::open(&state).unwrap();
                let rev = store.team(TEAM).unwrap().unwrap().revision;
                let request = request(
                    rev,
                    Operation::ArchiveMember {
                        member: "m1".into(),
                    },
                );
                crate::teams::ops::apply(&mut store, &person(), &request, T0).unwrap();
            }))
        });
        a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
        assert!(
            crate::desktop::original_child::AFTER_SPAWN.with(|slot| slot.borrow().is_none()),
            "{reply}: the removal did not land"
        );
        assert_eq!(p.inner.deliveries.len(), 1, "{reply}");
        let rows = h.store.effects(TEAM, 200, None).unwrap().rows;
        let e = &rows[0];
        assert_eq!(e.state, "superseded", "{reply}");
        let outcome: HandoutOutcome = serde_json::from_value(e.outcome.clone().unwrap()).unwrap();
        assert_eq!(
            matches!(outcome, HandoutOutcome::Submitted { .. }),
            reply == "submitted",
            "{reply}: {outcome:?}"
        );
        assert_eq!(h.node(1).implementer, None, "{reply}");
        passes(&mut h, &mut a, &mut p, T0 + MINUTE, 10);
        assert_eq!(items(&h), [told(&e.request_id)], "{reply}");
        no_rows(&a);
        assert_eq!(cards(&h), 0, "{reply}");
        assert_eq!(p.inner.deliveries.len(), 1, "{reply}");
    }
}

/// A paused member keeps its submitted hand-out and the pass sends nobody
/// else anything: that wait is intended (zerus-63rru.77, unchanged).
#[test]
fn a_paused_members_hand_out_is_kept() {
    let (mut h, mut a, mut p, _) = team();
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    row(&p, &h, "m2", &[]);
    h.run(
        &person(),
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    passes(&mut h, &mut a, &mut p, T0 + MINUTE, 10);
    assert_eq!(effects(&h), [("submitted".into(), "m1".into(), 1)]);
    assert!(!h.node(1).hold);
    // zerus-63rru.86: the lead is told once that the paused member holds
    // started work; nothing else is sent.
    assert_eq!(items(&h).len(), 1, "{:?}", items(&h));
    assert!(items(&h)[0].0.starts_with("member_started:1:m1:"));
    assert_eq!(p.deliveries.len(), 1);
}

/// zerus-63rru.77, C: m1 started task 1 and was then removed.
fn started_then_removed() -> (Harness, super::super::Runtime, Production, Actor, String) {
    let (mut h, mut a, mut p, lead) = team();
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    let id = effect_id(&h);
    h.observe(1, crate::teams::ops::TaskStage::InProgress);
    row(&p, &h, "m2", &[]);
    removed(&mut h);
    (h, a, p, lead, id)
}

/// The removal takes the started task back; the old session later reports.
/// The reports step tells the lead once and ends there: ten passes, one
/// item, no report recorded, no retry gate, no failure, no card.
#[test]
fn a_report_written_after_the_removal_is_told_once_over_ten_passes() {
    let (mut h, mut a, mut p, _, id) = started_then_removed();
    assert_eq!(effects(&h), [("superseded".into(), "m1".into(), 1)]);
    assert!(h.node(1).hold);
    let path = h.store.member(TEAM, "m1").unwrap().unwrap().worktree;
    let first = items(&h);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, format!("member_removed:{id}"));
    assert!(first[0].1.contains(&format!("in the worktree at {path}")));
    h.observe(1, crate::teams::ops::TaskStage::InReview);
    passes(&mut h, &mut a, &mut p, T0 + MINUTE, 10);

    let told: Vec<_> = items(&h)
        .into_iter()
        .filter(|(kind, _)| kind == "removed_report")
        .collect();
    assert_eq!(
        told,
        [(
            "removed_report".to_owned(),
            format!("Member m1 reported task 1 after it was removed; the team does not record that report. Its work is in the worktree at {path}.")
        )]
    );
    assert_eq!(items(&h).len(), 2);
    let generation = h.store.generation(TEAM, 1).unwrap().unwrap();
    assert!(h.store.report(TEAM, 1, generation).unwrap().is_none());
    assert!(a
        .ledger
        .prefix_page(TEAM, "gate:report:", "")
        .unwrap()
        .is_empty());
    no_rows(&a);
    assert_eq!(cards(&h), 0);
    assert_eq!(p.deliveries.len(), 1);
}

/// From the held state the lead's Reassign reaches another member through
/// the real pass: one delivery to m2, nothing more to m1.
#[test]
fn the_lead_reassigns_a_task_taken_back_while_in_progress() {
    let (mut h, mut a, mut p, lead, _) = started_then_removed();
    passes(&mut h, &mut a, &mut p, T0 + MINUTE, 3);
    assert_eq!(p.deliveries.len(), 1);
    h.run(
        &lead,
        Operation::Reassign {
            node: 1,
            member: "m2".into(),
            reason: "Another member takes it".into(),
        },
    )
    .unwrap();
    passes(&mut h, &mut a, &mut p, T0 + 10 * MINUTE, 3);
    assert_eq!(p.deliveries.len(), 2, "{:?}", p.deliveries);
    assert!(
        p.deliveries[1].starts_with("m2 -> Outcome(Submitted"),
        "{:?}",
        p.deliveries
    );
    assert_eq!(cards(&h), 0);
}

/// A paused member's started task is untouched: no hand-back, no item.
#[test]
fn a_paused_members_started_task_is_untouched() {
    let (mut h, mut a, mut p, _) = team();
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    h.observe(1, crate::teams::ops::TaskStage::InProgress);
    h.run(
        &person(),
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    passes(&mut h, &mut a, &mut p, T0 + MINUTE, 10);
    assert_eq!(effects(&h), [("submitted".into(), "m1".into(), 1)]);
    assert!(!h.node(1).hold);
    assert!(items(&h).is_empty());
    assert_eq!(cards(&h), 0);
}

/// A report written at the task level before the removal, and not yet
/// recorded by the team when the removal lands, is still the team's: the
/// real reports step records it, tells no late report and leaves no gate.
#[test]
fn a_report_written_before_the_removal_is_recorded_through_the_real_pass() {
    /// Production, with the member's report text and head answered.
    struct Reporting {
        inner: Production,
    }
    impl Ports for Reporting {
        fn changes(
            &mut self,
            project: &str,
            generation: &str,
            offset: u64,
        ) -> crate::desktop::teams::actor::ports::Result<zerus_replica::Changes> {
            Ports::changes(&mut self.inner, project, generation, offset)
        }
        fn observe(
            &mut self,
            store: &mut crate::teams::store::Store,
            task: &TaskRef,
        ) -> crate::desktop::teams::actor::ports::Result<()> {
            Ports::observe(&mut self.inner, store, task)
        }
        fn free(
            &mut self,
            team: &Team,
            member: &Member,
        ) -> crate::desktop::teams::actor::ports::Result<bool> {
            Ports::free(&mut self.inner, team, member)
        }
        fn availability(
            &mut self,
            team: &Team,
            member: &Member,
        ) -> crate::desktop::teams::actor::ports::Result<
            crate::desktop::teams::actor::ports::Availability,
        > {
            Ports::availability(&mut self.inner, team, member)
        }
        fn deliver(&mut self, t: &Team, n: &Node, m: &Member, id: &str) -> DeliveryResult {
            Ports::deliver(&mut self.inner, t, n, m, id)
        }
        fn report(
            &mut self,
            _: &TaskRef,
        ) -> crate::desktop::teams::actor::ports::Result<Option<String>> {
            Ok(Some("Implemented and checked".into()))
        }
        fn reported(
            &mut self,
            _: &Team,
            _: &Member,
        ) -> crate::desktop::teams::actor::ports::Result<crate::teams::integrate::Reported>
        {
            Ok(crate::teams::integrate::Reported {
                commit: COMMIT.into(),
                branch: "teams/member".into(),
                dirty: false,
            })
        }
    }
    let (mut h, mut a, p, _) = team();
    let mut p = Reporting { inner: p };
    a.tick(&mut h.store, &mut p, TEAM, T0, false).unwrap();
    h.observe(1, crate::teams::ops::TaskStage::InProgress);
    h.observe(1, crate::teams::ops::TaskStage::InReview);
    removed(&mut h);
    passes(&mut h, &mut a, &mut p, T0 + MINUTE, 5);

    let generation = h.store.generation(TEAM, 1).unwrap().unwrap();
    let report = h.store.report(TEAM, 1, generation).unwrap().unwrap();
    assert_eq!(report.member, "m1");
    assert!(items(&h).iter().all(|(kind, _)| kind != "removed_report"));
    assert!(items(&h)
        .iter()
        .all(|(kind, _)| !kind.starts_with("member_removed:")));
    assert!(a
        .ledger
        .prefix_page(TEAM, "gate:report:", "")
        .unwrap()
        .is_empty());
    assert_eq!(cards(&h), 0);
}

/// zerus-63rru.77: the person's Cancel of a started stage of a removed
/// reviewer, with the real `team-cancel` and the actor's own reading and
/// recording of its reply; with the reviewer's session present, and gone.
fn person_cancels_a_removed_reviewers_started_stage(
    session_gone: bool,
) -> crate::tasks::RealCancel {
    use super::super::{cancel::Fact, native::Native, ports::TaskReply};
    crate::tasks::removed_stage_cancel(session_gone, |store, reply, node| {
        let team = store.team(TEAM).unwrap().unwrap();
        let input = super::super::native::clear_request(&team, node);
        let answer = super::super::native::cancel_answer(
            crate::desktop::original_child::Ended::Exited {
                code: 0,
                output: reply.to_vec(),
            },
            &input,
        )
        .unwrap();
        // As the actor does (`cancel_needs_observation`): a cancelled answer
        // in a live team is recorded by the task's ordinary observation,
        // which the harness runs next; only another answer is a fact.
        if matches!(answer.reply, TaskReply::Cancelled)
            && matches!(
                team.status,
                zerus_team_plan::model::TeamStatus::Running
                    | zerus_team_plan::model::TeamStatus::Paused
            )
        {
            return;
        }
        let pending = Native::pending_cancels(store, TEAM, node - 1)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(pending.node, node);
        let fact = Fact {
            request_id: uuid::Uuid::new_v4().to_string(),
            revision: team.revision,
            pending,
            attempt: answer.attempt.as_ref().map_or(0, |a| a.number),
            attempt_id: answer
                .attempt
                .as_ref()
                .map_or_else(String::new, |a| a.id.clone()),
            refused: matches!(answer.reply, TaskReply::Refused { .. }),
            released: matches!(answer.reply, TaskReply::Released),
            reason: match &answer.reply {
                TaskReply::Refused { reason } => reason.clone(),
                _ => String::new(),
            },
        };
        Native::save_cancel(store, &team, &fact, T0).unwrap();
    })
}

/// It ends as a task's Cancel does: the real command cancels the stage's
/// task and tells the reviewer's session to stop (when that session is gone,
/// nothing is sent and nothing fails); the observation settles the pending
/// cancel and ends the stage; the parent comes back to the lead with one
/// report item. This is what the stage's item promises.
#[test]
fn the_persons_cancel_of_a_removed_reviewers_started_stage_ends_as_a_tasks_cancel() {
    for gone in [false, true] {
        let seen = person_cancels_a_removed_reviewers_started_stage(gone);
        assert_eq!(seen.reply["state"], "cancelled", "{gone}: {seen:?}");
        match &seen.notices {
            Some(notice) => {
                assert!(!gone, "{seen:?}");
                let notice: Value = serde_json::from_str(notice.trim()).unwrap();
                assert_eq!(notice["session"]["session"], "codex/deliver-m3");
                assert!(notice["text"]
                    .as_str()
                    .unwrap()
                    .contains("The person cancelled this task. Stop working on it."));
            }
            None => assert!(gone, "{seen:?}"),
        }
        assert_eq!(seen.cancel, ("cancel".into(), "Settled".into()), "{gone}");
        assert_eq!(
            seen.stage,
            ("cancelled".into(), false, (false, None, false)),
            "{gone}"
        );
        assert_eq!(seen.task, ("review".into(), true), "{gone}");
        assert_eq!(
            seen.parent,
            ("reported".into(), (false, None, false)),
            "{gone}"
        );
        assert_eq!(seen.run, "lead", "{gone}");
        let open: Vec<_> = seen
            .items
            .iter()
            .filter(|(_, _, state, _)| state == "open")
            .collect();
        assert_eq!(open.len(), 1, "{gone}: {:?}", seen.items);
        assert_eq!((open[0].1.as_str(), open[0].0), ("report", 1), "{gone}");
        assert!(
            open[0]
                .3
                .ends_with("The person overrode the pipeline's stage task."),
            "{gone}: {}",
            open[0].3
        );
    }
}
