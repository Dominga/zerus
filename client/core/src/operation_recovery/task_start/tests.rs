use super::*;
use serde_json::json;

const TASK: &str = "11111111-1111-4111-8111-111111111111";
const REQUEST: &str = "22222222-2222-4222-8222-222222222222";
const OTHER: &str = "33333333-3333-4333-8333-333333333333";
const PRIVATE: &str = "PRIVATE ORIGINAL PROMPT AND HOST ERROR";

struct Fixture {
    record: Value,
    bytes: Vec<u8>,
    input: Vec<u8>,
    binding: Binding,
}
impl Fixture {
    fn new() -> Self {
        let record = json!({"schema":1,"id":REQUEST,"phase":"review","error":PRIVATE,
            "intent":{"plan":{"schema":1,"id":REQUEST,"project":"example","task":TASK,
                "stage_at":123,"target":{"kind":"any"},"revision":"original-revision",
                "selection":{"host":"original-peer","name":"codex/example/worker","session_id":"run_id:worker","run_id":"worker","conversation_id":"original-conversation"},
                "session":{"machine":"original-machine","session_id":"run_id:worker","session":"codex/example/worker"},
                "request":{"request_id":REQUEST,"expected_run_id":"worker","expected_conversation_id":"original-conversation","text":PRIVATE,"attachments":[]},
                "future_private":{"keep":PRIVATE}}},"future_private":PRIVATE});
        let binding = Binding {
            key: Key {
                kind: Kind::TaskStartReconcile,
                schema: 1,
                configuration: "a".repeat(64),
                project: "example".into(),
                task: TASK.into(),
                request_id: REQUEST.into(),
            },
            marker_id: REQUEST.into(),
            source_record_sha256: String::new(),
            ui_snapshot_sha256: String::new(),
            task_start_reconcile_input_v1_sha256: String::new(),
            executor: Executor {
                machine: "original-machine".into(),
                session_id: "run_id:worker".into(),
                run_id: "worker".into(),
                conversation_id: "original-conversation".into(),
            },
        };
        let mut f = Self {
            record,
            bytes: Vec::new(),
            input: Vec::new(),
            binding,
        };
        f.refresh();
        f
    }
    fn refresh(&mut self) {
        self.bytes = serde_json::to_vec_pretty(&self.record).unwrap();
        self.input = b" \n".to_vec();
        self.input
            .extend(serde_json::to_vec_pretty(&self.record["intent"]["plan"]).unwrap());
        self.input.extend(b"\n ");
        self.binding.source_record_sha256 = sha256(&self.bytes);
        self.binding.ui_snapshot_sha256 = outbox::snapshot(&self.record);
        self.binding.task_start_reconcile_input_v1_sha256 = sha256(&self.input);
    }
    fn original(&self) -> Original {
        Original::verify(self.binding.clone(), &self.bytes, &self.input).unwrap()
    }
    fn reply(&self, state: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"schema":1,"id":REQUEST,"project":"example","uuid":TASK,"state":state,
            "error":PRIVATE,"task":{"uuid":TASK,"project":"example","text":PRIVATE,"log":[PRIVATE]}})).unwrap()
    }
}

fn review(decision: Decision, expected: Review) {
    assert!(matches!(decision,Decision::NeedsReview(reason) if reason==expected));
}

#[test]
fn proposal_is_only_the_unchanged_original_recover_only_request() {
    let f = Fixture::new();
    let original = f.original();
    review(original.step(Input::Review, false), Review::NoLookupResult);
    let Decision::Proposed(proposal) = original.step(Input::LookupRequested, false) else {
        panic!("missing proposal")
    };
    assert_eq!(proposal.input(), f.input);
    assert_eq!(proposal.binding(), &f.binding);
    assert_eq!(
        proposal.args(),
        [
            "task",
            "start-deliver",
            TASK,
            "--project",
            "example",
            "--json",
            "--recover-only"
        ]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(proposal.input()).unwrap(),
        f.record["intent"]["plan"]
    );
    assert!(!format!("{proposal:?}").contains(PRIVATE));
}

#[test]
fn exact_source_record_ui_snapshot_and_input_digests_are_distinct() {
    let f = Fixture::new();
    assert_ne!(f.binding.source_record_sha256, f.binding.ui_snapshot_sha256);
    assert_ne!(
        f.binding.task_start_reconcile_input_v1_sha256,
        sha256(f.record["intent"]["plan"].to_string().as_bytes())
    );
    let mut changed = f.bytes.clone();
    changed.push(b' ');
    assert!(matches!(
        Original::verify(f.binding.clone(), &changed, &f.input),
        Err(Review::IdentityChanged)
    ));
    let mut changed = f.input.clone();
    changed.push(b' ');
    assert_eq!(
        serde_json::from_slice::<Value>(&changed).unwrap(),
        f.record["intent"]["plan"]
    );
    assert!(matches!(
        Original::verify(f.binding.clone(), &f.bytes, &changed),
        Err(Review::InputChanged)
    ));
    let mut wrong = f.binding.clone();
    wrong.ui_snapshot_sha256 = wrong.source_record_sha256.clone();
    assert!(matches!(
        Original::verify(wrong, &f.bytes, &f.input),
        Err(Review::IdentityChanged)
    ));
}

#[test]
fn a_new_digest_cannot_hide_a_changed_plan_or_consent() {
    let mut f = Fixture::new();
    let mut input = f.record["intent"]["plan"].clone();
    input["request"]["text"] = json!("changed consent");
    let input = serde_json::to_vec(&input).unwrap();
    f.binding.task_start_reconcile_input_v1_sha256 = sha256(&input);
    assert!(matches!(
        Original::verify(f.binding.clone(), &f.bytes, &input),
        Err(Review::InputChanged)
    ));
}

#[test]
fn every_original_executor_identity_and_marker_is_required() {
    let f = Fixture::new();
    for field in 0..8 {
        let mut wrong = f.binding.clone();
        match field {
            0 => wrong.executor.machine = "remapped-machine".into(),
            1 => wrong.executor.session_id = "run_id:replacement".into(),
            2 => wrong.executor.run_id = "replacement-run".into(),
            3 => wrong.executor.conversation_id = "replacement-conversation".into(),
            4 => wrong.marker_id = OTHER.into(),
            5 => wrong.key.project = "another-project".into(),
            6 => wrong.key.task = OTHER.into(),
            _ => wrong.key.request_id = OTHER.into(),
        }
        assert!(Original::verify(wrong, &f.bytes, &f.input).is_err());
    }
    for field in ["expected_run_id", "expected_conversation_id"] {
        let mut f = Fixture::new();
        f.record["intent"]["plan"]["request"][field] = json!("replacement");
        f.refresh();
        assert!(matches!(
            Original::verify(f.binding.clone(), &f.bytes, &f.input),
            Err(Review::IdentityChanged)
        ));
    }
}

#[test]
fn future_new_session_or_incomplete_plans_remain_reviewable() {
    for kind in ["new_session", "future"] {
        let mut f = Fixture::new();
        f.record["intent"]["plan"]["target"]["kind"] = json!(kind);
        f.refresh();
        assert!(matches!(
            Original::verify(f.binding.clone(), &f.bytes, &f.input),
            Err(Review::UnsupportedPlan)
        ));
    }
    for field in ["stage_at", "revision", "selection"] {
        let mut f = Fixture::new();
        f.record["intent"]["plan"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        f.refresh();
        assert!(Original::verify(f.binding.clone(), &f.bytes, &f.input).is_err());
    }
    let mut f = Fixture::new();
    f.record["schema"] = json!(2);
    f.refresh();
    assert!(matches!(
        Original::verify(f.binding.clone(), &f.bytes, &f.input),
        Err(Review::InvalidRecord)
    ));
}

#[test]
fn historical_started_or_released_records_never_prove_current_settlement() {
    for phase in ["started", "released"] {
        let mut f = Fixture::new();
        f.record["phase"] = json!(phase);
        f.refresh();
        let original = f.original();
        review(
            original.step(Input::Review, false),
            Review::HistoricalTerminal,
        );
        review(
            original.step(Input::LookupRequested, false),
            Review::HistoricalTerminal,
        );
        review(
            original.step(Input::Reply(&f.reply("submitted")), false),
            Review::HistoricalTerminal,
        );
    }
}

#[test]
fn unknown_restart_and_cancel_never_propose_a_replay() {
    let f = Fixture::new();
    let binding: Binding =
        serde_json::from_slice(&serde_json::to_vec(&f.binding).unwrap()).unwrap();
    let restarted = Original::verify(binding, &f.bytes, &f.input).unwrap();
    review(
        restarted.step(Input::Unknown, false),
        Review::UnknownOutcome,
    );
    review(restarted.step(Input::Review, false), Review::NoLookupResult);
    review(
        restarted.step(Input::LookupRequested, true),
        Review::Cancelled,
    );
    review(
        restarted.step(Input::Reply(&f.reply("submitted")), true),
        Review::Cancelled,
    );
    // A later separately admitted lookup may only propose the same original
    // bytes. The immutable unknown attempt is not rewritten by this decision.
    let Decision::Proposed(next) = restarted.step(Input::LookupRequested, false) else {
        panic!("missing proposal")
    };
    assert_eq!(next.input(), f.input);
    assert_eq!(next.binding().key.request_id, REQUEST);
}

#[test]
fn only_exact_submitted_receipt_metadata_proposes_settlement() {
    let f = Fixture::new();
    let original = f.original();
    let outcome = original.step(Input::Reply(&f.reply("submitted")), false);
    let debug = format!("{outcome:?}");
    assert!(!debug.contains(PRIVATE));
    let Decision::SettledSubmitted { binding, receipt } = outcome else {
        panic!("expected submitted evidence")
    };
    assert_eq!(binding, f.binding);
    assert_eq!(
        serde_json::to_value(&receipt).unwrap(),
        json!({"schema":1,"id":REQUEST,"project":"example","uuid":TASK,"state":"submitted"})
    );
    let encoded = serde_json::to_vec(&receipt).unwrap();
    assert!(encoded.len() < 16 << 10);
    assert!(!String::from_utf8(encoded).unwrap().contains(PRIVATE));
    for state in ["uncertain", "refused", "released", "running", ""] {
        review(
            original.step(Input::Reply(&f.reply(state)), false),
            Review::UnknownOutcome,
        );
    }
}

#[test]
fn mismatched_or_private_error_replies_cannot_settle_an_original_attempt() {
    let f = Fixture::new();
    let original = f.original();
    for field in ["schema", "id", "project", "uuid", "task"] {
        let mut reply: Value = serde_json::from_slice(&f.reply("submitted")).unwrap();
        reply[field] = json!("wrong");
        review(
            original.step(Input::Reply(&serde_json::to_vec(&reply).unwrap()), false),
            Review::InvalidReply,
        );
    }
    for field in ["project", "uuid"] {
        let mut reply: Value = serde_json::from_slice(&f.reply("submitted")).unwrap();
        reply["task"][field] = json!("wrong");
        review(
            original.step(Input::Reply(&serde_json::to_vec(&reply).unwrap()), false),
            Review::InvalidReply,
        );
    }
    review(
        original.step(Input::Reply(PRIVATE.as_bytes()), false),
        Review::InvalidReply,
    );
}

#[test]
fn hard_byte_and_identity_bounds_refuse_without_truncation_or_replacement() {
    let f = Fixture::new();
    assert!(matches!(
        Original::verify(f.binding.clone(), &vec![0; PRIVATE_BYTES + 1], &f.input),
        Err(Review::TooLarge)
    ));
    assert!(matches!(
        Original::verify(f.binding.clone(), &f.bytes, &vec![0; PRIVATE_BYTES + 1]),
        Err(Review::TooLarge)
    ));
    assert!(matches!(
        Original::verify(f.binding.clone(), &f.bytes, b""),
        Err(Review::MissingRetainedInput)
    ));
    review(
        f.original()
            .step(Input::Reply(&vec![0; REPLY_BYTES + 1]), false),
        Review::TooLarge,
    );
    let mut wrong = f.binding.clone();
    wrong.executor.machine = "x".repeat(IDENTITY_BYTES + 1);
    assert!(matches!(
        Original::verify(wrong, &f.bytes, &f.input),
        Err(Review::IdentityChanged)
    ));
    let mut wrong = f.binding.clone();
    wrong.key.project = "x".repeat(129);
    assert!(matches!(
        Original::verify(wrong, &f.bytes, &f.input),
        Err(Review::IdentityChanged)
    ));
}
