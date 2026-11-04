// The deferred-input checkpoint: pure identity data, never a permission.
use super::*;

fn intent(kind: Kind) -> Intent {
    Intent {
        kind,
        draft_revision: 7,
        message_sha256: message_sha256(
            "hello",
            [("notes.txt", "text/plain", "[notes.txt]", b"abc".as_slice())],
        ),
        run_id: "run-1".into(),
        conversation_id: "conversation-1".into(),
        question: (kind == Kind::Answer).then(|| Question {
            id: "question-1".into(),
            identity_sha256: identity_sha256(&json!({"id": "question-1"})),
        }),
        start: (kind == Kind::ResumeToSend).then_some(Start::Resume),
        retry_of: None,
        consent: false,
    }
}

#[test]
fn each_kind_reserves_a_fresh_request_id_for_its_own_receipt_family() {
    let families = [
        (Kind::Send, "input_receipts"),
        (Kind::Answer, "question_receipts"),
        (Kind::CompactToSend, "compact_receipts"),
        (Kind::ResumeToSend, "resume_input"),
    ];
    for (kind, family) in families {
        let first = Checkpoint::reserve(intent(kind));
        let second = Checkpoint::reserve(intent(kind));
        assert!(first.valid(), "{kind:?}");
        assert_eq!(first.receipts, family);
        assert_eq!(first.version, VERSION);
        assert!(canonical_uuid(&first.request_id));
        assert_ne!(first.request_id, second.request_id, "one id per dispatch");
    }
}

#[test]
fn the_record_round_trips_and_refuses_unknown_fields() {
    let checkpoint = Checkpoint::reserve(intent(Kind::Answer));
    let bytes = serde_json::to_vec(&checkpoint).unwrap();
    let back: Checkpoint = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(back, checkpoint);
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["permission"] = json!(true);
    assert!(serde_json::from_value::<Checkpoint>(value).is_err());
}

#[test]
fn every_inconsistent_field_is_invalid() {
    let good = Checkpoint::reserve(intent(Kind::Send));
    type Change = Box<dyn Fn(&mut Checkpoint)>;
    let cases: Vec<(&str, Change)> = vec![
        ("version", Box::new(|c| c.version = 2)),
        (
            "request id",
            Box::new(|c| c.request_id = "not-a-uuid".into()),
        ),
        (
            "receipts",
            Box::new(|c| c.receipts = "question_receipts".into()),
        ),
        (
            "message digest",
            Box::new(|c| c.message_sha256 = "abc".into()),
        ),
        ("run", Box::new(|c| c.run_id = "x".repeat(MAX_ID + 1))),
        (
            "conversation",
            Box::new(|c| c.conversation_id = "a\nb".into()),
        ),
        (
            "empty retry",
            Box::new(|c| c.retry_of = Some(String::new())),
        ),
        (
            "question on a send",
            Box::new(|c| c.question = intent(Kind::Answer).question),
        ),
        (
            "start on a send",
            Box::new(|c| c.start = Some(Start::Fresh)),
        ),
    ];
    for (name, change) in cases {
        let mut bad = good.clone();
        change(&mut bad);
        assert!(!bad.valid(), "{name}");
    }
    let mut answer = Checkpoint::reserve(intent(Kind::Answer));
    answer.question = None;
    assert!(!answer.valid(), "an answer names its question");
    let mut resume = Checkpoint::reserve(intent(Kind::ResumeToSend));
    resume.start = None;
    assert!(!resume.valid(), "a resume-then-send names its start");
}

#[test]
fn the_message_digest_binds_text_and_every_attachment_in_order() {
    let a = ("a.txt", "text/plain", "[a.txt]", b"one".as_slice());
    let b = ("b.txt", "text/plain", "[b.txt]", b"two".as_slice());
    let base = message_sha256("hi", [a, b]);
    assert_eq!(base, message_sha256("hi", [a, b]));
    assert_ne!(base, message_sha256("hi!", [a, b]));
    assert_ne!(base, message_sha256("hi", [b, a]));
    assert_ne!(base, message_sha256("hi", [a]));
    assert_ne!(
        base,
        message_sha256(
            "hi",
            [a, ("b.txt", "text/plain", "[b.txt]", b"tw0".as_slice())]
        )
    );
}
