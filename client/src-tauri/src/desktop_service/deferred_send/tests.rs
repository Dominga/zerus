use super::*;
use zerus_client_core::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};

const SESSION: &str = "claude/private-send";
const EPOCH: &str = "33333333-3333-4333-8333-333333333333";

fn scope() -> wire::Scope {
    wire::Scope {
        configuration: "a".repeat(64),
        service_epoch: EPOCH.into(),
    }
}
/// A Send's checkpoint and its exact stdin, as the composer builds them.
pub(super) fn original(text: &str) -> (Checkpoint, Vec<u8>) {
    let checkpoint = Checkpoint::reserve(checkpoint::Intent {
        kind: checkpoint::Kind::Send,
        draft_revision: 2,
        message_sha256: checkpoint::message_sha256(text, Vec::<(&str, &str, &str, &[u8])>::new()),
        run_id: "run-1".into(),
        conversation_id: "conversation-1".into(),
        question: None,
        start: None,
        retry_of: None,
        consent: false,
    });
    let target = zerus_client_core::Target {
        computer: String::new(),
        name: SESSION.into(),
        run_id: "run-1".into(),
        conversation_id: "conversation-1".into(),
    };
    let stdin = zerus_client_core::actions::send_input(
        &target,
        text,
        &[],
        None,
        None,
        &checkpoint.request_id,
    )
    .unwrap();
    (checkpoint, stdin)
}
fn verified_of(checkpoint: &Checkpoint, stdin: &[u8]) -> wire::Verified {
    intent(&scope(), SESSION, checkpoint, stdin)
        .unwrap()
        .verify()
        .unwrap()
}
fn verified() -> wire::Verified {
    let (checkpoint, stdin) = original("hello");
    verified_of(&checkpoint, &stdin)
}
fn receipt(
    verified: &wire::Verified,
    phase: wire::Phase,
    drained: bool,
    code: &str,
) -> wire::Receipt {
    wire::Receipt {
        binding: verified.binding.clone(),
        admission_epoch: EPOCH.into(),
        revision: 3,
        phase,
        drained,
        code: code.into(),
        outcome: (phase == wire::Phase::Recorded).then(|| wire::Outcome {
            request_id: verified.plan.request_id.clone(),
            status: wire::SUBMITTED.into(),
            input_sha256: verified.plan.input_sha256.clone(),
        }),
    }
}

#[test]
fn the_composers_exact_stdin_verifies_and_its_plan_is_found_again_after_a_reload() {
    let (checkpoint, stdin) = original("hello");
    let verified = verified_of(&checkpoint, &stdin);
    // The one call's stdin, byte for byte, is the original.
    assert_eq!(verified.intent.original.as_bytes(), stdin.as_slice());
    assert_eq!(
        plan(SESSION, &checkpoint, &stdin).as_ref(),
        Some(&verified.plan)
    );
    // Another draft under the same checkpoint is never this original's plan.
    let target = zerus_client_core::Target {
        computer: String::new(),
        name: SESSION.into(),
        run_id: "run-1".into(),
        conversation_id: "conversation-1".into(),
    };
    let other = zerus_client_core::actions::send_input(
        &target,
        "hello!",
        &[],
        None,
        None,
        &checkpoint.request_id,
    )
    .unwrap();
    assert_eq!(plan(SESSION, &checkpoint, &other), None);
    // A DeepSeek or OpenCode session is never this kind's.
    assert_eq!(plan("dsh/example", &checkpoint, &stdin), None);
}

#[test]
fn only_the_exact_drained_receipt_settles_and_a_drained_unknown_is_unconfirmed() {
    let verified = verified();
    let plan = &verified.plan;
    match settled(&receipt(&verified, wire::Phase::Recorded, true, ""), plan) {
        Some(SendAnswer::Recorded(outcome)) => assert!(outcome.matches(plan)),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        settled(
            &receipt(&verified, wire::Phase::Failed, true, wire::NOT_STARTED),
            plan
        ),
        Some(SendAnswer::Unsent(NOT_STARTED.into()))
    );
    assert_eq!(
        settled(
            &receipt(
                &verified,
                wire::Phase::Unknown,
                true,
                "deferred_send_receipt_missing"
            ),
            plan
        ),
        Some(SendAnswer::Unconfirmed(UNCONFIRMED.into()))
    );
    // Queued, Running and an undrained Unknown never settle.
    for (phase, code) in [
        (wire::Phase::Queued, ""),
        (wire::Phase::Running, ""),
        (wire::Phase::Unknown, "awaiting_resource_drain"),
    ] {
        assert_eq!(settled(&receipt(&verified, phase, false, code), plan), None);
    }
    // Another original's receipt never settles this one.
    let (checkpoint, stdin) = original("other");
    let other = intent(&scope(), SESSION, &checkpoint, &stdin)
        .unwrap()
        .verify()
        .unwrap();
    assert_eq!(
        settled(
            &receipt(&verified, wire::Phase::Recorded, true, ""),
            &other.plan
        ),
        None
    );
}

#[test]
fn without_a_live_service_a_send_stays_direct_and_a_check_knows_nothing() {
    let (checkpoint, stdin) = original("hello");
    assert_eq!(
        call(None, SESSION, &checkpoint, &stdin, &AtomicBool::new(false)),
        SendAnswer::Direct
    );
    assert_eq!(check(None, &verified().plan), Checked::Unavailable);
}
