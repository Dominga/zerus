//! Complete settings only: never fill a partial/unknown read with defaults.
use serde_json::Value;
use zerus_team_plan::Settings;

pub fn complete(value: &Value) -> Option<Settings> {
    let settings: Settings = serde_json::from_value(value.clone()).ok()?;
    // Settings uses serde defaults for legacy store reads. An editor may not
    // silently turn those defaults into a replacement of an incomplete read.
    (serde_json::to_value(&settings).ok()?.eq(value)).then_some(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upstream_teams::action::{prepare, TeamCall};
    use serde_json::json;

    fn written() -> Value {
        serde_json::from_str::<Value>(include_str!("fixtures/cli-status.json")).unwrap()["team"]
            ["settings"]
            .clone()
    }

    #[test]
    fn the_status_writer_settings_reach_the_team_view_whole() {
        let status: Value = serde_json::from_str(include_str!("fixtures/cli-status.json")).unwrap();
        let view = crate::upstream_teams::view::team(&status, &Value::Null);
        assert_eq!(
            serde_json::to_value(view.settings.unwrap()).unwrap(),
            written()
        );
    }

    #[test]
    fn settings_from_the_cli_writer_round_trip_whole() {
        let settings = written();
        assert_eq!(
            serde_json::to_value(complete(&settings).unwrap()).unwrap(),
            settings
        );
    }

    #[test]
    fn incomplete_or_unknown_settings_are_never_defaulted_for_editing() {
        let source = written();
        for group in ["delegations", "caps", "timers"] {
            let keys: Vec<_> = source[group].as_object().unwrap().keys().cloned().collect();
            for key in keys {
                let mut partial = source.clone();
                partial[group].as_object_mut().unwrap().remove(&key);
                assert!(complete(&partial).is_none(), "{group}.{key}");
            }
            let mut future = source.clone();
            future[group]["future_setting"] = json!(true);
            assert!(complete(&future).is_none());
        }
        for key in source.as_object().unwrap().keys() {
            let mut partial = source.clone();
            partial.as_object_mut().unwrap().remove(key);
            assert!(complete(&partial).is_none(), "{key}");
        }
        assert!(complete(&Value::Null).is_none());
    }

    #[test]
    fn the_native_action_validates_settings_before_building_a_write() {
        let mut action = json!({
            "team":"00000000-0000-4000-8000-00000000beef", "team_name":"Fixes",
            "request_id":"00000000-0000-4000-8000-000000000001", "expected_revision":16,
            "action":{"kind":"settings", "settings":written()}
        });
        assert!(prepare(&serde_json::from_value::<TeamCall>(action.clone()).unwrap()).is_ok());
        action["action"]["settings"]["caps"]["max_open_tasks"] = json!(501);
        assert!(prepare(&serde_json::from_value::<TeamCall>(action.clone()).unwrap()).is_err());
        action["action"]["settings"] = written();
        action["action"]["settings"]["phase_acceptance"] = json!("pipeline");
        assert!(prepare(&serde_json::from_value::<TeamCall>(action).unwrap()).is_err());
    }
}
