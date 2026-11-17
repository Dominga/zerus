// Availability controls inject faults into the real product reply. They do not
// hand-write saved rows, and no kernel refusal is reclassified as a warning.
use super::*;
use serde_json::{json, Value};

fn reply() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../../crates/team-plan/fixtures/cli-phase-context.json"
    ))
    .unwrap()
}

#[test]
fn a_published_phase_never_falls_back_to_retained_draft_estimates() {
    let mut value = reply()["draft"].clone();
    value["draft"]["phase"]["state"] = json!("approved");
    value["draft"]["editable"] = json!(false);
    for node in value["draft"]["nodes"].as_array_mut().unwrap() {
        node["estimate_minutes"] = json!(30);
    }
    let view = crate::upstream_teams::view::phases::draft(&value, 1).unwrap();
    assert_eq!(
        view.metrics.unavailable,
        Some(Unavailable::LiveSnapshotUnavailable)
    );
    assert_eq!(view.metrics.critical_path_minutes, None);
}

#[test]
fn an_unknown_draft_field_keeps_numbers_unavailable() {
    let mut value = reply()["draft"].clone();
    value["draft"]["nodes"][0]["future_input"] = json!(true);
    let view = crate::upstream_teams::view::phases::draft(&value, 1).unwrap();
    assert_eq!(view.metrics.unavailable, Some(Unavailable::UnknownFields));
    assert!(view.metrics.warnings.is_empty());
}

#[test]
fn operation_written_unknown_estimates_stay_unknown_and_partial_reads_stay_unavailable() {
    let value = reply()["draft"].clone();
    let view = crate::upstream_teams::view::phases::draft(&value, 1).unwrap();
    assert_eq!(
        view.metrics.unavailable,
        Some(Unavailable::MissingEstimates)
    );
    assert_eq!(view.metrics.known_estimates, 0);
    assert_eq!(view.metrics.total_nodes, 2);
    let mut value = value;
    value["draft"]["complete"] = json!(false);
    value["draft"]["editable"] = json!(false);
    let view = crate::upstream_teams::view::phases::draft(&value, 1).unwrap();
    assert_eq!(view.metrics.unavailable, Some(Unavailable::IncompletePhase));
}
