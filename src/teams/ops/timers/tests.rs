use super::*;
use crate::teams::ops::tests::*;
mod answers;
mod attempts;
mod cancels;
mod cards;
mod hold;
mod retire;
mod route;
mod stages;
mod terminal;

pub(super) const T0: i64 = 1234;
pub(super) const MINUTE: i64 = 60_000;

/// Apply at an exact time with a fresh revision, as a service pass does.
pub(super) fn at(h: &mut Harness, actor: &Actor, op: Operation, now: i64) -> Result<Reply> {
    let rev = h.store.team(TEAM).unwrap().map_or(0, |t| t.revision);
    crate::teams::ops::apply(&mut h.store, actor, &request(rev, op), now)
}
pub(super) fn timer(
    kind: TimerKind,
    node: u32,
    generation: u32,
    item: u64,
    since: i64,
) -> Operation {
    Operation::FireTimer {
        timer: kind,
        node,
        generation,
        item,
        since_ms: since,
    }
}
pub(super) fn items(h: &Harness, kind: &str) -> Vec<crate::teams::store::InboxItem> {
    let page = h.store.inbox(TEAM, 200, None).unwrap();
    page.rows.into_iter().filter(|i| i.kind == kind).collect()
}
pub(super) fn cards_of(h: &Harness, kind: &str) -> Vec<crate::teams::store::Card> {
    let page = h.store.cards(TEAM, 200, None).unwrap();
    page.rows.into_iter().filter(|c| c.kind == kind).collect()
}
/// A reported node whose report item waits for the lead, and that item.
pub(super) fn waiting_report(h: &mut Harness) -> (u32, Actor, u64) {
    let (node, lead) = h.reported();
    let item = items(h, "report")[0].seq;
    (node, lead, item)
}

#[test]
fn ended_without_a_report_fires_once_and_not_after_the_report() {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    let wait = 120_000;
    let early = timer(TimerKind::NoReport, node, 1, 0, T0);
    refused(
        at(&mut h, &service(), early.clone(), T0 + wait - 1),
        code::INVALID_STATE,
    );
    let before = timer(TimerKind::NoReport, node, 1, 0, T0 - 1);
    refused(
        at(&mut h, &service(), before, T0 + 10 * wait),
        code::INVALID_STATE,
    );
    at(&mut h, &service(), early.clone(), T0 + wait).unwrap();
    assert_eq!(items(&h, "no_report").len(), 1);
    refused(
        at(&mut h, &service(), early, T0 + 2 * wait),
        code::DUPLICATE,
    );
    h.report(node, "m1");
    assert!(items(&h, "no_report").iter().all(|i| i.state == "closed"));
    let other = h.add(&person(), 2);
    h.bind("m2");
    h.submit(other, "m2");
    h.report(other, "m2");
    let late = timer(TimerKind::NoReport, other, 1, 0, T0);
    refused(
        at(&mut h, &service(), late, T0 + 10 * wait),
        code::INVALID_STATE,
    );
    assert_eq!(items(&h, "no_report").len(), 1);
}

#[test]
fn not_started_counts_from_the_stored_outcome_and_fires_once() {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    let op = timer(TimerKind::NotStarted, node, 1, 0, 0);
    refused(
        at(&mut h, &service(), op.clone(), T0 + 180_000 - 1),
        code::INVALID_STATE,
    );
    at(&mut h, &service(), op.clone(), T0 + 180_000).unwrap();
    refused(at(&mut h, &service(), op, T0 + 360_000), code::DUPLICATE);
    assert_eq!(items(&h, "not_started").len(), 1);
    let started = h.add(&person(), 2);
    h.submit(started, "m2");
    let op = timer(TimerKind::NotStarted, started, 1, 0, 0);
    refused(
        at(&mut h, &service(), op, T0 + 360_000),
        code::INVALID_STATE,
    );
    assert_eq!(items(&h, "not_started").len(), 1);
}

#[test]
fn an_outcome_records_its_own_time_once() {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    let op = Operation::HandoutOutcome {
        effect_request_id: id.clone(),
        outcome: HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    };
    at(&mut h, &service(), op, 9_999).unwrap();
    let db = h.store.db.as_ref().unwrap();
    let stored: i64 = db
        .query_row(
            "SELECT at_ms FROM effect_outcomes WHERE request_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, 9_999);
}

#[test]
fn an_undecided_report_reminds_the_lead_then_the_person_once_each() {
    let mut h = Harness::new();
    let (node, _, item) = waiting_report(&mut h);
    let lead_op = timer(TimerKind::RemindLead, node, 1, item, 0);
    refused(
        at(&mut h, &service(), lead_op.clone(), T0 + 15 * MINUTE - 1),
        code::INVALID_STATE,
    );
    at(&mut h, &service(), lead_op.clone(), T0 + 15 * MINUTE).unwrap();
    refused(
        at(&mut h, &service(), lead_op, T0 + 16 * MINUTE),
        code::DUPLICATE,
    );
    let person_op = timer(TimerKind::RemindPerson, node, 1, item, 0);
    refused(
        at(&mut h, &service(), person_op.clone(), T0 + 60 * MINUTE - 1),
        code::INVALID_STATE,
    );
    at(&mut h, &service(), person_op.clone(), T0 + 60 * MINUTE).unwrap();
    refused(
        at(&mut h, &service(), person_op, T0 + 61 * MINUTE),
        code::DUPLICATE,
    );
    assert_eq!(items(&h, "report_reminder").len(), 1);
    assert_eq!(cards_of(&h, "report_waiting").len(), 1);
}

#[test]
fn a_decided_report_reminds_nobody() {
    let mut h = Harness::new();
    let (node, lead, item) = waiting_report(&mut h);
    h.run(&lead, Operation::Accept { node }).unwrap();
    for kind in [TimerKind::RemindLead, TimerKind::RemindPerson] {
        let op = timer(kind, node, 1, item, 0);
        refused(
            at(&mut h, &service(), op, T0 + 120 * MINUTE),
            code::INVALID_STATE,
        );
    }
    assert!(cards_of(&h, "report_waiting").is_empty());
}

#[test]
fn the_silent_lead_card_needs_the_oldest_item_and_an_idle_lead() {
    let mut h = Harness::new();
    let (node, _, item) = waiting_report(&mut h);
    let wait = 20 * MINUTE;
    let quiet_since = T0 + 5 * MINUTE;
    let op = |since| timer(TimerKind::SilentLead, node, 1, item, since);
    refused(
        at(&mut h, &service(), op(quiet_since), T0 + wait),
        code::INVALID_STATE,
    );
    refused(
        at(&mut h, &service(), op(T0), T0 + wait - 1),
        code::INVALID_STATE,
    );
    let other = timer(TimerKind::SilentLead, node, 1, item + 1, T0);
    refused(
        at(&mut h, &service(), other, T0 + 2 * wait),
        code::INVALID_STATE,
    );
    at(&mut h, &service(), op(quiet_since), quiet_since + wait).unwrap();
    refused(
        at(&mut h, &service(), op(T0), T0 + 3 * wait),
        code::DUPLICATE,
    );
    assert_eq!(cards_of(&h, "silent_lead").len(), 1);
}

#[test]
fn no_timer_fires_while_the_team_is_paused_or_stopped() {
    let mut h = Harness::new();
    let (node, lead, item) = waiting_report(&mut h);
    let op = timer(TimerKind::RemindLead, node, 1, item, 0);
    h.run(&lead, Operation::Pause).unwrap();
    refused(
        at(&mut h, &service(), op.clone(), T0 + 30 * MINUTE),
        code::INVALID_STATE,
    );
    h.run(&lead, Operation::Resume).unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    refused(
        at(&mut h, &service(), op, T0 + 30 * MINUTE),
        code::INVALID_STATE,
    );
    assert!(items(&h, "report_reminder").is_empty());
}

#[test]
fn two_passes_racing_on_one_timer_make_one_card() {
    let mut h = Harness::new();
    let (node, _, item) = waiting_report(&mut h);
    let op = timer(TimerKind::RemindPerson, node, 1, item, 0);
    let mut second = Store::open(h.temp.path()).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let gate = barrier.clone();
    let racing = op.clone();
    let other = std::thread::spawn(move || {
        let mut results = Vec::new();
        for _ in 0..3 {
            gate.wait();
            let rev = second.team(TEAM).unwrap().unwrap().revision;
            results.push(
                crate::teams::ops::apply(
                    &mut second,
                    &service(),
                    &request(rev, racing.clone()),
                    T0 + 61 * MINUTE,
                )
                .map(|r| r.ok),
            );
        }
        results
    });
    let mut mine = Vec::new();
    for _ in 0..3 {
        barrier.wait();
        mine.push(at(&mut h, &service(), op.clone(), T0 + 61 * MINUTE).map(|r| r.ok));
    }
    let theirs = other.join().unwrap();
    let wins = mine
        .iter()
        .chain(theirs.iter())
        .filter(|r| r.is_ok())
        .count();
    assert_eq!(wins, 1, "{mine:?} {theirs:?}");
    assert_eq!(cards_of(&h, "report_waiting").len(), 1);
}

#[test]
fn only_the_service_fires_a_timer() {
    let mut h = Harness::new();
    let (node, lead, item) = waiting_report(&mut h);
    for actor in [person(), lead, other()] {
        let op = timer(TimerKind::RemindLead, node, 1, item, 0);
        refused(at(&mut h, &actor, op, T0 + 30 * MINUTE), code::NOT_ALLOWED);
    }
}

#[test]
fn a_task_in_review_before_its_report_is_recorded_did_not_end_without_one() {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, TaskStage::InReview);
    let op = timer(TimerKind::NoReport, node, 1, 0, T0);
    refused(
        at(&mut h, &service(), op, T0 + 10 * MINUTE),
        code::INVALID_STATE,
    );
    assert!(items(&h, "no_report").is_empty());
}
