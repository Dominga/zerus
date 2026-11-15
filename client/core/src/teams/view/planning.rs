//! Unknown planning state never becomes an unpaused planning team.
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlanningState {
    pub known: bool,
    pub paused: bool,
    pub goal: String,
}

pub(super) fn project(status: &Value) -> Option<PlanningState> {
    if status["team"]["status"] != "planning" {
        return None;
    }
    let value = &status["planning"];
    let goal = super::text(&status["team"], "goal", 16_384);
    let valid = value.as_object().is_some_and(|m| m.len() == 2)
        && value["awaiting_initial_approval"] == true;
    Some(match value["paused"].as_bool().filter(|_| valid) {
        Some(paused) => PlanningState {
            known: true,
            paused,
            goal,
        },
        None => PlanningState {
            known: false,
            paused: true,
            goal,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wire() -> Value {
        use crate::actions::Exit;
        use crate::teams::{TeamsFollow, TeamsModel, TeamsUpdate};
        let source: Value = serde_json::from_str(include_str!(
            "../../../../../crates/team-plan/fixtures/cli-planning.json"
        ))
        .unwrap();
        let now = std::time::Instant::now();
        let mut model = TeamsModel::default();
        let team = source["status"]["team"]["id"].as_str().unwrap();
        model.follow(
            TeamsFollow {
                team: Some(team.into()),
                ..Default::default()
            },
            now,
        );
        let mut final_view = None;
        for key in ["status", "queue"] {
            let read = model.next(now).unwrap();
            let value = crate::teams::read::decode(
                &read,
                Exit::Code {
                    code: 0,
                    stdout: serde_json::to_vec(&source[key]).unwrap(),
                    stderr: String::new(),
                },
            );
            for (_, update) in model.apply(&read, value, now) {
                if let TeamsUpdate::Team(view) = update {
                    final_view = Some(view);
                }
            }
        }
        let mut coalescer = crate::Coalescer::new();
        coalescer.push(crate::Change::teams(
            crate::ComputerId("planning-fixture".into()),
            format!("team:{team}"),
            TeamsUpdate::Team(final_view.unwrap()),
        ));
        let batch = serde_json::to_value(coalescer.take()).unwrap();
        let _: crate::Batch = serde_json::from_value(batch.clone()).unwrap();
        let catalog: Value = serde_json::from_str(include_str!(
            "../../../../../crates/team-plan/fixtures/cli-catalog.json"
        ))
        .unwrap();
        json!({"view":batch["teams"][0]["payload"],"catalog":crate::teams::catalog::form_catalog(&catalog).unwrap()["catalog"]})
    }

    fn fixture_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/ui/teams/fixtures/planning-start.json")
    }

    #[test]
    fn the_real_planning_writer_reaches_the_window_with_its_goal_and_closed_roster() {
        let projected = wire();
        assert_eq!(projected["view"]["bar"]["status"], "planning");
        assert_eq!(projected["view"]["planning"]["known"], true);
        assert_eq!(projected["view"]["planning"]["paused"], false);
        assert_eq!(
            projected["view"]["planning"]["goal"],
            "Prepare a release with reviewable acceptance criteria."
        );
        assert!(projected["view"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["state"] == "not_created"));
        assert_eq!(
            projected["view"]["bar"]["counts"]["nodes"]["in_progress"],
            0
        );
        let pinned: Value =
            serde_json::from_str(&std::fs::read_to_string(fixture_path()).unwrap()).unwrap();
        assert_eq!(projected, pinned);
    }

    #[test]
    #[ignore = "generates only the writer/core/batch planning UI fixture"]
    fn regenerate_planning_ui_fixture() {
        std::fs::write(
            fixture_path(),
            serde_json::to_string_pretty(&wire()).unwrap() + "\n",
        )
        .unwrap();
    }

    #[test]
    fn a_cli_goal_longer_than_the_bar_excerpt_stays_whole_in_planning() {
        let goal = "x".repeat(16_384);
        let result = project(&json!({"team":{"status":"planning","goal":goal},"planning":{"paused":false,"awaiting_initial_approval":true}})).unwrap();
        assert_eq!(result.goal, goal);
    }

    #[test]
    fn unknown_planning_state_is_never_read_as_unpaused() {
        for field in [
            Value::Null,
            json!({}),
            json!({"paused":false}),
            json!({"paused":"false","awaiting_initial_approval":true}),
            json!({"paused":false,"awaiting_initial_approval":true,"future":true}),
        ] {
            let view = project(&json!({"team":{"status":"planning"},"planning":field})).unwrap();
            assert!(!view.known);
            assert!(view.paused);
        }
        for paused in [false, true] {
            assert_eq!(
                project(
                    &json!({"team":{"status":"planning"},"planning":{"paused":paused,"awaiting_initial_approval":true}})
                ),
                Some(PlanningState {
                    known: true,
                    paused,
                    goal: String::new(),
                })
            );
        }
        assert!(project(&json!({"team":{"status":"running"}})).is_none());
    }
}
