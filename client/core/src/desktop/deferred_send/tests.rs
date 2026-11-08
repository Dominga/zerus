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
            "deferred_send_message_refused",
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
    // Only a terminal agent's session: OpenCode and DeepSeek route `hgs send`
    // to their own native backends and receipts, so they, and any other
    // agent, stay on the direct path with their own code.
    for session in [
        "dsh/example",
        "dsh/example/main",
        "opencode/example/main",
        "gemini/example",
    ] {
        let other = Intent {
            session: session.into(),
            ..base.clone()
        };
        assert_eq!(other.verify().err(), Some(AGENT_REFUSED), "{session}");
    }
    for session in ["codex/example", "kimi/example/main", SESSION] {
        let terminal = Intent {
            session: session.into(),
            ..base.clone()
        };
        assert!(terminal.verify().is_ok(), "{session}");
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
        input_sha256: plan.input_sha256.clone(),
    });
    let recorded = receipt(&verified, Phase::Recorded, true, "", submitted.clone());
    assert!(recorded.valid() && recorded.matches(&intent));
    assert_eq!(recorded.result(plan), Some(Ok(())));
    assert!(!recorded.not_started(plan));
    let failed = receipt(&verified, Phase::Failed, true, NOT_STARTED, None);
    assert!(failed.valid());
    assert_eq!(failed.result(plan), Some(Err(NOT_STARTED.into())));
    assert!(failed.not_started(plan));
    // An undrained Failed receipt is a valid shape but never settles.
    let undrained = receipt(&verified, Phase::Failed, false, NOT_STARTED, None);
    assert!(undrained.valid());
    assert_eq!(undrained.result(plan), None);
    assert!(!undrained.not_started(plan));
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
    // In-progress, another request's outcome or another stdin's never
    // settles as recorded.
    for outcome in [
        Outcome {
            request_id: plan.request_id.clone(),
            status: "in_progress".into(),
            input_sha256: plan.input_sha256.clone(),
        },
        Outcome {
            request_id: "00000000-0000-4000-8000-000000000001".into(),
            status: SUBMITTED.into(),
            input_sha256: plan.input_sha256.clone(),
        },
        Outcome {
            request_id: plan.request_id.clone(),
            status: SUBMITTED.into(),
            input_sha256: "d".repeat(64),
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
    // The declared plan digest never overrides a conflicting original field
    // the binding repeats, for Recorded and Failed alike.
    type Change = fn(&mut Binding);
    let changes: [(&str, Change); 7] = [
        ("session", |b| b.session = "claude/other-send".into()),
        ("request", |b| {
            b.request_id = "00000000-0000-4000-8000-000000000001".into()
        }),
        ("run", |b| b.run_id = "run-2".into()),
        ("conversation", |b| {
            b.conversation_id = "conversation-2".into()
        }),
        ("input", |b| b.input_sha256 = "d".repeat(64)),
        ("message", |b| b.message_sha256 = "d".repeat(64)),
        ("plan digest", |b| b.plan_sha256 = "e".repeat(64)),
    ];
    for (name, change) in changes {
        let mut changed = recorded.clone();
        change(&mut changed.binding);
        assert!(changed.valid(), "{name}: a valid shape");
        assert_eq!(changed.result(plan), None, "{name}");
        let mut changed = failed.clone();
        change(&mut changed.binding);
        assert!(changed.valid(), "{name}: a valid shape");
        assert_eq!(changed.result(plan), None, "{name}");
        assert!(!changed.not_started(plan), "{name}");
    }
}

#[test]
fn a_lookup_matches_only_its_own_query() {
    let intent = intent();
    let verified = intent.verify().unwrap();
    let query = Query::for_plan(scope(), &verified.plan).unwrap();
    // A non-terminal agent's session is never a deferred send's.
    let mut native = query.clone();
    native.session = "dsh/example".into();
    assert!(!native.valid());
    let mut plan = verified.plan.clone();
    plan.session = "opencode/example/main".into();
    assert!(!plan.valid() && Query::for_plan(scope(), &plan).is_none());
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
    // Another configuration's receipt, even in this scope's reply, is never
    // this query's.
    let mut foreign = found.clone();
    if let Some(receipt) = foreign.receipt.as_mut() {
        receipt.binding.configuration = "b".repeat(64);
        assert!(receipt.valid());
    }
    assert!(!foreign.matches(&query));
    // Another session's receipt, or an invalid one, is never this query's.
    let mut other_session = found.clone();
    if let Some(receipt) = other_session.receipt.as_mut() {
        receipt.binding.session = "claude/other-send".into();
        assert!(receipt.valid());
    }
    assert!(!other_session.matches(&query));
    let mut invalid = found.clone();
    if let Some(receipt) = invalid.receipt.as_mut() {
        receipt.phase = Phase::Recorded;
        receipt.drained = false;
        assert!(!receipt.valid());
    }
    assert!(!invalid.matches(&query));
    // The historical admission epoch stays separate from the current scope.
    let mut admitted_earlier = found.clone();
    if let Some(receipt) = admitted_earlier.receipt.as_mut() {
        receipt.admission_epoch = "00000000-0000-4000-8000-000000000093".into();
    }
    assert!(admitted_earlier.matches(&query));
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

/// The CLI's `input_receipts` record for `plan`, as `hgs send` writes it.
fn cli_receipt(plan: &Plan, status: &str) -> Value {
    json!({"request_id": plan.request_id, "name": plan.session, "run_id": plan.run_id,
        "conversation_id": plan.conversation_id, "first_message": false,
        "digest": plan.input_sha256, "status": status, "submitted_at": 1.5,
        "submitted_text": "see [File #1]", "text": "see [File #1]", "attachments": [],
        "transport": "terminal"})
}

#[test]
fn only_this_originals_exact_submitted_cli_receipt_is_an_outcome() {
    let verified = intent().verify().unwrap();
    let plan = &verified.plan;
    let bytes = |value: &Value| serde_json::to_vec(value).unwrap();
    let outcome = Outcome::from_receipt(plan, &bytes(&cli_receipt(plan, SUBMITTED))).unwrap();
    assert!(outcome.matches(plan));
    assert_eq!(outcome.input_sha256, plan.input_sha256);
    // Each field of another record: the CLI keeps an earlier submitted record
    // when a later submit of the same ID with other content is refused.
    for (field, value) in [
        ("request_id", json!("00000000-0000-4000-8000-000000000001")),
        ("name", json!("claude/other-send")),
        ("run_id", json!("run-2")),
        ("conversation_id", json!("conversation-2")),
        ("digest", json!("d".repeat(64))),
        ("transport", json!("native")),
        ("status", json!("failed")),
    ] {
        let mut other = cli_receipt(plan, SUBMITTED);
        other[field] = value;
        assert_eq!(
            Outcome::from_receipt(plan, &bytes(&other)),
            Err(RECEIPT_MISMATCH),
            "{field}"
        );
    }
    assert_eq!(
        Outcome::from_receipt(plan, &bytes(&cli_receipt(plan, "in_progress"))),
        Err(RECEIPT_IN_PROGRESS)
    );
    // Not a readable receipt: missing or duplicate fields, not JSON, or over
    // the bound.
    let mut missing = cli_receipt(plan, SUBMITTED);
    missing.as_object_mut().unwrap().remove("digest");
    let duplicate = String::from_utf8(bytes(&cli_receipt(plan, SUBMITTED)))
        .unwrap()
        .replacen('{', "{\"digest\":\"x\",", 1);
    let mut padded = cli_receipt(plan, SUBMITTED);
    padded["submitted_text"] = json!("x".repeat(RECEIPT_BYTES));
    for unreadable in [
        bytes(&missing),
        duplicate.into_bytes(),
        b"{".to_vec(),
        bytes(&padded),
    ] {
        assert_eq!(
            Outcome::from_receipt(plan, &unreadable),
            Err(RECEIPT_INVALID)
        );
    }
}

#[test]
fn verify_refuses_what_the_cli_itself_would_refuse_before_any_spawn() {
    let base = intent();
    let cp: Checkpoint = serde_json::from_str(&base.checkpoint).unwrap();
    let original: Value = serde_json::from_str(&base.original).unwrap();
    let with = |key: &str, value: Value| {
        let mut changed = original.clone();
        changed[key] = value;
        intent_of(&cp, &changed.to_string())
    };
    let file = |name: &str, mime: &str, data: &str, reference: &str| json!({"name": name, "mime": mime, "data_base64": data, "reference": reference});
    let hello = STANDARD.encode(b"hello");
    let cases: Vec<(&str, Intent)> = vec![
        ("empty", {
            let mut empty = original.clone();
            empty["text"] = json!("   ");
            empty["attachments"] = json!([]);
            intent_of(&cp, &empty.to_string())
        }),
        ("control", with("text", json!("see \u{1b}[31m"))),
        ("slash command", with("text", json!("  /compact now"))),
        ("shell", with("text", json!("!ls"))),
        (
            "nine files",
            with(
                "attachments",
                json!(vec![file("a.txt", "text/plain", &hello, ""); 9]),
            ),
        ),
        (
            "empty file",
            with(
                "attachments",
                json!([file("a.txt", "text/plain", "", "[File #1]")]),
            ),
        ),
        (
            "unnamed",
            with(
                "attachments",
                json!([file("", "text/plain", &hello, "[File #1]")]),
            ),
        ),
        (
            "long name",
            with(
                "attachments",
                json!([file(&"n".repeat(256), "text/plain", &hello, "[File #1]")]),
            ),
        ),
        (
            "control mime",
            with(
                "attachments",
                json!([file("a.txt", "text/\u{7}", &hello, "[File #1]")]),
            ),
        ),
        (
            "long mime",
            with(
                "attachments",
                json!([file(
                    "a.txt",
                    &format!("text/{}", "x".repeat(96)),
                    &hello,
                    "[File #1]"
                )]),
            ),
        ),
        (
            "control name",
            with(
                "attachments",
                json!([file("a\u{1}.txt", "text/plain", &hello, "[File #1]")]),
            ),
        ),
        (
            "ten-digit label",
            with(
                "attachments",
                json!([file("a.txt", "text/plain", &hello, "[File #1000000000]")]),
            ),
        ),
        (
            "bad label",
            with(
                "attachments",
                json!([file("a.txt", "text/plain", &hello, "[Img #1]")]),
            ),
        ),
        (
            "zero label",
            with(
                "attachments",
                json!([file("a.txt", "text/plain", &hello, "[File #0]")]),
            ),
        ),
        (
            "duplicate label",
            with(
                "attachments",
                json!([
                    file("a.txt", "text/plain", &hello, "[File #1]"),
                    file("b.txt", "text/plain", &hello, "[File #1]")
                ]),
            ),
        ),
        ("empty run", {
            // A checkpoint with no run, whose stdin repeats it.
            let mut runless = cp.clone();
            runless.run_id.clear();
            let mut empty = original.clone();
            empty["expected_run_id"] = json!("");
            intent_of(&runless, &empty.to_string())
        }),
    ];
    for (name, intent) in cases {
        assert_eq!(
            intent.verify().err(),
            Some("deferred_send_message_refused"),
            "{name}"
        );
    }
    // An absolute path in the first token is ordinary text, as for the CLI.
    let path = "/srv/example/file.png see [File #1]";
    let cp = reserved(path, &[FILE], checkpoint::Kind::Send);
    let original = stdin(&cp, path, &[FILE]).to_string();
    assert!(intent_of(&cp, &original).verify().is_ok());
}

#[test]
fn a_checkpoint_that_decodes_but_is_not_a_valid_send_checkpoint_is_refused() {
    let base = intent();
    let checkpoint: Value = serde_json::from_str(&base.checkpoint).unwrap();
    for (field, value) in [
        ("receipts", json!("question_receipts")),
        ("version", json!(2)),
        ("start", json!("resume")),
        (
            "question",
            json!({"id": "q", "identity_sha256": "c".repeat(64)}),
        ),
        ("message_sha256", json!("not-a-digest")),
    ] {
        let mut changed = checkpoint.clone();
        changed[field] = value;
        let intent = Intent {
            checkpoint: changed.to_string(),
            ..base.clone()
        };
        assert_eq!(
            intent.verify().err(),
            Some("invalid_deferred_send_checkpoint"),
            "{field}"
        );
    }
}

#[test]
fn a_plan_needs_a_run() {
    let mut plan = intent().verify().unwrap().plan;
    assert!(plan.valid());
    plan.run_id.clear();
    assert!(!plan.valid());
    assert_eq!(plan.fingerprint(), None);
}

/// zerus-tlb9q: only the exact closed refusal of this intent is not
/// admitted; another request, scope, ABI, outcome, code or any receipt is
/// not, and the client settles nothing from it.
#[test]
fn only_the_exact_refusal_of_this_intent_is_not_admitted() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let exact = || Admission {
        abi: ABI.into(),
        request_id: intent.request_id.clone(),
        scope: intent.scope.clone(),
        accepted: false,
        durable: false,
        code: NOT_ADMITTED.into(),
        receipt: None,
    };
    assert!(exact().not_admitted(&intent));
    let decoded: Admission =
        serde_json::from_value(serde_json::to_value(exact()).unwrap()).unwrap();
    assert!(decoded.not_admitted(&intent), "the exact wire form");
    type Change<'a> = Box<dyn Fn(&mut Admission) + 'a>;
    let cases: Vec<(&str, Change<'_>)> = vec![
        ("abi", Box::new(|a| a.abi = "other".into())),
        (
            "request",
            Box::new(|a| a.request_id = "00000000-0000-4000-8000-0000000000ff".into()),
        ),
        (
            "epoch",
            Box::new(|a| a.scope.service_epoch = "00000000-0000-4000-8000-0000000000fe".into()),
        ),
        (
            "configuration",
            Box::new(|a| a.scope.configuration = "b".repeat(64)),
        ),
        ("accepted", Box::new(|a| a.accepted = true)),
        ("durable", Box::new(|a| a.durable = true)),
        (
            "unavailable",
            Box::new(|a| a.code = "deferred_send_unavailable".into()),
        ),
        ("not started", Box::new(|a| a.code = NOT_STARTED.into())),
        (
            "another kind",
            Box::new(|a| a.code = "session_resume_not_admitted".into()),
        ),
        ("no code", Box::new(|a| a.code.clear())),
        (
            "receipt",
            Box::new(|a| a.receipt = Some(receipt(&verified, Phase::Unknown, false, "", None))),
        ),
    ];
    for (name, change) in cases {
        let mut admission = exact();
        change(&mut admission);
        assert!(!admission.not_admitted(&intent), "{name}");
    }
}
