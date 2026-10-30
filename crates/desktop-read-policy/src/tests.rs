use super::*;
use serde_json::json;

fn mark(reply: Option<&str>, signature: Option<&str>) -> ReadMark {
    ReadMark {
        computer: "peer".into(),
        name: "codex/example/session".into(),
        run_id: "run-1".into(),
        conversation_id: "conversation-1".into(),
        reply_id: reply.map(str::to_owned),
        attention_signature: signature.map(str::to_owned),
    }
}

#[test]
fn merging_preserves_unspecified_parts_and_explicit_empty_values() {
    let old = mark(Some("reply-1"), Some("signature-1"));
    assert_eq!(ReadMark::merged(&old, mark(None, None)), old);
    assert_eq!(
        ReadMark::merged(&old, mark(Some("reply-2"), None)),
        mark(Some("reply-2"), Some("signature-1"))
    );
    assert_eq!(
        ReadMark::merged(&old, mark(None, Some("signature-2"))),
        mark(Some("reply-1"), Some("signature-2"))
    );
    assert_eq!(
        ReadMark::merged(&old, mark(Some(""), Some(""))),
        mark(Some(""), Some(""))
    );
}

#[test]
fn another_run_or_conversation_replaces_instead_of_merging() {
    let old = mark(Some("reply-1"), Some("signature-1"));
    for field in ["run", "conversation"] {
        let mut next = mark(None, Some("signature-2"));
        if field == "run" {
            next.run_id = "run-2".into();
        } else {
            next.conversation_id = "conversation-2".into();
        }
        assert_eq!(ReadMark::merged(&old, next.clone()), next);
    }
}

#[test]
fn settlement_waits_for_each_exact_still_current_part() {
    let m = mark(Some("reply-1"), Some("signature-1"));
    for (read_reply, read_signature, expected) in [
        (None, None, false),
        (Some("reply-1"), None, false),
        (None, Some("signature-1"), false),
        (Some("reply-1"), Some("signature-1"), true),
        (Some("reply-2"), Some("signature-1"), false),
    ] {
        assert_eq!(
            m.settled(
                Some("run-1"),
                Some("conversation-1"),
                Some("reply-1"),
                Some("signature-1"),
                read_reply,
                read_signature
            ),
            expected
        );
    }
    assert!(!m.settled(
        Some("run-1"),
        Some("conversation-1"),
        Some("reply-2"),
        Some("signature-1"),
        None,
        None
    ));
    assert!(m.settled(
        Some("run-1"),
        Some("conversation-1"),
        Some("reply-2"),
        Some("signature-2"),
        None,
        None
    ));
    // A missing current part differs from the exact marked value, as before.
    assert!(m.settled(
        Some("run-1"),
        Some("conversation-1"),
        None,
        None,
        None,
        None
    ));
}

#[test]
fn identity_changes_and_empty_parts_settle_without_a_delivery_receipt() {
    let m = mark(Some("reply-1"), Some("signature-1"));
    for (run, conversation) in [
        (None, Some("conversation-1")),
        (Some("run-1"), None),
        (Some("run-2"), Some("conversation-1")),
        (Some("run-1"), Some("conversation-2")),
    ] {
        assert!(m.settled(
            run,
            conversation,
            Some("reply-1"),
            Some("signature-1"),
            None,
            None
        ));
    }
    for reply in [None, Some("")] {
        for signature in [None, Some("")] {
            let empty = mark(reply, signature);
            assert!(empty.is_empty());
            assert!(empty.settled(
                Some("run-1"),
                Some("conversation-1"),
                None,
                None,
                None,
                None
            ));
        }
    }
    assert!(!mark(Some("reply-1"), None).is_empty());
    assert!(!mark(None, Some("signature-1")).is_empty());
}

#[test]
fn receipts_require_explicit_application_of_all_marked_parts() {
    let both = mark(Some("reply-1"), Some("signature-1"));
    for receipt in [
        json!(null),
        json!({"status":"not_found"}),
        json!({"applied":{"reply_id":true}}),
        json!({"applied":{"reply_id":true,"attention_signature":"true"}}),
    ] {
        assert!(!applied_all(&both, &receipt));
    }
    assert!(applied_all(
        &both,
        &json!({"applied":{"reply_id":true,"attention_signature":true}})
    ));
    assert!(applied_all(
        &mark(Some("reply-1"), None),
        &json!({"applied":{"reply_id":true}})
    ));
    assert!(applied_all(
        &mark(None, Some("signature-1")),
        &json!({"applied":{"attention_signature":true}})
    ));
    assert!(applied_all(&mark(Some(""), None), &json!(null)));
}

#[test]
fn failure_backoff_caps_then_success_resets_it() {
    let mut retry = Retry::default();
    for expected in [10_000, 20_000, 40_000, 80_000, 160_000, 300_000, 300_000] {
        assert_eq!(retry.finished(false, false, 123) - 123, expected);
    }
    assert_eq!(retry.finished(true, true, 123), 120_123);
    assert_eq!(retry, Retry::default());
    assert_eq!(retry.finished(false, false, 0), 10_000);
}

#[test]
fn partial_resends_cap_independently_of_failures() {
    let mut retry = Retry::default();
    for expected in [120_000, 240_000, 480_000, 960_000, 960_000] {
        assert_eq!(retry.finished(true, false, 0), expected);
        assert_eq!(retry.finished(false, false, 0), 10_000);
    }
    assert_eq!(retry.finished(true, true, 0), 120_000);
    assert_eq!(retry.finished(true, false, 0), 120_000);
}

#[test]
fn extreme_counters_and_clocks_saturate_without_wrapping() {
    let mut retry = Retry {
        failures: u32::MAX,
        unapplied: u32::MAX,
    };
    assert_eq!(retry.finished(false, false, 0), 300_000);
    assert_eq!(retry.failures, u32::MAX);
    assert_eq!(retry.finished(true, false, 0), 960_000);
    assert_eq!(retry.unapplied, u32::MAX);
    assert_eq!(retry.finished(true, true, i64::MAX - 1), i64::MAX);
    assert_eq!(retry.finished(false, false, i64::MIN), i64::MIN + 10_000);
}
