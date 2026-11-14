//! The person's catalog and templates: the file is the person's own entries,
//! and nothing is written except by an explicit save or import.
use super::harness::*;
use crate::teams::catalog;
use crate::teams::cli::actor::Caller;
use serde_json::{json, Value};
use zerus_team_plan::builtin;

fn person_caller() -> Caller {
    Caller::Person
}

fn session_caller() -> Caller {
    Caller::Session(key(OTHER_SESSION))
}

fn run(harness: &Harness, line: &str, input: &str) -> (Value, i32) {
    harness.cli(&person_caller(), line, input)
}

#[test]
fn the_catalog_shows_the_built_in_entries_and_exports_only_the_persons() {
    let harness = Harness::new();
    let (reply, exit) = run(&harness, "catalog show", "");
    assert_eq!(exit, 0, "{reply}");
    let mut shown = reply["catalog"].clone();
    for template in shown["templates"].as_array_mut().unwrap() {
        let digest = template.as_object_mut().unwrap().remove("digest").unwrap();
        assert_eq!(digest.as_str().unwrap().len(), 64);
    }
    assert_eq!(shown, serde_json::to_value(builtin()).unwrap());
    let (reply, exit) = run(&harness, "catalog export", "");
    assert_eq!(exit, 0, "{reply}");
    assert_eq!(
        reply["catalog"],
        serde_json::to_value(zerus_team_plan::catalog::Catalog::default()).unwrap()
    );
    assert!(
        !catalog::path(&harness.config.dir).exists(),
        "reading writes nothing"
    );
}

#[test]
fn a_person_imports_a_catalog_and_an_unknown_field_is_refused_untouched() {
    let harness = Harness::new();
    let catalog_json = json!({
        "roles": [{"id": "architect", "name": "Architect", "agent": "claude"}]
    });
    let (reply, exit) = run(&harness, "catalog import", &catalog_json.to_string());
    assert_eq!(exit, 0, "{reply}");
    let person = catalog::person(&harness.config.dir).unwrap();
    assert_eq!(person.roles.len(), 1);
    assert_eq!(person.roles[0].id, "architect");
    let (reply, _) = run(&harness, "catalog show", "");
    assert!(reply["catalog"]["roles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|role| role["id"] == "architect"));
    assert_eq!(reply["catalog"]["roles"].as_array().unwrap().len(), 4);

    let before = std::fs::read_to_string(catalog::path(&harness.config.dir)).unwrap();
    let (reply, exit) = run(&harness, "catalog import", r#"{"colour":"red"}"#);
    assert_eq!((code(&reply), exit), ("unknown_field", 1), "{reply}");
    assert_eq!(reply["error"]["refs"][0], "colour");
    assert_eq!(
        std::fs::read_to_string(catalog::path(&harness.config.dir)).unwrap(),
        before
    );
}

#[test]
fn a_catalog_import_refuses_an_unseated_role_with_a_refused_agent_untouched() {
    let harness = Harness::new();
    let catalog_json = json!({
        "roles": [
            {"id": "architect", "name": "Architect", "agent": "claude"},
            {"id": "unused", "name": "Unused", "agent": "DeepSeek"}
        ]
    });
    let (reply, exit) = run(&harness, "catalog import", &catalog_json.to_string());
    assert_eq!((code(&reply), exit), ("invalid_state", 1), "{reply}");
    let message = reply["error"]["message"].as_str().unwrap();
    assert!(message.contains("'roles.unused.agent'"), "{message}");
    assert!(message.contains("'DeepSeek'"), "{message}");
    assert!(
        !catalog::path(&harness.config.dir).exists(),
        "nothing is written"
    );
}

#[test]
fn a_template_import_refuses_an_extra_unseated_role_with_a_refused_agent_untouched() {
    let harness = Harness::new();
    let mut bundle = catalog::export_bundle(&builtin(), "bugfix").expect("bugfix exports");
    let mut extra = bundle.roles[0].clone();
    extra.id = "unused".into();
    extra.name = "Unused".into();
    extra.agent = "DeepSeek".into();
    extra.pipeline.clear();
    bundle.roles.push(extra);
    let (reply, exit) = run(
        &harness,
        "template import",
        &serde_json::to_string(&bundle).unwrap(),
    );
    assert_eq!((code(&reply), exit), ("invalid_state", 1), "{reply}");
    let message = reply["error"]["message"].as_str().unwrap();
    assert!(message.contains("'roles.unused.agent'"), "{message}");
    assert!(
        !catalog::path(&harness.config.dir).exists(),
        "nothing is written"
    );
}

#[test]
fn templates_are_listed_shown_exported_saved_and_removed() {
    let harness = Harness::new();
    let (reply, exit) = run(&harness, "template list", "");
    assert_eq!(exit, 0, "{reply}");
    let ids: Vec<&str> = reply["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|template| template["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["feature-delivery", "bugfix", "small"]);

    let (reply, _) = run(&harness, "template show bugfix", "");
    assert_eq!(reply["template"]["id"], "bugfix");
    let (reply, _) = run(&harness, "template show ghost", "");
    assert_eq!(code(&reply), "not_found");

    let (reply, exit) = run(&harness, "template export bugfix", "");
    assert_eq!(exit, 0, "{reply}");
    let bundle = reply["bundle"].clone();
    assert_eq!(bundle["roles"].as_array().unwrap().len(), 2);
    assert_eq!(bundle["pipelines"].as_array().unwrap().len(), 1);

    // Save a changed copy of a built-in template into the person's own file.
    let mut template = reply["bundle"]["template"].clone();
    template["name"] = json!("Bugfix, ours");
    let (reply, exit) = run(&harness, "template save", &template.to_string());
    assert_eq!(
        (exit, reply["saved"].as_str()),
        (0, Some("bugfix")),
        "{reply}"
    );
    let (reply, _) = run(&harness, "template show bugfix", "");
    assert_eq!(reply["template"]["name"], "Bugfix, ours");

    // Remove it: the person's entry goes, the built-in one is still available.
    let (reply, exit) = run(&harness, "template remove bugfix", "");
    assert_eq!(
        (
            exit,
            reply["removed"].as_str(),
            reply["still_available"].as_bool()
        ),
        (0, Some("bugfix"), Some(true)),
        "{reply}"
    );
    let (reply, _) = run(&harness, "template show bugfix", "");
    assert_eq!(reply["template"]["name"], "Bugfix");
    let (reply, _) = run(&harness, "template remove bugfix", "");
    assert_eq!(
        code(&reply),
        "not_found",
        "a built-in template is not ours to remove"
    );
}

#[test]
fn a_template_bundle_imported_into_an_empty_catalog_is_complete() {
    let harness = Harness::new();
    let (reply, _) = run(&harness, "template export bugfix", "");
    let text = reply["bundle"].to_string();
    let (reply, exit) = run(&harness, "template import", &text);
    assert_eq!(
        (exit, reply["imported"].as_str()),
        (0, Some("bugfix")),
        "{reply}"
    );
    let person = catalog::person(&harness.config.dir).unwrap();
    assert_eq!(person.templates.len(), 1, "only the imported template");
    assert_eq!(person.roles.len(), 2);
    assert_eq!(person.pipelines.len(), 1);
    let (reply, _) = run(&harness, "template export bugfix", "");
    assert_eq!(
        reply["bundle"],
        serde_json::from_str::<Value>(&text).unwrap()
    );
}

#[test]
fn a_catalog_or_template_command_is_the_persons() {
    let harness = Harness::new();
    for line in [
        "catalog show",
        "catalog export",
        "template list",
        "catalog import",
    ] {
        let (reply, exit) = harness.cli(&session_caller(), line, "");
        assert_eq!((code(&reply), exit), ("not_allowed", 1), "{line}");
    }
    let (reply, _) = harness.cli(&session_caller(), "template export bugfix", "");
    assert_eq!(code(&reply), "not_allowed");
    assert!(!catalog::path(&harness.config.dir).exists());
}

#[test]
fn a_template_save_that_breaks_the_merged_catalog_writes_nothing() {
    let harness = Harness::new();
    let (reply, _) = run(&harness, "template show small", "");
    let mut template = reply["template"].clone();
    template["roster"] = json!([{"role": "ghost", "count": 1}]);
    let (reply, exit) = run(&harness, "template save", &template.to_string());
    assert_eq!((code(&reply), exit), ("unknown_role", 1), "{reply}");
    assert!(!catalog::path(&harness.config.dir).exists());
}

#[test]
fn every_catalog_reply_names_the_person_and_the_interim_rule() {
    let harness = Harness::new();
    let actor_is_shown = |reply: &Value, line: &str| {
        assert_eq!(reply["actor"]["kind"], "person", "{line}");
        assert_eq!(reply["actor"]["origin"], "cli_interim", "{line}");
        assert_eq!(reply["actor"]["interim"], true, "{line}");
    };
    for line in [
        "catalog show",
        "catalog export",
        "template list",
        "template show bugfix",
        "template export bugfix",
    ] {
        let (reply, exit) = run(&harness, line, "");
        assert_eq!(exit, 0, "{line}: {reply}");
        actor_is_shown(&reply, line);
    }
    let (reply, _) = run(
        &harness,
        "catalog import",
        r#"{"roles":[{"id":"architect","name":"Architect","agent":"claude"}]}"#,
    );
    actor_is_shown(&reply, "catalog import");
    let (bundle, _) = run(&harness, "template export bugfix", "");
    let (reply, _) = run(&harness, "template import", &bundle["bundle"].to_string());
    actor_is_shown(&reply, "template import");
    let (template, _) = run(&harness, "template show bugfix", "");
    let (reply, _) = run(&harness, "template save", &template["template"].to_string());
    actor_is_shown(&reply, "template save");
    let (reply, _) = run(&harness, "template remove bugfix", "");
    actor_is_shown(&reply, "template remove");
}

fn sized_catalog(target: usize) -> zerus_team_plan::catalog::Catalog {
    let mut catalog = builtin();
    let lead = catalog.templates[0].lead.clone();
    let settings = catalog.templates[0].settings.clone();
    let developer = catalog
        .roles
        .iter()
        .position(|role| role.id == "developer")
        .unwrap();
    catalog.roles[developer].prompt.clear();
    catalog.templates.push(zerus_team_plan::catalog::Template {
        id: "sized".into(),
        name: "Sized".into(),
        description: String::new(),
        lead,
        roster: vec![
            zerus_team_plan::catalog::Seat {
                role: "developer".into(),
                count: 1,
                optional: false,
            },
            zerus_team_plan::catalog::Seat {
                role: "reviewer".into(),
                count: 1,
                optional: false,
            },
        ],
        default_pipeline: "reviewed".into(),
        settings,
    });
    // A non-empty prompt adds its field name as well as its bytes; measure
    // that shape and subtract the one character so the target is exact.
    catalog.roles[developer].prompt = "a".into();
    let base = serde_json::to_string(
        &zerus_team_plan::bundle(&catalog, "sized").unwrap(),
    )
    .unwrap()
    .len();
    catalog.roles[developer].prompt = "a".repeat(target - (base - 1));
    assert_eq!(
        serde_json::to_string(&zerus_team_plan::bundle(&catalog, "sized").unwrap())
            .unwrap()
            .len(),
        target
    );
    catalog
}

#[test]
fn catalog_show_skips_the_digest_of_a_bundle_over_the_create_limit() {
    for (target, over) in [
        (crate::teams::catalog::CREATE_BUNDLE_LIMIT, false),
        (crate::teams::catalog::CREATE_BUNDLE_LIMIT + 1, true),
    ] {
        let harness = Harness::new();
        let catalog = sized_catalog(target);
        let (reply, exit) = run(
            &harness,
            "catalog import",
            &serde_json::to_string(&catalog).unwrap(),
        );
        assert_eq!(exit, 0, "{reply}");
        let (shown, exit) = run(&harness, "catalog show", "");
        assert_eq!(exit, 0, "{shown}");
        let row = shown["catalog"]["templates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|template| template["id"] == "sized")
            .unwrap();
        if over {
            assert!(row.get("digest").is_none(), "{row}");
            assert_eq!(row["over_limit"], true, "{row}");
        } else {
            assert_eq!(row["digest"].as_str().unwrap().len(), 64, "{row}");
            assert!(row.get("over_limit").is_none(), "{row}");
        }
    }
}

#[test]
fn a_shown_catalog_row_saves_and_imports_with_its_read_only_fields() {
    let harness = Harness::new();
    let (shown, exit) = run(&harness, "catalog show", "");
    assert_eq!(exit, 0, "{shown}");
    let row = shown["catalog"]["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|template| template["id"] == "bugfix")
        .unwrap()
        .clone();
    assert_eq!(row["digest"].as_str().unwrap().len(), 64);
    let (reply, exit) = run(&harness, "template save", &row.to_string());
    assert_eq!(exit, 0, "{reply}");
    let (reply, exit) = run(
        &harness,
        "catalog import",
        &shown["catalog"].to_string(),
    );
    assert_eq!(exit, 0, "{reply}");
    // A stale read-only field is ignored, not refused.
    let mut stale = row;
    stale["digest"] = json!("0".repeat(64));
    stale["over_limit"] = json!(true);
    let (reply, exit) = run(&harness, "template save", &stale.to_string());
    assert_eq!(exit, 0, "{reply}");
    let mut catalog = shown["catalog"].clone();
    for template in catalog["templates"].as_array_mut().unwrap() {
        template["digest"] = json!("f".repeat(64));
        template["over_limit"] = json!(true);
    }
    let (reply, exit) = run(&harness, "catalog import", &catalog.to_string());
    assert_eq!(exit, 0, "{reply}");
}
