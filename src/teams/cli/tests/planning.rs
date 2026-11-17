//! Person-positive setup uses an explicit admitted-writer model; reads and
//! Session/negative caller checks still use the production CLI unchanged.
//! Only input requests are authored here; every row is produced by the CLI.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::{json, Value};

fn input() -> Value {
    json!({"request_id":uid(0xfa01),"team":TEAM,"expected_revision":0,
        "project":"example-project","name":"Planned release","goal":"Prepare a release with reviewable acceptance criteria.",
        "home":MACHINE,"integration_branch":"zerus/team/planned-release","base_commit":COMMIT})
}
fn start(h: &Harness) -> Value {
    let (reply, exit) = h.native_command("start-planning --template bugfix", &input().to_string());
    assert_eq!(exit, 0, "{reply}");
    reply
}
fn produced() -> Value {
    let h = Harness::new();
    // Seed the real creation writer at the harness's fixed clock. The CLI
    // creation/replay path is exercised separately; its wall-clock timestamp
    // must not make a committed inbox fixture vary on every regeneration.
    let input = input();
    let field = |key: &str| input[key].as_str().unwrap().to_owned();
    let bundle = zerus_team_plan::bundle(
        &crate::teams::catalog::load(&h.config.dir).unwrap(),
        "bugfix",
    )
    .unwrap();
    h.apply(
        &person(),
        crate::teams::ops::Operation::StartPlanning {
            expected_template: None,
            delegations: None,
            project: field("project"),
            name: field("name"),
            goal: field("goal"),
            home: field("home"),
            bundle: Box::new(bundle),
            integration_branch: field("integration_branch"),
            base_commit: field("base_commit"),
        },
    );
    let mut out = serde_json::Map::new();
    for command in ["status", "queue", "inbox"] {
        let (reply, exit) = h.cli(&Caller::Person, &format!("{command} --team {TEAM}"), "");
        assert_eq!(exit, 0, "{reply}");
        out.insert(command.into(), reply);
    }
    Value::Object(out)
}
fn destination() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/team-plan/fixtures/cli-planning.json")
}

#[test]
fn planning_cli_replay_keeps_the_original_bundle_after_the_catalog_changes() {
    let h = Harness::new();
    let first = start(&h);
    let mut template = crate::teams::catalog::load(&h.config.dir)
        .unwrap()
        .templates
        .into_iter()
        .find(|t| t.id == "bugfix")
        .unwrap();
    template.name.push_str(" Changed");
    // The public import writer changes the catalog, never the request row.
    let (saved, exit) = h.cli(
        &Caller::Person,
        "template save",
        &serde_json::to_string(&template).unwrap(),
    );
    assert_eq!(exit, 0, "{saved}");
    assert_eq!(start(&h), first);
    let (wrong, exit) = h.native_command("create --template bugfix", &input().to_string());
    assert_eq!(exit, 1);
    assert_eq!(code(&wrong), "duplicate");
    assert_eq!(h.store().inbox(TEAM, 200, None).unwrap().rows.len(), 1);
}

#[test]
fn a_planning_lead_reads_its_original_goal_from_current() {
    let h = Harness::new();
    start(&h);
    h.bind("lead", LEAD_SESSION);
    let (current, exit) = h.cli(&Caller::Session(key(LEAD_SESSION)), "current", "");
    assert_eq!(exit, 0, "{current}");
    assert_eq!(current["planning"]["goal"], input()["goal"]);
    assert_eq!(current["planning"]["state"]["paused"], false);
    assert_eq!(current["nodes"], json!([]));
    let (refused, exit) = h.cli(
        &Caller::Session(key(LEAD_SESSION)),
        "start-planning --template bugfix",
        &input().to_string(),
    );
    assert_eq!(exit, 1);
    assert_eq!(code(&refused), "not_allowed");
}

#[test]
fn planning_fixture_equals_real_cli_writer_output() {
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(destination()).unwrap()).unwrap();
    assert_eq!(produced(), expected);
}

#[test]
#[ignore = "regenerates the planning fixture through the real CLI writers"]
fn regenerate_planning_fixture() {
    std::fs::write(
        destination(),
        serde_json::to_string_pretty(&produced()).unwrap() + "\n",
    )
    .unwrap();
}
