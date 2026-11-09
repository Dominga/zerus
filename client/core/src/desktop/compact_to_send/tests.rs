use super::*;
use crate::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

const SESSION: &str = "claude/private-compact";
const RUN: &str = "run-1";
const CONVERSATION: &str = "conversation-1";
const TEXT: &str = "see [File #1]";
const FILE: (&str, &str, &str, &[u8]) = ("a.txt", "text/plain", "[File #1]", b"hello");

fn scope() -> Scope {
    Scope {
        configuration: "a".repeat(64),
        service_epoch: "00000000-0000-4000-8000-000000000090".into(),
    }
}
/// The version 2 checkpoint the client reserves for `text` and `files`.
fn reserved(text: &str, files: &[(&str, &str, &str, &[u8])]) -> Checkpoint {
    Checkpoint::reserve_compact_to_send(checkpoint::Intent {
        kind: checkpoint::Kind::CompactToSend,
        draft_revision: 3,
        message_sha256: checkpoint::message_sha256(text, files.iter().copied()),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
        question: None,
        start: None,
        retry_of: None,
        consent: true,
    })
    .unwrap()
}
/// `hgs compact-context`'s stdin as the client builds it.
fn compaction(checkpoint: &Checkpoint) -> Value {
    json!({"request_id": checkpoint.request_id, "expected_run_id": checkpoint.run_id,
        "expected_conversation_id": checkpoint.conversation_id})
}
/// `hgs send`'s stdin as the client builds it, naming the compaction.
fn send(checkpoint: &Checkpoint, text: &str, files: &[(&str, &str, &str, &[u8])]) -> Value {
    let attachments: Vec<Value> = files
        .iter()
        .map(|(name, mime, reference, bytes)| {
            json!({"name": name, "mime": mime, "data_base64": STANDARD.encode(bytes),
                "reference": reference})
        })
        .collect();
    json!({"request_id": checkpoint.send_request_id, "text": text, "attachments": attachments,
        "expected_run_id": checkpoint.run_id, "expected_conversation_id": checkpoint.conversation_id,
        "expected_compaction_id": checkpoint.request_id})
}
fn intent_of(checkpoint: &Checkpoint, compaction: &Value, original: &Value) -> Intent {
    Intent {
        scope: scope(),
        session: SESSION.into(),
        request_id: checkpoint.request_id.clone(),
        send_request_id: checkpoint.send_request_id.clone().unwrap(),
        checkpoint: serde_json::to_string(checkpoint).unwrap(),
        compaction: compaction.to_string(),
        original: original.to_string(),
    }
}
/// A valid compact-to-send with one attachment.
pub(crate) fn intent() -> Intent {
    let checkpoint = reserved(TEXT, &[FILE]);
    intent_of(
        &checkpoint,
        &compaction(&checkpoint),
        &send(&checkpoint, TEXT, &[FILE]),
    )
}

#[test]
fn a_saved_pair_and_its_exact_stdins_verify_into_one_plan() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let plan = &verified.plan;
    assert_eq!(plan.session, SESSION);
    assert_eq!(plan.compaction_request_id, intent.request_id);
    assert_eq!(plan.send_request_id, intent.send_request_id);
    assert_eq!(
        (plan.run_id.as_str(), plan.conversation_id.as_str()),
        (RUN, CONVERSATION)
    );
    // Each receipt digest is the SHA-256 of that child's exact stdin.
    assert_eq!(plan.compaction_sha256, hash(intent.compaction.as_bytes()));
    assert_eq!(plan.input_sha256, hash(intent.original.as_bytes()));
    assert_eq!(
        plan.compaction_args(),
        ["compact-context", SESSION, "--json"]
    );
    assert_eq!(plan.send_args(), ["send", SESSION, "--json"]);
    // The send half is exactly a deferred send's plan for its receipt rule.
    let send_plan = plan.send_plan();
    assert!(send_plan.valid());
    assert_eq!(send_plan.request_id, intent.send_request_id);
    assert_eq!(send_plan.input_sha256, plan.input_sha256);
    let binding = &verified.binding;
    assert!(binding.valid() && binding.agrees(plan));
    assert_eq!(binding.plan_sha256, plan.fingerprint().unwrap());
    // The key is the compaction's: another send under it is a conflict.
    let key = binding.operation_id().unwrap();
    let mut other = binding.clone();
    other.send_request_id = "00000000-0000-4000-8000-0000000000aa".into();
    assert_eq!(other.operation_id().unwrap(), key);
    other.compaction_request_id = "00000000-0000-4000-8000-0000000000ab".into();
    assert_ne!(other.operation_id().unwrap(), key);
    // Debug never shows the message.
    assert!(!format!("{intent:?}").contains("File #1"));
}

#[test]
fn only_one_target_and_one_pair_across_the_checkpoint_and_both_stdins_verify() {
    let base = intent();
    let checkpoint: Checkpoint = serde_json::from_str(&base.checkpoint).unwrap();
    let refused = |intent: Intent| intent.verify().err();
    let with = |change: &dyn Fn(&mut Intent)| {
        let mut copy = base.clone();
        change(&mut copy);
        refused(copy)
    };
    // The intent's own identities.
    for change in [
        &(|i: &mut Intent| i.scope.configuration = "short".into()) as &dyn Fn(&mut Intent),
        &|i| i.session = "@peer/claude/private-compact".into(),
        &|i| i.request_id = "not-a-uuid".into(),
        &|i| i.send_request_id = "00000000-0000-0000-0000-000000000000".into(),
        &|i| i.send_request_id = i.request_id.clone(),
        &|i| i.checkpoint.clear(),
        &|i| i.compaction.clear(),
        &|i| i.original.clear(),
    ] {
        assert_eq!(with(change), Some("invalid_compact_to_send_intent"));
    }
    // DeepSeek and OpenCode compact and send through their own backends.
    for session in ["dsh/example", "opencode/example"] {
        assert_eq!(
            with(&|i| i.session = session.into()),
            Some(AGENT_REFUSED),
            "{session}"
        );
    }
    // The checkpoint: version 2 of this pair, exactly.
    let single = Checkpoint::reserve(checkpoint::Intent {
        kind: checkpoint::Kind::CompactToSend,
        draft_revision: 3,
        message_sha256: checkpoint.message_sha256.clone(),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
        question: None,
        start: None,
        retry_of: None,
        consent: true,
    });
    for broken in [
        serde_json::to_string(&single).unwrap(),
        serde_json::to_string(&reserved(TEXT, &[FILE])).unwrap(),
        base.checkpoint.replacen('{', "{\"extra\":1,", 1),
        "{}".into(),
    ] {
        assert_eq!(
            with(&|i| i.checkpoint = broken.clone()),
            Some("invalid_compact_to_send_checkpoint")
        );
    }
    // The compaction's stdin: strict, and naming this pair's compaction.
    let exact = compaction(&checkpoint);
    for broken in [
        {
            let mut v = exact.clone();
            v["extra"] = json!(1);
            v.to_string()
        },
        {
            let mut v = exact.clone();
            v.as_object_mut()
                .unwrap()
                .remove("expected_conversation_id");
            v.to_string()
        },
        exact.to_string().replacen(
            '{',
            &format!("{{\"request_id\":\"{}\",", checkpoint.request_id),
            1,
        ),
        "[]".into(),
    ] {
        assert_eq!(
            with(&|i| i.compaction = broken.clone()),
            Some("invalid_compact_to_send_compaction"),
            "{broken}"
        );
    }
    for (key, value) in [
        ("request_id", json!(checkpoint.send_request_id)),
        ("expected_run_id", json!("run-2")),
        ("expected_conversation_id", json!("other")),
    ] {
        let mut v = exact.clone();
        v[key] = value;
        assert_eq!(
            with(&|i| i.compaction = v.to_string()),
            Some("compact_to_send_intent_changed"),
            "{key}"
        );
    }
    // The send's stdin: a deferred send's rules, and it must name this
    // compaction.
    let original = send(&checkpoint, TEXT, &[FILE]);
    for (key, value) in [
        ("request_id", json!(checkpoint.request_id)),
        ("expected_run_id", json!("run-2")),
        ("expected_conversation_id", json!("other")),
        ("expected_compaction_id", json!("")),
        ("expected_compaction_id", json!(checkpoint.send_request_id)),
    ] {
        let mut v = original.clone();
        v[key] = value.clone();
        assert_eq!(
            with(&|i| i.original = v.to_string()),
            Some("compact_to_send_intent_changed"),
            "{key} {value}"
        );
    }
    let mut missing = original.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("expected_compaction_id");
    assert_eq!(
        with(&|i| i.original = missing.to_string()),
        Some("compact_to_send_intent_changed")
    );
    let mut unknown = original.clone();
    unknown["extra"] = json!(1);
    assert_eq!(
        with(&|i| i.original = unknown.to_string()),
        Some("invalid_compact_to_send_original")
    );
    let mut command = original.clone();
    command["text"] = json!("/compact");
    assert_eq!(
        with(&|i| i.original = command.to_string()),
        Some("compact_to_send_message_refused")
    );
    let mut changed = original.clone();
    changed["text"] = json!("see [File #1] now");
    assert_eq!(
        with(&|i| i.original = changed.to_string()),
        Some("compact_to_send_message_changed")
    );
    // Bounds: the CLI's own compaction stdin and the control frame.
    let mut padded = exact.to_string();
    padded.push_str(&" ".repeat(MAX_COMPACTION));
    assert_eq!(
        with(&|i| i.compaction = padded.clone()),
        Some("compact_to_send_original_too_large")
    );
    let large = vec![b'x'; 3 << 20];
    let file = (
        "big.bin",
        "application/octet-stream",
        "[File #1]",
        large.as_slice(),
    );
    let big = reserved(TEXT, &[file]);
    assert_eq!(
        refused(intent_of(
            &big,
            &compaction(&big),
            &send(&big, TEXT, &[file])
        )),
        Some("compact_to_send_original_too_large")
    );
    // An empty conversation has no slot to complete.
    let mut empty = reserved(TEXT, &[FILE]);
    empty.conversation_id.clear();
    assert_eq!(
        refused(intent_of(
            &empty,
            &compaction(&empty),
            &send(&empty, TEXT, &[FILE])
        )),
        Some("invalid_compact_to_send_plan")
    );
}

fn compaction_record(plan: &Plan, status: &str) -> Value {
    json!({"request_id": plan.compaction_request_id, "name": plan.session, "run_id": plan.run_id,
        "conversation_id": plan.conversation_id, "status": status, "at": 7.5})
}

#[test]
fn only_the_compactions_exact_submitted_receipt_records_it() {
    let verified = intent().verify().unwrap();
    let plan = &verified.plan;
    let read = |value: &Value| Compacted::from_receipt(plan, value.to_string().as_bytes());
    let exact = read(&compaction_record(plan, SUBMITTED)).unwrap();
    assert!(exact.matches(plan));
    // The CLI's placeholder before any input holds only its status.
    assert_eq!(
        read(&json!({"status": "in_progress"})),
        Err(RECEIPT_IN_PROGRESS)
    );
    assert_eq!(
        read(&compaction_record(plan, "uncertain")),
        Err(RECEIPT_UNCERTAIN)
    );
    assert_eq!(
        read(&compaction_record(plan, "completed")),
        Err(RECEIPT_MISMATCH)
    );
    for (key, value) in [
        ("request_id", json!(plan.send_request_id)),
        ("name", json!("claude/other")),
        ("run_id", json!("run-2")),
        ("conversation_id", json!("other")),
    ] {
        let mut other = compaction_record(plan, SUBMITTED);
        other[key] = value;
        assert_eq!(read(&other), Err(RECEIPT_MISMATCH), "{key}");
        other.as_object_mut().unwrap().remove(key);
        assert_eq!(read(&other), Err(RECEIPT_MISMATCH), "missing {key}");
    }
    let duplicate = compaction_record(plan, "uncertain").to_string().replacen(
        '{',
        "{\"status\":\"submitted\",",
        1,
    );
    assert_eq!(
        Compacted::from_receipt(plan, duplicate.as_bytes()),
        Err(RECEIPT_INVALID)
    );
    let mut large = compaction_record(plan, SUBMITTED);
    large["padding"] = json!("x".repeat(RECEIPT_BYTES));
    assert_eq!(read(&large), Err(RECEIPT_INVALID));
    assert_eq!(read(&json!("submitted")), Err(RECEIPT_INVALID));
    // The send's receipt is a deferred send's, by the send plan.
    let send_plan = plan.send_plan();
    let record = json!({"request_id": plan.send_request_id, "name": plan.session,
        "run_id": plan.run_id, "conversation_id": plan.conversation_id,
        "digest": plan.input_sha256, "status": "submitted", "transport": "terminal"});
    let sent =
        deferred_send::Outcome::from_receipt(&send_plan, record.to_string().as_bytes()).unwrap();
    assert!(sent.matches(&send_plan));
    let mut other = record.clone();
    other["request_id"] = json!(plan.compaction_request_id);
    assert!(
        deferred_send::Outcome::from_receipt(&send_plan, other.to_string().as_bytes()).is_err()
    );
}

fn compacted(plan: &Plan) -> Compacted {
    Compacted {
        request_id: plan.compaction_request_id.clone(),
        status: SUBMITTED.into(),
    }
}
fn sent(plan: &Plan) -> deferred_send::Outcome {
    deferred_send::Outcome {
        request_id: plan.send_request_id.clone(),
        status: deferred_send::SUBMITTED.into(),
        input_sha256: plan.input_sha256.clone(),
    }
}
fn component<O>(step: Step, outcome: Option<O>) -> Component<O> {
    Component {
        step,
        code: match step {
            Step::NotStarted => NOT_STARTED.into(),
            Step::Unknown => "compact_to_send_timed_out".into(),
            _ => String::new(),
        },
        outcome,
    }
}
fn receipt(verified: &Verified, compaction: Step, send: Step, drained: bool) -> Receipt {
    let plan = &verified.plan;
    Receipt {
        binding: verified.binding.clone(),
        admission_epoch: "00000000-0000-4000-8000-000000000091".into(),
        revision: 1,
        compaction: component(
            compaction,
            (compaction == Step::Recorded).then(|| compacted(plan)),
        ),
        send: component(send, (send == Step::Recorded).then(|| sent(plan))),
        drained,
        code: String::new(),
    }
}

#[test]
fn only_a_drained_receipt_settles_and_the_send_never_follows_a_compaction_that_never_ran() {
    let verified = intent().verify().unwrap();
    let plan = &verified.plan;
    use Step::*;
    for (compaction, send, settled) in [
        (NotStarted, NotStarted, Settled::NotStarted),
        (Recorded, NotStarted, Settled::NotSent { compacted: true }),
        (Unknown, NotStarted, Settled::NotSent { compacted: false }),
        (Recorded, Recorded, Settled::Sent(sent(plan))),
        (Unknown, Recorded, Settled::Sent(sent(plan))),
        (Recorded, Unknown, Settled::Unknown),
        (Unknown, Unknown, Settled::Unknown),
    ] {
        let drained = receipt(&verified, compaction, send, true);
        assert!(drained.valid(), "{compaction:?} {send:?}");
        assert_eq!(
            drained.result(plan),
            Some(settled),
            "{compaction:?} {send:?}"
        );
        // Undrained or another plan never settles.
        let mut open = drained.clone();
        open.drained = false;
        assert!(open.valid());
        assert_eq!(open.result(plan), None);
        let mut other = plan.clone();
        other.input_sha256 = "f".repeat(64);
        assert_eq!(drained.result(&other), None);
    }
    // Never a send after a compaction that never started, nor before the
    // compaction settled.
    for (compaction, send) in [
        (NotStarted, Running),
        (NotStarted, Recorded),
        (NotStarted, Unknown),
        (Queued, Running),
        (Running, Running),
        (Running, Recorded),
    ] {
        assert!(
            !receipt(&verified, compaction, send, false).valid(),
            "{compaction:?} {send:?}"
        );
    }
    // A restart's retirement: a send never reserved is NotStarted while the
    // compaction still awaits its read-only drain.
    assert!(receipt(&verified, Running, NotStarted, false).valid());
    // Drained means both settled.
    for (compaction, send) in [(Running, NotStarted), (Recorded, Running), (Queued, Queued)] {
        assert!(
            !receipt(&verified, compaction, send, true).valid(),
            "{compaction:?} {send:?}"
        );
    }
    // Each component's own closed shape.
    let base = receipt(&verified, Recorded, Recorded, true);
    let broken = |change: &dyn Fn(&mut Receipt)| {
        let mut copy = base.clone();
        change(&mut copy);
        !copy.valid()
    };
    assert!(broken(&|r| r.compaction.outcome = None));
    assert!(broken(&|r| r.send.outcome = None));
    assert!(broken(&|r| r.send.code = "x".into()));
    assert!(broken(&|r| r
        .compaction
        .outcome
        .as_mut()
        .unwrap()
        .request_id =
        r.binding.send_request_id.clone()));
    assert!(broken(
        &|r| r.compaction.outcome.as_mut().unwrap().status = "uncertain".into()
    ));
    assert!(broken(
        &|r| r.send.outcome.as_mut().unwrap().request_id = r.binding.compaction_request_id.clone()
    ));
    assert!(broken(
        &|r| r.send.outcome.as_mut().unwrap().input_sha256 = "f".repeat(64)
    ));
    assert!(broken(&|r| r.code = "Not closed".into()));
    // The binding's own closed shape.
    assert!(broken(&|r| r.binding.conversation_id.clear()));
    assert!(broken(&|r| r.binding.run_id.clear()));
    assert!(broken(
        &|r| r.binding.send_request_id = r.binding.compaction_request_id.clone()
    ));
    assert!(broken(&|r| r.binding.session = "dsh/example".into()));
    assert!(broken(&|r| r.binding.schema = 2));
    assert!(broken(&|r| r.binding.input_sha256 = "short".into()));
    assert!(broken(&|r| r.revision = 0));
    let not_started = receipt(&verified, NotStarted, NotStarted, true);
    let mut wrong = not_started.clone();
    wrong.send.code = "compact_to_send_timed_out".into();
    assert!(!wrong.valid());
    let mut unexplained = receipt(&verified, Unknown, NotStarted, true);
    unexplained.compaction.code.clear();
    assert!(!unexplained.valid());
}

/// desktop-u R1: evidence only moves forward. A settled child never changes,
/// so the pre-spawn "both NotStarted" end can never overwrite a recorded
/// compaction or send.
#[test]
fn a_settled_child_never_changes_and_nothing_follows_the_drain() {
    let verified = intent().verify().unwrap();
    use Step::*;
    let at = |compaction: Step, send: Step, revision: u64, drained: bool| {
        let mut r = receipt(&verified, compaction, send, drained);
        r.revision = revision;
        r
    };
    // Forward.
    for (from, to) in [
        ((Queued, Queued), (Running, Queued)),
        ((Queued, Queued), (NotStarted, NotStarted)),
        ((Running, Queued), (Recorded, Queued)),
        ((Running, Queued), (Unknown, Queued)),
        ((Running, Queued), (NotStarted, NotStarted)),
        ((Recorded, Queued), (Recorded, Running)),
        ((Recorded, Running), (Recorded, Recorded)),
        ((Unknown, Running), (Unknown, Unknown)),
        ((Running, Queued), (Running, NotStarted)),
    ] {
        assert!(
            at(to.0, to.1, 2, false).follows(&at(from.0, from.1, 1, false)),
            "{from:?} -> {to:?}"
        );
    }
    // Never back, never a settled child changed.
    for (from, to) in [
        ((Recorded, Queued), (NotStarted, NotStarted)),
        ((Unknown, Queued), (NotStarted, NotStarted)),
        ((Recorded, Recorded), (Recorded, NotStarted)),
        ((Unknown, Unknown), (Unknown, NotStarted)),
        ((Unknown, Queued), (Recorded, Queued)),
        ((Running, Queued), (Queued, Queued)),
        ((Recorded, Running), (Recorded, Queued)),
    ] {
        assert!(
            !at(to.0, to.1, 2, false).follows(&at(from.0, from.1, 1, false)),
            "{from:?} -> {to:?}"
        );
    }
    // A later revision of the same admission, and nothing after the drain.
    let earlier = at(Running, Queued, 2, false);
    assert!(!at(Recorded, Queued, 2, false).follows(&earlier));
    let mut other = at(Recorded, Queued, 3, false);
    other.admission_epoch = "00000000-0000-4000-8000-000000000092".into();
    assert!(!other.follows(&earlier));
    let drained = at(Recorded, NotStarted, 3, true);
    assert!(drained.follows(&earlier));
    assert!(!at(Recorded, NotStarted, 4, true).follows(&drained));
}

#[test]
fn an_admission_and_a_lookup_match_only_their_own_operation() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let held = receipt(&verified, Step::Running, Step::Queued, false);
    let admission = Admission {
        abi: ABI.into(),
        request_id: intent.request_id.clone(),
        scope: intent.scope.clone(),
        accepted: true,
        durable: true,
        code: String::new(),
        receipt: Some(held.clone()),
    };
    assert!(admission.matches(&intent));
    let refused = |change: &dyn Fn(&mut Admission)| {
        let mut copy = admission.clone();
        change(&mut copy);
        !copy.matches(&intent)
    };
    assert!(refused(&|a| a.abi = deferred_send::ABI.into()));
    assert!(refused(&|a| a.request_id = intent.send_request_id.clone()));
    assert!(refused(&|a| a.durable = false));
    assert!(refused(&|a| a.code = "compact_to_send_unsupported".into()));
    assert!(refused(&|a| a.receipt = None));
    assert!(refused(&|a| a
        .receipt
        .as_mut()
        .unwrap()
        .binding
        .send_request_id =
        "00000000-0000-4000-8000-0000000000aa".into()));
    // The lookup is keyed by the compaction and the exact plan.
    let query = Query::for_plan(intent.scope.clone(), &verified.plan).unwrap();
    assert_eq!(query.request_id, intent.request_id);
    let lookup = Lookup {
        abi: ABI.into(),
        request_id: query.request_id.clone(),
        scope: query.scope.clone(),
        read_only: true,
        found: true,
        receipt: Some(held),
    };
    assert!(lookup.matches(&query));
    let missing = Lookup {
        found: false,
        receipt: None,
        ..lookup.clone()
    };
    assert!(missing.matches(&query));
    let refused = |change: &dyn Fn(&mut Lookup)| {
        let mut copy = lookup.clone();
        change(&mut copy);
        !copy.matches(&query)
    };
    assert!(refused(&|l| l.read_only = false));
    assert!(refused(&|l| l.found = false));
    assert!(refused(&|l| l.abi = deferred_send::ABI.into()));
    assert!(refused(&|l| l
        .receipt
        .as_mut()
        .unwrap()
        .binding
        .plan_sha256 = "f".repeat(64)));
    assert!(refused(&|l| l
        .receipt
        .as_mut()
        .unwrap()
        .binding
        .configuration = "b".repeat(64)));
    let mut wrong = query.clone();
    wrong.request_id = intent.send_request_id.clone();
    assert!(!lookup.matches(&wrong));
    // A receipt of another operation or session under this query's name.
    assert!(refused(&|l| l
        .receipt
        .as_mut()
        .unwrap()
        .binding
        .compaction_request_id =
        "00000000-0000-4000-8000-0000000000ab".into()));
    assert!(refused(
        &|l| l.receipt.as_mut().unwrap().binding.session = "claude/other".into()
    ));
}

#[test]
fn the_native_request_names_its_reserved_identity() {
    let intent = intent();
    let request = crate::desktop::ui_requests::Request::CompactToSend {
        intent: Box::new(intent.clone()),
    };
    assert_eq!(request.operation(), "compact_to_send");
    assert_eq!(request.id(), intent.request_id);
    // A plain deferred send never carries a compaction.
    let checkpoint: Checkpoint = serde_json::from_str(&intent.checkpoint).unwrap();
    let plain = deferred_send::Intent {
        scope: intent.scope.clone(),
        session: intent.session.clone(),
        request_id: intent.send_request_id.clone(),
        checkpoint: intent.checkpoint.clone(),
        original: intent.original.clone(),
    };
    assert!(plain.verify().is_err());
    assert_eq!(
        checkpoint.send_request_id.as_deref(),
        Some(intent.send_request_id.as_str())
    );
}

/// review-3 (V4): the checkpoint reserves exactly this send. The same
/// compaction with another reserved send, or today's version 1 form with
/// none, never verifies: a reload never derives another send ID.
#[test]
fn the_checkpoint_reserves_exactly_this_send() {
    let base = intent();
    let saved: Checkpoint = serde_json::from_str(&base.checkpoint).unwrap();
    // The same compaction with another reserved send.
    let mut other = saved.clone();
    other.send_request_id = Some("00000000-0000-4000-8000-0000000000ac".into());
    // Today's version 1 compact-to-send of the same compaction: no send reserved.
    let mut single = saved.clone();
    single.version = checkpoint::VERSION;
    single.send_request_id = None;
    for broken in [other, single] {
        assert!(broken.valid());
        let mut copy = base.clone();
        copy.checkpoint = serde_json::to_string(&broken).unwrap();
        assert_eq!(
            copy.verify().err(),
            Some("invalid_compact_to_send_checkpoint"),
            "{broken:?}"
        );
    }
}

/// review-3 (BD): a binding names two distinct requests, checked on its own:
/// a receipt whose only fault is a binding with one ID for both is invalid.
#[test]
fn a_binding_names_two_distinct_requests() {
    let verified = intent().verify().unwrap();
    let mut same = receipt(&verified, Step::Running, Step::Queued, false);
    assert!(same.valid());
    same.binding.send_request_id = same.binding.compaction_request_id.clone();
    assert!(!same.binding.valid());
    assert!(!same.valid());
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
            Box::new(|a| a.code = "compact_to_send_unavailable".into()),
        ),
        ("not started", Box::new(|a| a.code = NOT_STARTED.into())),
        (
            "another kind",
            Box::new(|a| a.code = "session_resume_not_admitted".into()),
        ),
        ("no code", Box::new(|a| a.code.clear())),
        (
            "receipt",
            Box::new(|a| {
                a.receipt = Some(receipt(
                    &verified,
                    Step::NotStarted,
                    Step::NotStarted,
                    false,
                ))
            }),
        ),
    ];
    for (name, change) in cases {
        let mut admission = exact();
        change(&mut admission);
        assert!(!admission.not_admitted(&intent), "{name}");
    }
}
