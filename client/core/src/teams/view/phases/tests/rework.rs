use super::*;

fn cases() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../../../crates/team-plan/fixtures/cli-phase-rework.json"
    ))
    .unwrap()
}

fn produced() -> Value {
    Value::Object(
        cases()
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, context)| (name.clone(), serde_json::json!({"team":wire_from(context)})))
            .collect(),
    )
}

#[test]
fn rework_writer_rows_keep_base_and_proposal_separate_through_model_coalescer_and_window() {
    let out = produced();
    let base = &out["sent_back"]["team"]["plan"]["draft"];
    assert_eq!(base["phase"]["state"], "awaiting_acceptance");
    assert_eq!(base["accept_available"], false);
    assert!(base["rework"]["draft"].is_null());
    for name in [
        "proposal", "rejected", "undone", "revised", "reserved", "partial",
    ] {
        let root = &out[name]["team"]["plan"]["draft"];
        assert!(!root.is_null(), "{name}");
        assert_eq!(root["nodes"], base["nodes"], "{name}");
        assert_eq!(root["publication"], base["publication"], "{name}");
        assert_eq!(
            root["rework"]["base_publication"],
            base["publication"]["id"]
        );
        assert_eq!(root["phase"]["state"], "awaiting_acceptance");
        assert_eq!(root["accept_available"], false);
        let proposal = &root["rework"]["draft"];
        assert!(proposal["generation"].as_u64().unwrap() > root["generation"].as_u64().unwrap());
        assert!(proposal["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| !root["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|old| old["id"] == n["id"])));
    }
    let published = &out["published"]["team"]["plan"]["draft"];
    assert_ne!(published["publication"]["id"], base["publication"]["id"]);
    assert_eq!(published["rework"]["state"], "published");
    assert!(published["rework"]["draft"].is_null());
    assert!(
        published["live"]["nodes"].as_array().unwrap().len()
            > published["nodes"].as_array().unwrap().len()
    );
    assert_eq!(
        out["accepted"]["team"]["plan"]["draft"]["phase"]["state"],
        "done"
    );
    assert_eq!(
        out,
        serde_json::from_str::<Value>(include_str!(
            "../../../../../../src/ui/teams/fixtures/phase-rework.json"
        ))
        .unwrap()
    );
}

#[test]
fn invalid_rework_episodes_and_control_changes_never_become_editable_or_accepted() {
    for fault in 0..11 {
        let mut c = cases()["proposal"].clone();
        let d = &mut c["draft"]["draft"];
        match fault {
            0 => d["rework"]["base_publication"] = "00000000-0000-4000-8000-000000000099".into(),
            1 => d["rework"]["draft"]["generation"] = 1.into(),
            2 => {
                let phase: zerus_team_plan::Phase =
                    serde_json::from_value(d["phase"].clone()).unwrap();
                d["rework"]["draft"]["phase"]["acceptance"] =
                    if phase.acceptance == zerus_team_plan::Acceptance::Person {
                        "lead".into()
                    } else {
                        "person".into()
                    };
            }
            3 => d["rework"]["draft"]["phase"]["barrier"] = true.into(),
            4 => d["rework"]["draft"]["phase"]["order"] = 999.into(),
            5 => d["rework"]["draft"]["revision"] = 999.into(),
            6 => d["rework"]["draft"]["rework"] = serde_json::json!({}),
            7 => {
                d["rework"]["draft"]["live"] =
                    cases()["published"]["draft"]["draft"]["live"].clone()
            }
            8 => d["accept_available"] = true.into(),
            9 => d["rework"]["comment"] = "Forbidden\u{202e}comment".into(),
            _ => d["rework"]["draft"]["nodes"][0]["id"] = d["nodes"][0]["id"].clone(),
        }
        let view = wire_from(&c);
        assert!(view["plan"]["draft"].is_null(), "fault {fault}: {view}");
        assert!(!view["plan"]["draft_failure"].is_null(), "fault {fault}");
    }
}

#[test]
#[ignore = "writes only the UI rework fixture through real core reader/model/batch"]
fn regenerate_rework_window_fixture() {
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/ui/teams/fixtures/phase-rework.json"),
        serde_json::to_string_pretty(&produced()).unwrap() + "\n",
    )
    .unwrap();
}
