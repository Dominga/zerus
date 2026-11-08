use super::*;
use crate::operation_recovery::deferred_input::{self as checkpoint, Checkpoint};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

const SESSION: &str = "claude/private-resume";
const RUN: &str = "run-1";
const RESUMED: &str = "00000000-0000-4000-8000-0000000000b1";
const CONVERSATION: &str = "conversation-1";
const TEXT: &str = "see [File #1]";
const FILE: (&str, &str, &str, &[u8]) = ("a.txt", "text/plain", "[File #1]", b"hello");

fn scope() -> Scope {
    Scope {
        configuration: "a".repeat(64),
        service_epoch: "00000000-0000-4000-8000-000000000090".into(),
    }
}
/// The version 3 checkpoint the client reserves for `text` and `files`.
fn reserved(text: &str, files: &[(&str, &str, &str, &[u8])]) -> Checkpoint {
    Checkpoint::reserve_session_resume(checkpoint::Intent {
        kind: checkpoint::Kind::ResumeToSend,
        draft_revision: 3,
        message_sha256: checkpoint::message_sha256(text, files.iter().copied()),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
        question: None,
        start: Some(checkpoint::Start::Resume),
        retry_of: None,
        consent: true,
    })
    .unwrap()
}
/// `hgs session-action`'s stdin for this resume, as the client builds it.
fn resume(checkpoint: &Checkpoint) -> Value {
    json!({"request_id": checkpoint.request_id, "action": "resume",
        "expected_run_id": checkpoint.run_id,
        "expected_conversation_id": checkpoint.conversation_id})
}
/// The send's template as the client builds it, naming the original run.
fn send(checkpoint: &Checkpoint, text: &str, files: &[(&str, &str, &str, &[u8])]) -> Value {
    let attachments: Vec<Value> = files
        .iter()
        .map(|(name, mime, reference, bytes)| {
            json!({"name": name, "mime": mime, "data_base64": STANDARD.encode(bytes),
                "reference": reference})
        })
        .collect();
    json!({"request_id": checkpoint.send_request_id, "text": text, "attachments": attachments,
        "expected_run_id": checkpoint.run_id, "expected_conversation_id": checkpoint.conversation_id})
}
fn intent_of(checkpoint: &Checkpoint, resume: &Value, original: &Value) -> Intent {
    Intent {
        scope: scope(),
        session: SESSION.into(),
        request_id: checkpoint.request_id.clone(),
        send_request_id: checkpoint.send_request_id.clone().unwrap(),
        checkpoint: serde_json::to_string(checkpoint).unwrap(),
        resume: resume.to_string(),
        original: original.to_string(),
    }
}
/// A valid resume-then-send with one attachment.
pub(crate) fn intent() -> Intent {
    let checkpoint = reserved(TEXT, &[FILE]);
    intent_of(
        &checkpoint,
        &resume(&checkpoint),
        &send(&checkpoint, TEXT, &[FILE]),
    )
}
/// The resume's own exact outcome for `plan`.
fn resumed(plan: &Plan) -> Resumed {
    Resumed {
        request_id: plan.resume_request_id.clone(),
        status: COMPLETED.into(),
        result_target: ResultTarget {
            name: plan.session.clone(),
            run_id: RESUMED.into(),
            conversation_id: plan.conversation_id.clone(),
        },
    }
}

#[test]
fn a_saved_pair_and_its_exact_stdins_verify_into_one_plan() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let plan = &verified.plan;
    assert_eq!(plan.session, SESSION);
    assert_eq!(plan.resume_request_id, intent.request_id);
    assert_eq!(plan.send_request_id, intent.send_request_id);
    assert_eq!(
        (plan.run_id.as_str(), plan.conversation_id.as_str()),
        (RUN, CONVERSATION)
    );
    // The raw frozen stdins, never reserialized equivalents.
    assert_eq!(plan.resume_sha256, hash(intent.resume.as_bytes()));
    assert_eq!(plan.template_sha256, hash(intent.original.as_bytes()));
    assert_eq!(
        plan.message_sha256,
        checkpoint::message_sha256(TEXT, [FILE])
    );
    assert_eq!(plan.resume_args(), ["session-action", SESSION, "--json"]);
    assert_eq!(plan.send_args(), ["send", SESSION, "--json"]);
    assert!(verified.binding.valid() && verified.binding.agrees(plan));
    assert_eq!(verified.binding.plan_sha256, plan.fingerprint().unwrap());
    assert!(verified.binding.operation_id().is_some());
}

#[test]
fn only_one_target_and_one_pair_across_the_checkpoint_and_both_stdins_verify() {
    let checkpoint = reserved(TEXT, &[FILE]);
    let good_resume = resume(&checkpoint);
    let good_send = send(&checkpoint, TEXT, &[FILE]);
    let refused =
        |resume: Value, original: Value| intent_of(&checkpoint, &resume, &original).verify().err();
    let changed = Some("session_resume_intent_changed");
    // The resume: exactly the CLI's resume shape for this request and target.
    let mut other_action = good_resume.clone();
    other_action["action"] = json!("restore");
    assert_eq!(refused(other_action, good_send.clone()), changed);
    let mut other_request = good_resume.clone();
    other_request["request_id"] = json!(checkpoint.send_request_id);
    assert_eq!(refused(other_request, good_send.clone()), changed);
    for key in ["expected_run_id", "expected_conversation_id"] {
        let mut other = good_resume.clone();
        other[key] = json!("other");
        assert_eq!(refused(other, good_send.clone()), changed, "{key}");
    }
    // Every optional key the CLI takes for other actions is refused here.
    for (key, value) in [
        ("archive_id", json!("00000000-0000-4000-8000-0000000000a1")),
        ("new_name", json!("claude/renamed")),
        ("tag", json!("t")),
        ("message_id", json!("m")),
        ("terminal_identity", json!({})),
        ("unknown", json!(1)),
    ] {
        let mut extra = good_resume.clone();
        extra[key] = value;
        assert_eq!(
            refused(extra, good_send.clone()),
            Some("invalid_session_resume_resume"),
            "{key}"
        );
    }
    // The template names the original run, its conversation and its own
    // reserved request, and never a compaction.
    for (key, value) in [
        ("expected_run_id", json!(RESUMED)),
        ("expected_conversation_id", json!("other")),
        ("request_id", json!(checkpoint.request_id)),
        ("expected_compaction_id", json!(checkpoint.request_id)),
    ] {
        let mut other = good_send.clone();
        other[key] = value;
        assert_eq!(refused(good_resume.clone(), other), changed, "{key}");
    }
    // Another message than the consented one.
    assert_eq!(
        refused(
            good_resume.clone(),
            send(&checkpoint, "other text", &[FILE])
        ),
        Some("session_resume_message_changed")
    );
    // A message the CLI itself refuses, and a template that is not a send.
    assert_eq!(
        refused(good_resume.clone(), send(&checkpoint, "/compact", &[])),
        Some("session_resume_message_refused")
    );
    let mut unknown = good_send.clone();
    unknown["unknown"] = json!(1);
    assert_eq!(
        refused(good_resume.clone(), unknown),
        Some("invalid_session_resume_original")
    );
    // Only a local Claude, Kimi or Codex session; another agent stays direct.
    for (session, code) in [
        ("opencode/private-resume", AGENT_REFUSED),
        ("dsh/private-resume", AGENT_REFUSED),
        (
            "claude/private-resume@peer",
            "invalid_session_resume_intent",
        ),
    ] {
        let mut other = intent();
        other.session = session.into();
        assert_eq!(other.verify().err(), Some(code), "{session}");
    }
    // A resume larger than the CLI's own bound stays direct.
    let mut large = good_resume.clone();
    large["expected_conversation_id"] = json!("c".repeat(9000));
    assert_eq!(
        refused(large, good_send),
        Some("session_resume_original_too_large")
    );
}

/// review: the checkpoint is exactly a version 3 resume with this pair. A
/// version 1 resume-then-send, a fresh start, a compact-to-send or another
/// reserved send never verifies: a reload never derives another send ID.
#[test]
fn the_checkpoint_reserves_exactly_this_resume_and_send() {
    let base = intent();
    let saved: Checkpoint = serde_json::from_str(&base.checkpoint).unwrap();
    let mut other = saved.clone();
    other.send_request_id = Some("00000000-0000-4000-8000-0000000000ac".into());
    let mut direct = saved.clone();
    direct.version = checkpoint::VERSION;
    direct.receipts = "resume_input".into();
    direct.send_request_id = None;
    let compact = Checkpoint::reserve_compact_to_send(checkpoint::Intent {
        kind: checkpoint::Kind::CompactToSend,
        draft_revision: 3,
        message_sha256: saved.message_sha256.clone(),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
        question: None,
        start: None,
        retry_of: None,
        consent: true,
    })
    .unwrap();
    for broken in [other, direct, compact] {
        assert!(broken.valid());
        let mut copy = base.clone();
        copy.checkpoint = serde_json::to_string(&broken).unwrap();
        assert_eq!(
            copy.verify().err(),
            Some("invalid_session_resume_checkpoint"),
            "{broken:?}"
        );
    }
}

/// The send's stdin is the template with only its expected run replaced by
/// the resumed run; nothing else changes, and only this plan's own resume
/// binds it.
#[test]
fn only_bind_derives_the_send_and_it_replaces_only_the_run() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let plan = &verified.plan;
    let outcome = resumed(plan);
    let (bound, digest) = bind(plan, &intent.original, &outcome).unwrap();
    assert_eq!(digest, hash(bound.as_bytes()));
    let mut expected: Value = serde_json::from_str(&intent.original).unwrap();
    expected["expected_run_id"] = json!(RESUMED);
    let actual: Value = serde_json::from_str(&bound).unwrap();
    assert_eq!(actual, expected);
    // The bound send is still exactly a send the CLI takes, of the same
    // message, request and conversation.
    let frozen = deferred_send::frozen(&bound).unwrap();
    assert_eq!(frozen.run_id, RESUMED);
    assert_eq!(frozen.request_id, plan.send_request_id);
    assert_eq!(frozen.conversation_id, CONVERSATION);
    assert_eq!(frozen.message_sha256, plan.message_sha256);
    assert!(frozen.compaction_id.is_empty());
    // Deterministic: the same inputs bind the same bytes.
    assert_eq!(bind(plan, &intent.original, &outcome).unwrap().1, digest);
    // Never another template, nor a resume that is not this plan's own.
    assert!(bind(plan, &format!("{} ", intent.original), &outcome).is_none());
    let not_bound = |change: &dyn Fn(&mut Resumed)| {
        let mut copy = outcome.clone();
        change(&mut copy);
        bind(plan, &intent.original, &copy).is_none()
    };
    assert!(not_bound(&|r| r.request_id = plan.send_request_id.clone()));
    assert!(not_bound(&|r| r.status = "uncertain".into()));
    assert!(not_bound(&|r| r.result_target.name = "claude/other".into()));
    assert!(not_bound(
        &|r| r.result_target.conversation_id = "other".into()
    ));
    assert!(not_bound(&|r| r.result_target.run_id = RUN.into()));
    assert!(not_bound(&|r| r.result_target.run_id = String::new()));
    assert!(not_bound(&|r| r.result_target.run_id = "a\nb".into()));
}

/// The CLI's own observation of an action receipt, as
/// `session_action::status` projects it.
fn observation(plan: &Plan, status: &str) -> Value {
    let mut value = json!({"request_id": plan.resume_request_id, "name": plan.session,
        "run_id": plan.run_id, "conversation_id": plan.conversation_id, "status": status,
        "at": 1.5});
    if status == COMPLETED {
        value["result_target"] = json!({"name": plan.session, "run_id": RESUMED,
            "conversation_id": plan.conversation_id});
    }
    value
}
fn read(plan: &Plan, value: &Value) -> Result<Resumed, &'static str> {
    Resumed::from_receipt(plan, &serde_json::to_vec(value).unwrap())
}

#[test]
fn only_the_resumes_exact_receipt_records_it_and_only_an_exact_failure_refuses() {
    let verified = intent().verify().unwrap();
    let plan = &verified.plan;
    assert_eq!(read(plan, &observation(plan, COMPLETED)), Ok(resumed(plan)));
    assert_eq!(read(plan, &observation(plan, "failed")), Err(REFUSED));
    assert_eq!(
        read(plan, &observation(plan, "uncertain")),
        Err(RECEIPT_UNCERTAIN)
    );
    // Another request, session, run, conversation or an archive is not this
    // original's, whatever its status.
    for status in [COMPLETED, "failed", "uncertain"] {
        for (key, value) in [
            ("request_id", json!(plan.send_request_id)),
            ("name", json!("claude/other")),
            ("run_id", json!(RESUMED)),
            ("conversation_id", json!("other")),
            ("archive_id", json!("00000000-0000-4000-8000-0000000000a1")),
            ("at", json!(-1.0)),
        ] {
            let mut other = observation(plan, status);
            other[key] = value;
            assert_eq!(read(plan, &other), Err(RECEIPT_MISMATCH), "{status} {key}");
        }
        let mut no_time = observation(plan, status);
        no_time.as_object_mut().unwrap().remove("at");
        assert_eq!(read(plan, &no_time), Err(RECEIPT_MISMATCH), "{status}");
    }
    // A completed resume names this session and conversation, a run that is
    // not the original, and no archive; a failed or uncertain one no target.
    for (key, value) in [
        ("name", json!("claude/other")),
        ("conversation_id", json!("other")),
        ("run_id", json!(RUN)),
        ("run_id", json!("")),
        ("archive_id", json!("00000000-0000-4000-8000-0000000000a1")),
    ] {
        let mut other = observation(plan, COMPLETED);
        other["result_target"][key] = value;
        assert_eq!(read(plan, &other), Err(RECEIPT_MISMATCH), "{key}");
    }
    let mut no_target = observation(plan, COMPLETED);
    no_target.as_object_mut().unwrap().remove("result_target");
    assert_eq!(read(plan, &no_target), Err(RECEIPT_MISMATCH));
    for status in ["failed", "uncertain"] {
        let mut with_target = observation(plan, status);
        with_target["result_target"] = observation(plan, COMPLETED)["result_target"].clone();
        assert_eq!(read(plan, &with_target), Err(RECEIPT_MISMATCH), "{status}");
    }
    for status in ["claimed", "unavailable", "submitted", ""] {
        assert_eq!(
            read(plan, &observation(plan, status)),
            Err(RECEIPT_MISMATCH),
            "{status}"
        );
    }
    // Not a readable observation: unknown keys, another shape, too large.
    let mut unknown = observation(plan, COMPLETED);
    unknown["error"] = json!("private");
    assert_eq!(read(plan, &unknown), Err(RECEIPT_INVALID));
    assert_eq!(Resumed::from_receipt(plan, b"{"), Err(RECEIPT_INVALID));
    let mut large = observation(plan, "failed");
    large["name"] = json!("x".repeat(RECEIPT_BYTES));
    assert_eq!(read(plan, &large), Err(RECEIPT_INVALID));
}

fn sent(plan: &Plan, digest: &str) -> deferred_send::Outcome {
    deferred_send::Outcome {
        request_id: plan.send_request_id.clone(),
        status: deferred_send::SUBMITTED.into(),
        input_sha256: digest.into(),
    }
}
fn component<O>(step: Step, outcome: Option<O>) -> Component<O> {
    let code = match step {
        Step::Failed => REFUSED,
        Step::NotStarted => NOT_STARTED,
        Step::Unknown => "session_resume_timed_out",
        _ => "",
    };
    Component {
        step,
        code: code.into(),
        outcome,
    }
}
/// A receipt of `verified` with these steps; a recorded resume carries its
/// outcome and its bound send's digest, a recorded send its outcome.
fn receipt(verified: &Verified, resume: Step, send: Step, drained: bool) -> Receipt {
    let intent = &verified.intent;
    let outcome = resumed(&verified.plan);
    let digest = bind(&verified.plan, &intent.original, &outcome).unwrap().1;
    Receipt {
        binding: verified.binding.clone(),
        admission_epoch: intent.scope.service_epoch.clone(),
        revision: 1,
        resume: component(resume, (resume == Step::Recorded).then(|| outcome.clone())),
        bound_sha256: (resume == Step::Recorded).then(|| digest.clone()),
        send: component(
            send,
            (send == Step::Recorded).then(|| sent(&verified.plan, &digest)),
        ),
        drained,
        code: String::new(),
    }
}

#[test]
fn only_a_drained_receipt_settles_and_the_send_follows_only_a_recorded_resume() {
    let intent = intent();
    let verified = intent.clone().verify().unwrap();
    let plan = &verified.plan;
    use Step::*;
    // Every shape the actor may write, and how a drained one settles.
    for (resume, send, settled) in [
        (Recorded, Recorded, None),
        (NotStarted, NotStarted, Some(Settled::NotStarted)),
        (
            Recorded,
            NotStarted,
            Some(Settled::NotSent { resume: Recorded }),
        ),
        (
            Failed,
            NotStarted,
            Some(Settled::NotSent { resume: Failed }),
        ),
        (
            Unknown,
            NotStarted,
            Some(Settled::NotSent { resume: Unknown }),
        ),
        (Recorded, Unknown, Some(Settled::Unknown)),
    ] {
        let drained = receipt(&verified, resume, send, true);
        assert!(drained.valid(), "{resume:?} {send:?}");
        assert!(drained.matches(&intent), "{resume:?} {send:?}");
        match settled {
            Some(settled) => assert_eq!(drained.result(plan), Some(settled)),
            None => assert!(matches!(drained.result(plan), Some(Settled::Sent(_)))),
        }
        // An undrained receipt never settles.
        let mut undrained = drained.clone();
        undrained.drained = false;
        assert_eq!(undrained.result(plan), None);
    }
    // The send never runs without a recorded resume: there is no resumed run
    // to bind it to. Only the resume may fail.
    for resume in [Queued, Running, Failed, NotStarted, Unknown] {
        for send in [Running, Recorded, Unknown] {
            let mut copy = receipt(&verified, Recorded, send, false);
            copy.resume = component(resume, None);
            copy.bound_sha256 = None;
            assert!(!copy.valid(), "{resume:?} {send:?}");
        }
    }
    let mut failed_send = receipt(&verified, Recorded, NotStarted, true);
    failed_send.send = component(Failed, None);
    assert!(!failed_send.valid());
    // A drained receipt has both children settled.
    for (resume, send) in [(Running, Queued), (Recorded, Queued), (Recorded, Running)] {
        assert!(
            !receipt(&verified, resume, send, true).valid(),
            "{resume:?} {send:?}"
        );
    }
    // The bound send exists exactly with a recorded resume, and the send's
    // outcome carries exactly its digest.
    let mut unbound = receipt(&verified, Recorded, Queued, false);
    unbound.bound_sha256 = None;
    assert!(!unbound.valid());
    let mut early = receipt(&verified, Running, Queued, false);
    early.bound_sha256 = Some("f".repeat(64));
    assert!(!early.valid());
    let mut other_digest = receipt(&verified, Recorded, Recorded, true);
    other_digest.send.outcome.as_mut().unwrap().input_sha256 = "f".repeat(64);
    assert!(!other_digest.valid());
    // A digest that is not the template's bound to this resume never matches
    // the intent, even when every field is valid.
    let mut forged = receipt(&verified, Recorded, Queued, false);
    forged.bound_sha256 = Some("f".repeat(64));
    assert!(forged.valid() && !forged.matches(&intent));
    // A recorded resume is this plan's own.
    let mut original_run = receipt(&verified, Recorded, Queued, false);
    original_run
        .resume
        .outcome
        .as_mut()
        .unwrap()
        .result_target
        .run_id = RUN.into();
    assert!(!original_run.valid());
}

#[test]
fn a_settled_child_never_changes_and_nothing_follows_the_drain() {
    let verified = intent().verify().unwrap();
    use Step::*;
    let at = |resume: Step, send: Step, revision: u64, drained: bool| {
        let mut r = receipt(&verified, resume, send, drained);
        r.revision = revision;
        r
    };
    // The forward order of one operation.
    let path = [
        at(Queued, Queued, 1, false),
        at(Running, Queued, 2, false),
        at(Recorded, Queued, 3, false),
        at(Recorded, Running, 4, false),
        at(Recorded, Recorded, 5, true),
    ];
    for pair in path.windows(2) {
        assert!(pair[1].follows(&pair[0]), "{:?}", pair[1]);
    }
    // A settled resume never changes; nothing returns to Queued; a revision
    // only grows; nothing follows the drain.
    assert!(!at(Unknown, Queued, 4, false).follows(&at(Recorded, Queued, 3, false)));
    assert!(!at(Recorded, Queued, 4, false).follows(&at(Failed, Queued, 3, false)));
    assert!(!at(Queued, Queued, 3, false).follows(&at(Running, Queued, 2, false)));
    assert!(!at(Running, Queued, 2, false).follows(&at(Running, Queued, 2, false)));
    assert!(!at(Recorded, Recorded, 6, true).follows(&at(Recorded, Recorded, 5, true)));
    // The bound send is fixed once.
    let mut rebound = at(Recorded, Running, 4, false);
    rebound.bound_sha256 = Some("f".repeat(64));
    assert!(!rebound.follows(&at(Recorded, Queued, 3, false)));
    // Another binding or admission is another operation.
    let mut other = at(Running, Queued, 2, false);
    other.admission_epoch = "00000000-0000-4000-8000-0000000000ff".into();
    assert!(!other.follows(&at(Queued, Queued, 1, false)));
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
    assert!(refused(
        &|a| a.abi = super::super::compact_to_send::ABI.into()
    ));
    assert!(refused(&|a| a.request_id = intent.send_request_id.clone()));
    assert!(refused(&|a| a.durable = false));
    assert!(refused(&|a| a.code = "session_resume_unsupported".into()));
    assert!(refused(&|a| a.receipt = None));
    assert!(refused(&|a| a
        .receipt
        .as_mut()
        .unwrap()
        .binding
        .send_request_id =
        "00000000-0000-4000-8000-0000000000aa".into()));
    // The lookup is keyed by the resume and the exact plan.
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
    assert!(refused(
        &|l| l.abi = super::super::compact_to_send::ABI.into()
    ));
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
    assert!(refused(&|l| l
        .receipt
        .as_mut()
        .unwrap()
        .binding
        .resume_request_id =
        "00000000-0000-4000-8000-0000000000ab".into()));
    assert!(refused(
        &|l| l.receipt.as_mut().unwrap().binding.session = "claude/other".into()
    ));
    let mut wrong = query.clone();
    wrong.request_id = intent.send_request_id.clone();
    assert!(!lookup.matches(&wrong));
}

#[test]
fn the_native_request_names_its_reserved_identity_and_no_other_kind_takes_it() {
    let intent = intent();
    let request = crate::desktop::ui_requests::Request::SessionResume {
        intent: Box::new(intent.clone()),
    };
    assert_eq!(request.operation(), "session_resume");
    assert_eq!(request.id(), intent.request_id);
    // A plain deferred send of the template, under the version 3 checkpoint,
    // never verifies: only this kind binds it to the resumed run.
    let plain = deferred_send::Intent {
        scope: intent.scope.clone(),
        session: intent.session.clone(),
        request_id: intent.send_request_id.clone(),
        checkpoint: intent.checkpoint.clone(),
        original: intent.original.clone(),
    };
    assert!(plain.verify().is_err());
}

/// A binding names two distinct requests, checked on its own: a receipt
/// whose only fault is a binding with one ID for both is invalid.
#[test]
fn a_binding_names_two_distinct_requests() {
    let verified = intent().verify().unwrap();
    let mut same = receipt(&verified, Step::Running, Step::Queued, false);
    assert!(same.valid());
    same.binding.send_request_id = same.binding.resume_request_id.clone();
    assert!(!same.binding.valid());
    assert!(!same.valid());
}

/// zerus-5wmh.66.4.10: a Codex resume-then-send verifies like Claude and
/// Kimi, and only Codex's readiness is the send's own observation, with
/// exactly its arguments. DeepSeek and OpenCode stay direct, decided before
/// any offer.
#[test]
fn a_codex_resume_verifies_and_only_codex_is_observed() {
    assert_eq!(AGENTS, ["claude", "kimi", "codex"]);
    assert_eq!(OBSERVED, ["codex"]);
    let agents = super::super::creation_launch::AGENTS
        .into_iter()
        .chain(["dsh", "opencode"]);
    for agent in agents {
        let mut copy = intent();
        copy.session = format!("{agent}/private-resume");
        let verified = copy.clone().verify();
        assert_eq!(observed(&copy.session), agent == "codex", "{agent}");
        if !AGENTS.contains(&agent) {
            assert_eq!(verified.err(), Some(AGENT_REFUSED), "{agent}");
            let mut plan = intent().verify().unwrap().plan;
            plan.session = copy.session.clone();
            assert!(!plan.valid(), "{agent}");
            continue;
        }
        let plan = verified.unwrap_or_else(|e| panic!("{agent}: {e}")).plan;
        assert_eq!(plan.observed(), agent == "codex", "{agent}");
        assert_eq!(
            plan.observe_args(),
            ["send", copy.session.as_str(), "--observe", "--json"]
        );
    }
    assert!(OBSERVE_MARKER.ends_with(": desktop-deferred-session-resume-codex-v1"));
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
            Box::new(|a| a.code = "session_resume_unavailable".into()),
        ),
        ("not started", Box::new(|a| a.code = NOT_STARTED.into())),
        (
            "another kind",
            Box::new(|a| a.code = "deferred_send_not_admitted".into()),
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
