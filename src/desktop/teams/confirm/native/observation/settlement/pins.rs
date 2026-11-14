//! Product-written exits and inspections, with explicit read-fault controls.
use super::*;

const TEN_MINUTES_MS: i64 = 10 * 60 * 1000;

#[test]
fn the_supervisor_exit_time_anchors_the_first_settlement_wait() {
    child("exit-deadline");
}

#[test]
fn the_ten_minute_wait_is_not_shortened_to_the_teardown_sum() {
    child("wait-cap");
}

#[test]
fn missing_or_malformed_archive_readiness_never_plans_a_resume() {
    child("readiness-fault");
}

#[test]
fn an_unarchived_supervisor_exit_needs_no_archive_readiness() {
    child("unarchived-exit");
}

#[test]
fn an_out_of_range_exit_time_fails_before_a_resume_claim() {
    child("exit-time-fault");
}

pub(super) fn run(case: &str) -> bool {
    match case {
        "exit-deadline" => exit_deadline(),
        "wait-cap" => wait_cap(),
        "readiness-fault" => readiness_fault(),
        "unarchived-exit" => unarchived_exit(),
        "exit-time-fault" => exit_time_fault(),
        _ => return false,
    }
    println!("SETTLEMENT_WRITERS_EXECUTED={case}");
    true
}

fn exit_ms(p: &WriterPorts) -> i64 {
    (p.exit.raw["exited_at"].as_f64().unwrap() * 1000.0).ceil() as i64
}

fn unchanged(p: &WriterPorts, before: (i64, i64, usize)) {
    assert_eq!(
        p.counts(),
        before,
        "waiting must write no effect, audit or card"
    );
    assert_eq!(p.originals, 0);
    assert_eq!(p.cards, 0);
}

fn exit_deadline() {
    let mut p = WriterPorts::new();
    p.exit.hold_pane();
    let exited = exit_ms(&p);
    let wait = p.observe(&p.binding.clone()).unwrap().resume_wait.unwrap();
    assert_eq!(
        wait.until,
        exited + engine::RESUME_SETTLE_MS,
        "the deadline must include the named bound from the supervisor's exit"
    );
    let before = p.counts();
    p.run(exited + 1);
    unchanged(&p, before);
    // A fresh observation uses the durable exit, rather than restarting its wait.
    assert_eq!(
        p.observe(&p.binding.clone())
            .unwrap()
            .resume_wait
            .unwrap()
            .until,
        wait.until
    );
    p.exit.release();
    p.run(exited + 2);
    assert_eq!((p.originals, p.admitted), (1, 1));
}

fn wait_cap() {
    let mut p = WriterPorts::new();
    p.exit.hold_pane();
    let exited = exit_ms(&p);
    let before = p.counts();
    // These times deliberately do not depend on the observed deadline or bound.
    // The reviewed policy allows ten minutes even when teardown budgets are less.
    for elapsed in [1, 9 * 60 * 1000, TEN_MINUTES_MS - 1] {
        p.run(exited + elapsed);
        unchanged(&p, before);
    }
    let wait = p.observe(&p.binding.clone()).unwrap().resume_wait.unwrap();
    assert!(wait.until >= exited + TEN_MINUTES_MS);
}

fn readiness_fault() {
    let mut p = WriterPorts::new();
    p.exit.hold_pane();
    let view = p.exit.view();
    assert_eq!(view["restore_name_busy"], true);
    let now = exit_ms(&p) + 1;
    let before = p.counts();
    // Deliberately corrupt one field in a real inspection response. These are
    // reader-fault inputs, never persisted or expected ledger/audit/card rows.
    for value in [None, Some(Value::Null), Some(json!("true")), Some(json!(1))] {
        let mut broken = view.clone();
        broken.as_object_mut().unwrap().remove("restore_name_busy");
        if let Some(value) = value {
            broken["restore_name_busy"] = value;
        }
        p.inspection_reply = Some(broken);
        let binding = p.binding.clone();
        let result = engine::step(&mut p, &binding, "readiness-fault", now);
        assert!(result.unwrap_err().contains("readiness is unavailable"));
        unchanged(&p, before);
    }
}

fn unarchived_exit() {
    let mut p = WriterPorts::with_exit(Fixture::unarchived());
    assert_eq!(p.exit.raw["completion_reason"], "unconfirmed_exit");
    assert_eq!(p.exit.raw["exit_code"], 1);
    assert!(p.exit.id.is_empty());
    let view = p.exit.view();
    assert!(view.get("restore_name_busy").is_none());
    let observed = p.observe(&p.binding.clone()).unwrap();
    assert!(observed.self_exit && observed.ended && !observed.live);
    assert!(observed.archive.is_none());
    assert!(observed.resume_wait.is_none());
    unchanged(&p, (0, 0, 0));
}

fn exit_time_fault() {
    let mut p = WriterPorts::new();
    p.exit.hold_pane();
    let now = exit_ms(&p) + 1;
    let before = p.counts();
    // Fault the raw read frame after the supervisor/archive writer produced it.
    // Its identities and the separate product inspection remain unchanged.
    p.exit.raw["exited_at"] = json!(1e300);
    let binding = p.binding.clone();
    let result = engine::step(&mut p, &binding, "exit-time-fault", now);
    assert_eq!(result.unwrap_err(), "The original exit time is invalid");
    unchanged(&p, before);
}
