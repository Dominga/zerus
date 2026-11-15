//! All team rows and refusal receipts here come from the real operation writer.
use super::*;
use serde_json::{json, Value};

fn cases() -> Value {
    let mut h = Harness::new();
    h.add(&person(), 1);
    h.add(&person(), 2);
    let before = h.store.team(TEAM).unwrap().unwrap();
    let journal = h.store.journal(TEAM, 200, None).unwrap().rows;
    let mut result = serde_json::Map::new();
    for (name, value) in [
        ("max_members", 0),
        ("max_phase_tasks", 0),
        ("max_open_tasks", 0),
        ("max_open_questions", 0),
        ("live_members", 1),
        ("live_tasks", 1),
    ] {
        let mut settings = before.settings.clone();
        let cap = match name {
            "max_members" | "live_members" => &mut settings.caps.max_members,
            "max_phase_tasks" => &mut settings.caps.max_phase_tasks,
            "max_open_questions" => &mut settings.caps.max_open_questions,
            _ => &mut settings.caps.max_open_tasks,
        };
        *cap = value;
        let problem = h
            .run(&person(), Operation::ChangeSettings { settings })
            .unwrap_err();
        let (reply, exit) = crate::teams::refusal(problem);
        assert_eq!(exit, 1);
        result.insert(name.into(), reply);
        assert_eq!(h.store.team(TEAM).unwrap().unwrap(), before);
        assert_eq!(h.store.journal(TEAM, 200, None).unwrap().rows, journal);
    }
    Value::Object(result)
}

fn payload() -> Value {
    use zerus_client_core::actions::Exit;
    use zerus_client_core::teams::action::{classify, TeamAction, TeamCall};
    let native = cases();
    let call = TeamCall {
        team: TEAM.into(),
        team_name: "Settings fixture".into(),
        expected_revision: Some(1),
        confirmed: false,
        request_id: "00000000-0000-4000-8000-000000000001".into(),
        action: TeamAction::Settings {
            settings: Default::default(),
        },
    };
    let outcomes = native
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, row)| {
            let outcome = classify(
                &call,
                Exit::Code {
                    code: 1,
                    stdout: row.to_string().into_bytes(),
                    stderr: String::new(),
                },
            );
            (name.clone(), serde_json::to_value(outcome).unwrap())
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"native": native, "outcomes": outcomes})
}

#[test]
fn settings_refusals_name_the_actual_field_without_changing_codes_or_writing() {
    let actual = cases();
    for (name, field) in [
        ("max_members", "max_members"),
        ("max_phase_tasks", "max_phase_tasks"),
        ("max_open_tasks", "max_open_tasks"),
        ("max_open_questions", "max_open_questions"),
        ("live_members", "max_members"),
        ("live_tasks", "max_open_tasks"),
    ] {
        assert_eq!(
            actual[name]["error"]["field"],
            format!("settings.caps.{field}")
        );
        assert_eq!(actual[name]["error"]["code"], code::OVER_CAP);
        let message = if name.starts_with("live_") {
            "Caps are below admitted tasks, saved drafts or live members."
        } else {
            "Settings exceed the supported storage bounds."
        };
        assert_eq!(actual[name]["error"]["message"], message);
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("client/core/src/teams/fixtures/settings-refusals.json");
    let pinned: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(actual, pinned["native"]);
    assert_eq!(payload(), pinned);
}

#[test]
#[ignore = "regenerates the settings refusal fixture from real product operations"]
fn regenerate_settings_refusals() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("client/core/src/teams/fixtures/settings-refusals.json");
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&payload()).unwrap()),
    )
    .unwrap();
}
