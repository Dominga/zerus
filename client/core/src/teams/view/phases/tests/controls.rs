//! Product-writer contexts pass through the real reader/model/coalescer.
//! Mutations below are deliberately malformed or incomplete read controls.
use super::*;

fn cases() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../../../crates/team-plan/fixtures/cli-phase-controls.json"
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
fn phase_control_writer_scopes_and_pause_owners_reach_the_window_without_substitution() {
    let all = produced();
    for (name, context) in cases().as_object().unwrap() {
        let draft = &all[name]["team"]["plan"]["draft"];
        assert!(!draft.is_null(), "{name}: {}", all[name]);
        assert_eq!(
            draft["controls"], context["draft"]["draft"]["controls"],
            "{name}"
        );
        assert_eq!(draft["phase"], context["draft"]["draft"]["phase"], "{name}");
    }
    assert_eq!(
        all["approved_unstarted"]["team"]["plan"]["draft"]["controls"]["unstarted"]["allowed"],
        true
    );
    assert_eq!(
        all["started"]["team"]["plan"]["draft"]["controls"]["unstarted"]["allowed"],
        false
    );
    let correction = &all["correction"]["team"]["plan"]["draft"];
    assert_eq!(
        correction["controls"]["scope"]["publication"],
        correction["publication"]["id"]
    );
    assert_eq!(
        correction["controls"]["scope"]["rework"]["id"],
        correction["rework"]["id"]
    );
    let saved = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/ui/teams/fixtures/phase-controls.json"),
    )
    .unwrap();
    assert_eq!(all, serde_json::from_str::<Value>(&saved).unwrap());
}

#[test]
fn malformed_or_partial_controls_never_reach_enabled_window_actions() {
    for (case, fault) in [
        ("draft", 0),
        ("draft", 1),
        ("draft", 2),
        ("draft", 3),
        ("draft", 4),
        ("draft", 5),
        ("draft", 6),
        ("awaiting", 8),
        ("paused_person", 9),
        ("paused_person", 10),
        ("paused_lead", 11),
        ("paused_person", 12),
        ("correction", 13),
        ("correction", 14),
        ("correction", 15),
        ("paused_person", 18),
        ("paused_lead", 19),
    ] {
        let mut context = cases()[case].clone();
        let d = &mut context["draft"]["draft"];
        match fault {
            0 => d["controls"]["scope"]["generation"] = 999.into(),
            1 => d["controls"]["scope"]["phase"] = 2.into(),
            2 => d["controls"]["version"] = 2.into(),
            3 => d["controls"]["earlier"]["target"] = Value::Null,
            4 => d["controls"]["earlier"]["target"] = d["controls"]["scope"].clone(),
            5 => d["controls"]["unstarted"]["reason"] = "Contradictory positive fact".into(),
            6 => d["phase"]["paused"] = true.into(),
            8 => {
                d["controls"]["acceptance"]["allowed"] = true.into();
                d["controls"]["acceptance"]["reason"] = Value::Null;
            }
            9 => d["controls"]["pause"]["episode"]["reason"] = "x\u{202e}y".into(),
            10 => d["controls"]["pause"]["episode"]["reason"] = "Ж".repeat(1025).into(),
            11 => {
                d["controls"]["pause"]["episode"]["owner"]["session"]["session_id"] =
                    "x\u{2028}y".into()
            }
            12 => d["controls"]["pause"]["episode"]["id"] = "NOT-AN-ORIGINAL".into(),
            13 => {
                d["controls"]["scope"]["rework"]["id"] =
                    "00000000-0000-4000-8000-000000000099".into()
            }
            14 => d["controls"]["scope"]["rework"]["generation"] = 999.into(),
            15 => {
                d["controls"]["scope"]["publication"] =
                    "00000000-0000-4000-8000-000000000099".into()
            }
            18 => d["phase"]["mode"] = "free".into(),
            _ => {
                d["controls"]["pause"]["episode"]["owner"]["session"]["machine"] =
                    "not-a-computer-uuid".into()
            }
        }
        let view = wire_from(&context);
        assert!(view["plan"]["draft"].is_null(), "fault {fault}: {view}");
        assert!(!view["plan"]["draft_failure"].is_null(), "fault {fault}");
    }
}

#[test]
fn partial_live_frames_keep_partial_display_but_disable_all_phase_controls() {
    for case in ["approved_unstarted", "started", "correction"] {
        for missing in [false, true] {
            let mut context = cases()[case].clone();
            let expected = context["draft"]["draft"]["controls"]["pause"].clone();
            if missing {
                context["draft"]["draft"]
                    .as_object_mut()
                    .unwrap()
                    .remove("live");
            } else {
                context["draft"]["draft"]["live"]["complete"] = false.into();
            }
            let view = wire_from(&context);
            let d = &view["plan"]["draft"];
            assert!(
                !d.is_null(),
                "a valid partial display must remain available: {view}"
            );
            assert_eq!(d["graph"]["complete"], false);
            assert!(d["metrics"]["critical_path_minutes"].is_null());
            assert_eq!(
                d["controls"]["pause"], expected,
                "the original owner/reason stays visible"
            );
            for flag in [
                "unstarted",
                "barrier",
                "pause_change",
                "resume",
                "acceptance",
            ] {
                assert_eq!(d["controls"][flag]["allowed"], false, "{case}/{flag}");
            }
            for field in ["earlier", "later"] {
                assert_eq!(d["controls"][field]["availability"]["allowed"], false);
            }
        }
    }
}

#[test]
fn missing_control_capability_remains_unavailable_without_inventing_a_pause_owner() {
    let mut context = cases()["paused_person"].clone();
    context["draft"]["draft"]
        .as_object_mut()
        .unwrap()
        .remove("controls");
    let view = wire_from(&context);
    let draft = &view["plan"]["draft"];
    assert_eq!(draft["phase"]["paused"], true);
    assert!(draft.get("controls").is_none());
}

#[test]
fn an_incomplete_draft_keeps_its_partial_display_without_control_availability() {
    let mut context = cases()["draft"].clone();
    context["draft"]["draft"]["complete"] = false.into();
    context["draft"]["draft"]["editable"] = false.into();
    let view = wire_from(&context);
    let draft = &view["plan"]["draft"];
    assert!(!draft.is_null());
    assert_eq!(draft["complete"], false);
    for flag in [
        "unstarted",
        "barrier",
        "pause_change",
        "resume",
        "acceptance",
    ] {
        assert_eq!(draft["controls"][flag]["allowed"], false);
    }
    for dir in ["earlier", "later"] {
        assert_eq!(draft["controls"][dir]["availability"]["allowed"], false);
    }
    assert!(draft["metrics"]["critical_path_minutes"].is_null());
}

#[test]
#[ignore = "writes only the S5 window fixture through the real core model and batch"]
fn regenerate_phase_controls_window_fixture() {
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/ui/teams/fixtures/phase-controls.json"),
        serde_json::to_string_pretty(&produced()).unwrap() + "\n",
    )
    .unwrap();
}
