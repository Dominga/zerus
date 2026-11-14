use super::harness::*;
use crate::teams::cli::actor::Caller;
use crate::teams::ops::{HandoutOutcome, Operation, StageTaskOutcome, TaskStage};
use serde_json::{json, Value};
use zerus_team_plan::model::Decider;

pub(super) const REVIEWER_SESSION: &str = "launch_id:00000000-0000-4000-8000-000000000033";
pub(super) fn stage_harness() -> (Harness, u32) {
    configured_stage(zerus_team_plan::catalog::StageKind::Review, 0, None)
}
pub(super) fn configured_stage(
    kind: zerus_team_plan::catalog::StageKind,
    rounds: u32,
    model: Option<&str>,
) -> (Harness, u32) {
    let h = Harness::new();
    let mut team = fixture().team;
    team.bundle.pipelines[0].max_rounds = rounds;
    team.bundle.pipelines[0].stages[0].kind = kind;
    team.bundle.template.settings.land_allowed = kind == zerus_team_plan::catalog::StageKind::Land;
    if let Some(model) = model {
        team.bundle
            .roles
            .iter_mut()
            .find(|r| r.id == "reviewer")
            .unwrap()
            .model = model.into();
    }
    h.apply(
        &person(),
        Operation::Create {
            expected_template: None,
            delegations: None,
            project: team.project,
            name: team.name,
            goal: team.goal,
            home: team.home,
            bundle: Box::new(team.bundle),
            integration_branch: team.integration_branch,
            base_commit: team.base_commit,
        },
    );
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    h.bind("m3", REVIEWER_SESSION);
    h.reported(1);
    let node = h.store().stage_tasks(TEAM, 1, None).unwrap().rows[0].node;
    let id = uid(0x7000);
    h.apply(
        &service(),
        Operation::StageTaskIntent {
            node,
            effect_request_id: id.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::StageTaskOutcome {
            effect_request_id: id,
            outcome: StageTaskOutcome::Created { task: task(100) },
        },
    );
    h.observe(node, TaskStage::Todo);
    let id = uid(0x7001);
    h.apply(
        &service(),
        Operation::HandoutIntent {
            node,
            member: "m3".into(),
            effect_request_id: id.clone(),
        },
    );
    h.apply(
        &service(),
        Operation::HandoutOutcome {
            effect_request_id: id,
            outcome: HandoutOutcome::Submitted {
                receipt: "Stage delivered".into(),
            },
        },
    );
    h.observe(node, TaskStage::InProgress);
    (h, node)
}
fn input(h: &Harness, node: u32) -> Value {
    json!({"request_id":uid(0x8000),"team":TEAM,"expected_revision":h.revision(),"node":node,"generation":1,"pass":true,"text":"Reviewed every acceptance criterion."})
}
fn reviewer() -> Caller {
    Caller::Session(key(REVIEWER_SESSION))
}
#[test]
fn verdict_runs_as_the_holder_and_replays_the_standard_write_reply() {
    let (h, node) = stage_harness();
    let input = input(&h, node);
    let before = h.revision();
    let (reply, exit) = h.cli(&reviewer(), "verdict", &input.to_string());
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["revision"], before + 1);
    assert_eq!(reply["team"], TEAM);
    assert_eq!(reply["actor"]["kind"], "session");
    assert_eq!(reply["actor"]["member"], "m3");
    assert_eq!(
        h.store().node(TEAM, node).unwrap().unwrap().decided_by,
        Some(Decider::Pipeline)
    );
    assert_eq!(
        h.cli(&reviewer(), "verdict", &input.to_string()),
        (reply, 0)
    );
    assert_eq!(h.revision(), before + 1);
    let mut repeat = input;
    repeat["request_id"] = json!(uid(0x8001));
    repeat["expected_revision"] = json!(h.revision());
    let (reply, exit) = h.cli(&reviewer(), "verdict", &repeat.to_string());
    assert_eq!((code(&reply), exit), ("invalid_state", 1));
}
#[test]
fn verdict_refuses_person_lead_wrong_holder_and_other_session() {
    let (h, node) = stage_harness();
    for caller in [
        Caller::Person,
        Caller::Session(key(LEAD_SESSION)),
        Caller::Session(key(M1_SESSION)),
        Caller::Session(key(OTHER_SESSION)),
    ] {
        let (reply, exit) = h.cli(&caller, "verdict", &input(&h, node).to_string());
        assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    }
}
#[test]
fn verdict_refuses_non_sessions_before_reading_the_request() {
    let (h, _) = stage_harness();
    for caller in [Caller::Person, Caller::Session(key(OTHER_SESSION))] {
        let (reply, exit) = h.cli(&caller, "verdict", "not JSON");
        assert_eq!((code(&reply), exit), ("not_allowed", 1), "{reply}");
    }
}
#[test]
fn verdict_refuses_a_stale_generation_and_a_request_that_names_its_actor_or_operation() {
    let (h, node) = stage_harness();
    let mut stale = input(&h, node);
    stale["generation"] = json!(2);
    let (reply, exit) = h.cli(&reviewer(), "verdict", &stale.to_string());
    assert_eq!((code(&reply), exit), ("invalid_state", 1));
    for field in ["actor", "op"] {
        let mut bad = input(&h, node);
        bad["request_id"] = json!(uid(0x8010 + u32::from(field == "op")));
        bad[field] = json!("service");
        let (reply, exit) = h.cli(&reviewer(), "verdict", &bad.to_string());
        assert_eq!((code(&reply), exit), ("unknown_field", 1), "{reply}");
    }
}
#[test]
fn verdict_keeps_the_request_bound_and_preview_gate() {
    let (h, _) = stage_harness();
    let (reply, exit) = h.cli(&reviewer(), "verdict", &"x".repeat(262145));
    assert_eq!((code(&reply), exit), ("over_cap", 1));
    std::fs::remove_file(h.state().join("teams/preview")).unwrap();
    let (reply, exit) = h.cli(&reviewer(), "verdict", "");
    assert_eq!((code(&reply), exit), ("teams_inactive", 1));
}
#[test]
fn verdict_is_listed_in_dispatcher_help() {
    assert!(super::super::USAGE.contains("verdict"));
}
