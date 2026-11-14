use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::{json, Value};

fn seed() -> Harness {
    let h = Harness::new();
    h.apply(&person(), crate::teams::ops::tests::create());
    h
}
fn input() -> Value {
    json!({"title":"Foundation","barrier":true,"nodes":[
        {"id":1,"title":"Build the foundation","text":"Implement the shared part.","criteria":["The foundation tests pass"],"role":"developer","pipeline":"reviewed","writes":["src/base.rs"]},
        {"id":2,"title":"Use the foundation","text":"Use the new contract.","criteria":["The feature tests pass"],"role":"developer","pipeline":"reviewed","deps":[{"node":1,"kind":"hard"}],"writes":["src/feature.rs"]}
    ]})
}
fn produced() -> (Value, Value) {
    let h = seed();
    let (v,exit)=h.cli(&Caller::Person,"submit-draft-phase",&json!({"request_id":uid(700),"team":TEAM,"expected_revision":h.revision(),"phase":1,"draft":input()}).to_string());
    assert_eq!(exit, 0, "{v}");
    let (list, e) = h.cli(&Caller::Person, &format!("phases --team {TEAM}"), "");
    assert_eq!(e, 0, "{list}");
    let (detail, e) = h.cli(
        &Caller::Person,
        &format!("phase --team {TEAM} --phase 1"),
        "",
    );
    assert_eq!(e, 0, "{detail}");
    (list, detail)
}

#[test]
fn phase_reads_and_edits_use_the_real_writer_and_refuse_members() {
    let h = seed();
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    let request = json!({"request_id":uid(700),"team":TEAM,"expected_revision":h.revision(),"phase":1,"draft":input()});
    let (v, e) = h.cli(
        &Caller::Session(key(LEAD_SESSION)),
        "submit-draft-phase",
        &request.to_string(),
    );
    assert_eq!(e, 0, "{v}");
    for line in [
        format!("phases --team {TEAM}"),
        format!("phase --team {TEAM} --phase 1"),
    ] {
        let (v, e) = h.cli(&Caller::Session(key(M1_SESSION)), &line, "");
        assert_eq!(e, 1);
        assert_eq!(v["error"]["code"], "not_allowed");
    }
    let (v, e) = h.cli(
        &Caller::Person,
        &format!("phase --team {TEAM} --phase 1"),
        "",
    );
    assert_eq!(e, 0, "{v}");
    assert_eq!(v["draft"]["nodes"].as_array().unwrap().len(), 2);
    let mut next = input();
    next["title"] = json!("Edited by the person");
    let (v,e)=h.cli(&Caller::Person,"edit-draft-phase",&json!({"request_id":uid(701),"team":TEAM,"expected_revision":h.revision(),"phase":1,"generation":1,"draft":next}).to_string());
    assert_eq!(e, 0, "{v}");
    assert_eq!(
        h.store().phase_draft(TEAM, 1).unwrap().phase.title,
        "Edited by the person"
    );
    let (v, e) = h.cli(
        &Caller::Person,
        &format!("phases --team {TEAM} --limit 1"),
        "",
    );
    assert_eq!(e, 0, "{v}");
    assert_eq!(v["rows"].as_array().unwrap().len(), 1);
    assert!(v["next"].is_string());
    let (last, e) = h.cli(
        &Caller::Person,
        &format!(
            "phases --team {TEAM} --limit 1 --after {}",
            v["next"].as_str().unwrap()
        ),
        "",
    );
    assert_eq!(e, 0, "{last}");
    assert_eq!(last["rows"][0]["n"], 1);
    assert!(h.store().queue(TEAM, 200, None).unwrap().rows.is_empty());
}

#[test]
fn phase_fixtures_equal_the_cli_output_of_the_product_writers() {
    let (list, detail) = produced();
    assert_eq!(
        list,
        serde_json::from_str::<Value>(include_str!(
            "../../../../crates/team-plan/fixtures/cli-phases.json"
        ))
        .unwrap()
    );
    assert_eq!(
        detail,
        serde_json::from_str::<Value>(include_str!(
            "../../../../crates/team-plan/fixtures/cli-phase-draft.json"
        ))
        .unwrap()
    );
}

#[test]
#[ignore = "writes only the new phase fixtures from the real writers"]
fn regenerate_phase_draft_fixtures() {
    let (list, detail) = produced();
    for (name, v) in [("cli-phases.json", list), ("cli-phase-draft.json", detail)] {
        std::fs::write(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("crates/team-plan/fixtures")
                .join(name),
            serde_json::to_string_pretty(&v).unwrap() + "\n",
        )
        .unwrap();
    }
}
