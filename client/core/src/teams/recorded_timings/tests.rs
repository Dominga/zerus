//! Raw data is pinned by root product-writer/read tests; malformed controls named.
use super::*;
use crate::actions::Exit;
use crate::teams::drawers::{decode, DrawerKind, DrawerRead};
use serde_json::json;
fn raw() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../crates/team-plan/fixtures/cli-journal-timings.json"
    ))
    .unwrap()
}
fn read(value: Value) -> Result<crate::teams::drawers::DrawerPage, TeamError> {
    decode(
        &DrawerRead {
            team: value["team"].as_str().unwrap().into(),
            kind: DrawerKind::Feed,
            after: None,
            recorded_timings: true,
            timing_seqs: None,
            timing_next: None,
        },
        Exit::Code {
            code: 0,
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: String::new(),
        },
    )
}
#[test]
fn writer_produced_effect_and_stage_facts_reach_the_bounded_core_view() {
    let value = raw();
    let page = read(value.clone()).unwrap();
    assert_eq!(page.rows.len(), value["rows"].as_array().unwrap().len());
    assert_eq!(
        page.timings.as_ref().unwrap().revision,
        value["timings"]["revision"]
    );
    assert!(page
        .rows
        .iter()
        .any(|r| r.timing.as_ref().unwrap().effect.is_some()));
    assert!(page
        .rows
        .iter()
        .any(|r| r.timing.as_ref().unwrap().stage.is_some()));
    assert!(page
        .rows
        .iter()
        .filter_map(|r| r.timing.as_ref().unwrap().effect.as_ref())
        .all(|e| e.bookkeeping_elapsed_ms == Some(0)));
    let pinned: Value =
        serde_json::from_str(include_str!("../fixtures/recorded-timings.json")).unwrap();
    assert_eq!(
        serde_json::to_value(page).unwrap(),
        pinned,
        "the UI timing fixture is the core projection of the product read"
    );
}

#[test]
fn handwritten_missing_partial_unknown_and_conflicting_frames_stay_unavailable() {
    let value = raw();
    for meta in [
        Value::Null,
        json!({"schema":2,"revision":1}),
        json!({"schema":1,"revision":0}),
        json!({"schema":1,"revision":1,"new":true}),
    ] {
        let mut bad = value.clone();
        bad["timings"] = meta;
        assert!(read(bad).is_err());
    }
    let mut partial = value.clone();
    partial["rows"][0]["timing"] = json!({});
    assert!(read(partial).is_err());
    let mut future = value.clone();
    future["rows"][0]["timing"]["unreviewed"] = json!(true);
    assert!(read(future).is_err());
    let mut contradictory = value;
    let row = contradictory["rows"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["timing"]["effect"].is_object())
        .unwrap();
    row["timing"]["effect_unavailable"] = json!("record_missing");
    assert!(read(contradictory).is_err());
}
#[test]
fn handwritten_backward_and_overflowing_clocks_never_become_zero_elapsed() {
    assert_eq!(elapsed(20, 19), None);
    assert_eq!(elapsed(i64::MIN, i64::MAX), None);
    assert_eq!(elapsed(1, 9_007_199_254_740_993), None);
    assert_eq!(elapsed(20, 20), Some(0));
    let mut value = raw();
    let row = value["rows"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["timing"]["effect"].is_object())
        .unwrap();
    row["timing"]["effect"]["reserved_ms"] = json!(2000);
    row["timing"]["effect"]["first_outcome_recorded_ms"] = json!(1000);
    let page = read(value).unwrap();
    let fact = page
        .rows
        .iter()
        .find_map(|r| r.timing.as_ref()?.effect.as_ref())
        .unwrap();
    assert_eq!(fact.bookkeeping_elapsed_ms, None);
    assert!(fact.clock_or_range_unknown);
}

#[test]
fn selected_event_join_keeps_the_original_opaque_next_and_rejects_another_set() {
    let value = raw();
    let ids = value["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["seq"].as_u64().unwrap())
        .collect::<Vec<_>>();
    let mut asked = DrawerRead {
        team: value["team"].as_str().unwrap().into(),
        kind: DrawerKind::Feed,
        after: None,
        recorded_timings: true,
        timing_seqs: Some(ids),
        timing_next: Some("t1.original-window".into()),
    };
    let exit = || Exit::Code {
        code: 0,
        stdout: serde_json::to_vec(&value).unwrap(),
        stderr: String::new(),
    };
    let page = decode(&asked, exit()).unwrap();
    assert_eq!(page.next.as_deref(), Some("t1.original-window"));
    assert!(page.timeline.unwrap().later_page);
    asked.timing_seqs.as_mut().unwrap().pop();
    assert!(decode(&asked, exit()).is_err());
}
