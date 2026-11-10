//! The question card's wire shape, shared with the webview's `CardView`
//! through a checked-in fixture: this test pins the Rust side, and the client
//! test reads the same file and renders every Text in Russian.
use serde_json::{json, Value};
use std::path::Path;
use zerus_client_core::questions::{
    card, AnswerDraft, CardContext, CardView, Question, ANSWER_NOT_STARTED, ANSWER_REFUSED,
    ANSWER_STILL_SENDING, UNCONFIRMED_BY_SERVICE,
};

fn ask(extra: Value) -> Value {
    let mut raw = json!({"question_id": "q1", "question_hash": "h", "run_id": "run-1",
        "conversation_id": "conv-1", "source": "hook", "created_at": 1.0, "can_answer": true,
        "questions": [{"id": "q_0", "question": "Which database?", "header": "Database",
            "options": [{"id": "pg", "label": "PostgreSQL", "description": "The **server** one"},
                        {"id": "lite", "label": "SQLite"}],
            "allow_other": true}]});
    if let (Some(raw), Some(extra)) = (raw.as_object_mut(), extra.as_object()) {
        raw.extend(extra.clone());
    }
    raw
}

fn samples() -> Vec<CardView> {
    let ready = CardContext {
        available: true,
        ..CardContext::default()
    };
    let draft = AnswerDraft::default();
    let required = Question::from_value(&ask(json!({})));
    let optional = Question::from_value(&ask(json!({"optional": true, "can_skip": true})));
    let approval = Question::from_value(&json!({"question_id": "ap", "question_hash": "h",
        "run_id": "run-1", "conversation_id": "conv-1", "source": "hook", "approval": true,
        "can_answer": true, "questions": [{"id": "q_0", "question": "Allow?",
            "options": [{"id": "allow", "label": "Allow"}, {"id": "deny", "label": "Deny"}]}]}));
    let multi = Question::from_value(&json!({"question_id": "oc", "question_hash": "h",
        "run_id": "run-1", "conversation_id": "conv-1", "source": "opencode_native",
        "can_answer": true, "questions": [
            {"id": "pick", "question": "Pick", "multi_select": true, "min_items": 1, "max_items": 2,
             "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]},
            {"id": "more", "question": "More", "multi_select": true, "min_items": 2,
             "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]},
            {"id": "few", "question": "Few", "multi_select": true, "max_items": 2,
             "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]},
            {"id": "any", "question": "Any", "multi_select": true,
             "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}]},
            {"id": "name", "question": "Name", "allow_other": true}]}));
    vec![
        // A required question, then one with a stored notice kept verbatim.
        card(&required, &draft, &ready),
        card(
            &required,
            &AnswerDraft {
                notice: "Answer transfer failed (exit 1).".into(),
                ..AnswerDraft::default()
            },
            &ready,
        ),
        // An optional question in a queue of three, and while its answer loads.
        card(
            &optional,
            &draft,
            &CardContext {
                queue_index: 1,
                queue_count: 3,
                pending: 3,
                ..ready.clone()
            },
        ),
        card(
            &optional,
            &draft,
            &CardContext {
                loading: true,
                ..ready.clone()
            },
        ),
        // An approval with its own buttons, and a session that cannot take answers.
        card(&approval, &draft, &ready),
        card(
            &approval,
            &draft,
            &CardContext {
                available: false,
                unavailable_reason: "This session is not running.".into(),
                ..CardContext::default()
            },
        ),
        // Multi-select bounds and a free-text item.
        card(&multi, &draft, &ready),
        // Each answer state's notice is the card's or the app's prose, which
        // the webview translates: sent and waiting, still sent by the desktop
        // service (locked like one under way), never started or refused by
        // it, unconfirmed by it, and uncertain with a detail as written.
        card(&required, &noticed(|d| d.set_answered(false)), &ready),
        card(
            &required,
            &noticed(|d| d.set_pending(ANSWER_STILL_SENDING)),
            &CardContext {
                in_flight: true,
                ..ready.clone()
            },
        ),
        card(
            &required,
            &noticed(|d| d.set_error(ANSWER_NOT_STARTED, false)),
            &ready,
        ),
        card(
            &required,
            &noticed(|d| d.set_error(ANSWER_REFUSED, false)),
            &ready,
        ),
        card(
            &required,
            &noticed(|d| d.set_unconfirmed(UNCONFIRMED_BY_SERVICE)),
            &ready,
        ),
        card(
            &required,
            &noticed(|d| d.set_error("Answer transfer failed (exit 1).", true)),
            &ready,
        ),
    ]
}

/// A draft after `change`.
fn noticed(change: impl FnOnce(&mut AnswerDraft)) -> AnswerDraft {
    let mut draft = AnswerDraft::default();
    change(&mut draft);
    draft
}

/// The card shows the answer states' notices as catalog prose: the source
/// itself, or the uncertain prefix around a detail kept as written. A
/// message the card does not know stays verbatim.
#[test]
fn the_answer_notices_are_prose_and_other_messages_stay_as_written() {
    let cards = samples();
    let status = |english: &str| {
        cards
            .iter()
            .find(|c| c.status.english() == english)
            .unwrap_or_else(|| panic!("{english}"))
            .status
            .clone()
    };
    for known in [
        "Answer sent. Waiting for the agent…",
        ANSWER_STILL_SENDING,
        ANSWER_NOT_STARTED,
        ANSWER_REFUSED,
        UNCONFIRMED_BY_SERVICE,
    ] {
        let text = status(known);
        assert_eq!(
            (text.source.as_ref(), text.args.len()),
            (known, 0),
            "{known}"
        );
    }
    let uncertain = status(
        "Delivery is not confirmed. Check Terminal before retrying. Answer transfer failed (exit 1).",
    );
    assert_eq!(
        uncertain.source,
        "Delivery is not confirmed. Check Terminal before retrying. %1"
    );
    assert_eq!(
        serde_json::to_value(&uncertain.args).unwrap(),
        json!([{"kind": "text", "value": {"source": "%1", "args": [{"kind": "verbatim",
            "value": "Answer transfer failed (exit 1)."}]}}])
    );
    assert_eq!(status("Answer transfer failed (exit 1).").source, "%1");
}

#[test]
fn the_question_card_wire_fixture_is_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/question-card-v1.json");
    let current = serde_json::to_string_pretty(&samples()).unwrap() + "\n";
    if std::env::var_os("ZERUS_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &current).unwrap();
    }
    // Task #33: compared as text. A Windows checkout with Git's autocrlf
    // holds the fixture with CRLF, and the bytes then differed although the
    // contents are equal.
    assert_eq!(
        std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n"),
        current,
        "the CardView wire shape changed: rerun with ZERUS_UPDATE_FIXTURES=1 and update CardView in client/src/types.ts"
    );
}

#[test]
fn the_fixture_covers_every_card_text_kind() {
    let cards = samples();
    let modes: std::collections::BTreeSet<_> = cards
        .iter()
        .flat_map(|c| c.forms.iter().map(|f| f.mode.source.clone()))
        .collect();
    assert_eq!(modes.len(), 5, "{modes:?}");
    assert!(cards.iter().any(|c| c.nav.is_some()));
    assert!(cards
        .iter()
        .any(|c| c.progress.is_some() && c.pending.is_some()));
    assert!(cards.iter().any(|c| c.approve.review_label.is_some()));
    assert!(cards.iter().any(|c| c.status.source == "%1"));
    let statuses: std::collections::BTreeSet<_> =
        cards.iter().map(|c| c.status.english()).collect();
    assert!(statuses.len() >= 5, "{statuses:?}");
}
