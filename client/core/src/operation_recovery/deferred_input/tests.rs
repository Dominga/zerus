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

/// zerus-5wmh.66.4.8: a version 2 compact-to-send reserves its send with its
/// compaction; version 2 is exactly that, with a distinct canonical send ID.
#[test]
fn a_compact_to_send_reserves_its_send_with_its_compaction() {
    for kind in [Kind::Send, Kind::Answer, Kind::ResumeToSend] {
        assert!(Checkpoint::reserve_compact_to_send(intent(kind)).is_none());
    }
    let paired = Checkpoint::reserve_compact_to_send(intent(Kind::CompactToSend)).unwrap();
    assert_eq!(paired.version, COMPACT_TO_SEND_VERSION);
    assert_eq!(paired.receipts, "compact_receipts");
    let send = paired.send_request_id.clone().unwrap();
    assert!(canonical_uuid(&send) && send != paired.request_id);
    assert!(paired.valid());
    // Round trip; a version 1 record never carries the key.
    let text = serde_json::to_string(&paired).unwrap();
    assert_eq!(serde_json::from_str::<Checkpoint>(&text).unwrap(), paired);
    let single = Checkpoint::reserve(intent(Kind::CompactToSend));
    assert!(!serde_json::to_string(&single)
        .unwrap()
        .contains("send_request_id"));
    // Every inconsistency is invalid.
    let invalid = |change: &dyn Fn(&mut Checkpoint)| {
        let mut copy = paired.clone();
        change(&mut copy);
        !copy.valid()
    };
    assert!(invalid(&|c| c.send_request_id = None));
    assert!(invalid(&|c| c.send_request_id = Some(c.request_id.clone())));
    assert!(invalid(&|c| c.send_request_id = Some("not-a-uuid".into())));
    assert!(invalid(&|c| c.send_request_id = Some(send.to_uppercase())));
    assert!(invalid(&|c| c.version = VERSION));
    assert!(invalid(&|c| c.version = 3));
    assert!(invalid(&|c| {
        c.kind = Kind::Send;
        c.receipts = Kind::Send.receipts().into();
    }));
    let mut single_with_send = single.clone();
    single_with_send.send_request_id = Some(send);
    assert!(!single_with_send.valid());
}

/// zerus-5wmh.66.4.9: a version 3 resume-then-send reserves its send with
/// its receipted resume. Version 3 is exactly a resume (never a fresh start
/// or a resume with compaction), its family `action_receipts`, with a
/// distinct canonical send ID; version 1 keeps the direct `resume_input`.
#[test]
fn a_session_resume_reserves_its_send_with_its_resume() {
    for kind in [Kind::Send, Kind::Answer, Kind::CompactToSend] {
        assert!(Checkpoint::reserve_session_resume(intent(kind)).is_none());
    }
    for start in [Start::Fresh, Start::ResumeCompact] {
        let mut other = intent(Kind::ResumeToSend);
        other.start = Some(start);
        assert!(Checkpoint::reserve_session_resume(other).is_none());
    }
    let paired = Checkpoint::reserve_session_resume(intent(Kind::ResumeToSend)).unwrap();
    assert_eq!(paired.version, SESSION_RESUME_VERSION);
    assert_eq!(paired.receipts, ACTION_RECEIPTS);
    assert_eq!(paired.receipt_family(), ACTION_RECEIPTS);
    assert_eq!(paired.start, Some(Start::Resume));
    let send = paired.send_request_id.clone().unwrap();
    assert!(canonical_uuid(&send) && send != paired.request_id);
    assert!(paired.valid());
    let text = serde_json::to_string(&paired).unwrap();
    assert_eq!(serde_json::from_str::<Checkpoint>(&text).unwrap(), paired);
    // Version 1 of the same kind keeps the direct path's family.
    let direct = Checkpoint::reserve(intent(Kind::ResumeToSend));
    assert_eq!(direct.receipt_family(), "resume_input");
    assert!(direct.valid());
    let invalid = |change: &dyn Fn(&mut Checkpoint)| {
        let mut copy = paired.clone();
        change(&mut copy);
        !copy.valid()
    };
    assert!(invalid(&|c| c.send_request_id = None));
    assert!(invalid(&|c| c.send_request_id = Some(c.request_id.clone())));
    assert!(invalid(&|c| c.send_request_id = Some("not-a-uuid".into())));
    assert!(invalid(&|c| c.send_request_id = Some(send.to_uppercase())));
    assert!(invalid(&|c| c.receipts = "resume_input".into()));
    assert!(invalid(&|c| c.version = VERSION));
    assert!(invalid(&|c| c.version = COMPACT_TO_SEND_VERSION));
    assert!(invalid(&|c| c.start = Some(Start::Fresh)));
    assert!(invalid(&|c| c.start = Some(Start::ResumeCompact)));
    assert!(invalid(&|c| c.start = None));
    assert!(invalid(&|c| {
        c.kind = Kind::CompactToSend;
        c.start = None;
    }));
    // A version 1 record with action_receipts, or with a send, is invalid.
    let mut direct_action = direct.clone();
    direct_action.receipts = ACTION_RECEIPTS.into();
    assert!(!direct_action.valid());
    let mut direct_with_send = direct;
    direct_with_send.send_request_id = Some(send);
    assert!(!direct_with_send.valid());
}
