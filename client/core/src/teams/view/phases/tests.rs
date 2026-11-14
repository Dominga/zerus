use super::*;
use crate::actions::Exit;
use crate::teams::{
    read::{decode, TeamRead},
    TeamsFollow, TeamsModel, TeamsUpdate,
};
use std::time::Instant;

const TEAM: &str = "00000000-0000-4000-8000-00000000beef";
fn context() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../../crates/team-plan/fixtures/cli-phase-context.json"
    ))
    .unwrap()
}
fn decoded(read: &TeamRead, value: Value) -> Result<Value, crate::teams::read::TeamError> {
    decode(
        read,
        Exit::Code {
            code: 0,
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: String::new(),
        },
    )
}
fn ready() -> (TeamsModel, Instant) {
    let now = Instant::now();
    let mut m = TeamsModel::default();
    m.follow(
        TeamsFollow {
            team: Some(TEAM.into()),
            ..Default::default()
        },
        now,
    );
    let source = context();
    for key in ["status", "queue"] {
        let read = m.next(now).unwrap();
        m.apply(&read, decoded(&read, source[key].clone()), now);
    }
    assert!(m.next(now).is_none());
    (m, now)
}
fn view() -> crate::teams::view::TeamView {
    let (mut m, now) = ready();
    m.follow_phases(TEAM, Some(1), true, now).unwrap();
    let source = context();
    let mut latest = None;
    for key in ["phases", "draft"] {
        let read = m.next(now).unwrap();
        let result = m.apply(&read, decoded(&read, source[key].clone()), now);
        for (_, update) in result {
            if let TeamsUpdate::Team(v) = update {
                latest = Some((*v).clone());
            }
        }
    }
    latest.unwrap()
}
fn wire() -> Value {
    let v = view();
    let mut c = crate::Coalescer::new();
    c.push(crate::Change::teams(
        crate::ComputerId("phase-fixture".into()),
        format!("team:{TEAM}"),
        TeamsUpdate::Team(std::sync::Arc::new(v)),
    ));
    let data = serde_json::to_value(c.take()).unwrap();
    let _: crate::Batch = serde_json::from_value(data.clone()).unwrap();
    data["teams"][0]["payload"].clone()
}

#[test]
fn producer_drafts_reach_the_window_through_read_model_graph_and_batch() {
    let wire = wire();
    let plan = &wire["plan"];
    let draft = &plan["draft"];
    assert_eq!(draft["complete"], true);
    assert_eq!(draft["editable"], true);
    assert_eq!(draft["nodes"][0]["title"], "Build the foundation");
    assert_eq!(
        draft["nodes"][0]["criteria"][0],
        "The foundation tests pass"
    );
    assert_eq!(draft["graph"]["nodes"][0]["layer"], 0);
    assert_eq!(draft["graph"]["nodes"][1]["layer"], 1);
    assert_eq!(draft["graph"]["edges"][0]["from"], 1);
    assert_eq!(draft["graph"]["edges"][0]["to"], 2);
    assert_eq!(
        wire,
        serde_json::from_str::<Value>(include_str!(
            "../../../../../src/ui/teams/fixtures/phase-pane.json"
        ))
        .unwrap()
    );
}

#[test]
fn unknown_draft_fields_are_visible_but_never_stripped_by_an_edit() {
    let mut source = context()["draft"].clone();
    source["draft"]["nodes"][0]["future_instruction"] = serde_json::json!({"kept":true});
    let read = TeamRead::Draft {
        team: TEAM.into(),
        phase: 1,
    };
    let checked = decoded(&read, source).unwrap();
    let out = draft(&checked, 1).unwrap();
    assert!(out.unknown_fields);
    assert!(!out.editable);
    assert_eq!(out.nodes[0].title, "Build the foundation");
}

#[test]
fn missing_invalid_duplicate_or_wrong_phase_data_never_becomes_an_empty_editable_plan() {
    for case in 0..6 {
        let mut source = context()["draft"].clone();
        match case {
            0 => {
                source["draft"].as_object_mut().unwrap().remove("complete");
            }
            1 => source["draft"]["phase"]["state"] = serde_json::json!("queued"),
            2 => source["draft"]["nodes"][1]["id"] = serde_json::json!(1),
            3 => source["draft"]["phase"]["n"] = serde_json::json!(2),
            4 => source["draft"]["generation"] = Value::Null,
            _ => source["draft"]["nodes"] = Value::Null,
        }
        assert!(
            decoded(
                &TeamRead::Draft {
                    team: TEAM.into(),
                    phase: 1
                },
                source
            )
            .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn closing_and_reopening_a_phase_refuses_the_old_in_flight_answer() {
    let (mut m, now) = ready();
    m.follow_phases(TEAM, Some(1), true, now).unwrap();
    let old = m.next(now).unwrap();
    assert!(matches!(old, TeamRead::Phases { .. }));
    m.follow_phases(TEAM, None, false, now).unwrap();
    let (epoch, _) = m.follow_phases(TEAM, Some(1), true, now).unwrap();
    assert!(m
        .apply(&old, decoded(&old, context()["phases"].clone()), now)
        .is_empty());
    let fresh = m.next(now).unwrap();
    let out = m.apply(&fresh, decoded(&fresh, context()["phases"].clone()), now);
    let TeamsUpdate::Team(v) = &out[0].1 else {
        panic!("phase update")
    };
    assert_eq!(v.plan.as_ref().unwrap().epoch, epoch);
    assert!(v.plan.as_ref().unwrap().draft.is_none());
}

#[test]
fn phase_selection_and_team_scope_refuse_late_drafts() {
    let (mut m, now) = ready();
    m.follow_phases(TEAM, Some(1), true, now).unwrap();
    let list = m.next(now).unwrap();
    m.apply(&list, decoded(&list, context()["phases"].clone()), now);
    let old = m.next(now).unwrap();
    assert!(matches!(old, TeamRead::Draft { phase: 1, .. }));
    m.follow_phases(TEAM, Some(2), true, now).unwrap();
    assert!(m
        .apply(&old, decoded(&old, context()["draft"].clone()), now)
        .is_empty());
    m.follow(TeamsFollow::default(), now);
    assert!(m.follow_phases(TEAM, Some(1), true, now).is_none());
    assert!(m
        .apply(&old, decoded(&old, context()["draft"].clone()), now)
        .is_empty());
}

#[test]
fn an_old_panes_cleanup_cannot_disable_its_successor() {
    let (mut m, now) = ready();
    let (old, _) = m.follow_phases(TEAM, Some(1), true, now).unwrap();
    let (current, _) = m.follow_phases(TEAM, Some(2), true, now).unwrap();
    let (still, updates) = m.close_phases(TEAM, Some(old), now).unwrap();
    assert_eq!(still, current);
    assert!(updates.is_empty());
    let source = context();
    let read = m.next(now).unwrap();
    let out = m.apply(&read, decoded(&read, source["phases"].clone()), now);
    let TeamsUpdate::Team(v) = &out[0].1 else {
        panic!("phase update")
    };
    assert_eq!(v.plan.as_ref().unwrap().selected, Some(2));
}

#[test]
fn older_cli_is_unavailable_without_polling_or_inventing_a_plan() {
    let (mut m, now) = ready();
    m.follow_phases(TEAM, None, true, now).unwrap();
    let read = m.next(now).unwrap();
    let out = m.apply(
        &read,
        Err(crate::teams::read::TeamError::Refused {
            code: "not_found".into(),
            message: "Unknown team command: phases".into(),
        }),
        now,
    );
    let TeamsUpdate::Team(v) = &out[0].1 else {
        panic!("phase update")
    };
    let plan = v.plan.as_ref().unwrap();
    assert!(plan.list.is_none());
    assert_eq!(plan.list_failure.as_ref().unwrap().code, "not_found");
    assert!(m.next(now).is_none());
    m.follow_phases(TEAM, None, true, now).unwrap();
    assert!(matches!(m.next(now), Some(TeamRead::Phases { .. })));
    m.set_hidden(true, now);
    assert!(m.next(now).is_none());
}

#[test]
#[ignore = "writes the UI fixture from real CLI rows through core and Batch"]
fn regenerate_phase_pane_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/teams/fixtures/phase-pane.json");
    std::fs::write(path, serde_json::to_string_pretty(&wire()).unwrap() + "\n").unwrap();
}
