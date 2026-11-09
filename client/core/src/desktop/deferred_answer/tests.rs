use super::*;
use crate::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};
use serde_json::json;

const SESSION: &str = "claude/private-answer";
const RUN: &str = "run-1";
const CONVERSATION: &str = "conversation-1";
const HASH: &str = "abababababababababababababababababababababababababababababababab";

fn scope() -> Scope {
    Scope {
        configuration: "a".repeat(64),
        service_epoch: "00000000-0000-4000-8000-000000000090".into(),
    }
}
/// A native question of `source`, as the session's inspection carries it.
fn question(source: &str) -> Value {
    json!({"question_id": "q-1", "question_hash": HASH, "run_id": RUN,
        "conversation_id": CONVERSATION, "source": source, "answer_transport": "claude_tui",
        "questions": [{"id": "q-1", "question": "Allow this folder?",
            "options": [{"id": "yes", "label": "Yes"}, {"id": "no", "label": "No"}]}],
        "can_answer": true, "status": "pending"})
}
/// A native question of `source` with the ID its source gives it: Codex's
/// update question, and only it, is `codex-update:<hash>`.
fn sourced(source: &str) -> Value {
    let mut asked = question(source);
    if source == "codex_update" {
        asked["question_id"] = json!(format!("codex-update:{HASH}"));
    }
    asked
}
/// A choice of `option` for `asked`'s own question.
fn choice_of(asked: &Value, option: &str) -> Vec<AnswerItem> {
    vec![AnswerItem::Choice {
        question_id: asked["question_id"].as_str().unwrap().into(),
        selected_option_ids: vec![option.into()],
        text: String::new(),
    }]
}
fn choice(option: &str) -> Vec<AnswerItem> {
    vec![AnswerItem::Choice {
        question_id: "q-1".into(),
        selected_option_ids: vec![option.into()],
        text: String::new(),
    }]
}
/// The stdin the client builds (`actions::answer_input`).
fn stdin(request: &str, question: &Value, answers: &[AnswerItem]) -> String {
    let text = |key: &str| question[key].as_str().unwrap_or("").to_owned();
    String::from_utf8(
        serde_json::to_vec(
            &json!({"request_id": request, "question_id": text("question_id"),
            "expected_question_hash": text("question_hash"), "expected_run_id": text("run_id"),
            "expected_conversation_id": text("conversation_id"), "answers": answers}),
        )
        .unwrap(),
    )
    .unwrap()
}
/// The checkpoint the client reserves at the click
/// (`composer/questions.rs`).
fn reserved(question: &Value, answers: &[AnswerItem]) -> Checkpoint {
    let identity = crate::questions::consent_identity(question);
    Checkpoint::reserve(checkpoint::Intent {
        kind: checkpoint::Kind::Answer,
        draft_revision: 2,
        message_sha256: checkpoint::message_sha256(
            &serde_json::to_string(answers).unwrap(),
            Vec::<(&str, &str, &str, &[u8])>::new(),
        ),
        run_id: question["run_id"].as_str().unwrap().into(),
        conversation_id: question["conversation_id"].as_str().unwrap().into(),
        question: Some(checkpoint::Question {
            id: question["question_id"].as_str().unwrap().into(),
            identity_sha256: checkpoint::identity_sha256(&identity),
        }),
        start: None,
        retry_of: None,
        consent: false,
    })
}
fn intent_of(question: &Value, answers: &[AnswerItem]) -> Intent {
    let checkpoint = reserved(question, answers);
    Intent {
        scope: scope(),
        session: SESSION.into(),
        request_id: checkpoint.request_id.clone(),
        checkpoint: serde_json::to_string(&checkpoint).unwrap(),
        pinned: crate::questions::consent_identity(question).to_string(),
        original: stdin(&checkpoint.request_id, question, answers),
    }
}
/// A valid deferred answer to a Claude folder-trust question.
pub(crate) fn intent() -> Intent {
    intent_of(&question("claude_folder_trust"), &choice("yes"))
}

#[test]
fn a_saved_answer_checkpoint_its_consent_and_exact_stdin_verify_into_one_plan() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let plan = &verified.plan;
    assert_eq!(
        (
            plan.session.as_str(),
            plan.run_id.as_str(),
            plan.conversation_id.as_str()
        ),
        (SESSION, RUN, CONVERSATION)
    );
    assert_eq!(
        (plan.question_id.as_str(), plan.question_hash.as_str()),
        ("q-1", HASH)
    );
    // The CLI's receipt digest is the SHA-256 of the raw stdin.
    assert_eq!(plan.input_sha256, hash(intent.original.as_bytes()));
    assert_eq!(plan.args(), ["answer", SESSION, "--json"]);
    assert!(verified.binding.valid() && verified.binding.agrees(plan));
    assert_eq!(
        verified.pinned,
        *crate::questions::consent_identity(&question("claude_folder_trust"))
            .as_object()
            .unwrap()
    );
    // Any other byte of the stdin is another binding of the same key.
    let mut spaced = intent.clone();
    spaced.original = format!(" {}", intent.original);
    let other = spaced.verify().unwrap();
    assert_ne!(other.binding, verified.binding);
    assert_ne!(other.plan.input_sha256, plan.input_sha256);
    assert_eq!(
        other.binding.operation_id(),
        verified.binding.operation_id()
    );
}

#[test]
fn only_an_answer_matching_its_checkpoint_and_pinned_consent_verifies() {
    let base = intent();
    let asked = question("claude_folder_trust");
    let cp: Checkpoint = serde_json::from_str(&base.checkpoint).unwrap();
    let raw: Value = serde_json::from_str(&base.original).unwrap();
    let with = |key: &str, value: Value| {
        let mut changed = raw.clone();
        changed[key] = value;
        Intent {
            original: changed.to_string(),
            ..base.clone()
        }
    };
    let pinned_with = |key: &str, value: Value| {
        let mut changed = crate::questions::consent_identity(&asked);
        changed[key] = value;
        Intent {
            pinned: changed.to_string(),
            ..base.clone()
        }
    };
    let mut send_kind = cp.clone();
    send_kind.kind = checkpoint::Kind::Send;
    send_kind.receipts = "input_receipts".into();
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
            "invalid_deferred_answer_intent",
        ),
        (
            "remote alias",
            Intent {
                session: "@peer/claude".into(),
                ..base.clone()
            },
            "invalid_deferred_answer_intent",
        ),
        (
            "DeepSeek session",
            Intent {
                session: "dsh/example".into(),
                ..base.clone()
            },
            AGENT_REFUSED,
        ),
        (
            "OpenCode session",
            Intent {
                session: "opencode/example/main".into(),
                ..base.clone()
            },
            AGENT_REFUSED,
        ),
        (
            "send checkpoint",
            Intent {
                checkpoint: serde_json::to_string(&send_kind).unwrap(),
                ..base.clone()
            },
            "invalid_deferred_answer_checkpoint",
        ),
        (
            "checkpoint json",
            Intent {
                checkpoint: "{}".into(),
                ..base.clone()
            },
            "invalid_deferred_answer_checkpoint",
        ),
        (
            "request id",
            Intent {
                request_id: "00000000-0000-4000-8000-000000000001".into(),
                ..base.clone()
            },
            "invalid_deferred_answer_checkpoint",
        ),
        // The consent identity is exactly the one the checkpoint hashed.
        (
            "a consent field changed",
            pinned_with("approval", json!("allow_always")),
            "deferred_answer_consent_changed",
        ),
        (
            "a field outside the projection",
            pinned_with("status", json!("pending")),
            "deferred_answer_consent_changed",
        ),
        (
            "not an object",
            Intent {
                pinned: "[]".into(),
                ..base.clone()
            },
            "invalid_deferred_answer_consent",
        ),
        (
            "stdin request id",
            with("request_id", json!("00000000-0000-4000-8000-000000000001")),
            "deferred_answer_intent_changed",
        ),
        (
            "stdin run",
            with("expected_run_id", json!("run-2")),
            "deferred_answer_intent_changed",
        ),
        (
            "stdin conversation",
            with("expected_conversation_id", json!("other")),
            "deferred_answer_intent_changed",
        ),
        (
            "stdin question",
            with("question_id", json!("q-2")),
            "deferred_answer_intent_changed",
        ),
        (
            "stdin question hash",
            with("expected_question_hash", json!("c".repeat(64))),
            "deferred_answer_intent_changed",
        ),
        (
            "another answer",
            with(
                "answers",
                json!([{"question_id": "q-1", "selected_option_ids": ["no"], "text": ""}]),
            ),
            "deferred_answer_message_changed",
        ),
        (
            "a field the client never sends",
            with(
                "answers",
                json!([{"question_id": "q-1", "selected_option_ids": ["yes"], "text": "", "skip": false}]),
            ),
            "deferred_answer_message_changed",
        ),
        (
            "unknown stdin field",
            with("agent_id", json!("agent-1")),
            "invalid_deferred_answer_original",
        ),
        (
            "not json",
            Intent {
                original: "{".into(),
                ..base.clone()
            },
            "invalid_deferred_answer_original",
        ),
    ];
    for (name, intent, code) in cases {
        assert_eq!(intent.verify().err(), Some(code), "{name}");
    }
    // A duplicate key is refused, as the CLI refuses it.
    let duplicate = Intent {
        original: base.original.replacen("{", "{\"question_id\":\"q-2\",", 1),
        ..base.clone()
    };
    assert_eq!(
        duplicate.verify().err(),
        Some("invalid_deferred_answer_original")
    );
    // A field outside the closed projection, even when the checkpoint
    // hashed it too: only the projection the client pins is consent.
    let mut extended = crate::questions::consent_identity(&asked);
    extended["status"] = json!("pending");
    let mut hashed: Checkpoint = serde_json::from_str(&base.checkpoint).unwrap();
    hashed.question.as_mut().unwrap().identity_sha256 = checkpoint::identity_sha256(&extended);
    let outside = Intent {
        checkpoint: serde_json::to_string(&hashed).unwrap(),
        pinned: extended.to_string(),
        ..base.clone()
    };
    assert_eq!(
        outside.verify().err(),
        Some("deferred_answer_consent_changed")
    );
    // Each binding field on its own: `pinned` names another question, run
    // or conversation than the checkpoint and the stdin, although the
    // checkpoint hashed exactly that identity.
    for (key, value) in [
        ("question_id", json!("q-2")),
        ("run_id", json!("run-2")),
        ("conversation_id", json!("conversation-2")),
    ] {
        let mut changed = crate::questions::consent_identity(&asked);
        changed[key] = value;
        let mut rehashed = cp.clone();
        rehashed.question.as_mut().unwrap().identity_sha256 = checkpoint::identity_sha256(&changed);
        let drifted = Intent {
            checkpoint: serde_json::to_string(&rehashed).unwrap(),
            pinned: changed.to_string(),
            ..base.clone()
        };
        assert_eq!(
            drifted.verify().err(),
            Some("deferred_answer_consent_changed"),
            "{key}"
        );
    }
    // A checkpoint the client never saves, of the right kind and family
    // and naming this request.
    let mut versioned = cp.clone();
    versioned.version += 1;
    let versioned = Intent {
        checkpoint: serde_json::to_string(&versioned).unwrap(),
        ..base.clone()
    };
    assert_eq!(
        versioned.verify().err(),
        Some("invalid_deferred_answer_checkpoint")
    );
    // No answers: the CLI's own refusal, never offered.
    let empty = intent_of(&asked, &[]);
    assert_eq!(empty.verify().err(), Some("deferred_answer_refused"));
    // A Skip, alone or beside a choice: the CLI skips only an optional Codex
    // asynchronous question, which v1 never takes, and refuses any other
    // before its receipt.
    for source in SOURCES {
        let mut optional = sourced(source);
        optional["optional"] = json!(true);
        let skip = AnswerItem::Skip {
            question_id: optional["question_id"].as_str().unwrap().into(),
            skip: true,
        };
        assert_eq!(
            intent_of(&optional, std::slice::from_ref(&skip))
                .verify()
                .err(),
            Some("deferred_answer_refused"),
            "{source}"
        );
    }
    let mut two = asked.clone();
    two["questions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "q-2", "question": "And this one?",
            "options": [{"id": "yes", "label": "Yes"}]}));
    let mut mixed = choice("yes");
    mixed.push(AnswerItem::Skip {
        question_id: "q-2".into(),
        skip: true,
    });
    assert_eq!(
        intent_of(&two, &mixed).verify().err(),
        Some("deferred_answer_refused")
    );
    let mut both = choice("yes");
    both.push(AnswerItem::Choice {
        question_id: "q-2".into(),
        selected_option_ids: vec!["yes".into()],
        text: String::new(),
    });
    assert!(intent_of(&two, &both).verify().is_ok());
}

#[test]
fn an_intent_beyond_the_control_line_stays_on_the_direct_path() {
    // A Kimi plan review's consent identity carries the whole plan, so its
    // pinned identity alone can exceed the control line.
    let mut review = question("kimi_plan_review");
    review["questions"][0]["body"] = json!("x".repeat(zerus_watch_protocol::CONTROL_LINE_BYTES));
    let large = intent_of(&review, &choice("yes"));
    assert!(large.original.len() <= MAX_INPUT && large.checkpoint.len() < 1024);
    assert_eq!(
        large.clone().verify().err(),
        Some("deferred_answer_original_too_large")
    );
    // Decided before any decoding.
    let undecodable = Intent {
        checkpoint: "{".into(),
        original: "{".into(),
        ..large
    };
    assert_eq!(
        undecodable.verify().err(),
        Some("deferred_answer_original_too_large")
    );
    // The same plan in a line that fits verifies.
    review["questions"][0]["body"] = json!("x".repeat(1 << 10));
    assert!(intent_of(&review, &choice("yes")).verify().is_ok());
}

#[test]
fn only_the_v1_allowlisted_sources_are_offered() {
    // Allowed: every source whose `rejected` receipt proves no delivery.
    for source in SOURCES {
        assert!(
            intent_of(&sourced(source), &choice_of(&sourced(source), "yes"))
                .verify()
                .is_ok(),
            "{source}"
        );
    }
    // Direct: sources whose `rejected` may follow a delivery, and any
    // source the CLI does not take natively or that is new.
    for source in [
        "codex_async",
        "hook",
        "claude_tool_approval",
        "codex_goal_resume",
        "claude_new_kind",
        "",
    ] {
        assert_eq!(
            intent_of(&question(source), &choice("yes")).verify().err(),
            Some(SOURCE_REFUSED),
            "{source}"
        );
    }
}

#[test]
fn an_empty_conversation_verifies_only_for_the_clis_startup_questions() {
    for (id, allowed) in [
        (format!("claude-trust:{HASH}"), true),
        (format!("claude-permissions:{HASH}"), true),
        (format!("claude-resume:{HASH}"), true),
        (format!("claude-usage-limit:{HASH}"), true),
        (format!("codex-hooks-trust:{HASH}"), true),
        (format!("codex-update:{HASH}"), true),
        (format!("codex-goal-resume:{HASH}"), true),
        (format!("codex-trust:{HASH}"), true),
        (format!("kimi-trust:{HASH}"), true),
        ("q-1".to_owned(), false),
        (format!("claude-tool:{HASH}"), false),
        (format!("x-claude-trust:{HASH}"), false),
    ] {
        // Codex's update question is the codex_update source's own.
        let mut asked = question(if id.starts_with("codex-update:") {
            "codex_update"
        } else {
            "claude_folder_trust"
        });
        asked["question_id"] = json!(id);
        asked["conversation_id"] = json!("");
        let answers = vec![AnswerItem::Choice {
            question_id: id.clone(),
            selected_option_ids: vec!["yes".into()],
            text: String::new(),
        }];
        let result = intent_of(&asked, &answers).verify();
        assert_eq!(result.is_ok(), allowed, "{id}");
        if !allowed {
            assert_eq!(result.err(), Some("deferred_answer_refused"), "{id}");
        }
    }
}

#[test]
fn the_fresh_consent_comparison_catches_every_drift_and_a_spoofed_source() {
    let asked = question("claude_folder_trust");
    let verified = intent().verify().unwrap();
    assert!(consent_current(&verified.pinned, &asked));
    // Fields outside the projection never matter.
    let mut status = asked.clone();
    status["status"] = json!("answering");
    assert!(consent_current(&verified.pinned, &status));
    // Any consent field drift, with the id and hash unchanged.
    for (key, value) in [
        ("source", json!("claude_tool_approval")),
        ("answer_transport", json!("claude_hook")),
        ("approval", json!("allow_always")),
        ("trust_request", json!({"folder": "/other"})),
        ("tool", json!("Bash")),
        ("questions", json!([])),
    ] {
        let mut changed = asked.clone();
        changed[key] = value;
        assert!(!consent_current(&verified.pinned, &changed), "{key}");
    }
    // A field the native question gained or lost.
    let mut gained = asked.clone();
    gained["optional"] = json!(true);
    assert!(!consent_current(&verified.pinned, &gained));
    let mut lost = asked.clone();
    lost.as_object_mut().unwrap().remove("answer_transport");
    assert!(!consent_current(&verified.pinned, &lost));
    // A pinned source the native question does not have.
    let spoof = question("codex_async");
    let mut pinned = verified.pinned.clone();
    pinned.insert("source".into(), json!("claude_folder_trust"));
    assert!(!consent_current(&pinned, &spoof));
}

fn cli_receipt(plan: &Plan, status: &str) -> Value {
    json!({"request_id": plan.request_id, "name": plan.session, "run_id": plan.run_id,
        "conversation_id": plan.conversation_id, "question_id": plan.question_id,
        "question_hash": plan.question_hash, "digest": plan.input_sha256, "status": status,
        "submitted_at": 7.5, "answers": []})
}

#[test]
fn only_this_originals_exact_receipt_records_and_an_exact_rejected_proves_no_delivery() {
    let verified = intent().verify().unwrap();
    let plan = &verified.plan;
    let read = |value: &Value| Outcome::read(plan, value.to_string().as_bytes());
    for status in [ANSWERED, SUBMITTED] {
        match read(&cli_receipt(plan, status)) {
            Read::Recorded(outcome) => {
                assert!(outcome.matches(plan));
                assert_eq!(outcome.status, status);
                assert!(!outcome.open_terminal);
            }
            other => panic!("{status}: {other:?}"),
        }
    }
    // The Terminal hand-off travels as data only.
    let mut terminal = cli_receipt(plan, ANSWERED);
    terminal["open_terminal"] = json!(true);
    assert!(matches!(read(&terminal), Read::Recorded(o) if o.open_terminal));
    assert_eq!(read(&cli_receipt(plan, "rejected")), Read::Rejected);
    assert_eq!(
        read(&cli_receipt(plan, "in_progress")),
        Read::Unknown(RECEIPT_IN_PROGRESS)
    );
    assert_eq!(
        read(&cli_receipt(plan, "failed")),
        Read::Unknown(RECEIPT_MISMATCH)
    );
    for (key, value) in [
        ("request_id", json!("00000000-0000-4000-8000-000000000001")),
        ("name", json!("claude/other")),
        ("run_id", json!("run-2")),
        ("conversation_id", json!("other")),
        ("question_id", json!("q-2")),
        ("question_hash", json!("c".repeat(64))),
        ("digest", json!("d".repeat(64))),
    ] {
        for status in [ANSWERED, "rejected"] {
            let mut other = cli_receipt(plan, status);
            other[key] = value.clone();
            assert_eq!(
                read(&other),
                Read::Unknown(RECEIPT_MISMATCH),
                "{key} {status}"
            );
        }
    }
    // Missing, duplicate and oversized are never an outcome.
    let mut missing = cli_receipt(plan, ANSWERED);
    missing.as_object_mut().unwrap().remove("digest");
    assert_eq!(read(&missing), Read::Unknown(RECEIPT_INVALID));
    let duplicate =
        cli_receipt(plan, "rejected")
            .to_string()
            .replacen("{", "{\"status\":\"answered\",", 1);
    assert_eq!(
        Outcome::read(plan, duplicate.as_bytes()),
        Read::Unknown(RECEIPT_INVALID)
    );
    let mut large = cli_receipt(plan, ANSWERED);
    large["answers"] = json!("x".repeat(RECEIPT_BYTES));
    assert_eq!(read(&large), Read::Unknown(RECEIPT_INVALID));
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
fn only_a_drained_recorded_receipt_or_a_proven_non_delivery_settles() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let plan = &verified.plan;
    let answered = Outcome {
        request_id: plan.request_id.clone(),
        status: ANSWERED.into(),
        input_sha256: plan.input_sha256.clone(),
        open_terminal: false,
        codex_update: None,
    };
    let recorded = receipt(&verified, Phase::Recorded, true, "", Some(answered.clone()));
    assert!(recorded.valid() && recorded.matches(&intent));
    assert_eq!(recorded.result(plan), Some(Ok(answered.clone())));
    assert!(!recorded.not_delivered(plan));
    for code in [NOT_STARTED, REJECTED] {
        let failed = receipt(&verified, Phase::Failed, true, code, None);
        assert!(failed.valid());
        assert_eq!(failed.result(plan), Some(Err(code.into())));
        assert!(failed.not_delivered(plan));
    }
    // Unknown, running, queued and undrained never settle.
    for unsettled in [
        receipt(&verified, Phase::Unknown, true, RECEIPT_IN_PROGRESS, None),
        receipt(&verified, Phase::Running, false, "", None),
        receipt(&verified, Phase::Queued, false, "", None),
        receipt(&verified, Phase::Failed, false, NOT_STARTED, None),
    ] {
        assert!(unsettled.valid(), "{unsettled:?}");
        assert_eq!(unsettled.result(plan), None, "{unsettled:?}");
        assert!(!unsettled.not_delivered(plan));
    }
    // Invalid shapes: Failed is only the two closed codes; Recorded needs
    // its drained outcome; nothing else carries one.
    for invalid in [
        receipt(
            &verified,
            Phase::Failed,
            true,
            "deferred_answer_exited",
            None,
        ),
        receipt(
            &verified,
            Phase::Failed,
            true,
            NOT_STARTED,
            Some(answered.clone()),
        ),
        receipt(
            &verified,
            Phase::Recorded,
            false,
            "",
            Some(answered.clone()),
        ),
        receipt(&verified, Phase::Recorded, true, "", None),
        receipt(&verified, Phase::Unknown, true, "", Some(answered.clone())),
    ] {
        assert!(!invalid.valid(), "{invalid:?}");
        assert_eq!(invalid.result(plan), None);
    }
    // A Recorded receipt settles only with this plan's own outcome: another
    // status, request or stdin digest never does.
    for outcome in [
        Outcome {
            status: "rejected".into(),
            ..answered.clone()
        },
        Outcome {
            status: "in_progress".into(),
            ..answered.clone()
        },
        Outcome {
            request_id: "00000000-0000-4000-8000-000000000001".into(),
            ..answered.clone()
        },
        Outcome {
            input_sha256: "d".repeat(64),
            ..answered.clone()
        },
    ] {
        assert!(!outcome.matches(plan), "{outcome:?}");
        let mismatched = receipt(&verified, Phase::Recorded, true, "", Some(outcome));
        assert!(mismatched.valid() && mismatched.binding.agrees(plan));
        assert_eq!(mismatched.result(plan), None, "{mismatched:?}");
    }
    // Another plan's receipt is never this one's, through fields its
    // outcome does not carry too: it neither settles nor proves that
    // nothing was delivered.
    let others: Vec<(&str, Plan)> = vec![
        (
            "session",
            Plan {
                session: "claude/other-answer".into(),
                ..plan.clone()
            },
        ),
        (
            "run",
            Plan {
                run_id: "run-2".into(),
                ..plan.clone()
            },
        ),
        (
            "conversation",
            Plan {
                conversation_id: "conversation-2".into(),
                ..plan.clone()
            },
        ),
        (
            "question",
            Plan {
                question_id: "q-2".into(),
                ..plan.clone()
            },
        ),
        (
            "question hash",
            Plan {
                question_hash: "c".repeat(64),
                ..plan.clone()
            },
        ),
        (
            "identity",
            Plan {
                identity_sha256: "e".repeat(64),
                ..plan.clone()
            },
        ),
        (
            "message",
            Plan {
                message_sha256: "f".repeat(64),
                ..plan.clone()
            },
        ),
        (
            "stdin",
            Plan {
                input_sha256: "d".repeat(64),
                ..plan.clone()
            },
        ),
    ];
    for (name, other) in &others {
        assert!(other.valid(), "{name}");
        assert_eq!(recorded.result(other), None, "{name}");
        assert!(!recorded.not_delivered(other), "{name}");
        for code in [NOT_STARTED, REJECTED] {
            let failed = receipt(&verified, Phase::Failed, true, code, None);
            assert_eq!(failed.result(other), None, "{name} {code}");
            assert!(!failed.not_delivered(other), "{name} {code}");
        }
    }
    // Another intent's receipt: one stdin byte more, or an intent that
    // does not verify.
    let spaced = Intent {
        original: format!(" {}", intent.original),
        ..intent.clone()
    };
    assert!(spaced.clone().verify().is_ok());
    assert!(!recorded.matches(&spaced));
    let unverified = Intent {
        checkpoint: "{}".into(),
        ..intent.clone()
    };
    assert!(!recorded.matches(&unverified));
}

#[test]
fn an_admission_matches_only_its_own_durable_acceptance() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let admitted = Admission {
        abi: ABI.into(),
        request_id: intent.request_id.clone(),
        scope: scope(),
        accepted: true,
        durable: true,
        code: String::new(),
        receipt: Some(receipt(&verified, Phase::Queued, false, "", None)),
    };
    assert!(admitted.matches(&intent));
    let spaced = Intent {
        original: format!(" {}", intent.original),
        ..intent.clone()
    };
    let another = spaced.verify().unwrap();
    let cases: Vec<(&str, Admission)> = vec![
        (
            "abi",
            Admission {
                abi: "x".into(),
                ..admitted.clone()
            },
        ),
        (
            "request",
            Admission {
                request_id: "00000000-0000-4000-8000-000000000001".into(),
                ..admitted.clone()
            },
        ),
        (
            "scope",
            Admission {
                scope: Scope {
                    service_epoch: "00000000-0000-4000-8000-000000000092".into(),
                    ..scope()
                },
                ..admitted.clone()
            },
        ),
        (
            "refused",
            Admission {
                accepted: false,
                ..admitted.clone()
            },
        ),
        (
            "not durable",
            Admission {
                durable: false,
                ..admitted.clone()
            },
        ),
        (
            "code",
            Admission {
                code: "deferred_answer_busy".into(),
                ..admitted.clone()
            },
        ),
        (
            "no receipt",
            Admission {
                receipt: None,
                ..admitted.clone()
            },
        ),
        (
            "invalid receipt",
            Admission {
                receipt: Some(receipt(&verified, Phase::Recorded, false, "", None)),
                ..admitted.clone()
            },
        ),
        (
            "another original's receipt",
            Admission {
                receipt: Some(receipt(&another, Phase::Queued, false, "", None)),
                ..admitted.clone()
            },
        ),
    ];
    for (name, other) in cases {
        assert!(!other.matches(&intent), "{name}");
    }
}

#[test]
fn a_lookup_matches_only_its_own_query() {
    let verified = intent().verify().unwrap();
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
    ] {
        assert!(!other.matches(&query));
    }
    let mut another = query.clone();
    another.plan_sha256 = "f".repeat(64);
    assert!(!found.matches(&another));
    // An absent receipt matches only as not found.
    let missing = Lookup {
        found: false,
        receipt: None,
        ..found.clone()
    };
    assert!(missing.matches(&query));
    for other in [
        Lookup {
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
    // A valid receipt of another configuration, session or request, even in
    // this scope's reply, is never this query's; nor is an invalid one.
    let bound = |change: &dyn Fn(&mut Receipt)| {
        let mut lookup = found.clone();
        change(lookup.receipt.as_mut().unwrap());
        lookup
    };
    for (name, other) in [
        (
            "configuration",
            bound(&|r: &mut Receipt| r.binding.configuration = "b".repeat(64)),
        ),
        (
            "session",
            bound(&|r: &mut Receipt| r.binding.session = "claude/other-answer".into()),
        ),
        (
            "request",
            bound(&|r: &mut Receipt| {
                r.binding.request_id = "00000000-0000-4000-8000-000000000001".into()
            }),
        ),
    ] {
        assert!(other.receipt.as_ref().unwrap().valid(), "{name}");
        assert!(!other.matches(&query), "{name}");
    }
    let invalid = bound(&|r: &mut Receipt| r.drained = true);
    assert!(!invalid.receipt.as_ref().unwrap().valid());
    assert!(!invalid.matches(&query));
    // The historical admission epoch stays separate from the current scope.
    let earlier =
        bound(&|r: &mut Receipt| r.admission_epoch = "00000000-0000-4000-8000-000000000093".into());
    assert!(earlier.matches(&query));
    // A DeepSeek session is never this kind's.
    let mut native = query.clone();
    native.session = "dsh/example".into();
    assert!(!native.valid());
}

#[test]
fn the_native_request_names_its_reserved_identity() {
    let intent = intent();
    let request = crate::desktop::ui_requests::Request::DeferredAnswer {
        intent: Box::new(intent.clone()),
    };
    assert_eq!(request.operation(), "deferred_answer");
    assert_eq!(request.id(), intent.request_id);
}

/// Codex's update question as `hgs inspect` shows it, answered with `option`.
fn update_intent(option: &str) -> Intent {
    let asked = sourced("codex_update");
    intent_of(&asked, &choice_of(&asked, option))
}

/// zerus-5wmh.66.4.7.2: the source and the question ID agree, so the plan's
/// question ID alone says whether an update phase may follow.
#[test]
fn codex_updates_source_and_question_id_go_together() {
    assert!(update_intent("update_now").verify().is_ok());
    let mut renamed = question("codex_update");
    renamed["question_id"] = json!("q-1");
    assert_eq!(
        intent_of(&renamed, &choice("yes")).verify().err(),
        Some("deferred_answer_consent_changed")
    );
    let mut borrowed = question("claude_folder_trust");
    borrowed["question_id"] = json!(format!("codex-update:{HASH}"));
    assert_eq!(
        intent_of(&borrowed, &choice_of(&borrowed, "yes"))
            .verify()
            .err(),
        Some("deferred_answer_consent_changed")
    );
}

/// The raw CLI receipt: a phase is read only for Update now (answered, in
/// Terminal) of Codex's update question, and only as one of two values.
#[test]
fn only_update_now_records_a_phase_and_only_one_of_two_values() {
    let verified = update_intent("update_now").verify().unwrap();
    let plan = &verified.plan;
    let read = |value: &Value| Outcome::read(plan, value.to_string().as_bytes());
    let update_now = |phase: Value| {
        let mut receipt = cli_receipt(plan, ANSWERED);
        receipt["open_terminal"] = json!(true);
        receipt["codex_update"] = phase;
        receipt
    };
    for (raw, phase) in [
        ("installing", UpdatePhase::Installing),
        ("exited", UpdatePhase::Exited),
    ] {
        match read(&update_now(json!(raw))) {
            Read::Recorded(outcome) => {
                assert_eq!(outcome.codex_update, Some(phase));
                assert!(outcome.open_terminal && outcome.matches(plan));
            }
            other => panic!("{raw}: {other:?}"),
        }
    }
    // Absence is the only "none"; every other value or shape refuses.
    for malformed in [
        Value::Null,
        json!("none"),
        json!("Installing"),
        json!("EXITED"),
        json!(""),
        json!("installed"),
        json!(true),
        json!(1),
        json!(["installing"]),
        json!({"phase": "installing"}),
    ] {
        assert_eq!(
            read(&update_now(malformed.clone())),
            Read::Unknown(RECEIPT_PHASE),
            "{malformed}"
        );
    }
    let mut missing = update_now(Value::Null);
    missing.as_object_mut().unwrap().remove("codex_update");
    assert_eq!(read(&missing), Read::Unknown(RECEIPT_PHASE), "missing");
    let duplicate = update_now(json!("installing")).to_string().replacen(
        '{',
        "{\"codex_update\":\"exited\",",
        1,
    );
    assert_eq!(
        Outcome::read(plan, duplicate.as_bytes()),
        Read::Unknown(RECEIPT_INVALID)
    );
    // Never on another status: a rejected answer with a phase is not the
    // CLI's refusal before delivery.
    for status in [SUBMITTED, "rejected", "in_progress"] {
        let mut other = update_now(json!("installing"));
        other["status"] = json!(status);
        assert_eq!(read(&other), Read::Unknown(RECEIPT_PHASE), "{status}");
    }
    let mut stays = update_now(json!("installing"));
    stays["open_terminal"] = json!(false);
    assert_eq!(read(&stays), Read::Unknown(RECEIPT_PHASE));
    // Both Skip choices: answered, no Terminal, no phase.
    for skip in ["not_now", "skip_version"] {
        let verified = update_intent(skip).verify().unwrap();
        let plan = &verified.plan;
        match Outcome::read(plan, cli_receipt(plan, ANSWERED).to_string().as_bytes()) {
            Read::Recorded(outcome) => {
                assert_eq!(outcome.codex_update, None);
                assert!(!outcome.open_terminal && outcome.matches(plan));
            }
            other => panic!("{skip}: {other:?}"),
        }
        // An explicit null is no absence, even where no phase belongs.
        let mut null = cli_receipt(plan, ANSWERED);
        null["codex_update"] = Value::Null;
        assert_eq!(
            Outcome::read(plan, null.to_string().as_bytes()),
            Read::Unknown(RECEIPT_PHASE),
            "{skip}"
        );
    }
    // Another question never carries one, even in Terminal.
    let ordinary = intent().verify().unwrap();
    let mut hooks = cli_receipt(&ordinary.plan, ANSWERED);
    hooks["open_terminal"] = json!(true);
    assert!(matches!(
        Outcome::read(&ordinary.plan, hooks.to_string().as_bytes()),
        Read::Recorded(Outcome {
            codex_update: None,
            open_terminal: true,
            ..
        })
    ));
    hooks["codex_update"] = json!("installing");
    assert_eq!(
        Outcome::read(&ordinary.plan, hooks.to_string().as_bytes()),
        Read::Unknown(RECEIPT_PHASE)
    );
}

/// The same pairing holds for a serialized outcome, receipt and lookup, not
/// only the raw read; both values survive a round trip; an old row without
/// the key still parses.
#[test]
fn a_serialized_outcome_receipt_and_lookup_keep_the_pairing() {
    let verified = update_intent("update_now").verify().unwrap();
    let plan = &verified.plan;
    let outcome = |status: &str, open_terminal: bool, phase: Option<UpdatePhase>| Outcome {
        request_id: plan.request_id.clone(),
        status: status.into(),
        input_sha256: plan.input_sha256.clone(),
        open_terminal,
        codex_update: phase,
    };
    let query = Query::for_plan(scope(), plan).unwrap();
    let lookup = |receipt: Receipt| Lookup {
        abi: ABI.into(),
        request_id: plan.request_id.clone(),
        scope: scope(),
        read_only: true,
        found: true,
        receipt: Some(receipt),
    };
    for phase in [UpdatePhase::Installing, UpdatePhase::Exited] {
        let good = receipt(
            &verified,
            Phase::Recorded,
            true,
            "",
            Some(outcome(ANSWERED, true, Some(phase))),
        );
        assert!(good.valid());
        assert_eq!(
            good.result(plan),
            Some(Ok(outcome(ANSWERED, true, Some(phase))))
        );
        assert!(lookup(good.clone()).matches(&query));
        let text = serde_json::to_string(&good).unwrap();
        assert!(text.contains(&format!(
            "\"codex_update\":{}",
            serde_json::to_string(&phase).unwrap()
        )));
        assert!(serde_json::from_str::<Receipt>(&text).unwrap() == good);
    }
    for (name, bad) in [
        ("missing", outcome(ANSWERED, true, None)),
        (
            "submitted",
            outcome(SUBMITTED, true, Some(UpdatePhase::Installing)),
        ),
        (
            "no terminal",
            outcome(ANSWERED, false, Some(UpdatePhase::Exited)),
        ),
    ] {
        assert!(!bad.matches(plan), "{name}");
        let row = receipt(&verified, Phase::Recorded, true, "", Some(bad));
        assert!(!row.valid(), "{name}");
        assert_eq!(row.result(plan), None, "{name}");
        assert!(!lookup(row).matches(&query), "{name}");
    }
    // An ordinary question's outcome never carries one.
    let ordinary = intent().verify().unwrap();
    let mut stray = Outcome {
        request_id: ordinary.plan.request_id.clone(),
        status: ANSWERED.into(),
        input_sha256: ordinary.plan.input_sha256.clone(),
        open_terminal: true,
        codex_update: Some(UpdatePhase::Installing),
    };
    assert!(!stray.matches(&ordinary.plan));
    assert!(!receipt(&ordinary, Phase::Recorded, true, "", Some(stray.clone())).valid());
    stray.codex_update = None;
    assert!(stray.matches(&ordinary.plan));
    // Decoding: an old row without the key is None; null, "none" and an
    // unknown value never decode.
    let mut json = serde_json::to_value(&stray).unwrap();
    assert!(json.get("codex_update").is_none());
    assert_eq!(
        serde_json::from_value::<Outcome>(json.clone()).unwrap(),
        stray
    );
    for value in [Value::Null, json!("none"), json!("Installing"), json!(true)] {
        json["codex_update"] = value.clone();
        assert!(
            serde_json::from_value::<Outcome>(json.clone()).is_err(),
            "{value}"
        );
    }
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
            Box::new(|a| a.code = "deferred_answer_unavailable".into()),
        ),
        ("not started", Box::new(|a| a.code = NOT_STARTED.into())),
        (
            "another kind",
            Box::new(|a| a.code = "deferred_send_not_admitted".into()),
        ),
        ("no code", Box::new(|a| a.code.clear())),
        (
            "receipt",
            Box::new(|a| a.receipt = Some(receipt(&verified, Phase::Running, false, "", None))),
        ),
    ];
    for (name, change) in cases {
        let mut admission = exact();
        change(&mut admission);
        assert!(!admission.not_admitted(&intent), "{name}");
    }
}
