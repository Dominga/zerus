//! The tests of zerus-63rru.23: a hand-out refused by the project's own
//! limits is a fact about the team. The task command's side (the typed cause
//! beside the sentence) is pinned in the task command's own tests.
use super::super::ports::{DeliveryResult, Ports};
use super::super::Runtime;
use super::*;
use crate::teams::ops::{Actor, HandoutOutcome, KeyState, Operation, TaskObservation, TaskStage};

struct ProjectRefused {
    /// The two project sentences the real command answers with.
    reason: String,
    calls: u32,
    /// From this call on, the delivery is submitted (the project admits).
    submit_now: bool,
    /// From this call on, the delivery's answer is uncertain.
    uncertain: bool,
    /// From this call on, the delivery never started.
    not_started: bool,
}
impl Ports for ProjectRefused {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> super::super::ports::Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> super::super::ports::Result<()> {
        Ok(())
    }
    fn free(&mut self, _: &Team, m: &Member) -> super::super::ports::Result<bool> {
        Ok(m.id == "m1" || m.id == "m2" || m.id == "m3")
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
        self.calls += 1;
        if self.submit_now {
            return HandoutOutcome::Submitted {
                receipt: "fixture hand-over".into(),
            }
            .into();
        }
        if self.uncertain {
            return HandoutOutcome::Uncertain {
                reason: "The original result is unknown".into(),
            }
            .into();
        }
        if self.not_started {
            return DeliveryResult::NotStarted {
                reason: "No input was started".into(),
            };
        }
        DeliveryResult::ProjectLimit {
            reason: self.reason.clone(),
        }
    }
}

fn team(h: &Harness) -> Team {
    h.store.team(TEAM).unwrap().unwrap()
}
fn open_cards(h: &Harness) -> Vec<crate::teams::store::Card> {
    h.store
        .cards(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|c| c.state == "open")
        .collect()
}
fn fact_row(_h: &Harness, a: &Runtime) -> Option<serde_json::Value> {
    a.ledger
        .get(TEAM, "project_limit")
        .unwrap()
        .map(|r| r.data)
}

/// The first refusal opens the team's card at once and delivers nothing
/// more while the fact stands; one probe on the retry table's delay, and a
/// further refusal waits longer (1, 2, 4, 8, 15 minutes).
#[test]
fn a_project_limit_refusal_opens_the_team_card_and_probes_on_the_delay() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    // The first pass refuses once and opens the card at once.
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    assert!(h.store.node(TEAM, node).unwrap().unwrap().assignee.is_none());
    let effect = h.store.effects(TEAM, 200, None).unwrap().rows.remove(0);
    assert_eq!(effect.state, "refused");
    let cards = open_cards(&h);
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].kind, "team_actor_blocked");
    assert_eq!(cards[0].node, None, "bound to no task");
    assert!(
        cards[0]
            .text
            .starts_with("The project's own limit refused the latest hand-out:"),
        "{:?}",
        cards[0].text
    );
    assert!(cards[0].text.contains(&p.reason), "{:?}", cards[0].text);
    assert_eq!(
        cards[0].decisions,
        vec!["retry".to_owned(), "acknowledged".to_owned()]
    );
    // The gate row exists with the event, so a dismissal lasts while the
    // sentence is the same.
    assert!(a.ledger.get(TEAM, "gate:project_limit").unwrap().is_some());
    // While it stands, nothing is delivered: no hand-out, no reopen.
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 30_000, false).unwrap();
    assert_eq!(p.calls, 1, "standing: nothing delivered");
    // The first probe at +60s is refused and waits longer (2 minutes).
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 60_000, false).unwrap();
    assert_eq!(p.calls, 2, "the probe at the delay");
    let row = fact_row(&h, &a).unwrap();
    assert_eq!(row["refusals"], 2);
    assert_eq!(row["until"], 1000 + 60_000 + 120_000);
    // Nothing before the next delay.
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 90_000, false).unwrap();
    assert_eq!(p.calls, 2);
    // The next probe at +180s waits four minutes.
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 180_000, false).unwrap();
    assert_eq!(p.calls, 3);
    let row = fact_row(&h, &a).unwrap();
    assert_eq!(row["until"], 1000 + 180_000 + 240_000);
    assert_eq!(open_cards(&h).len(), 1, "still one card");
}

/// A probe the project admits ends the fact, closes the card, and the same
/// pass hands the rest of the queue out.
#[test]
fn a_probe_the_project_admits_ends_the_fact_and_the_pass_goes_on() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.bind("m3");
    h.add(&person(), 1);
    h.add(&person(), 2);
    h.add(&person(), 3);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agent_limit: The project's hourly agent start limit is reached.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    assert_eq!(open_cards(&h).len(), 1);
    // The setting now allows: the probe at +60s submits, the fact ends, the
    // card closes, and the same pass hands out the remaining ready tasks.
    p.submit_now = true;
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 60_000, false).unwrap();
    // The fixture's roster has two developers, so the rest of the queue is
    // one more hand-out in the same pass (the third member is a reviewer).
    assert_eq!(p.calls, 3, "the probe plus the rest of the queue");
    assert!(fact_row(&h, &a).is_none(), "the fact ended");
    assert!(open_cards(&h).is_empty(), "the card closed");
    assert!(a.ledger.get(TEAM, "gate:project_limit").unwrap().is_none());
}

/// An acknowledged dismissal lasts while the sentence is the same; the
/// retry decision probes on the next pass whatever the backoff says.
#[test]
fn a_dismissal_lasts_for_the_same_sentence_and_retry_probes_at_once() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    let card = open_cards(&h).remove(0);
    // The acknowledged dismissal: the probe at +60s is refused with the
    // same sentence, the dismissal lasts, the card stays closed and one.
    h.run(
        &person(),
        Operation::DecideCard {
            card: card.seq,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 60_000, false).unwrap();
    assert_eq!(p.calls, 2);
    assert!(open_cards(&h).is_empty());
    assert_eq!(h.store.cards(TEAM, 200, None).unwrap().rows.len(), 1);
    // The retry decision (a fresh team) probes on the next pass despite the
    // backoff's remaining time.
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    let card = open_cards(&h).remove(0);
    h.run(
        &person(),
        Operation::DecideCard {
            card: card.seq,
            decision: "retry".into(),
        },
    )
    .unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 30_000, false).unwrap();
    assert_eq!(p.calls, 2, "retry probes on the next pass despite the delay");
}

/// An older task command's answer without the cause keeps today's per-task
/// behaviour: the refusal gates the task, the card waits for three failures
/// and another member is tried (zerus-63rru.23 compatibility).
#[test]
fn an_answer_without_the_cause_keeps_the_per_task_behaviour() {
    struct Plain {
        calls: u32,
    }
    impl Ports for Plain {
        fn changes(&mut self, _: &str, _: &str, _: u64) -> super::super::ports::Result<zerus_replica::Changes> {
            Ok(zerus_replica::Changes {
                generation: "journal".into(),
                offset: 0,
                documents: Some(Default::default()),
            })
        }
        fn observe(&mut self, _: &mut Store, _: &TaskRef) -> super::super::ports::Result<()> {
            Ok(())
        }
        fn free(&mut self, _: &Team, m: &Member) -> super::super::ports::Result<bool> {
            Ok(m.id == "m1" || m.id == "m2")
        }
        fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
            self.calls += 1;
            HandoutOutcome::Refused {
                reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
            }
            .into()
        }
    }
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = Plain { calls: 0 };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    // The per-task gate, no project fact, no card yet.
    assert!(a.ledger.get(TEAM, "gate:handout:1").unwrap().is_some());
    assert!(fact_row(&h, &a).is_none());
    assert!(open_cards(&h).is_empty());
    // Retried on the task's own backoff and card after three failures.
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 60_000, false).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 180_000, false).unwrap();
    assert_eq!(p.calls, 3);
    let cards = open_cards(&h);
    assert_eq!(cards.len(), 1);
    assert!(cards[0].text.starts_with("handout:1:"), "{:?}", cards[0].text);
    let _ = node;
}

/// A restart between the refusal and the next pass keeps the fact, its card
/// and its schedule.
#[test]
fn a_restart_keeps_the_fact_and_its_card() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    drop(a);
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    assert!(fact_row(&h, &a).is_some(), "the fact survives");
    assert_eq!(open_cards(&h).len(), 1, "the card survives");
    // Still standing: nothing delivered.
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 30_000, false).unwrap();
    assert_eq!(p.calls, 1);
}

/// A crash at the outcome boundary keeps the fact: the recovery settles the
/// kept delivery and records it.
#[test]
fn a_crash_at_the_outcome_boundary_keeps_the_fact() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    super::super::delivery::CRASH.with(|s| s.set(Some("outcome")));
    let result = catch_unwind(AssertUnwindSafe(|| {
        a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    }));
    let remaining = super::super::delivery::CRASH.with(|s| s.replace(None));
    assert!(result.is_err() && remaining.is_none(), "the crash point was visited (calls={})", p.calls);
    drop(a);
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert!(fact_row(&h, &a).is_some(), "the recovery recorded the fact");
    assert_eq!(open_cards(&h).len(), 1);
}

/// A member's terminal fact and the team's project fact coexist: while the
/// project fact stands the pass delivers nothing, and the member fact row
/// stays.
#[test]
fn a_terminal_fact_and_the_project_fact_coexist() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    a.terminal_refused(TEAM, "m1", None, None, 1000).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 30_000, false).unwrap();
    assert_eq!(p.calls, 1, "the project fact gates the pass");
    assert!(a.ledger.get(TEAM, "terminal:m1").unwrap().is_some());
    assert!(fact_row(&h, &a).is_some());
}

/// The lead reads the fact among the team status reply's service items.
#[test]
fn the_lead_reads_the_fact_among_the_status_items() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agent_limit: The project's hourly agent start limit is reached.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    let items = crate::desktop::teams::service_items(h.temp.path(), TEAM, Some(true), 1000)
        .expect("the items exist");
    let listed: Vec<_> = items
        .items
        .iter()
        .filter(|i| i.kind == "project_limit")
        .collect();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].reason, p.reason);
    assert_eq!(listed[0].failures, 1);
}

/// Two teams of one project each keep their own facts: one team's fact never
/// gates the other's pass.
#[test]
fn two_teams_keep_their_own_facts() {
    let mut h = Harness::empty();
    let t = fixture().team;
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle.pipelines[0].stages.clear();
    }
    h.run(&person(), create).unwrap();
    let second = "00000000-0000-4000-8000-0000000000ff";
    let mut t2 = t.clone();
    t2.id = second.into();
    t2.name = "second".into();
    t2.integration_branch = "zerus/team/second".into();
    let mut create2 = Operation::Create {
        expected_template: None,
        delegations: None,
        project: t2.project,
        name: t2.name,
        goal: t2.goal,
        home: t2.home,
        bundle: Box::new(t2.bundle),
        integration_branch: t2.integration_branch,
        base_commit: t2.base_commit,
    };
    if let Operation::Create { bundle, .. } = &mut create2 {
        bundle.pipelines[0].stages.clear();
    }
    h.run_for(second, &person(), create2).unwrap();
    h.bind("m1");
    h.add(&person(), 1);
    // A session binds to one member, so the second team's m1 takes m2's
    // fixture session, which the first team does not use.
    let m2 = fixture().members.iter().find(|m| m.id == "m2").unwrap().clone();
    h.run_for(
        second,
        &Actor::Service {
            person: "local".into(),
            team: second.into(),
        },
        Operation::SetMemberSession {
            member: "m1".into(),
            session: m2.session.clone().unwrap(),
            account: m2.account.clone(),
            worktree: m2.worktree.clone(),
            ownership_nonce: m2.ownership_nonce.clone(),
        },
    )
    .unwrap();
    h.run_for(
        second,
        &person(),
        Operation::AddTask {
            // A task lives in one team's queue; the second team takes its own.
            task: task(101),
            role: None,
            pipeline: None,
        },
    )
    .unwrap();
    h.run_for(
        second,
        &Actor::Service {
            person: "local".into(),
            team: second.into(),
        },
        Operation::ObserveTask {
            node: 1,
            observation: TaskObservation {
                key: KeyState::Ours,
                revision: "token-2".into(),
                previous_revision: String::new(),
                stage: TaskStage::Todo,
                cancelled: false,
                current_cancelled: false,
                rank: "a00000001".into(),
                title: "Task 1".into(),
                attempt: 0,
                attempt_id: String::new(),
                attempt_conflict: false,
            },
        },
    )
    .unwrap();
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    assert!(a.ledger.get(TEAM, "project_limit").unwrap().is_some());
    assert!(a.ledger.get(second, "project_limit").unwrap().is_none());
    // The other team's pass still hands out.
    a.tick(&mut h.store, &mut p, second, 1000, false).unwrap();
    assert_eq!(p.calls, 2);
    assert!(a.ledger.get(second, "project_limit").unwrap().is_some());
    assert_eq!(
        a.ledger.get(second, "project_limit").unwrap().unwrap().data["refusals"],
        1
    );
}

/// Two hundred ready tasks and three members leave one row and one card:
/// the first refusal stops the pass, nothing per task is gated.
#[test]
fn two_hundred_ready_tasks_leave_one_row_and_one_card() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.bind("m3");
    for n in 1..=200 {
        h.add(&person(), n);
    }
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1, "one refused effect, not eight");
    assert_eq!(
        h.store.effects(TEAM, 200, None).unwrap().rows.len(),
        1,
        "one refused effect per probe"
    );
    assert_eq!(open_cards(&h).len(), 1);
    let gates: Vec<_> = a
        .ledger
        .page(TEAM, "")
        .unwrap()
        .into_iter()
        .filter(|r| r.key.starts_with("gate:handout:"))
        .collect();
    assert!(gates.is_empty(), "no per-task gates: {gates:?}");
    assert_eq!(a.ledger.get(TEAM, "project_limit").unwrap().unwrap().data["refusals"], 1);
}

/// A reopen is not delivered while the fact stands.
#[test]
fn a_reopen_is_not_delivered_while_the_fact_stands() {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    let _ = node;
    // A pending reopen appears while the fact stands: reported by the ops
    // chain, returned by the lead, reopened for m1.
    let lead = Actor::Session {
        person: "local".into(),
        team: TEAM.into(),
        member: "lead".into(),
        session: fixture()
            .members
            .iter()
            .find(|m| m.id == "lead")
            .unwrap()
            .session
            .clone()
            .unwrap(),
    };
    let reported = h.add(&person(), 2);
    h.submit(reported, "m1");
    h.report(reported, "m1");
    // The lead's Return decision stays pending: the reopen pass would
    // deliver it. The fact stands, so the pass returns before the held-step
    // machinery, no delivery is attempted and no hold gate is written.
    h.run(
        &lead,
        Operation::Return {
            node: reported,
            reason: "One more pass".into(),
        },
    )
    .unwrap();
    let team = team(&h);
    a.reopens(&mut h.store, &mut p, &team, 1000 + 30_000).unwrap();
    assert_eq!(p.calls, 1, "no reopen delivery while the fact stands");
    assert!(
        a.ledger.get(TEAM, "gate:held:reopen:2").unwrap().is_none(),
        "the reopen pass returns before the held step"
    );
}

/// Round 2, the reviewer's P2: a crash after the fact and card are written
/// but before the delivery saved `gated` must not count the same command
/// refusal again when the recovery settles the same delivery.
#[test]
fn a_replayed_settle_counts_the_same_refusal_once() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    super::super::retry::CRASH.with(|s| s.set(Some("notice_saved")));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    }));
    let remaining = super::super::retry::CRASH.with(|s| s.replace(None));
    assert!(result.is_err() && remaining.is_none(), "the crash point was not visited");
    assert_eq!(fact_row(&h, &a).unwrap()["refusals"], 1);
    drop(a);
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    let row = fact_row(&h, &a).unwrap();
    assert_eq!(
        row["refusals"], 1,
        "one command refusal was counted again after the recovery: {row}"
    );
    assert_eq!(row["until"], 61_000, "{row}");
    assert_eq!(open_cards(&h).len(), 1);
}

/// Round 2, the reviewer's P2: an uncertain probe keeps the fact; nothing
/// proves the project admitted anything.
#[test]
fn an_uncertain_probe_keeps_the_fact() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    p.uncertain = true;
    a.tick(&mut h.store, &mut p, TEAM, 1000 + 60_000, false).unwrap();
    assert!(fact_row(&h, &a).is_some(), "an uncertain probe ended the fact");
    assert_eq!(open_cards(&h).len(), 1);
}

/// Round 2, the reviewer's P2: non-starts keep the fact, the fifth (which
/// the actor converts into an ordinary refusal) included.
#[test]
fn nonstarts_keep_the_fact_even_after_the_fifth() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    p.not_started = true;
    for now in [60_000, 120_000, 180_000, 240_000, 300_000, 360_000, 420_000, 480_000, 540_000, 600_000, 660_000, 720_000, 780_000, 840_000, 900_000, 960_000, 1_200_000, 1_800_000] {
        a.tick(&mut h.store, &mut p, TEAM, 1000 + now, false).unwrap();
    }
    assert!(p.calls >= 6, "one project refusal and at least five non-starts");
    assert!(fact_row(&h, &a).is_some(), "a converted non-start ended the fact");
}

/// Round 2: the command's own refusal with another cause ends the fact and
/// is handled as that cause (the task's own gate).
#[test]
fn the_commands_own_other_refusal_ends_the_fact() {
    let mut h = Harness::new();
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let machine = team(&h).home;
    let mut a = Runtime::open(h.temp.path(), &machine).unwrap();
    let mut p = ProjectRefused {
        reason: "project_agents_other: This project allows agents to start tasks only in their own session.".into(),
        calls: 0,
        submit_now: false,
        uncertain: false,
        not_started: false,
    };
    a.tick(&mut h.store, &mut p, TEAM, 1000, false).unwrap();
    assert_eq!(p.calls, 1);
    // The probe answers a plain refusal without the typed cause.
    struct Plain {
        calls: u32,
    }
    impl Ports for Plain {
        fn changes(&mut self, _: &str, _: &str, _: u64) -> super::super::ports::Result<zerus_replica::Changes> {
            Ok(zerus_replica::Changes {
                generation: "journal".into(),
                offset: 0,
                documents: Some(Default::default()),
            })
        }
        fn observe(&mut self, _: &mut Store, _: &TaskRef) -> super::super::ports::Result<()> {
            Ok(())
        }
        fn free(&mut self, _: &Team, m: &Member) -> super::super::ports::Result<bool> {
            Ok(m.id == "m1" || m.id == "m2")
        }
        fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
            self.calls += 1;
            HandoutOutcome::Refused {
                reason: "The member's session is gone.".into(),
            }
            .into()
        }
    }
    let mut q = Plain { calls: 0 };
    a.tick(&mut h.store, &mut q, TEAM, 1000 + 60_000, false).unwrap();
    assert_eq!(q.calls, 1);
    assert!(fact_row(&h, &a).is_none(), "the other-cause refusal did not end the fact");
    assert!(open_cards(&h).is_empty(), "the card stayed after the fact ended");
    let _ = node;
}
