#[path = "support/operation_port.rs"]
mod port;

use port::{check_trace, Fixture};
use zerus_client_core::operation_recovery::task_start::{Decision, Input, Review};
use zerus_desktop_migration::sha256;

fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../fixtures/desktop-operation-port-v1.json"
    ))
    .unwrap()
}

#[test]
fn frozen_utf8_vectors_match_the_actual_pure_policy_without_reserialization() {
    let f = fixture();
    assert_eq!(f.abi, "desktop-operation-contract-v1");
    assert!(f.model_only);
    let original = f.original(true).unwrap();
    let Decision::Proposed(proposal) = original.step(Input::LookupRequested, false) else {
        panic!("expected original proposal");
    };
    assert_eq!(proposal.input(), f.retained_input.as_bytes());
    assert_eq!(sha256(proposal.input()), f.expected_plan_sha256);
    assert_eq!(proposal.args().last().unwrap(), "--recover-only");
    let mut consent = b"task-start-reconcile-consent-v1\0".to_vec();
    consent.extend_from_slice(proposal.input());
    assert_eq!(sha256(&consent), f.expected_consent_sha256);
    let key = &f.binding.key;
    let mut operation = b"desktop-operation-key-v1\0".to_vec();
    for field in [
        "task_start_reconcile",
        "1",
        &key.configuration,
        &key.project,
        &key.task,
        &key.request_id,
    ] {
        operation.extend_from_slice(&(u32::try_from(field.len()).unwrap()).to_be_bytes());
        operation.extend_from_slice(field.as_bytes());
    }
    assert_eq!(sha256(&operation), f.expected_operation_id);
    assert_ne!(f.binding.source_record_sha256, f.binding.ui_snapshot_sha256);
}

#[test]
fn fake_port_cases_cover_admission_order_identity_unknown_and_resource_drain() {
    let f = fixture();
    assert_eq!(f.cases.len(), 27);
    for case in &f.cases {
        let result = check_trace(&f, case);
        println!("contract vector {}: {:?}", case.name, result);
        assert_eq!(
            result.err(),
            case.expected_error.as_deref(),
            "{}",
            case.name
        );
        if !case.input_available {
            assert!(matches!(
                f.original(false),
                Err(Review::MissingRetainedInput)
            ));
        }
    }
}

#[test]
fn a_bounded_stop_or_restart_never_counts_as_pipe_drain() {
    let f = fixture();
    for name in [
        "unknown-holds-admission-after-bounded-stop",
        "restart-recovers-unknown-without-pretending-drain",
    ] {
        let case = f.cases.iter().find(|case| case.name == name).unwrap();
        assert!(check_trace(&f, case).is_ok());
        assert!(!case.trace.iter().any(|event| event.t == "release"));
        let mut invalid = case.clone();
        invalid.trace.push(port::Event {
            t: "release".into(),
            ticket: Some("first".into()),
            fresh: None,
            input_sha256: None,
            outcome: None,
            reason: None,
            receipt_id: None,
        });
        assert_eq!(
            check_trace(&f, &invalid),
            Err("verified_drain_before_release")
        );
    }
}

#[test]
fn a_wrong_epoch_lease_or_revision_cannot_finish_the_reserved_ticket() {
    let mut f = fixture();
    let case = f
        .cases
        .iter()
        .find(|case| case.name == "wrong-ticket-cannot-finish")
        .unwrap()
        .clone();
    let first = f.tickets["first"].clone();
    for field in [
        "lease",
        "writer_epoch",
        "service_epoch",
        "revision",
        "admission_id",
        "binding_sha256",
        "operation_id",
        "attempt_id",
    ] {
        let mut changed = first.clone();
        match field {
            "lease" => changed.lease = "changed".into(),
            "writer_epoch" => changed.writer_epoch = "changed".into(),
            "service_epoch" => changed.service_epoch = "changed".into(),
            "revision" => changed.revision += 1,
            "admission_id" => changed.admission_id = "changed".into(),
            "operation_id" => changed.operation_id = "changed".into(),
            "attempt_id" => changed.attempt_id = "changed".into(),
            _ => changed.binding_sha256 = "c".repeat(64),
        }
        f.tickets.insert("foreign".into(), changed);
        assert_eq!(
            check_trace(&f, &case),
            Err("exact_ticket_required"),
            "{field}"
        );
    }
}

#[test]
fn unknown_and_review_policy_inputs_never_propose_an_automatic_lookup() {
    let f = fixture();
    let original = f.original(true).unwrap();
    assert!(matches!(
        original.step(Input::Unknown, false),
        Decision::NeedsReview(Review::UnknownOutcome)
    ));
    assert!(matches!(
        original.step(Input::Review, false),
        Decision::NeedsReview(Review::NoLookupResult)
    ));
    assert!(matches!(
        original.step(Input::LookupRequested, true),
        Decision::NeedsReview(Review::Cancelled)
    ));
}
