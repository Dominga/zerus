//! The member's doorbell for the lead's answers, through `Timers::answers`.
use super::*;
use crate::teams::{ops::Operation, store::timers::Gate};

const QUESTION: &str = "Which width exactly?";
const ANSWER: &str = "Keep the old width, carefully.";
const TEXT_ONE: &str = "1 answer from your lead waits. Read it with: hgs team current";

struct Asked {
    w: World,
    lead: Actor,
    m1: Actor,
    node: u32,
}
/// m1 holds a task in progress and is idle; the lead is bound.
fn asked() -> Asked {
    let mut w = World::new();
    let lead = w.h.bind("lead");
    let m1 = w.h.bind("m1");
    w.h.bind("m2");
    let node = w.h.add(&person(), 1);
    w.h.submit(node, "m1");
    w.sessions.idle("m1", "run-m1");
    Asked { w, lead, m1, node }
}
impl Asked {
    fn answer(&mut self) {
        let op = Operation::Ask {
            node: self.node,
            generation: 1,
            text: QUESTION.into(),
        };
        let item = self.w.h.run(&self.m1, op).unwrap().item.unwrap();
        let reply = Operation::Reply {
            item,
            text: ANSWER.into(),
        };
        self.w.h.run(&self.lead, reply).unwrap();
    }
    fn states(&self) -> Vec<String> {
        let mut s = self
            .w
            .ledger
            .connection()
            .prepare("SELECT state FROM timer_answer_bells ORDER BY at_ms,rowid")
            .unwrap();
        let rows = s.query_map([], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<Vec<String>>>().unwrap()
    }
}

#[test]
fn an_answer_rings_its_member_with_the_count_and_the_command_only() {
    let mut a = asked();
    a.answer();
    a.w.answers(T0);
    let rang = &a.w.effects.rang;
    assert_eq!(rang.len(), 1);
    assert_eq!(rang[0].text, TEXT_ONE);
    assert!(!rang[0].text.contains(QUESTION) && !rang[0].text.contains(ANSWER));
    assert_eq!(
        (rang[0].run.as_str(), rang[0].conversation.as_str()),
        ("run-m1", "conversation-m1")
    );
    let Gate::Answer(bell) = &rang[0].gate else {
        panic!("a member's gate")
    };
    assert_eq!(
        (bell.member.as_str(), bell.node, bell.generation),
        ("m1", a.node, 1)
    );
    a.w.answers(T0 + 10 * MINUTE);
    assert_eq!(a.w.effects.rang.len(), 1, "once for its mark");
}

#[test]
fn two_replies_before_one_doorbell_are_counted_in_one_message() {
    let mut a = asked();
    a.answer();
    a.answer();
    a.w.answers(T0);
    assert_eq!(a.w.effects.rang.len(), 1);
    assert_eq!(
        a.w.effects.rang[0].text,
        "2 answers from your lead wait. Read them with: hgs team current"
    );
    a.answer();
    a.w.answers(T0 + 2 * MINUTE);
    assert_eq!(
        a.w.effects.rang[1].text, TEXT_ONE,
        "only the reply no doorbell counted"
    );
}

#[test]
fn a_paused_team_rings_nothing_and_rings_after_the_resume() {
    let mut a = asked();
    a.answer();
    a.w.h.run(&person(), Operation::Pause).unwrap();
    a.w.answers(T0);
    assert!(a.w.effects.rang.is_empty());
    a.w.h.run(&person(), Operation::Resume).unwrap();
    a.w.answers(T0 + MINUTE);
    assert_eq!(a.w.effects.rang.len(), 1);
}

#[test]
fn an_uncertain_doorbell_is_not_sent_again_and_a_newer_reply_rings_a_new_one() {
    let mut a = asked();
    a.answer();
    a.w.effects
        .rungs
        .push_back(Rung::Uncertain("Lost receipt".into()));
    a.w.answers(T0);
    a.w.answers(T0 + 10 * MINUTE);
    assert_eq!(a.w.effects.rang.len(), 1);
    a.answer();
    a.w.answers(T0 + 11 * MINUTE);
    assert_eq!(a.w.effects.rang.len(), 2);
    assert_eq!(a.states(), ["uncertain", "submitted"]);
}

#[test]
fn a_refused_doorbell_rings_again_after_the_interval() {
    let mut a = asked();
    a.answer();
    a.w.effects.rungs.push_back(Rung::Refused("Not now".into()));
    a.w.answers(T0);
    a.w.answers(T0 + 60_000);
    assert_eq!(a.w.effects.rang.len(), 1);
    a.w.answers(T0 + 120_000);
    assert_eq!(a.w.effects.rang.len(), 2);
    assert_eq!(a.states(), ["refused", "submitted"]);
}

#[test]
fn a_decision_or_a_stop_sends_nothing() {
    let ends: [fn(u32) -> Operation; 4] = [
        |node| Operation::Accept { node },
        |node| Operation::Return {
            node,
            reason: "Fix it".into(),
        },
        |node| Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Fresh eyes".into(),
        },
        |node| Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    ];
    for end in ends {
        let mut a = asked();
        a.answer();
        a.w.h.report(a.node, "m1");
        let lead = a.lead.clone();
        a.w.h.run(&lead, end(a.node)).unwrap();
        a.w.answers(T0);
        assert!(a.w.effects.rang.is_empty());
    }
    let mut a = asked();
    a.answer();
    a.w.h.run(&person(), Operation::Stop).unwrap();
    a.w.answers(T0);
    assert!(a.w.effects.rang.is_empty());
}

#[test]
fn an_attempt_that_ends_between_the_reservation_and_the_spawn_starts_nothing() {
    let mut a = asked();
    a.answer();
    a.w.h.report(a.node, "m1");
    let (path, node) = (a.w.h.temp.path().to_owned(), a.node);
    a.w.effects.before_spawn = Some(Box::new(move || {
        let mut store = Store::open(&path).unwrap();
        let rev = store.team(TEAM).unwrap().unwrap().revision;
        crate::teams::ops::apply(
            &mut store,
            &person(),
            &request(rev, Operation::Accept { node }),
            T0,
        )
        .unwrap();
    }));
    a.w.answers(T0);
    assert!(a.w.effects.rang.is_empty());
    assert_eq!(a.w.effects.refusals, 1);
    assert_eq!(a.states(), ["not_started"]);
}

#[test]
fn a_busy_or_unknown_member_gets_no_doorbell() {
    for busy in [true, false] {
        let mut a = asked();
        a.answer();
        if busy {
            a.w.sessions.working("m1");
        } else {
            a.w.sessions.0.remove("m1");
        }
        a.w.answers(T0);
        assert!(a.w.effects.rang.is_empty());
    }
}

/// A later observation of the task, written by another writer: from the
/// newest one stored, with the given changes.
fn later(
    store: &mut Store,
    node: u32,
    change: impl FnOnce(&mut crate::teams::ops::TaskObservation),
) {
    let old = store.task_observation(TEAM, node).unwrap().unwrap();
    let mut o = old.clone();
    o.previous_revision = old.revision.clone();
    o.revision = format!("{}-later", old.revision);
    change(&mut o);
    let rev = store.team(TEAM).unwrap().unwrap().revision;
    crate::teams::ops::apply(
        store,
        &service(),
        &request(
            rev,
            Operation::ObserveTask {
                node,
                observation: o,
            },
        ),
        T0,
    )
    .unwrap();
}
fn person_attempt(o: &mut crate::teams::ops::TaskObservation) {
    o.stage = crate::teams::ops::TaskStage::InProgress;
    o.attempt += 1;
    o.attempt_id = "00000000-0000-4000-8000-00000000bbbb".into();
}
fn board_cancel(o: &mut crate::teams::ops::TaskObservation) {
    o.cancelled = true;
}
fn key_left(o: &mut crate::teams::ops::TaskObservation) {
    o.key = crate::teams::ops::KeyState::Absent;
}

/// A task moved to Done on the board ends its attempt without a decision:
/// its mark is dropped and nothing rings.
#[test]
fn a_task_done_on_the_board_rings_nothing() {
    let mut a = asked();
    a.answer();
    a.w.h.observe(a.node, crate::teams::ops::TaskStage::Done);
    a.w.answers(T0);
    assert!(a.w.effects.rang.is_empty());
    assert!(a.states().is_empty());
}

/// Between the reservation and the spawn: a person's execution taken over,
/// the task cancelled on the board, or its key gone. None starts the send.
/// The key's departure drops no mark: the admission's holder check alone
/// refuses it.
#[test]
fn an_attempt_taken_over_cancelled_or_left_before_the_spawn_starts_nothing() {
    let changes: [fn(&mut crate::teams::ops::TaskObservation); 3] =
        [person_attempt, board_cancel, key_left];
    for change in changes {
        let mut a = asked();
        a.answer();
        let (path, node) = (a.w.h.temp.path().to_owned(), a.node);
        a.w.effects.before_spawn = Some(Box::new(move || {
            let mut store = Store::open(&path).unwrap();
            later(&mut store, node, change);
        }));
        a.w.answers(T0);
        assert!(a.w.effects.rang.is_empty());
        assert_eq!(a.w.effects.refusals, 1);
        assert_eq!(a.states(), ["not_started"]);
    }
}

/// A mark whose attempt the member no longer holds (the task's key left
/// the team, which drops no mark) reserves nothing: the pass's own holder
/// check.
#[test]
fn a_mark_whose_attempt_is_no_longer_held_reserves_nothing() {
    let mut a = asked();
    a.answer();
    let node = a.node;
    later(&mut a.w.h.store, node, key_left);
    assert_eq!(a.w.h.store.answer_marks(TEAM, "", 10).unwrap().len(), 1);
    a.w.answers(T0);
    assert!(a.w.effects.rang.is_empty());
    assert!(a.states().is_empty(), "no reservation");
}

/// A marked member who is not active (paused by the person) gets no
/// doorbell: the pass's member filter.
#[test]
fn a_paused_member_gets_no_doorbell() {
    let mut a = asked();
    a.answer();
    a.w.h
        .run(
            &person(),
            Operation::PauseMember {
                member: "m1".into(),
            },
        )
        .unwrap();
    a.w.answers(T0);
    assert!(a.w.effects.rang.is_empty());
    assert!(a.states().is_empty(), "no reservation");
}
