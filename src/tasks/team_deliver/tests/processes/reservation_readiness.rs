//! A supported Hold writer races private real children at the existing
//! reservation boundary. The native input port is a recorded test double.
use super::*;

fn wait_for(f: &Fixture, prefix: &str) {
    let until = Instant::now() + Duration::from_secs(8);
    while !std::fs::read_dir(&f.config.state)
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
    {
        assert!(Instant::now() < until, "child did not reach {prefix}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_explicit_hold_winning_before_final_reservation_prevents_attempt_and_input() {
    let mut f = Fixture::new();
    let (child, path) = f.spawn(
        "submitted",
        &f.args(false),
        &serde_json::to_string(&f.body).unwrap(),
        true,
    );
    wait_for(&f, "ready-");
    let before = f.document().view().unwrap();
    // Explicit modeled native Person producer; no flag or canonical row is
    // injected. Hold must actually succeed against the outstanding effect.
    let held =
        f.h.run(&person(), ops::Operation::Hold { node: f.body.node });
    std::fs::write(f.config.state.join("release"), "go").unwrap();
    let out = f.finish(child, path);
    held.expect("current Hold writer is available after spawn");
    assert!(f.h.store.node(TEAM, f.body.node).unwrap().unwrap().hold);
    assert_eq!(out["reply"]["state"], "not_started", "{out}");
    assert_eq!(f.count(), 0, "held work reached the fake input port");
    let after = f.document();
    assert!(after.attempt(&f.body.id).unwrap().is_none());
    // The existing admission fence may upgrade document metadata before
    // refusal. It must preserve work and create no execution attempt.
    let after = after.view().unwrap();
    assert_eq!(after.stage, before.stage);
    assert_eq!(after.target, before.target);
    assert_eq!(after.session, before.session);
    assert_eq!(after.title, before.title);
    assert_eq!(after.text, before.text);
}

#[test]
fn reservation_winning_before_hold_preserves_original_submission_and_receipt_only_recovery() {
    let mut f = Fixture::new();
    let (child, path) = f.spawn(
        "type-after-pause",
        &f.args(false),
        &serde_json::to_string(&f.body).unwrap(),
        false,
    );
    wait_for(&f, "typing-");
    assert!(f.document().attempt(&f.body.id).unwrap().is_some());
    let held =
        f.h.run(&person(), ops::Operation::Hold { node: f.body.node });
    std::fs::write(f.config.state.join("release-typing"), "go").unwrap();
    let out = f.finish(child, path);
    held.expect("later explicit Hold is available");
    assert_eq!(out["reply"]["state"], "submitted", "{out}");
    assert_eq!(f.count(), 1);
    let before = f.document().snapshot().unwrap();
    let recovered = f.run("submitted", false);
    assert_eq!(recovered["reply"]["state"], "submitted", "{recovered}");
    assert_eq!(f.count(), 1, "recovery typed another input");
    assert_eq!(f.document().snapshot().unwrap(), before);
    // The actual executor's TaskDoc report/save and Service observation stay
    // allowed for work whose original reservation already won.
    completion_tests::reported(&mut f);
    assert_eq!(f.count(), 1);
}

#[test]
fn an_unknown_original_under_a_later_hold_never_reserves_or_types_again() {
    let mut f = Fixture::new();
    assert_eq!(f.run("uncertain", false)["reply"]["state"], "uncertain");
    assert_eq!(f.count(), 1);
    f.h.run(&person(), ops::Operation::Hold { node: f.body.node })
        .unwrap();
    let before = f.document().snapshot().unwrap();
    let recovered = f.run("submitted", false);
    assert_eq!(recovered["reply"]["state"], "uncertain", "{recovered}");
    assert_eq!(f.count(), 1);
    assert_eq!(f.document().snapshot().unwrap(), before);
}
