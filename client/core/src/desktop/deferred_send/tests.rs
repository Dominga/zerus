use super::*;
use crate::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};
use serde_json::{json, Value};

const SESSION: &str = "claude/private-send";
const RUN: &str = "run-1";
const CONVERSATION: &str = "conversation-1";

fn scope() -> Scope {
    Scope {
        configuration: "a".repeat(64),
        service_epoch: "00000000-0000-4000-8000-000000000090".into(),
    }
}
fn reserved(text: &str, files: &[(&str, &str, &str, &[u8])], kind: checkpoint::Kind) -> Checkpoint {
    Checkpoint::reserve(checkpoint::Intent {
        kind,
        draft_revision: 3,
        message_sha256: checkpoint::message_sha256(text, files.iter().copied()),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
        question: (kind == checkpoint::Kind::Answer).then(|| checkpoint::Question {
            id: "q".into(),
            identity_sha256: "c".repeat(64),
        }),
        start: (kind == checkpoint::Kind::ResumeToSend).then_some(checkpoint::Start::Resume),
        retry_of: None,
        consent: true,
    })
}
/// The stdin the client builds for `checkpoint`.
fn stdin(checkpoint: &Checkpoint, text: &str, files: &[(&str, &str, &str, &[u8])]) -> Value {
    let attachments: Vec<Value> = files
        .iter()
        .map(|(name, mime, reference, bytes)| {
            json!({"name": name, "mime": mime, "data_base64": STANDARD.encode(bytes),
                "reference": reference})
        })
        .collect();
    json!({"request_id": checkpoint.request_id, "text": text, "attachments": attachments,
        "expected_run_id": checkpoint.run_id, "expected_conversation_id": checkpoint.conversation_id})
}
fn intent_of(checkpoint: &Checkpoint, original: &str) -> Intent {
    Intent {
        scope: scope(),
        session: SESSION.into(),
        request_id: checkpoint.request_id.clone(),
        checkpoint: serde_json::to_string(checkpoint).unwrap(),
        original: original.into(),
    }
}
const FILE: (&str, &str, &str, &[u8]) = ("a.txt", "text/plain", "[File #1]", b"hello");
/// A valid deferred send with one attachment.
pub(crate) fn intent() -> Intent {
    let checkpoint = reserved("see [File #1]", &[FILE], checkpoint::Kind::Send);
    let original = stdin(&checkpoint, "see [File #1]", &[FILE]).to_string();
    intent_of(&checkpoint, &original)
}

#[test]
fn a_saved_send_checkpoint_and_its_exact_stdin_verify_into_one_plan() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    assert_eq!(verified.plan.session, SESSION);
    assert_eq!(verified.plan.request_id, intent.request_id);
    assert_eq!(
        (
            verified.plan.run_id.as_str(),
            verified.plan.conversation_id.as_str()
        ),
        (RUN, CONVERSATION)
    );
    // The CLI's receipt digest is the SHA-256 of the exact stdin.
    assert_eq!(verified.plan.input_sha256, hash(intent.original.as_bytes()));
    assert_eq!(verified.plan.args(), ["send", SESSION, "--json"]);
    assert!(verified.binding.valid());
    assert_eq!(
        verified.binding.plan_sha256,
        verified.plan.fingerprint().unwrap()
    );
    assert_eq!(
        verified.binding.operation_id(),
        intent.clone().verify().unwrap().binding.operation_id()
    );
    // Any other byte of the stdin is another binding.
    let spaced = intent_of(
        &serde_json::from_str(&intent.checkpoint).unwrap(),
        &format!(" {}", intent.original),
    );
    let other = spaced.verify().unwrap();
    assert_ne!(other.binding, verified.binding);
    assert_eq!(
        other.binding.operation_id(),
        verified.binding.operation_id()
    );
}

#[test]
fn only_a_plain_send_matching_its_checkpoint_verifies() {
    let base = intent();
    let cp: Checkpoint = serde_json::from_str(&base.checkpoint).unwrap();
    let original: Value = serde_json::from_str(&base.original).unwrap();
    let with = |key: &str, value: Value| {
        let mut changed = original.clone();
        changed[key] = value;
        intent_of(&cp, &changed.to_string())
    };
    let cases: Vec<(&str, Intent, &str)> = vec![
        (
            "scope",
            Intent {
                scope: Scope {
                    configuration: "x".into(),
                    ..scope()
                },
                ..base.clone()
            },
            "invalid_deferred_send_intent",
        ),
        (
            "remote alias",
            Intent {
                session: "@peer/claude".into(),
                ..base.clone()
            },
            "invalid_deferred_send_intent",
        ),
        (
            "one-part session",
            Intent {
                session: "claude".into(),
                ..base.clone()
            },
            "invalid_deferred_send_intent",
        ),
        (
            "request id",
            Intent {
                request_id: "00000000-0000-4000-8000-000000000001".into(),
                ..base.clone()
            },
            "invalid_deferred_send_checkpoint",
        ),
        (
            "checkpoint json",
            Intent {
                checkpoint: "{}".into(),
                ..base.clone()
            },
            "invalid_deferred_send_checkpoint",
        ),
        (
            "stdin request id",
            with("request_id", json!("00000000-0000-4000-8000-000000000001")),
            "deferred_send_intent_changed",
        ),
        (
            "stdin run",
            with("expected_run_id", json!("run-2")),
            "deferred_send_intent_changed",
        ),
        (
            "stdin conversation",
            with("expected_conversation_id", json!("other")),
            "deferred_send_intent_changed",
        ),
        (
            "stdin text",
            with("text", json!("see [File #1]!")),
            "deferred_send_message_changed",
        ),
        (
            "compaction",
            with("expected_compaction_id", json!("c-1")),
            "deferred_send_kind_refused",
        ),
        (
            "subagent target",
            with("agent_id", json!("agent-1")),
            "invalid_deferred_send_original",
        ),
        (
            "bad base64",
            with(
                "attachments",
                json!([{"name":"a.txt","mime":"text/plain","data_base64":"!!","reference":"[File #1]"}]),
            ),
            "invalid_deferred_send_original",
        ),
        (
            "not json",
            intent_of(&cp, "{"),
            "invalid_deferred_send_original",
        ),
    ];
    for (name, intent, code) in cases {
        assert_eq!(intent.verify().err(), Some(code), "{name}");
    }
    // A duplicate key is refused, as the CLI refuses it.
    let duplicate = base.original.replacen("{", "{\"text\":\"other\",", 1);
    assert_eq!(
        intent_of(&cp, &duplicate).verify().err(),
        Some("invalid_deferred_send_original")
    );
    // Another kind's checkpoint never names a send.
    for kind in [
        checkpoint::Kind::Answer,
        checkpoint::Kind::CompactToSend,
        checkpoint::Kind::ResumeToSend,
    ] {
        let other = reserved("hi", &[], kind);
        let original = stdin(&other, "hi", &[]).to_string();
        assert_eq!(
            intent_of(&other, &original).verify().err(),
            Some("invalid_deferred_send_checkpoint"),
            "{kind:?}"
        );
    }
    // A send that does not fit the control frame stays on the direct path.
    let big = vec![b'x'; 70 << 10];
    let file = ("big.bin", "application/octet-stream", "", big.as_slice());
    let cp = reserved("hi", &[file], checkpoint::Kind::Send);
    let original = stdin(&cp, "hi", &[file]).to_string();
    assert_eq!(
        intent_of(&cp, &original).verify().err(),
        Some("deferred_send_original_too_large")
    );
}

fn receipt(
    verified: &Verified,
    phase: Phase,
    drained: bool,
    code: &str,
    outcome: Option<Outcome>,
) -> Receipt {
    Receipt {
        binding: verified.binding.clone(),
        admission_epoch: "00000000-0000-4000-8000-000000000091".into(),
        revision: 1,
        phase,
        drained,
        code: code.into(),
        outcome,
    }
}

#[test]
fn only_a_drained_submitted_receipt_or_the_no_spawn_boundary_settles() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let plan = &verified.plan;
    let submitted = Some(Outcome {
        request_id: plan.request_id.clone(),
        status: SUBMITTED.into(),
    });
    let recorded = receipt(&verified, Phase::Recorded, true, "", submitted.clone());
    assert!(recorded.valid() && recorded.matches(&intent));
    assert_eq!(recorded.result(plan), Some(Ok(())));
    assert!(!recorded.not_started(plan));
    let failed = receipt(&verified, Phase::Failed, true, NOT_STARTED, None);
    assert!(failed.valid());
    assert_eq!(failed.result(plan), Some(Err(NOT_STARTED.into())));
    assert!(failed.not_started(plan));
    // Unknown, running and undrained never settle.
    for unsettled in [
        receipt(
            &verified,
            Phase::Unknown,
            true,
            "deferred_send_receipt_missing",
            None,
        ),
        receipt(&verified, Phase::Running, false, "", None),
        receipt(&verified, Phase::Queued, false, "", None),
    ] {
        assert!(unsettled.valid(), "{unsettled:?}");
        assert_eq!(unsettled.result(plan), None, "{unsettled:?}");
        assert!(!unsettled.not_started(plan));
    }
    // Invalid shapes: Failed is only the no-spawn code; Recorded needs its
    // drained outcome; nothing else carries one.
    for invalid in [
        receipt(&verified, Phase::Failed, true, "deferred_send_exited", None),
        receipt(&verified, Phase::Recorded, false, "", submitted.clone()),
        receipt(&verified, Phase::Recorded, true, "", None),
        receipt(&verified, Phase::Unknown, true, "", submitted.clone()),
        receipt(&verified, Phase::Running, true, "", None),
    ] {
        assert!(!invalid.valid(), "{invalid:?}");
        assert_eq!(invalid.result(plan), None);
    }
    // In-progress, or another request's outcome, never settles as recorded.
    for outcome in [
        Outcome {
            request_id: plan.request_id.clone(),
            status: "in_progress".into(),
        },
        Outcome {
            request_id: "00000000-0000-4000-8000-000000000001".into(),
            status: SUBMITTED.into(),
        },
    ] {
        let other = receipt(&verified, Phase::Recorded, true, "", Some(outcome));
        assert_eq!(other.result(plan), None);
    }
    // Another plan's receipt is never this one's.
    let mut other_plan = plan.clone();
    other_plan.input_sha256 = "d".repeat(64);
    assert_eq!(recorded.result(&other_plan), None);
    assert!(!failed.not_started(&other_plan));
}

#[test]
fn a_lookup_matches_only_its_own_query() {
    let intent = intent();
    let verified = intent.verify().unwrap();
    let query = Query::for_plan(scope(), &verified.plan).unwrap();
    let found = Lookup {
        abi: ABI.into(),
        request_id: query.request_id.clone(),
        scope: scope(),
        read_only: true,
        found: true,
        receipt: Some(receipt(&verified, Phase::Running, false, "", None)),
    };
    assert!(found.matches(&query));
    let missing = Lookup {
        found: false,
        receipt: None,
        ..found.clone()
    };
    assert!(missing.matches(&query));
    for other in [
        Lookup {
            abi: "x".into(),
            ..found.clone()
        },
        Lookup {
            read_only: false,
            ..found.clone()
        },
        Lookup {
            found: false,
            ..found.clone()
        },
        Lookup {
            found: true,
            receipt: None,
            ..found.clone()
        },
        Lookup {
            request_id: "00000000-0000-4000-8000-000000000001".into(),
            ..found.clone()
        },
        Lookup {
            scope: Scope {
                service_epoch: "00000000-0000-4000-8000-000000000092".into(),
                ..scope()
            },
            ..found.clone()
        },
    ] {
        assert!(!other.matches(&query), "{other:?}");
    }
    let mut other_query = query.clone();
    other_query.plan_sha256 = "e".repeat(64);
    assert!(!found.matches(&other_query));
}

#[test]
fn the_native_request_names_its_reserved_identity() {
    let intent = intent();
    let request = super::super::ui_requests::Request::DeferredSend {
        intent: Box::new(intent.clone()),
    };
    assert_eq!(request.operation(), "deferred_send");
    assert_eq!(request.id(), intent.request_id);
    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["op"], "deferred_send");
    let back: super::super::ui_requests::Request = serde_json::from_value(value).unwrap();
    assert_eq!(back, request);
}
