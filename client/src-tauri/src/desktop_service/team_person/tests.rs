//! Admission/recovery policy models; no native dialog gesture is fabricated.
use super::*;
use zerus_client_core::{desktop::creation_clone::Scope, teams::action::TeamAction};

fn call() -> TeamCall {
    TeamCall {
        team: "00000000-0000-4000-8000-00000000beef".into(),
        team_name: "Untrusted renderer caption".into(),
        action: TeamAction::Decide {
            card: 1,
            decision: "acknowledged".into(),
        },
        expected_revision: Some(2),
        confirmed: true,
        request_id: uuid::Uuid::new_v4().to_string(),
    }
}
fn review(call: &TeamCall) -> wire::Review {
    wire::Review {
        intent: wire::Intent {
            scope: Scope {
                configuration: "a".repeat(64),
                service_epoch: uuid::Uuid::new_v4().to_string(),
            },
            machine: uuid::Uuid::new_v4().to_string(),
            store: "b".repeat(64),
            facts: "c".repeat(64),
            plan: wire::Plan::from_call(call).unwrap(),
        },
        team_name: "Canonical team caption".into(),
        card_kind: "launch_refused".into(),
        card_text: "No account available".into(),
        card_parameters: Vec::new(),
        card_notes: Vec::new(),
    }
}
#[test]
fn renderer_confirmation_cannot_replace_native_acceptance() {
    let call = call();
    assert!(call.confirmed);
    assert!(Confirmed::after_dialog(review(&call), false).is_none());
    let review = review(&call);
    let expected = review.intent.clone();
    assert_eq!(Confirmed::after_dialog(review, true).unwrap().0, expected);
}

#[test]
fn checking_missing_or_foreign_evidence_never_becomes_a_fresh_review() {
    let decisions = Decisions::default();
    let intent = review(&call()).intent;
    assert!(matches!(decisions.path(&intent.plan, true), Path::Unknown));
    assert!(decisions.reserve(&intent));
    assert!(
        matches!(decisions.path(&intent.plan, true), Path::Read(original) if *original == intent)
    );
    let mut another = intent.plan.clone();
    another.card += 1;
    assert!(matches!(decisions.path(&another, true), Path::Unknown));
    decisions.dismiss(&intent.plan.request_id);
    assert!(matches!(decisions.path(&intent.plan, true), Path::Unknown));
    assert!(matches!(decisions.path(&intent.plan, false), Path::Review));
}
#[test]
fn unresolved_originals_are_bounded_and_never_replaced_or_adopted() {
    let decisions = Decisions::default();
    for _ in 0..zerus_client_core::teams::action::intents::INTENTS_KEPT {
        assert!(decisions.reserve(&review(&call()).intent));
    }
    assert!(!decisions.reserve(&review(&call()).intent));
    let first = decisions
        .originals
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert!(!decisions.reserve(&first));
    let mut different = first.plan.clone();
    different.card += 1;
    assert!(decisions.original(&different).is_none());
    assert_eq!(decisions.original(&first.plan), Some(first.clone()));
    decisions.dismiss(&first.plan.request_id);
    assert!(decisions.original(&first.plan).is_none());
    assert!(decisions.reserve(&review(&call()).intent));
}
#[test]
fn exact_original_receipt_only_settles_a_decision_and_storage_failure_stays_unknown() {
    let call = call();
    let intent = review(&call).intent;
    let reply = json!({"intent":intent,"reply":{"ok":true,"request_id":call.request_id,"team":call.team,"revision":3,"actor":{"kind":"person","person":"local","origin":"desktop_ui"}}});
    assert!(matches!(
        outcome(&call, &intent, &serde_json::to_vec(&reply).unwrap()),
        TeamOutcome::Done { .. }
    ));
    for field in [
        "intent", "request", "revision", "actor", "storage", "missing",
    ] {
        let mut reply = reply.clone();
        match field {
            "intent" => reply["intent"]["facts"] = json!("d".repeat(64)),
            "request" => reply["reply"]["request_id"] = json!(uuid::Uuid::new_v4().to_string()),
            "revision" => reply["reply"]["revision"] = json!(9),
            "actor" => reply["reply"]["actor"] = json!({"kind":"service"}),
            "storage" => {
                reply["reply"] = json!({"ok":false,"request_id":call.request_id,"code":"storage_failed","message":"Unconfirmed"})
            }
            "missing" => reply = json!({"ok":false,"code":"team_person_evidence_unavailable"}),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                outcome(&call, &intent, &serde_json::to_vec(&reply).unwrap()),
                TeamOutcome::Uncertain { .. }
            ),
            "{field}"
        );
    }
}
