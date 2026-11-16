//! Preparation fixtures use operation-written phase snapshots through the real
//! reader/model, then the pure estimate writer. Observed-page/model integration
//! is separately gated on actual .173.3 admission.
use serde_json::Value;
use std::time::Instant;
use zerus_client_core::{
    actions::Exit,
    teams::{
        estimate_projection::{project, EstimateContext},
        read::decode,
        TeamsFollow, TeamsModel, TeamsUpdate,
    },
};

fn produced() -> Value {
    let sources: Value = serde_json::from_str(include_str!(
        "../../../crates/team-plan/fixtures/cli-phase-publication.json"
    ))
    .unwrap();
    let mut out = serde_json::Map::new();
    for name in [
        "draft",
        "rejected",
        "revised",
        "publishing",
        "partial",
        "published",
        "awaiting",
        "accepted",
    ] {
        let source = &sources[name];
        let team = source["draft"]["team"].as_str().unwrap();
        let now = Instant::now();
        let mut model = TeamsModel::default();
        model.follow(
            TeamsFollow {
                team: Some(team.into()),
                ..Default::default()
            },
            now,
        );
        let mut latest = None;
        for key in ["status", "queue", "phases", "draft"] {
            if key == "phases" {
                model.follow_phases(team, Some(1), true, now).unwrap();
            }
            let read = model.next(now).unwrap();
            let reply = decode(
                &read,
                Exit::Code {
                    code: 0,
                    stdout: serde_json::to_vec(&source[key]).unwrap(),
                    stderr: String::new(),
                },
            );
            for (_, update) in model.apply(&read, reply, now) {
                if let TeamsUpdate::Team(view) = update {
                    latest = Some(view);
                }
            }
        }
        let view = latest.unwrap();
        let draft = view.plan.as_ref().unwrap().draft.as_ref().unwrap();
        let context = EstimateContext {
            team: view.bar.id.clone(),
            home: source["status"]["team"]["home"].as_str().unwrap().into(),
            revision: view.bar.revision,
        };
        out.insert(
            name.into(),
            serde_json::to_value(project(&context, draft, None)).unwrap(),
        );
    }
    Value::Object(out)
}

fn fixture() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/teams/fixtures/phase-estimates.json")
}

#[test]
fn product_snapshots_supply_the_standalone_estimate_inputs() {
    let value = produced();
    assert_eq!(value["draft"]["scope"]["basis"], "unapproved_preview");
    assert_eq!(value["published"]["scope"]["basis"], "frozen_publication");
    assert_eq!(value["published"]["rows"][0]["estimate_minutes"], 30);
    assert_eq!(value["published"]["rows"][1]["estimate_minutes"], 30);
    assert!(!value["partial"]["cohort_complete"].as_bool().unwrap());
    assert!(value["partial"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["estimate_minutes"].is_null()));
    assert_eq!(
        value,
        serde_json::from_str::<Value>(&std::fs::read_to_string(fixture()).unwrap()).unwrap()
    );
}

#[test]
#[ignore = "writes standalone estimate inputs from operation-written snapshots through reader/model/projection"]
fn regenerate_phase_estimate_inputs() {
    std::fs::write(
        fixture(),
        serde_json::to_string_pretty(&produced()).unwrap() + "\n",
    )
    .unwrap();
}
