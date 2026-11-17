//! Policy models, not evidence of a physical person's gesture.
use super::*;
use zerus_client_core::{desktop::creation_clone::Scope, upstream_teams::action::TeamAction};

pub(super) fn call() -> TeamCall {
    TeamCall {
        team: uuid::Uuid::new_v4().to_string(),
        team_name: "Untrusted caption".into(),
        action: TeamAction::RenameTeam {
            name: "New name".into(),
        },
        expected_revision: Some(2),
        confirmed: true,
        request_id: uuid::Uuid::new_v4().to_string(),
    }
}
pub(super) fn review(call: &TeamCall) -> wire::Review {
    wire::Review {
        intent: wire::Intent {
            scope: Scope {
                configuration: "a".repeat(64),
                service_epoch: uuid::Uuid::new_v4().to_string(),
            },
            machine: uuid::Uuid::new_v4().to_string(),
            store: wire::StoreBinding::Existing {
                fingerprint: "b".repeat(64),
            },
            facts: "c".repeat(64),
            request: "d".repeat(64),
            plan: wire::Plan::from_call(call).unwrap().reference().unwrap(),
        },
        team_name: "Canonical caption".into(),
        title: "Confirm team action".into(),
        subject: "Rename".into(),
        details: vec![("Name".into(), "New name".into())],
    }
}
#[test]
fn renderer_confirmed_never_substitutes_for_native_acceptance() {
    let call = call();
    assert!(call.confirmed);
    assert!(Confirmed::after_dialog(review(&call), false).is_none());
    let review = review(&call);
    let expected = review.intent.clone();
    assert_eq!(Confirmed::after_dialog(review, true).unwrap().0, expected);
}
#[test]
fn lost_results_only_read_the_exact_original_and_never_adopt() {
    let call = call();
    let plan = wire::Plan::from_call(&call).unwrap();
    let actions = Actions::default();
    assert!(matches!(actions.path(&plan, true), Path::Unknown));
    let review = review(&call);
    assert!(actions.reserve(&review.intent));
    assert!(!actions.reserve(&review.intent));
    assert!(matches!(actions.path(&plan,true),Path::Read(intent) if *intent==review.intent));
    let mut changed = plan.clone();
    changed.expected_revision += 1;
    assert!(matches!(actions.path(&changed, true), Path::Unknown));
    actions.dismiss(&plan.request_id);
    assert!(matches!(actions.path(&plan, true), Path::Unknown));
}
#[test]
fn successful_receipts_bind_identity_and_storage_failure_stays_unknown() {
    let call = call();
    let intent = review(&call).intent;
    let mut value = json!({"intent":intent,"reply":{"ok":true,"request_id":call.request_id,"team":call.team,"revision":3,"actor":{"kind":"person","person":"local","origin":"desktop_ui"}}});
    assert!(matches!(
        outcome(&call, &intent, &serde_json::to_vec(&value).unwrap()),
        TeamOutcome::Done { .. }
    ));
    value["reply"]["actor"]["origin"] = json!("cli_interim");
    assert!(matches!(
        outcome(&call, &intent, &serde_json::to_vec(&value).unwrap()),
        TeamOutcome::Uncertain { .. }
    ));
    value["reply"] = json!({"ok":false,"request_id":call.request_id,"code":"storage_failed","message":"Unknown"});
    assert!(matches!(
        outcome(&call, &intent, &serde_json::to_vec(&value).unwrap()),
        TeamOutcome::Uncertain { .. }
    ));
}
