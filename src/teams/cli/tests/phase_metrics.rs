//! Estimated phase inputs are admitted and serialized by the real product.
use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::{json, Value};

fn produced() -> Value {
    let h = Harness::new();
    h.apply(&person(), crate::teams::ops::tests::create());
    let nodes: Vec<_> = [
        (1, 10, vec![]),
        (2, 20, vec![1]),
        (3, 30, vec![1]),
        (4, 5, vec![2, 3]),
    ]
    .into_iter()
    .map(|(id, estimate, deps)| {
        json!({"id":id,"title":format!("Estimated task {id}"),"text":"Implement the planned work.",
                "criteria":["The targeted checks pass"],"role":"developer","pipeline":"reviewed",
                "estimate_minutes":estimate,"writes":[format!("src/metric-{id}.rs")],
                "deps":deps.into_iter().map(|node|json!({"node":node})).collect::<Vec<_>>()})
    })
    .collect();
    let request = json!({"request_id":uid(740),"team":TEAM,"expected_revision":h.revision(),"phase":1,
        "draft":{"title":"Estimated plan","nodes":nodes}});
    let (reply, exit) = h.cli(&Caller::Person, "submit-draft-phase", &request.to_string());
    assert_eq!(exit, 0, "{reply}");
    let mut context = serde_json::Map::new();
    for (key, command) in [
        ("status", format!("status --team {TEAM}")),
        ("queue", format!("queue --team {TEAM}")),
        ("phases", format!("phases --team {TEAM}")),
        ("draft", format!("phase --team {TEAM} --phase 1")),
    ] {
        let (mut reply, exit) = h.cli(&Caller::Person, &command, "");
        assert_eq!(exit, 0, "{reply}");
        // Keep the reader's actual caller metadata. Service health is not a
        // metric input and need not enter this otherwise stable fixture.
        if key == "status" {
            reply = json!({"ok":true,"actor":reply["actor"],"team":reply["team"],"members":reply["members"],"counts":reply["counts"]});
        }
        context.insert(key.into(), reply);
    }
    Value::Object(context)
}

#[test]
fn estimated_phase_fixture_is_the_product_writers_output() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/team-plan/fixtures/cli-phase-metrics.json");
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(produced(), expected);
}

#[test]
#[ignore = "writes the metric fixture through actual operations and CLI readers"]
fn regenerate_estimated_phase_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("crates/team-plan/fixtures/cli-phase-metrics.json");
    std::fs::write(
        path,
        serde_json::to_string_pretty(&produced()).unwrap() + "\n",
    )
    .unwrap();
}
