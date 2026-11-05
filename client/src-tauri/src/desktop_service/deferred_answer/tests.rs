use super::*;
use zerus_client_core::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};
use zerus_client_core::questions::AnswerItem;

pub(super) const SESSION: &str = "claude/private-answer";
const EPOCH: &str = "33333333-3333-4333-8333-333333333333";
const HASH: &str = "abababababababababababababababababababababababababababababababab";

fn scope() -> wire::Scope {
    wire::Scope {
        configuration: "a".repeat(64),
        service_epoch: EPOCH.into(),
    }
}
/// A native question of `source`, as the card holds it. Codex's update
/// question, and only it, has a `codex-update:` ID.
pub(super) fn question(source: &str) -> Value {
    let id = if source == "codex_update" {
        format!("codex-update:{HASH}")
    } else {
        "q-1".to_owned()
    };
    json!({"question_id": id, "question_hash": HASH, "run_id": "run-1",
        "conversation_id": "conversation-1", "source": source, "answer_transport": "claude_tui",
        "questions": [{"id": "q-1", "question": "Allow this folder?",
            "options": [{"id": "yes", "label": "Yes"}, {"id": "no", "label": "No"}]}],
        "can_answer": true})
}
fn choice(option: &str) -> Vec<AnswerItem> {
    vec![AnswerItem::Choice {
        question_id: "q-1".into(),
        selected_option_ids: vec![option.into()],
        text: String::new(),
    }]
}
/// An answer's checkpoint, its consent identity and its exact stdin, as the
/// card builds them.
pub(super) fn original(source: &str, option: &str) -> (Checkpoint, Value, Vec<u8>) {
    let question = question(source);
    let answers = choice(option);
    let pinned = zerus_client_core::questions::consent_identity(&question);
    let checkpoint = Checkpoint::reserve(checkpoint::Intent {
        kind: checkpoint::Kind::Answer,
        draft_revision: 2,
        message_sha256: checkpoint::message_sha256(
            &serde_json::to_string(&answers).unwrap(),
            Vec::<(&str, &str, &str, &[u8])>::new(),
        ),
        run_id: "run-1".into(),
        conversation_id: "conversation-1".into(),
        question: Some(checkpoint::Question {
            id: question["question_id"].as_str().unwrap().into(),
            identity_sha256: checkpoint::identity_sha256(&pinned),
        }),
        start: None,
        retry_of: None,
        consent: false,
    });
    let stdin = zerus_client_core::actions::answer_input(
        SESSION,
        &question,
        &answers,
        &checkpoint.request_id,
    )
    .unwrap();
    (checkpoint, pinned, stdin)
}
fn verified() -> wire::Verified {
    let (checkpoint, pinned, stdin) = original("claude_folder_trust", "yes");
    intent(&scope(), SESSION, &checkpoint, &pinned, &stdin)
        .unwrap()
        .verify()
        .unwrap()
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
            status: wire::ANSWERED.into(),
            input_sha256: verified.plan.input_sha256.clone(),
            open_terminal: false,
            codex_update: None,
        }),
    }
}

#[test]
fn the_cards_exact_stdin_verifies_and_its_plan_is_found_again_after_a_reload() {
    let (checkpoint, pinned, stdin) = original("claude_folder_trust", "yes");
    let verified = intent(&scope(), SESSION, &checkpoint, &pinned, &stdin)
        .unwrap()
        .verify()
        .unwrap();
    // The one call's stdin, byte for byte, is the original.
    assert_eq!(verified.intent.original.as_bytes(), stdin.as_slice());
    assert_eq!(
        plan(SESSION, &checkpoint, &pinned, &stdin).as_ref(),
        Some(&verified.plan)
    );
    // Another answer under the same checkpoint, or a changed question, is
    // never this original's plan.
    let other = zerus_client_core::actions::answer_input(
        SESSION,
        &question("claude_folder_trust"),
        &choice("no"),
        &checkpoint.request_id,
    )
    .unwrap();
    assert_eq!(plan(SESSION, &checkpoint, &pinned, &other), None);
    let mut changed = pinned.clone();
    changed["approval"] = json!("allow_always");
    assert_eq!(plan(SESSION, &checkpoint, &changed, &stdin), None);
    // A DeepSeek or OpenCode session, or a source v1 keeps direct, is never
    // this kind's.
    assert_eq!(plan("dsh/example", &checkpoint, &pinned, &stdin), None);
    let (checkpoint, pinned, stdin) = original("codex_async", "yes");
    assert_eq!(plan(SESSION, &checkpoint, &pinned, &stdin), None);
}

#[test]
fn only_the_exact_drained_receipt_settles_and_each_failure_says_why() {
    let verified = verified();
    let plan = &verified.plan;
    match settled(&receipt(&verified, wire::Phase::Recorded, true, ""), plan) {
        Some(AnswerResult::Recorded(outcome)) => assert!(outcome.matches(plan)),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        settled(
            &receipt(&verified, wire::Phase::Failed, true, wire::NOT_STARTED),
            plan
        ),
        Some(AnswerResult::Unsent(NOT_STARTED.into()))
    );
    assert_eq!(
        settled(
            &receipt(&verified, wire::Phase::Failed, true, wire::REJECTED),
            plan
        ),
        Some(AnswerResult::Unsent(REFUSED.into()))
    );
    assert_eq!(
        settled(
            &receipt(
                &verified,
                wire::Phase::Unknown,
                true,
                "deferred_answer_receipt_missing"
            ),
            plan
        ),
        Some(AnswerResult::Unconfirmed(UNCONFIRMED.into()))
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
    let (checkpoint, pinned, stdin) = original("claude_folder_trust", "no");
    let other = intent(&scope(), SESSION, &checkpoint, &pinned, &stdin)
        .unwrap()
        .verify()
        .unwrap();
    for (phase, code) in [
        (wire::Phase::Recorded, ""),
        (wire::Phase::Failed, wire::REJECTED),
    ] {
        assert_eq!(
            settled(&receipt(&verified, phase, true, code), &other.plan),
            None
        );
    }
}

#[test]
fn without_a_live_service_an_answer_stays_direct_and_a_check_knows_nothing() {
    let (checkpoint, pinned, stdin) = original("claude_folder_trust", "yes");
    assert_eq!(
        call(
            None,
            SESSION,
            &checkpoint,
            &pinned,
            &stdin,
            &AtomicBool::new(false)
        ),
        AnswerResult::Direct
    );
    assert_eq!(check(None, &verified().plan), Checked::Unavailable);
}
