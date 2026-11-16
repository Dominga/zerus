//! Operation-written published phase and observations -> reader/model -> batch.
use serde_json::Value;
use std::time::Instant;
use zerus_client_core::{
    actions::Exit,
    teams::{
        observed_timing::DEFAULT_WINDOW_MS, read::decode, TeamsFollow, TeamsModel, TeamsUpdate,
    },
    Change, Coalescer, ComputerId,
};

fn source() -> Value {
    serde_json::from_str(include_str!(
        "../../../crates/team-plan/fixtures/cli-observed-phase.json"
    ))
    .unwrap()
}
fn apply(model: &mut TeamsModel, value: Value, now: Instant) -> Vec<(String, TeamsUpdate)> {
    let read = model.next(now).expect("a due scoped read");
    let reply = decode(
        &read,
        Exit::Code {
            code: 0,
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: String::new(),
        },
    );
    model.apply(&read, reply, now)
}
fn loaded(value: &Value, now: Instant) -> TeamsModel {
    let team = value["team"].as_str().unwrap();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            team: Some(team.into()),
            ..Default::default()
        },
        now,
    );
    for key in ["status", "queue"] {
        apply(&mut model, value["context"][key].clone(), now);
    }
    model.follow_phases(team, Some(1), true, now).unwrap();
    for key in ["phases", "draft"] {
        apply(&mut model, value["context"][key].clone(), now);
    }
    model
        .follow_observed(team, 1, None, DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    model
}
fn batch(updates: Vec<(String, TeamsUpdate)>) -> Value {
    let mut coalescer = Coalescer::new();
    for (key, update) in updates {
        coalescer.push(Change::teams(ComputerId::local(), key, update));
    }
    let value = serde_json::to_value(coalescer.take()).unwrap();
    let _: zerus_client_core::Batch = serde_json::from_value(value.clone()).unwrap();
    value
}
#[test]
fn publication_estimates_reach_the_actual_selected_page_without_fact_membership_backfill() {
    let value = source();
    let now = Instant::now();
    let mut model = loaded(&value, now);
    let wire = batch(apply(&mut model, value.clone(), now));
    let view = &wire["teams"][0]["payload"];
    assert_eq!(
        view["plan"]["estimates"]["scope"]["version"]["publication"],
        value["context"]["draft"]["draft"]["publication"]["id"]
    );
    assert_eq!(view["plan"]["estimates"]["rows"][0]["estimate_minutes"], 30);
}

fn view(value: &Value) -> Value {
    let now = Instant::now();
    let mut model = loaded(value, now);
    batch(apply(&mut model, value.clone(), now))["teams"][0]["payload"].clone()
}
fn preview_source() -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/team-plan/fixtures/cli-estimate-preview.json");
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
#[test]
fn an_empty_lazy_page_does_not_hide_known_unapproved_product_estimates() {
    let source = preview_source();
    let value = view(&source);
    let estimates = &value["plan"]["estimates"];
    assert_eq!(estimates["scope"]["basis"], "unapproved_preview");
    assert_eq!(estimates["rows"][0]["estimate_minutes"], 30);
    assert!(estimates["units"].as_array().unwrap().is_empty());
    assert!(estimates["window"]["to_ms"].is_null());
    assert_eq!(value["plan"]["observed"]["page"]["as_of_ms"], Value::Null);
}
#[test]
fn original_unit_membership_is_not_filled_from_the_selected_publication() {
    let value = view(&source());
    let units = value["plan"]["estimates"]["units"].as_array().unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0]["unknown"], "original_publication_unavailable");
    assert!(units[0]["estimate_minutes"].is_null());
    assert!(units[0]["coverage"]["key"]["publication"].is_null());
    assert!(units[0]["coverage"]["implementation_observed_ms"].is_null());
}
#[test]
fn valid_but_wrong_home_and_team_uuids_do_not_authorize_an_estimate_context() {
    let wrong = "00000000-0000-4000-8000-00000000abcd";
    for mode in 0..4 {
        let mut value = source();
        match mode {
            0 => {
                value["observed"]["scope"]["home"] = wrong.into();
                for u in value["observed"]["cohort"]["units"].as_array_mut().unwrap() {
                    u["key"]["home"] = wrong.into();
                }
                for f in value["observed"]["rows"].as_array_mut().unwrap() {
                    f["key"]["home"] = wrong.into();
                }
            }
            1 => value["context"]["status"]["team"]["home"] = wrong.into(),
            2 => value["context"]["status"]["team"]["id"] = wrong.into(),
            _ => {
                value["observed"]["scope"]["team"] = wrong.into();
                for u in value["observed"]["cohort"]["units"].as_array_mut().unwrap() {
                    u["key"]["team"] = wrong.into();
                }
                for f in value["observed"]["rows"].as_array_mut().unwrap() {
                    f["key"]["team"] = wrong.into();
                }
            }
        }
        let now = Instant::now();
        let mut model = loaded(&value, now);
        let result = batch(apply(&mut model, value, now));
        assert!(
            result["teams"][0]["payload"]["plan"]["estimates"].is_null(),
            "mode {mode}"
        );
    }
}
#[test]
fn mismatched_revisions_and_schema1_even_zero_actuals_are_unavailable() {
    for mode in 0..5 {
        let mut value = source();
        match mode {
            0 => value["context"]["status"]["team"]["revision"] = 13.into(),
            1 => value["observed"]["source_revision"] = 13.into(),
            2 => {
                value["context"]["draft"]["draft"]["revision"] = 13.into();
                value["context"]["draft"]["draft"]["live"]["revision"] = 13.into();
            }
            3 => value["observed"]["rows"][0]["implementation_observed_ms"] = 0.into(),
            _ => value["observed"]["rows"][0]["implementation_observed_ms"] = 10.into(),
        }
        assert!(view(&value)["plan"]["estimates"].is_null(), "mode {mode}");
    }
    assert_eq!(
        view(&source())["plan"]["estimates"]["rows"][0]["estimate_minutes"],
        30
    );
}
#[test]
fn actual_rework_frames_choose_retained_base_then_settled_new() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/team-plan/fixtures/cli-observed-rework.json");
    let inputs: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    let mut ids = Vec::new();
    for state in ["reserved", "published"] {
        let input = &inputs[state];
        let value = view(input);
        assert_eq!(
            value["plan"]["estimates"]["scope"]["version"]["publication"],
            input["context"]["draft"]["draft"]["publication"]["id"],
            "{state}"
        );
        assert_eq!(
            value["plan"]["estimates"]["scope"]["version"]["draft_generation"],
            input["context"]["draft"]["draft"]["generation"]
        );
        assert!(value["plan"]["estimates"]["units"]
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u["coverage"]["implementation_observed_ms"].is_null()
                && u["coverage"]["key"]["publication"].is_null()));
        ids.push(value["plan"]["estimates"]["scope"]["version"]["publication"].clone());
    }
    assert_ne!(ids[0], ids[1]);
}
#[test]
fn the_existing_single_poller_drops_derived_output_on_hide_and_reads_fresh_on_unhide() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };
    use std::time::Duration;
    use zerus_client_core::teams::{read::TeamRead, TeamsPoller};
    let (tx, rx) = mpsc::channel();
    let count = Arc::new(AtomicUsize::new(0));
    let reads = count.clone();
    let source = source();
    let team = source["team"].as_str().unwrap().to_owned();
    let input = source.clone();
    let poller = TeamsPoller::spawn_with(
        Box::new(move |read| {
            let value = match read {
                TeamRead::Status { .. } => input["context"]["status"].clone(),
                TeamRead::Queue { .. } => input["context"]["queue"].clone(),
                TeamRead::Phases { .. } => input["context"]["phases"].clone(),
                TeamRead::Draft { .. } => input["context"]["draft"].clone(),
                TeamRead::Observed { .. } => {
                    reads.fetch_add(1, Ordering::SeqCst);
                    input.clone()
                }
                _ => panic!("no unrelated or eager read"),
            };
            decode(
                read,
                Exit::Code {
                    code: 0,
                    stdout: serde_json::to_vec(&value).unwrap(),
                    stderr: String::new(),
                },
            )
        }),
        Box::new(move |change| {
            let _ = tx.send(change);
        }),
    )
    .unwrap();
    let await_state = |known: bool| {
        for _ in 0..30 {
            if let Change::Teams { payload, .. } = rx.recv_timeout(Duration::from_secs(2)).unwrap()
            {
                if payload["plan"].is_object() && payload["plan"]["estimates"].is_object() == known
                {
                    return payload;
                }
            }
        }
        panic!("derived pane state was not published")
    };
    poller.follow(TeamsFollow {
        team: Some(team.clone()),
        ..Default::default()
    });
    poller.phases(&team, Some(1), true, None).unwrap();
    poller
        .observed(&team, 1, None, DEFAULT_WINDOW_MS, true, None)
        .unwrap();
    let active = await_state(true);
    assert_eq!(
        active["plan"]["estimates"]["rows"][0]["estimate_minutes"],
        30
    );
    let before = count.load(Ordering::SeqCst);
    poller.hidden(true);
    let hidden = await_state(false);
    assert!(hidden["plan"]["observed"].is_null());
    poller.hidden(false);
    await_state(true);
    assert!(count.load(Ordering::SeqCst) > before);
}
fn produced_views() -> Value {
    let mut values = serde_json::Map::new();
    values.insert("published".into(), view(&source()));
    values.insert("preview".into(), view(&preview_source()));
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/team-plan/fixtures/cli-observed-rework.json");
    let inputs: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    for name in ["reserved", "published"] {
        values.insert(format!("rework_{name}"), view(&inputs[name]));
    }
    Value::Object(values)
}
fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/teams/fixtures/phase-estimates-view.json")
}
#[test]
fn mounted_estimate_frames_equal_the_actual_model_and_batch_writer() {
    assert_eq!(
        produced_views(),
        serde_json::from_slice::<Value>(&std::fs::read(fixture_path()).unwrap()).unwrap()
    );
}
#[test]
#[ignore = "writes estimate integration frames from operation-written root sources through model/coalescer/batch"]
fn regenerate_estimate_views() {
    std::fs::write(
        fixture_path(),
        serde_json::to_string_pretty(&produced_views()).unwrap() + "\n",
    )
    .unwrap();
}
