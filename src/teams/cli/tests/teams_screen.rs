//! The Teams screen's pages come from the product operations and core views.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::{json, Value};
use zerus_client_core::teams::view;

fn read(h: &Harness, command: &str) -> Value {
    let (reply, exit) = h.cli(&Caller::Person, command, "");
    assert_eq!(exit, 0, "{reply}");
    reply
}

fn payload() -> Value {
    let h = Harness::new();
    let mut last = String::new();
    for n in 1..=7 {
        let id = uid(0x2000 + n);
        h.create_team(&id, &format!("Team {n}"), &format!("zerus/team/browse-{n}"));
        h.bind_for(&id, "lead", &format!("launch_id:{}", uid(0x3000 + n)));
        last = id;
    }
    let mut pages = Vec::new();
    let mut after = None;
    loop {
        let command = after.as_ref().map_or_else(
            || "list --limit 1".into(),
            |cursor| format!("list --limit 1 --after {cursor}"),
        );
        let reply = read(&h, &command);
        let mut page = view::list(&reply);
        page.after = after;
        after = page.next.clone();
        pages.push(page);
        if after.is_none() {
            break;
        }
        assert!(pages.len() < 8, "the bounded fixture's cursor must advance");
    }
    let progress = super::progress::payload();
    json!({
        "pages": pages,
        "empty": view::list(&read(&Harness::new(), "list")),
        "team": view::team(&read(&h, &format!("status --team {last}")), &read(&h, &format!("queue --team {last}"))),
        "running": view::list(&progress["running"]),
        "stuck": view::list(&progress["stuck"]),
    })
}

fn path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("client/src/ui/screens/fixtures/teams-screen.json")
}

#[test]
fn teams_screen_fixture_is_written_by_product_operations_and_core_views() {
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(path()).unwrap()).unwrap();
    let actual = payload();
    assert_eq!(actual, expected);
    assert_eq!(actual["pages"].as_array().unwrap().len(), 8);
    assert_eq!(actual["pages"][7]["next"], Value::Null);
    assert_eq!(actual["empty"]["teams"], json!([]));
}

#[test]
#[ignore = "regenerates the Teams screen fixture from the product writers"]
fn regenerate_teams_screen_fixture() {
    let path = path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&payload()).unwrap()),
    )
    .unwrap();
}
