use super::*;
use crate::actions::Exit;
use serde_json::{json, Value};

fn call() -> TeamCall {
    serde_json::from_value(json!({
        "team":"00000000-0000-4000-8000-00000000beef","team_name":"Planned release",
        "request_id":"00000000-0000-4000-8000-00000000f001",
        "action":{"kind":"start_planning","template":"bugfix","expected_template":"shown-digest",
            "project":"example-project","name":"Planned release","goal":"Prepare a reviewable release plan.",
            "home":"00000000-0000-4000-8000-0000000000aa","integration":"branch",
            "integration_branch":"zerus/team/planned-release","base_commit":"0123456789abcdef0123456789abcdef01234567",
            "delegations":{"accept_results":false,"answer_members":false,"confirm_trust":false,"resume_exited":false,"merge_accepted":false}}
    })).unwrap()
}

#[test]
fn planned_creation_keeps_one_zero_revision_intent_and_the_confirmed_bundle_fields() {
    let call = call();
    let (wire, id) = prepare(&call).unwrap();
    assert_eq!(
        wire.args,
        ["team", "start-planning", "--template", "bugfix", "--json"]
    );
    let body: Value = serde_json::from_slice(wire.stdin.as_ref().unwrap()).unwrap();
    assert_eq!(body["expected_revision"], 0);
    assert_eq!(body["request_id"], id);
    assert_eq!(body["goal"], "Prepare a reviewable release plan.");
    assert_eq!(body["expected_template"], "shown-digest");
    assert!(!call.action.names_team());
    let mut intents = Intents::default();
    assert_eq!(intents.admit(&call), Admission::Send);
    let unknown = classify(&call, Exit::TimedOut);
    intents.settle(&call, unknown);
    assert!(matches!(
        intents.admit(&call),
        Admission::Recorded(TeamOutcome::Uncertain { .. })
    ));
    assert_eq!(intents.admit_again(&call), Admission::Send);
    assert_eq!(prepare(&call).unwrap().0.stdin, wire.stdin);
    let mut missing = serde_json::to_value(call).unwrap();
    missing["action"]["goal"] = " ".into();
    let missing: TeamCall = serde_json::from_value(missing).unwrap();
    assert!(prepare(&missing).is_err());
}
