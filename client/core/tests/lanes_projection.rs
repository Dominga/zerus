//! The nested stage keeps the holder already written by the real hand-out.
use serde_json::{json, Value};
use zerus_client_core::teams::view;

fn fixture() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/teams/fixtures/lanes-view.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn project(data: &Value, queue: &Value) -> Value {
    serde_json::to_value(view::team(&data["source_status"], queue)).unwrap()
}

fn review(projected: &Value) -> &Value {
    projected["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["cards"].as_array().unwrap())
        .find(|card| card["node"] == 1)
        .unwrap()["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["node"] == 2)
        .unwrap()
}

#[test]
fn a_product_delivered_stage_keeps_its_holder_when_folded_under_the_task() {
    let data = fixture();
    let queue = &data["source_queue"];
    let source = queue["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == 2)
        .unwrap();
    assert_eq!(source["assignee"], Value::Null);
    assert_eq!(source["holder"], "m3");
    let projected = project(&data, queue);
    assert_eq!(review(&projected)["assignee"], "");
    assert_eq!(review(&projected)["holder"], source["holder"]);
}

#[test]
fn legacy_absent_null_and_synthetic_invalid_or_oversized_holders_are_omitted() {
    let data = fixture();
    for invalid in [
        None,
        Some(Value::Null),
        Some(json!("")),
        Some(json!(17)),
        Some(json!({"member":"m3"})),
        Some(json!("m".repeat(view::NAME_CHARS + 1))),
    ] {
        // Deliberately corrupt one metadata field of the product-written row.
        let mut queue = data["source_queue"].clone();
        let row = queue["rows"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["id"] == 2)
            .unwrap()
            .as_object_mut()
            .unwrap();
        if let Some(value) = invalid {
            row.insert("holder".into(), value);
        } else {
            row.remove("holder");
        }
        let projected = project(&data, &queue);
        assert!(review(&projected).get("holder").is_none(), "{projected}");
    }
}
