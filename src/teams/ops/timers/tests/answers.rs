//! A lead's answer reaches its member: the member's own read of the current
//! attempt's questions, and one mark per member that a doorbell rings for.
use super::*;
use crate::teams::store::timers::{admit_answer, AnswerBell, AnswerMark};

fn ask(h: &mut Harness, member: &Actor, node: u32, text: &str) -> u64 {
    let generation = h.store.generation(TEAM, node).unwrap().unwrap();
    let op = Operation::Ask {
        node,
        generation,
        text: text.into(),
    };
    h.run(member, op).unwrap().item.unwrap()
}
fn reply(h: &mut Harness, actor: &Actor, item: u64, text: &str) {
    h.run(
        actor,
        Operation::Reply {
            item,
            text: text.into(),
        },
    )
    .unwrap();
}
fn marks(h: &Harness) -> Vec<AnswerMark> {
    h.store.answer_marks(TEAM, "", 200).unwrap()
}
/// m1 holds node 1 in progress; the lead is bound.
fn holding() -> (Harness, Actor, Actor, u32) {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let m1 = h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    (h, lead, m1, node)
}

#[test]
fn an_answer_is_read_by_its_asker_and_marks_only_them() {
    let (mut h, lead, m1, node) = holding();
    let item = ask(&mut h, &m1, node, "Which width?");
    let shown = h.store.questions(TEAM, node, 1, "m1").unwrap();
    assert_eq!(shown.rows[0].state, "open");
    assert_eq!(shown.rows[0].answer, None);
    reply(&mut h, &lead, item, "Keep the old width.");
    let shown = h.store.questions(TEAM, node, 1, "m1").unwrap();
    assert_eq!(
        (shown.rows[0].item, shown.rows[0].state, shown.more),
        (item, "answered", false)
    );
    assert_eq!(shown.rows[0].question, "Which width?");
    assert_eq!(shown.rows[0].answer.as_deref(), Some("Keep the old width."));
    assert!(
        h.store
            .questions(TEAM, node, 1, "m2")
            .unwrap()
            .rows
            .is_empty(),
        "nobody else's"
    );
    let m = marks(&h);
    assert_eq!(m.len(), 1);
    assert_eq!(
        (
            m[0].member.as_str(),
            m[0].node,
            m[0].generation,
            m[0].replies
        ),
        ("m1", node, 1, 1)
    );
}

#[test]
fn an_earlier_attempts_question_is_not_shown_and_its_reply_marks_nobody() {
    let (mut h, lead, m1, node) = holding();
    let old = ask(&mut h, &m1, node, "Old question");
    h.report(node, "m1");
    h.run(
        &lead,
        Operation::Return {
            node,
            reason: "Fix the edge case".into(),
        },
    )
    .unwrap();
    let id = h.reopen(node, "m1");
    h.reopen_outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    h.observe(node, TaskStage::InProgress);
    assert!(h
        .store
        .questions(TEAM, node, 2, "m1")
        .unwrap()
        .rows
        .is_empty());
    let closed = h.store.questions(TEAM, node, 1, "m1").unwrap();
    assert_eq!(
        closed.rows[0].state, "closed",
        "the report closed it unanswered"
    );
    let reopened = h.run(
        &lead,
        Operation::Reply {
            item: old,
            text: "Late".into(),
        },
    );
    assert!(reopened.is_err() || marks(&h).is_empty());
    assert!(marks(&h).is_empty());
}

#[test]
fn a_reply_to_an_older_generation_marks_nobody() {
    let (mut h, lead, m1, node) = holding();
    let old = ask(&mut h, &m1, node, "Asked in round one");
    // Round one's effect stays behind as reported; round two is the
    // member's again, and the question of round one is still open.
    let db = h.store.db.as_ref().unwrap();
    db.execute(
        "UPDATE effects SET state='reported' WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, node],
    )
    .unwrap();
    db.execute("INSERT INTO effects(team,node,member,generation,request_id,actor,kind,state,created_ms) VALUES(?1,?2,'m1',2,'00000000-0000-4000-8000-0000000000e3','{}','reopen','submitted',1)", rusqlite::params![TEAM, node]).unwrap();
    db.execute(
        "UPDATE nodes SET generation=2 WHERE team=?1 AND id=?2",
        rusqlite::params![TEAM, node],
    )
    .unwrap();
    reply(&mut h, &lead, old, "Too late for round two");
    assert!(marks(&h).is_empty());
}

#[test]
fn a_reply_marks_nobody_when_the_asker_no_longer_holds_the_attempt() {
    let (mut h, lead, m1, node) = holding();
    let item = ask(&mut h, &m1, node, "Still mine?");
    let db = h.store.db.as_ref().unwrap();
    db.execute(
        "UPDATE effects SET state='superseded' WHERE team=?1 AND node=?2",
        rusqlite::params![TEAM, node],
    )
    .unwrap();
    db.execute("INSERT INTO effects(team,node,member,generation,request_id,actor,kind,state,created_ms) VALUES(?1,?2,'m2',1,'00000000-0000-4000-8000-0000000000e2','{}','handout','submitted',1)", rusqlite::params![TEAM, node]).unwrap();
    reply(&mut h, &lead, item, "Answer");
    assert!(marks(&h).is_empty());
}

#[test]
fn ten_questions_newest_first_and_a_flag_for_more() {
    let (mut h, lead, m1, node) = holding();
    let mut items = Vec::new();
    for n in 0..11 {
        let item = ask(&mut h, &m1, node, &format!("Question {n}"));
        reply(&mut h, &lead, item, &format!("Answer {n}"));
        items.push(item);
    }
    let shown = h.store.questions(TEAM, node, 1, "m1").unwrap();
    assert_eq!(shown.rows.len(), 10);
    assert!(shown.more);
    let expected: Vec<u64> = items.iter().rev().take(10).copied().collect();
    assert_eq!(
        shown.rows.iter().map(|q| q.item).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        marks(&h)[0].replies,
        11,
        "one mark, counting every reply of the attempt"
    );
}

#[test]
fn a_decision_drops_the_mark_and_a_reply_after_it_marks_nobody() {
    let decisions: [fn(u32) -> Operation; 4] = [
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
    for decide in decisions {
        let (mut h, lead, m1, node) = holding();
        h.bind("m2");
        let first = ask(&mut h, &m1, node, "First");
        let second = ask(&mut h, &m1, node, "Second");
        reply(&mut h, &lead, first, "One");
        h.report(node, "m1");
        assert_eq!(marks(&h).len(), 1);
        h.run(&lead, decide(node)).unwrap();
        assert!(marks(&h).is_empty(), "the decision drops the mark");
        let late = h.run(
            &lead,
            Operation::Reply {
                item: second,
                text: "Two".into(),
            },
        );
        assert!(late.is_err() || marks(&h).is_empty());
        assert!(
            marks(&h).is_empty(),
            "a reply after the decision marks nobody"
        );
    }
    let (mut h, lead, m1, node) = holding();
    let item = ask(&mut h, &m1, node, "Before the stop");
    reply(&mut h, &lead, item, "Answer");
    h.run(&person(), Operation::Stop).unwrap();
    assert!(marks(&h).is_empty(), "a stop drops every mark");
}

#[test]
fn the_admission_reads_the_team_the_binding_the_mark_and_the_holder() {
    let (mut h, lead, m1, node) = holding();
    let item = ask(&mut h, &m1, node, "Question");
    reply(&mut h, &lead, item, "Answer");
    let Actor::Session { session, .. } = m1.clone() else {
        unreachable!()
    };
    let mark = marks(&h).remove(0);
    let bell = AnswerBell {
        team: TEAM.into(),
        member: "m1".into(),
        session,
        node,
        generation: 1,
        mark: mark.revision,
    };
    let db = h.store.db.as_ref().unwrap();
    admit_answer(db, &bell).unwrap();
    let mut newer = bell.clone();
    newer.mark += 1;
    assert!(
        admit_answer(db, &newer).is_err(),
        "a mark newer than the stored one"
    );
    let mut other = bell.clone();
    other.session.session_id = "run_id:another-run".into();
    assert!(admit_answer(db, &other).is_err(), "another binding");
    h.run(&lead, Operation::Pause).unwrap();
    assert!(
        admit_answer(h.store.db.as_ref().unwrap(), &bell).is_err(),
        "paused"
    );
    h.run(&lead, Operation::Resume).unwrap();
    admit_answer(h.store.db.as_ref().unwrap(), &bell).unwrap();
    h.report(node, "m1");
    h.run(&lead, Operation::Accept { node }).unwrap();
    assert!(
        admit_answer(h.store.db.as_ref().unwrap(), &bell).is_err(),
        "the mark was dropped"
    );
}

/// An attempt that ends without a decision drops its mark, as a decision
/// does: the task Done on the board (with branch integration the node then
/// waits for its merge), the task cancelled on the board, and a person's
/// execution taken over.
#[test]
fn an_attempt_that_ends_without_a_decision_drops_its_mark() {
    for end in ["done", "cancelled", "taken over"] {
        let (mut h, lead, m1, node) = holding();
        let item = ask(&mut h, &m1, node, "Which width?");
        reply(&mut h, &lead, item, "The old one.");
        assert_eq!(marks(&h).len(), 1, "{end}");
        match end {
            "done" => h.observe(node, TaskStage::Done),
            "cancelled" => h.cancel_fact(node),
            _ => {
                let mut o = h.observation(node, TaskStage::InProgress);
                o.attempt = 2;
                o.attempt_id = "00000000-0000-4000-8000-00000000bbbb".into();
                h.run(
                    &service(),
                    Operation::ObserveTask {
                        node,
                        observation: o,
                    },
                )
                .unwrap();
                assert_eq!(h.store.generation(TEAM, node).unwrap(), Some(2));
            }
        }
        assert!(marks(&h).is_empty(), "{end}");
    }
}

/// A lead's Cancel or Reassign leaves the member's question open; a reply
/// to it while that decision is pending marks nobody.
#[test]
fn a_reply_while_a_cancel_or_reassign_is_pending_marks_nobody() {
    let decisions: [fn(u32) -> Operation; 2] = [
        |node| Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
        |node| Operation::Reassign {
            node,
            member: "m2".into(),
            reason: "Fresh eyes".into(),
        },
    ];
    for decide in decisions {
        let (mut h, lead, m1, node) = holding();
        h.bind("m2");
        let item = ask(&mut h, &m1, node, "Which width?");
        h.run(&lead, decide(node)).unwrap();
        reply(&mut h, &lead, item, "Too late.");
        assert!(marks(&h).is_empty());
    }
}
