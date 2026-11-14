use super::*;
use crate::teams::ops::Operation;

fn delivered(w: &mut World) -> u32 {
    w.h.bind("lead");
    w.h.bind("m1");
    w.sessions.idle("lead", "run-1");
    w.sessions.idle("m1", "run-m1");
    let node = w.h.add(&person(), 1);
    w.h.submit(node, "m1");
    node
}

#[test]
fn ended_without_a_report_counts_from_the_delivery_and_fires_once() {
    let mut w = World::new();
    delivered(&mut w);
    w.pass(T0 + 1);
    w.pass(T0 + 120_000);
    assert!(w.items("no_report").is_empty());
    w.pass(T0 + 120_001);
    w.pass(T0 + 10 * MINUTE);
    assert_eq!(w.items("no_report").len(), 1);
}

#[test]
fn after_a_restart_the_idle_count_starts_again_late_never_early() {
    let mut w = World::new();
    delivered(&mut w);
    w.pass(T0 + 60_000);
    w.restart("epoch-2");
    w.pass(T0 + 130_000);
    w.pass(T0 + 249_999);
    assert!(w.items("no_report").is_empty());
    w.pass(T0 + 250_000);
    assert_eq!(w.items("no_report").len(), 1);
}

#[test]
fn a_member_seen_working_restarts_its_count() {
    let mut w = World::new();
    delivered(&mut w);
    w.pass(T0);
    w.sessions.working("m1");
    w.pass(T0 + 60_000);
    w.sessions.idle("m1", "run-m1");
    w.pass(T0 + 90_000);
    w.pass(T0 + 209_999);
    assert!(w.items("no_report").is_empty());
    w.pass(T0 + 210_000);
    assert_eq!(w.items("no_report").len(), 1);
}

#[test]
fn a_hand_out_not_started_reaches_the_lead_after_its_timeout() {
    let mut w = World::new();
    w.h.bind("lead");
    w.h.bind("m1");
    let node = w.h.add(&person(), 1);
    let id = w.h.intent(node, "m1");
    w.h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    w.pass(T0 + 179_999);
    assert!(w.items("not_started").is_empty());
    w.pass(T0 + 180_000);
    w.pass(T0 + 10 * MINUTE);
    assert_eq!(w.items("not_started").len(), 1);
}

/// zerus-63rru.77: the same hand-out, its member removed before the timeout.
/// The removal settled the effect and told the lead why; the timer never
/// adds a "has not started" item without the cause.
#[test]
fn a_removed_members_settled_hand_out_never_says_it_has_not_started() {
    let mut w = World::new();
    w.h.bind("lead");
    w.h.bind("m1");
    let node = w.h.add(&person(), 1);
    let id = w.h.intent(node, "m1");
    w.h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "receipt-example".into(),
        },
    );
    w.pass(T0 + 60_000);
    w.h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    for minute in 3..=30 {
        w.pass(T0 + minute * MINUTE);
    }
    assert!(w.items("not_started").is_empty());
    assert_eq!(w.items(&format!("member_removed:{id}")).len(), 1);
    assert!(w.h.node(node).hold);
}

#[test]
fn an_undecided_report_reminds_the_lead_then_the_person_once() {
    let mut w = World::new();
    w.reported();
    w.pass(T0 + 15 * MINUTE - 1);
    assert!(w.items("report_reminder").is_empty());
    w.pass(T0 + 15 * MINUTE);
    assert_eq!(w.items("report_reminder").len(), 1);
    assert!(w.cards("report_waiting").is_empty());
    w.pass(T0 + 60 * MINUTE);
    w.pass(T0 + 120 * MINUTE);
    assert_eq!(w.items("report_reminder").len(), 1);
    assert_eq!(w.cards("report_waiting").len(), 1);
}

#[test]
fn a_silent_lead_reaches_the_person_once() {
    let mut w = World::new();
    w.reported();
    w.pass(T0 + MINUTE);
    w.pass(T0 + 21 * MINUTE - 1);
    assert!(
        w.cards("silent_lead").is_empty(),
        "the lead was seen quiet for less than 20 minutes"
    );
    w.pass(T0 + 21 * MINUTE);
    w.pass(T0 + 90 * MINUTE);
    assert_eq!(w.cards("silent_lead").len(), 1);
}

#[test]
fn a_lead_seen_working_is_not_silent() {
    let mut w = World::new();
    w.reported();
    w.pass(T0);
    w.sessions.working("lead");
    w.pass(T0 + 10 * MINUTE);
    w.sessions.idle("lead", "run-1");
    w.pass(T0 + 25 * MINUTE);
    w.pass(T0 + 30 * MINUTE - 1);
    assert!(w.cards("silent_lead").is_empty());
}

#[test]
fn a_paused_team_fires_nothing_and_forgets_what_the_service_saw() {
    let mut w = World::new();
    w.reported();
    w.pass(T0);
    w.h.run(&person(), Operation::Pause).unwrap();
    w.pass(T0 + 90 * MINUTE);
    assert!(w.items("report_reminder").is_empty() && w.cards("report_waiting").is_empty());
    assert_eq!(w.effects.rang.len(), 1, "no doorbell while paused");
    w.h.run(&person(), Operation::Resume).unwrap();
    w.pass(T0 + 91 * MINUTE);
    assert_eq!(
        w.items("report_reminder").len(),
        1,
        "stored times still count"
    );
    assert!(
        w.cards("silent_lead").is_empty(),
        "the lead's quiet time starts again"
    );
}

#[test]
fn two_services_passing_together_fire_each_timer_once() {
    let mut w = World::new();
    w.reported();
    let mut other = Timers::new("epoch-2");
    for minutes in [0, 16, 61] {
        w.pass(T0 + minutes * MINUTE);
        let World {
            h,
            ledger,
            sessions,
            effects,
            ..
        } = &mut w;
        other
            .team(
                &mut h.store,
                ledger,
                sessions,
                effects,
                TEAM,
                T0 + minutes * MINUTE,
            )
            .unwrap();
    }
    assert_eq!(w.items("report_reminder").len(), 1);
    assert_eq!(w.cards("report_waiting").len(), 1);
}
