//! SessionOrganization::addFolder (258-269), through the existing edit Controller.
use serde_json::{json, Value};
use zerus_client_core::paths::Style;
use zerus_client_core::swarm::edit::{apply, Intent};

fn organization() -> Value {
    json!({"projects": [{"id": "p", "name": "Project", "sessions": [], "folders": []}]})
}

fn intent(machine: &str, path: &str, style: Style) -> Intent {
    Intent::AddFolder {
        id: "11111111-2222-4333-8444-555555555555".into(),
        project: "p".into(),
        machine: machine.into(),
        path: path.into(),
        name: "  Work  ".into(),
        style,
    }
}

#[test]
fn add_folder_cleans_the_path_and_keeps_an_existing_identity_and_name() {
    let first = apply(
        &organization(),
        &intent("studio", "/srv/repo/../work/", Style::Posix),
    )
    .unwrap();
    let folder = &first["projects"][0]["folders"][0];
    assert!(zerus_client_core::creation::canonical_uuid(
        folder["id"].as_str().unwrap()
    ));
    assert_eq!(folder["id"], "11111111-2222-4333-8444-555555555555");
    assert_eq!(
        apply(
            &organization(),
            &intent("studio", "/srv/repo/../work/", Style::Posix)
        )
        .unwrap(),
        first,
        "the edit is deterministic"
    );
    assert_eq!(folder["machine"], "studio");
    assert_eq!(folder["path"], "/srv/work");
    assert_eq!(folder["name"], "Work");
    assert_eq!(
        apply(&first, &intent("studio", "/srv/work", Style::Posix)).unwrap(),
        first
    );
    assert_eq!(
        apply(&first, &intent("other", "/srv/work", Style::Posix)).unwrap()["projects"][0]
            ["folders"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn add_folder_uses_the_owning_computers_style_and_eighty_character_name() {
    let mut choice = intent("desktop", "C:\\Work\\..\\Repo\\", Style::Windows);
    if let Intent::AddFolder { name, .. } = &mut choice {
        *name = format!("  {}  ", "x".repeat(81));
    }
    let first = apply(&organization(), &choice).unwrap();
    assert_eq!(first["projects"][0]["folders"][0]["path"], "C:/Repo");
    assert_eq!(first["projects"][0]["folders"][0]["name"], "x".repeat(80));
    assert_eq!(
        apply(&first, &intent("desktop", "c:/repo", Style::Windows)).unwrap(),
        first
    );
}

#[test]
fn add_folder_keeps_qt_s_no_ops_for_blank_computers_bad_paths_and_gone_projects() {
    for (machine, path) in [
        ("  ", "/srv/repo"),
        ("studio", "repo"),
        ("studio", "/srv/repo\nother"),
    ] {
        assert_eq!(
            apply(&organization(), &intent(machine, path, Style::Posix)).unwrap(),
            organization()
        );
    }
    assert_eq!(
        apply(
            &json!({"projects": []}),
            &intent("studio", "/srv/repo", Style::Posix)
        )
        .unwrap(),
        json!({"projects": []})
    );
}
