//! The shared golden fixture `tests/fixtures/folder-paths.json`: folder paths
//! in their computer's form (`paths.rs`: `of`, `absolute`, `clean`, `same`,
//! `within`, `leaf`, `native`, `split`, `join`) and New session's folder helpers
//! (`launch::matching_project_folder`, `prefill_target`, `restored_folder`).
//! The desktop client's TypeScript port (`client/src/ui/sessions/paths.ts`)
//! checks the same answers in `paths.test.ts`, so the two cannot drift.
//!
//! After a change to these rules, write the new answers with
//! `cargo test --test folder_paths -- --ignored`, then review the diff.
use serde_json::{json, Value};
use std::path::PathBuf;
use zerus_client_core::launch::{self, Project};
use zerus_client_core::paths::{self, Style};

fn file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/folder-paths.json")
}

fn read() -> Value {
    serde_json::from_str(&std::fs::read_to_string(file()).expect("fixture")).expect("fixture JSON")
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn style(value: &Value) -> Style {
    serde_json::from_value(value["style"].clone()).expect("style")
}

fn project(value: &Value) -> Option<Project> {
    serde_json::from_value(value["project"].clone()).expect("project")
}

fn style_name(style: Style) -> &'static str {
    match style {
        Style::Posix => "posix",
        Style::Windows => "windows",
    }
}

/// One path's answers in one style.
fn single(path: &str, style: Style) -> Value {
    json!({
        "absolute": paths::absolute(path, style),
        "clean": paths::clean(path, style),
        "leaf": paths::leaf(path, style),
        "native": paths::native(path, style),
        "split": paths::split(path, style),
        "join": paths::join(path, "child"),
    })
}

/// The fixture with every answer computed by the core.
fn answered(fixture: &Value) -> Value {
    let mut out = fixture.clone();
    for entry in out["paths"].as_array_mut().expect("paths") {
        let path = text(entry, "path");
        *entry = json!({
            "path": path,
            "posix": single(&path, Style::Posix),
            "windows": single(&path, Style::Windows),
        });
    }
    for entry in out["pairs"].as_array_mut().expect("pairs") {
        let (path, folder) = (text(entry, "path"), text(entry, "folder"));
        for style in [Style::Posix, Style::Windows] {
            entry[style_name(style)] = json!({
                "same": paths::same(&path, &folder, style),
                "within": paths::within(&path, &folder, style),
            });
        }
    }
    for entry in out["styles"].as_array_mut().expect("styles") {
        let platform = entry["platform"].as_str().map(str::to_owned);
        let local = entry["local"].as_bool().expect("local");
        // Without a platform this computer follows the build: the fixture says so.
        let follows_build = local && platform.as_deref().is_none_or(str::is_empty);
        entry["style"] = if follows_build {
            json!("build")
        } else {
            json!(style_name(paths::of(platform.as_deref(), local)))
        };
    }
    for entry in out["matching_project_folder"]
        .as_array_mut()
        .expect("matching_project_folder")
    {
        let project = project(entry);
        entry["answer"] = json!(launch::matching_project_folder(
            &text(entry, "path"),
            &text(entry, "canonical"),
            project.as_ref(),
            &text(entry, "host"),
            style(entry),
        ));
    }
    for entry in out["prefill_target"]
        .as_array_mut()
        .expect("prefill_target")
    {
        let project = project(entry);
        entry["answer"] = json!(launch::prefill_target(
            &entry["catalog"],
            &text(entry, "folder"),
            &text(entry, "canonical"),
            project.as_ref(),
            &text(entry, "host"),
            style(entry),
        ));
    }
    for entry in out["restored_folder"]
        .as_array_mut()
        .expect("restored_folder")
    {
        let (path, auto) = launch::restored_folder(&entry["plan"]);
        entry["answer"] = json!([path, auto]);
    }
    out
}

#[test]
fn the_shared_fixture_holds_the_cores_answers() {
    let fixture = read();
    assert_eq!(fixture["version"], 1);
    let expected = answered(&fixture);
    for section in [
        "paths",
        "pairs",
        "styles",
        "matching_project_folder",
        "prefill_target",
        "restored_folder",
    ] {
        let (found, wanted) = (&fixture[section], &expected[section]);
        assert!(
            found.as_array().is_some_and(|list| !list.is_empty()),
            "{section} is empty"
        );
        for (index, (found, wanted)) in found
            .as_array()
            .unwrap()
            .iter()
            .zip(wanted.as_array().unwrap())
            .enumerate()
        {
            assert_eq!(found, wanted, "{section}[{index}]");
        }
    }
    // The build style the fixture leaves to each side.
    assert_eq!(paths::of(None, true), paths::build());
    assert_eq!(paths::of(Some(""), true), paths::build());
}

#[test]
#[ignore = "writes the fixture's answers; run after a change to the rules"]
fn regenerate() {
    let mut text = serde_json::to_string_pretty(&answered(&read())).expect("JSON");
    text.push('\n');
    std::fs::write(file(), text).expect("write fixture");
}
