use super::*;
use zerus_client_core::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};

const SESSION: &str = "claude/private-resume";
const EPOCH: &str = "33333333-3333-4333-8333-333333333333";
pub(super) const RESUMED: &str = "00000000-0000-4000-8000-0000000000b1";

fn scope() -> wire::Scope {
    wire::Scope {
        configuration: "a".repeat(64),
        service_epoch: EPOCH.into(),
    }
}
/// The version 3 checkpoint of a "Resume and send" and the exact stdins of
/// its two calls, as the composer builds them.
pub(super) fn original(text: &str) -> (Checkpoint, Vec<u8>, Vec<u8>) {
    let checkpoint = Checkpoint::reserve_session_resume(checkpoint::Intent {
        kind: checkpoint::Kind::ResumeToSend,
        draft_revision: 2,
        message_sha256: checkpoint::message_sha256(text, Vec::<(&str, &str, &str, &[u8])>::new()),
        run_id: "run-1".into(),
        conversation_id: "conversation-1".into(),
        question: None,
        start: Some(checkpoint::Start::Resume),
        retry_of: None,
        consent: false,
    })
    .unwrap();
    let (resume, send) = stdins(SESSION, &checkpoint, text, &[]).unwrap();
    (checkpoint, resume, send)
}
fn verified_of(checkpoint: &Checkpoint, resume: &[u8], send: &[u8]) -> wire::Verified {
    intent(&scope(), SESSION, checkpoint, resume, send)
        .unwrap()
        .verify()
        .unwrap()
}
fn verified() -> wire::Verified {
    let (checkpoint, resume, send) = original("hello");
    verified_of(&checkpoint, &resume, &send)
}
/// A receipt of `verified` with its two children at `resume` and `send`.
pub(super) fn receipt(
    verified: &wire::Verified,
    resume: wire::Step,
    send: wire::Step,
    drained: bool,
) -> wire::Receipt {
    fn code(step: wire::Step) -> String {
        match step {
            wire::Step::Failed => wire::REFUSED.into(),
            wire::Step::NotStarted => wire::NOT_STARTED.into(),
            wire::Step::Unknown => "session_resume_timed_out".into(),
            _ => String::new(),
        }
    }
    let plan = &verified.plan;
    let resumed = wire::Resumed {
        request_id: plan.resume_request_id.clone(),
        status: wire::COMPLETED.into(),
        result_target: wire::ResultTarget {
            name: plan.session.clone(),
            run_id: RESUMED.into(),
            conversation_id: plan.conversation_id.clone(),
        },
    };
    let bound = wire::bind(plan, &verified.intent.original, &resumed)
        .unwrap()
        .1;
    let recorded = resume == wire::Step::Recorded;
    wire::Receipt {
        binding: verified.binding.clone(),
        admission_epoch: verified.intent.scope.service_epoch.clone(),
        revision: 3,
        resume: wire::Component {
            step: resume,
            code: code(resume),
            outcome: recorded.then(|| resumed.clone()),
        },
        bound_sha256: recorded.then(|| bound.clone()),
        send: wire::Component {
            step: send,
            code: code(send),
            outcome: (send == wire::Step::Recorded).then(|| {
                zerus_client_core::desktop::deferred_send::Outcome {
                    request_id: plan.send_request_id.clone(),
                    status: zerus_client_core::desktop::deferred_send::SUBMITTED.into(),
                    input_sha256: bound.clone(),
                }
            }),
        },
        drained,
        code: String::new(),
    }
}

#[test]
fn the_composers_exact_stdins_verify_and_the_plan_is_found_again_after_a_reload() {
    let (checkpoint, resume, send) = original("hello");
    let verified = verified_of(&checkpoint, &resume, &send);
    // Each call's stdin, byte for byte, is the original's.
    assert_eq!(verified.intent.resume.as_bytes(), resume.as_slice());
    assert_eq!(verified.intent.original.as_bytes(), send.as_slice());
    let input: Value = serde_json::from_slice(&resume).unwrap();
    assert_eq!(input["action"], "resume");
    assert_eq!(
        input["request_id"],
        verified.plan.resume_request_id.as_str()
    );
    let template: Value = serde_json::from_slice(&send).unwrap();
    assert_eq!(
        template["request_id"],
        verified.plan.send_request_id.as_str()
    );
    assert_eq!(template["expected_run_id"], "run-1");
    assert!(template.get("expected_compaction_id").is_none());
    assert_eq!(
        plan(SESSION, &checkpoint, &resume, &send).as_ref(),
        Some(&verified.plan)
    );
    // Another draft under the same checkpoint is never this original's plan.
    let (_, other) = stdins(SESSION, &checkpoint, "hello!", &[]).unwrap();
    assert_eq!(plan(SESSION, &checkpoint, &resume, &other), None);
    // A Codex session is this kind's, observed (zerus-5wmh.66.4.10);
    // DeepSeek and OpenCode sessions never are.
    let codex = plan("codex/example", &checkpoint, &resume, &send).expect("a Codex plan");
    assert!(codex.observed() && !verified.plan.observed());
    for session in ["dsh/example", "opencode/example"] {
        assert_eq!(
            plan(session, &checkpoint, &resume, &send),
            None,
            "{session}"
        );
    }
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
            assert_eq!(outcome.request_id, plan.send_request_id)
        }
        other => panic!("{other:?}"),
    }
    for (resume, send, answer) in [
        (
            NotStarted,
            NotStarted,
            SendAnswer::Unsent(NOT_STARTED.into()),
        ),
        (Failed, NotStarted, SendAnswer::Unsent(NOT_RESUMED.into())),
        (Recorded, NotStarted, SendAnswer::Unsent(NOT_READY.into())),
        (Unknown, NotStarted, SendAnswer::Unsent(NOT_READY.into())),
        (
            Recorded,
            Unknown,
            SendAnswer::Unconfirmed(UNCONFIRMED.into()),
        ),
    ] {
        assert_eq!(
            settled(&receipt(&verified, resume, send, true), plan),
            Some(answer),
            "{resume:?} {send:?}"
        );
    }
    // Nothing before the drain settles: a resume that runs or ran, the
    // wait, or a send that runs.
    for (resume, send) in [
        (Running, Queued),
        (Recorded, Queued),
        (Recorded, Running),
        (Recorded, Recorded),
    ] {
        assert_eq!(
            settled(&receipt(&verified, resume, send, false), plan),
            None,
            "{resume:?} {send:?}"
        );
    }
    // Another original's receipt never settles this one.
    let (checkpoint, resume, send) = original("other");
    let other = verified_of(&checkpoint, &resume, &send);
    assert_eq!(
        settled(&receipt(&verified, Recorded, Recorded, true), &other.plan),
        None
    );
}

#[test]
fn without_a_live_service_it_stays_direct_and_a_check_knows_nothing() {
    let (checkpoint, resume, send) = original("hello");
    assert_eq!(
        call(
            None,
            SESSION,
            &checkpoint,
            &resume,
            &send,
            &|_| true,
            &AtomicBool::new(false)
        ),
        ResumeAnswer::Direct
    );
    assert_eq!(check(None, &verified().plan), (Checked::Unavailable, None));
}
