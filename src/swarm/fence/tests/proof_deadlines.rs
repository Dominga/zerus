//! A history scan's resource allowance never extends native admission.
use super::*;
use zerus_replica::with_proof_pause_for_test;

const PAUSE: Duration = Duration::from_millis(3100);

#[test]
fn a_certificate_proof_cannot_reserve_after_the_original_deadline() {
    let m = chain_abc();
    let (uuid, _) = certified(&m);
    let runs = std::cell::Cell::new(0);
    let deadline = Instant::now() + Duration::from_millis(250);
    let (answer, steps) = with_proof_pause_for_test(0, PAUSE, || {
        admit_certified(&m.configs["a"], "p", &uuid, deadline, send(FIRST, &runs))
    });
    assert!(steps > 0 && Instant::now() >= deadline);
    assert_eq!(
        (runs.get(), attempts(&m, &uuid)),
        (0, 0),
        "proof allowance cannot extend native admission: {answer:?}"
    );
    assert_eq!(answer.unwrap().unwrap_err().reason, "deadline");
}

#[test]
fn a_fresh_proof_cannot_reserve_after_the_original_deadline() {
    let m = chain_abc();
    let task = floored(&m, "a");
    let proof = prove(
        &m.configs["a"],
        &InProcess::new(&m, Faults::default()),
        "p",
        task.uuid(),
        soon(),
    )
    .unwrap()
    .1
    .unwrap();
    let runs = std::cell::Cell::new(0);
    let deadline = Instant::now() + Duration::from_millis(250);
    let (answer, steps) = with_proof_pause_for_test(0, PAUSE, || {
        proof.admit(&m.configs["a"], deadline, send(FIRST, &runs))
    });
    assert!(steps > 0 && Instant::now() >= deadline);
    assert_eq!(
        (runs.get(), attempts(&m, task.uuid())),
        (0, 0),
        "proof allowance cannot extend native admission: {answer:?}"
    );
    assert_eq!(answer.unwrap().unwrap_err().reason, "deadline");
}

#[test]
fn the_expected_mode_load_cannot_reserve_after_the_original_deadline() {
    let m = chain_abc();
    let (uuid, _) = certified(&m);
    // Count the actual certificate scans without a business write. The next
    // step in admit_matching is in run's additional expected-mode load.
    let (control, before_run) = with_proof_pause_for_test(usize::MAX, Duration::ZERO, || {
        admit_certified(&m.configs["a"], "p", &uuid, soon(), |_, _| Ok(()))
    });
    assert_eq!(control.unwrap(), Ok(()));
    assert!(before_run > 0);
    let runs = std::cell::Cell::new(0);
    let net = counted(&m, Faults::default());
    let deadline = Instant::now() + Duration::from_secs(2);
    let (answer, steps) = with_proof_pause_for_test(before_run, PAUSE, || {
        admit_matching(
            &m.configs["a"],
            &net,
            "p",
            &uuid,
            deadline,
            Some(start_mode::Mode::Managed),
            send(FIRST, &runs),
        )
    });
    assert!(steps > before_run && Instant::now() >= deadline);
    assert_eq!(
        (runs.get(), attempts(&m, &uuid), net.calls.get()),
        (0, 0, 0),
        "expected-mode proof cannot extend admission or cause fallback: {answer:?}"
    );
    assert_eq!(outcome(answer.unwrap()), ("deadline", None, false));
}

fn a_save_past_the_deadline_is_uncertain(certified_path: bool) {
    let m = chain_abc();
    let uuid = if certified_path {
        certified(&m).0
    } else {
        let task = floored(&m, "a");
        observe_direct(&m, "a");
        task.uuid().to_owned()
    };
    let runs = std::cell::Cell::new(0);
    let net = counted(&m, Faults::default());
    let deadline = Instant::now() + Duration::from_secs(2);
    let reserve = send(FIRST, &runs);
    let answer = admit_attempt(
        &m.configs["a"],
        &net,
        "p",
        &uuid,
        deadline,
        |admitted, task| with_proof_pause_for_test(0, PAUSE, || reserve(admitted, task)).0,
    )
    .unwrap();
    assert!(Instant::now() >= deadline);
    assert!(
        matches!(answer, Admission::Uncertain(_)),
        "a save after entry must not authorize input or become a retryable refusal: {answer:?}"
    );
    assert_eq!((runs.get(), attempts(&m, &uuid)), (1, 1));
    let saved = held(&m, "a", &uuid)
        .unwrap()
        .view()
        .unwrap()
        .execution
        .unwrap();
    assert_eq!(saved.current.unwrap().id, FIRST, "keep the original UUID");
    assert_eq!(
        net.calls.get() == 0,
        certified_path,
        "no replay or fresh fallback"
    );
}

#[test]
fn a_certificate_save_past_the_original_deadline_is_uncertain() {
    a_save_past_the_deadline_is_uncertain(true);
}

#[test]
fn a_fresh_proof_save_past_the_original_deadline_is_uncertain() {
    a_save_past_the_deadline_is_uncertain(false);
}
