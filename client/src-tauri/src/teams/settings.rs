//! Renderer-only admission. CLI inputs, stored requests and peers use their
//! existing types and defaults; this guard changes none of those contracts.
use serde_json::Value;
use zerus_client_core::teams::action::{canonical_uuid, TeamCall, TeamOutcome};

pub(super) fn checked_call(value: Value) -> Result<TeamCall, TeamOutcome> {
    let request_id = value["request_id"]
        .as_str()
        .filter(|id| canonical_uuid(id))
        .unwrap_or_default()
        .to_owned();
    let failed = |detail: &str| TeamOutcome::Failed {
        detail: detail.into(),
        code: "invalid_state".into(),
        request_id: request_id.clone(),
    };
    if value["action"]["kind"].as_str() == Some("settings")
        && zerus_client_core::teams::settings::complete(&value["action"]["settings"]).is_none()
    {
        return Err(failed(
            "The complete team settings are required. Nothing was sent.",
        ));
    }
    serde_json::from_value(value)
        .map_err(|_| failed("The team action could not be read. Nothing was sent."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn written_call() -> Value {
        let status: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../core/src/teams/fixtures/cli-status.json"
        )))
        .unwrap();
        json!({"team":status["team"]["id"], "request_id":"00000000-0000-4000-8000-000000000001", "expected_revision":16, "action":{"kind":"settings", "settings":status["team"]["settings"]}})
    }

    #[test]
    fn incomplete_settings_are_refused_at_the_tauri_boundary_before_an_intent_or_child() {
        for group in ["delegations", "caps", "timers"] {
            let original = written_call();
            let keys: Vec<_> = original["action"]["settings"][group]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            for key in keys {
                let mut partial = original.clone();
                partial["action"]["settings"][group]
                    .as_object_mut()
                    .unwrap()
                    .remove(&key);
                assert!(
                    matches!(checked_call(partial), Err(TeamOutcome::Failed { detail, request_id, .. }) if detail == "The complete team settings are required. Nothing was sent." && request_id == "00000000-0000-4000-8000-000000000001")
                );
            }
        }
        let mut partial = written_call();
        partial["action"]["settings"]
            .as_object_mut()
            .unwrap()
            .remove("delegations");
        assert!(checked_call(partial).is_err());
    }

    #[test]
    fn complete_calls_keep_the_cli_action_and_replay_identity_exactly() {
        let original = written_call();
        let ordinary: TeamCall = serde_json::from_value(original.clone()).unwrap();
        let checked = checked_call(original).unwrap();
        assert_eq!(
            serde_json::to_value(&checked).unwrap(),
            serde_json::to_value(ordinary).unwrap()
        );
        let invocation = zerus_client_core::teams::action::prepare(&checked)
            .unwrap()
            .0;
        let sent: Value = serde_json::from_slice(invocation.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(sent["settings"], written_call()["action"]["settings"]);
        assert_eq!(sent["request_id"], checked.request_id);
        assert_eq!(sent["expected_revision"], 16);
    }

    #[test]
    fn other_desktop_actions_keep_their_shape() {
        let mut call = written_call();
        call["action"] = json!({"kind":"pause"});
        assert!(checked_call(call).is_ok());
    }
}
