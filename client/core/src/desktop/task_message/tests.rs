use super::*;
use crate::desktop::ui_requests::{fits_frontend_line, Request};
use crate::tasks_write::{input_digest, message_input, message_intent, outbox};
use serde_json::json;

const TASK: &str = "11111111-1111-4111-8111-111111111111";
const REQUEST: &str = "22222222-2222-4222-8222-222222222222";
const EPOCH: &str = "33333333-3333-4333-8333-333333333333";
const DISPATCH: &str = "44444444-4444-4444-8444-444444444444";

fn scope() -> Scope {
    Scope {
        configuration: "a".repeat(64),
        service_epoch: EPOCH.into(),
    }
}
/// A record as `outbox::reserve_message` saves a canonical frozen stdin.
fn record(mode: &str, dispatch: &str) -> Value {
    let intent = message_intent("hello", &[], mode, dispatch);
    json!({"id": REQUEST, "intent": intent, "input_sha256": input_digest(&message_input(&intent))})
}
pub(super) fn frozen() -> Intent {
    intent(record("queue", "").to_string())
}
fn intent(original: String) -> Intent {
    Intent {
        scope: scope(),
        project: "example".into(),
        task_id: TASK.into(),
        request_id: REQUEST.into(),
        original,
    }
}

#[test]
fn a_frozen_queue_or_note_original_verifies_to_its_exact_call() {
    for (mode, dispatch) in [("queue", ""), ("note", DISPATCH)] {
        let saved = record(mode, dispatch);
        let original = saved.to_string();
        let verified = intent(original.clone()).verify().unwrap();
        let plan = &verified.plan;
        assert_eq!(
            (plan.mode.as_str(), plan.dispatch_id.as_str()),
            (mode, dispatch)
        );
        let call = verified.call(false).unwrap();
        assert_eq!(call.input, message_input(&saved["intent"]));
        assert_eq!(hash(call.input.as_bytes()), plan.input_sha256);
        assert!(call.args.windows(2).any(|p| p == ["--request-id", REQUEST]));
        assert!(call.args.windows(2).any(|p| p == ["--mode", mode]));
        assert!(!call.args.iter().any(|a| a == "--recover-only"));
        assert!(verified
            .call(true)
            .unwrap()
            .args
            .ends_with(&["--recover-only".to_owned()]));
        let binding = &verified.binding;
        assert!(binding.valid());
        assert_eq!(binding.source_sha256, hash(original.as_bytes()));
        assert_eq!(binding.ui_snapshot_sha256, outbox::snapshot(&saved));
        assert_eq!(binding.input_sha256, plan.input_sha256);
        assert_eq!(binding.plan_sha256, plan.fingerprint().unwrap());
        // The same original always binds the same way.
        assert_eq!(intent(original).verify().unwrap().binding, *binding);
    }
    // Exact non-canonical frozen bytes are sent as saved.
    let mut saved = record("queue", "");
    let pretty = "{ \"attachments\":[], \"text\":\"hello\" }\n";
    saved.as_object_mut().unwrap().remove("input_sha256");
    saved["input"] = json!(pretty);
    let verified = intent(saved.to_string()).verify().unwrap();
    assert_eq!(verified.call(false).unwrap().input, pretty);
    assert_eq!(verified.plan.input_sha256, hash(pretty.as_bytes()));
}

#[test]
fn legacy_unsupported_and_changed_originals_are_refused() {
    let refused = |value: Value| intent(value.to_string()).verify().err();
    // A legacy record froze no stdin: review-only.
    let mut legacy = record("queue", "");
    legacy.as_object_mut().unwrap().remove("input_sha256");
    assert_eq!(refused(legacy), Some("task_message_input_not_frozen"));
    // Send, reopen and other modes stay review-only.
    for mode in ["send", "reopen", "comment", ""] {
        assert_eq!(
            refused(record(mode, "")),
            Some("task_message_mode_refused"),
            "{mode}"
        );
    }
    let mut dispatch = record("queue", "");
    dispatch["intent"]["dispatch"] = json!("not-a-uuid");
    dispatch["input_sha256"] = json!(input_digest(&message_input(&dispatch["intent"])));
    assert_eq!(refused(dispatch), Some("task_message_mode_refused"));
    // Another identity, or a frozen input this content no longer produces.
    let mut other = record("queue", "");
    other["id"] = json!(DISPATCH);
    assert_eq!(refused(other), Some("task_message_intent_changed"));
    let mut changed = record("queue", "");
    changed["intent"]["content"]["text"] = json!("changed");
    assert_eq!(refused(changed), Some("invalid_task_message_checkpoint"));
    for malformed in [json!(null), json!(42), json!("b".repeat(64))] {
        let mut bad = record("queue", "");
        bad["input_sha256"] = malformed;
        assert_eq!(refused(bad), Some("invalid_task_message_checkpoint"));
    }
    // Duplicate keys at the record or intent level never reach a Value.
    let raw = record("queue", "").to_string();
    let duplicated = raw.replacen("{\"id\"", "{\"id\":\"x\",\"id\"", 1);
    assert_ne!(duplicated, raw);
    assert_eq!(
        intent(duplicated).verify().err(),
        Some("invalid_task_message_checkpoint")
    );
    let intent_duplicate = raw.replacen("\"mode\"", "\"mode\":\"note\",\"mode\"", 1);
    assert_ne!(intent_duplicate, raw);
    assert_eq!(
        intent(intent_duplicate).verify().err(),
        Some("invalid_task_message_checkpoint")
    );
    // The intent's own fields.
    let mut empty = intent(String::new());
    assert_eq!(
        empty.clone().verify().err(),
        Some("invalid_task_message_intent")
    );
    empty.original = record("queue", "").to_string();
    empty.request_id = "not-a-uuid".into();
    assert_eq!(empty.verify().err(), Some("invalid_task_message_intent"));
    let mut unscoped = intent(record("queue", "").to_string());
    unscoped.scope.configuration = "z".into();
    assert_eq!(unscoped.verify().err(), Some("invalid_task_message_intent"));
}

#[test]
fn the_complete_service_line_bounds_the_inline_original() {
    let padded = |n: usize| {
        let mut value = record("queue", "");
        value["opaque"] = json!("x".repeat(n));
        intent(value.to_string())
    };
    let fits = |n: usize| {
        fits_frontend_line(&Request::TaskMessage {
            intent: Box::new(padded(n)),
        })
    };
    // The largest original that fits the complete line, found exactly.
    let (mut low, mut high) = (0, zerus_watch_protocol::CONTROL_LINE_BYTES);
    assert!(fits(low) && !fits(high));
    while high - low > 1 {
        let middle = (low + high) / 2;
        if fits(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    assert!(padded(low).verify().is_ok());
    assert_eq!(
        padded(low + 1).verify().err(),
        Some("task_message_original_too_large"),
        "never sent, so it stays reviewable"
    );
    // The bare envelope alone would still have looked small enough.
    let envelope = serde_json::to_vec(&crate::desktop::ui_requests::Envelope {
        abi: crate::desktop::ui_requests::ABI.into(),
        attempt: REQUEST.into(),
        request: Request::TaskMessage {
            intent: Box::new(padded(low + 1)),
        },
    })
    .unwrap();
    assert!(envelope.len() < zerus_watch_protocol::CONTROL_LINE_BYTES);
}

#[test]
fn only_the_exact_drained_not_started_original_counts_as_never_sent() {
    let verified = intent(record("queue", "").to_string()).verify().unwrap();
    let plan = &verified.plan;
    let receipt = Receipt {
        binding: verified.binding.clone(),
        admission_epoch: EPOCH.into(),
        revision: 3,
        phase: Phase::Failed,
        drained: true,
        code: NOT_STARTED.into(),
        outcome: None,
    };
    assert!(receipt.valid() && receipt.not_started(plan));
    // A started child's exit, an Unknown or an undrained original with the
    // same code, or another original's plan, never counts.
    let mut exited = receipt.clone();
    exited.code = EXITED.into();
    assert!(exited.valid() && !exited.not_started(plan));
    let mut unknown = receipt.clone();
    unknown.phase = Phase::Unknown;
    assert!(unknown.valid() && !unknown.not_started(plan));
    let mut undrained = receipt.clone();
    undrained.drained = false;
    assert!(!undrained.not_started(plan));
    let other = intent(record("note", DISPATCH).to_string())
        .verify()
        .unwrap();
    assert!(!receipt.not_started(&other.plan));
}

#[test]
fn only_the_exact_drained_recorded_outcome_settles() {
    let verified = intent(record("queue", "").to_string()).verify().unwrap();
    let plan = &verified.plan;
    let recorded = Receipt {
        binding: verified.binding.clone(),
        admission_epoch: EPOCH.into(),
        revision: 3,
        phase: Phase::Recorded,
        drained: true,
        code: String::new(),
        outcome: Some(Outcome {
            request_id: REQUEST.into(),
            mode: "queue".into(),
            dispatch_id: String::new(),
            state: "queued".into(),
        }),
    };
    assert!(recorded.matches(&verified.intent));
    assert_eq!(recorded.result(plan), Some(Ok(())));
    // A note's outcome is not a queued one, and a running original or an
    // undrained receipt never settles.
    let mut wrong = recorded.clone();
    wrong.outcome.as_mut().unwrap().state = "note".into();
    assert_eq!(wrong.result(plan), None);
    for phase in [
        Phase::Queued,
        Phase::Reserved,
        Phase::Running,
        Phase::Unknown,
    ] {
        let mut open = recorded.clone();
        open.phase = phase;
        assert!(!open.valid(), "an outcome belongs only to Recorded");
        open.outcome = None;
        open.drained = phase.terminal();
        assert!(open.valid());
        assert_eq!(open.result(plan), None, "{phase:?}");
    }
    let mut undrained = recorded.clone();
    undrained.drained = false;
    assert!(!undrained.valid());
    let mut failed = recorded.clone();
    failed.phase = Phase::Failed;
    failed.outcome = None;
    failed.code = "task_message_refused".into();
    assert_eq!(
        failed.result(plan),
        Some(Err("task_message_refused".into()))
    );
    // A lookup answers only its exact original.
    let query = Query::for_plan(scope(), plan).unwrap();
    let lookup = Lookup {
        abi: ABI.into(),
        request_id: REQUEST.into(),
        scope: scope(),
        read_only: true,
        found: true,
        receipt: Some(recorded.clone()),
    };
    assert!(lookup.matches(&query));
    let mut missing = lookup.clone();
    missing.receipt = None;
    assert!(!missing.matches(&query), "found without a receipt");
    missing.found = false;
    assert!(missing.matches(&query), "Missing never authorizes anything");
    let mut other = query.clone();
    other.plan_sha256 = "b".repeat(64);
    assert!(!lookup.matches(&other));
}
