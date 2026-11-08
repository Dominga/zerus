use super::*;
use zerus_client_core::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};

const SESSION: &str = "claude/private-compact";
const EPOCH: &str = "33333333-3333-4333-8333-333333333333";

fn scope() -> wire::Scope {
    wire::Scope {
        configuration: "a".repeat(64),
        service_epoch: EPOCH.into(),
    }
}
/// The version 2 checkpoint of a "Compact and continue" and the exact
/// stdins of its two calls, as the composer builds them.
pub(super) fn original(text: &str) -> (Checkpoint, Vec<u8>, Vec<u8>) {
    let checkpoint = Checkpoint::reserve_compact_to_send(checkpoint::Intent {
        kind: checkpoint::Kind::CompactToSend,
        draft_revision: 2,
        message_sha256: checkpoint::message_sha256(text, Vec::<(&str, &str, &str, &[u8])>::new()),
        run_id: "run-1".into(),
        conversation_id: "conversation-1".into(),
        question: None,
        start: None,
        retry_of: None,
        consent: false,
    })
    .unwrap();
    let (compaction, send) = stdins(SESSION, &checkpoint, text, &[]).unwrap();
    (checkpoint, compaction, send)
}
fn verified_of(checkpoint: &Checkpoint, compaction: &[u8], send: &[u8]) -> wire::Verified {
    intent(&scope(), SESSION, checkpoint, compaction, send)
        .unwrap()
        .verify()
        .unwrap()
}
fn verified() -> wire::Verified {
    let (checkpoint, compaction, send) = original("hello");
    verified_of(&checkpoint, &compaction, &send)
}
/// A receipt of `verified` with its two children at `compaction` and `send`.
pub(super) fn receipt(
    verified: &wire::Verified,
    compaction: wire::Step,
    send: wire::Step,
    drained: bool,
) -> wire::Receipt {
    fn code(step: wire::Step) -> String {
        match step {
            wire::Step::NotStarted => wire::NOT_STARTED.into(),
            wire::Step::Unknown => "compact_to_send_timed_out".into(),
            _ => String::new(),
        }
    }
    let plan = &verified.plan;
    wire::Receipt {
        binding: verified.binding.clone(),
        admission_epoch: verified.intent.scope.service_epoch.clone(),
        revision: 3,
        compaction: wire::Component {
            step: compaction,
            code: code(compaction),
            outcome: (compaction == wire::Step::Recorded).then(|| wire::Compacted {
                request_id: plan.compaction_request_id.clone(),
                status: wire::SUBMITTED.into(),
            }),
        },
        send: wire::Component {
            step: send,
            code: code(send),
            outcome: (send == wire::Step::Recorded).then(|| {
                zerus_client_core::desktop::deferred_send::Outcome {
                    request_id: plan.send_request_id.clone(),
                    status: zerus_client_core::desktop::deferred_send::SUBMITTED.into(),
                    input_sha256: plan.input_sha256.clone(),
                }
            }),
        },
        drained,
        code: String::new(),
    }
}

#[test]
fn the_composers_exact_stdins_verify_and_the_plan_is_found_again_after_a_reload() {
    let (checkpoint, compaction, send) = original("hello");
    let verified = verified_of(&checkpoint, &compaction, &send);
    // Each call's stdin, byte for byte, is the original's.
    assert_eq!(verified.intent.compaction.as_bytes(), compaction.as_slice());
    assert_eq!(verified.intent.original.as_bytes(), send.as_slice());
    let input: Value = serde_json::from_slice(&send).unwrap();
    assert_eq!(input["request_id"], verified.plan.send_request_id.as_str());
    assert_eq!(
        input["expected_compaction_id"],
        verified.plan.compaction_request_id.as_str()
    );
    assert_eq!(
        plan(SESSION, &checkpoint, &compaction, &send).as_ref(),
        Some(&verified.plan)
    );
    // Another draft under the same checkpoint is never this original's plan.
    let (_, other) = stdins(SESSION, &checkpoint, "hello!", &[]).unwrap();
    assert_eq!(plan(SESSION, &checkpoint, &compaction, &other), None);
    // A DeepSeek or OpenCode session is never this kind's.
    assert_eq!(plan("dsh/example", &checkpoint, &compaction, &send), None);
    // A version 1 checkpoint reserves no send: nothing to offer.
    let mut v1 = checkpoint.clone();
    v1.version = 1;
    v1.send_request_id = None;
    assert_eq!(stdins(SESSION, &v1, "hello", &[]), None);
}

#[test]
fn only_the_exact_drained_receipt_settles_and_each_ending_keeps_its_meaning() {
    use wire::Step::*;
    let verified = verified();
    let plan = &verified.plan;
    match settled(&receipt(&verified, Recorded, Recorded, true), plan) {
        Some(SendAnswer::Recorded(outcome)) => {
            assert!(outcome.matches(&plan.send_plan()))
        }
        other => panic!("{other:?}"),
    }
    for (compaction, send, answer) in [
        (
            NotStarted,
            NotStarted,
            SendAnswer::Unsent(NOT_STARTED.into()),
        ),
        (
            Recorded,
            NotStarted,
            SendAnswer::Unsent(COMPACTION_UNCONFIRMED.into()),
        ),
        (
            Unknown,
            NotStarted,
            SendAnswer::Unsent(COMPACTION_INCOMPLETE.into()),
        ),
        (
            Recorded,
            Unknown,
            SendAnswer::Unconfirmed(UNCONFIRMED.into()),
        ),
        (
            Unknown,
            Unknown,
            SendAnswer::Unconfirmed(UNCONFIRMED.into()),
        ),
    ] {
        assert_eq!(
            settled(&receipt(&verified, compaction, send, true), plan),
            Some(answer),
            "{compaction:?} {send:?}"
        );
    }
    // Nothing before the drain settles: a compaction that runs or ran,
    // the wait, or a send that runs.
    for (compaction, send) in [
        (Running, Queued),
        (Recorded, Queued),
        (Unknown, Queued),
        (Recorded, Running),
        (Recorded, Recorded),
    ] {
        assert_eq!(
            settled(&receipt(&verified, compaction, send, false), plan),
            None,
            "{compaction:?} {send:?}"
        );
    }
    // Another original's receipt never settles this one.
    let (checkpoint, compaction, send) = original("other");
    let other = verified_of(&checkpoint, &compaction, &send);
    assert_eq!(
        settled(&receipt(&verified, Recorded, Recorded, true), &other.plan),
        None
    );
}

#[test]
fn without_a_live_service_it_stays_direct_and_a_check_knows_nothing() {
    let (checkpoint, compaction, send) = original("hello");
    assert_eq!(
        call(
            None,
            SESSION,
            &checkpoint,
            &compaction,
            &send,
            &|_| true,
            &AtomicBool::new(false)
        ),
        CompactAnswer::Direct
    );
    assert_eq!(check(None, &verified().plan), Checked::Unavailable);
}
