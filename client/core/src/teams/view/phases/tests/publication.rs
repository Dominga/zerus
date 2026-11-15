use super::*;

fn cases()->Value {
    serde_json::from_str(include_str!("../../../../../../../crates/team-plan/fixtures/cli-phase-publication.json")).unwrap()
}
fn produced()->Value {
    Value::Object(cases().as_object().unwrap().iter().map(|(stage,context)| {
        (stage.clone(),serde_json::json!({"team":wire_from(context),"waiting":crate::teams::view::waiting(&context["waiting"])}))
    }).collect())
}

#[test]
fn publication_writer_contexts_reach_reader_model_coalescer_and_window() {
    let out=produced();
    let published=&out["published"]["team"]["plan"]["draft"];
    assert_eq!(published["live"]["complete"],true);
    assert_eq!(published["live"]["nodes"].as_array().unwrap().len(),2);
    assert_eq!(published["metrics"]["critical_path_minutes"],60);
    assert_eq!(out["partial"]["team"]["plan"]["draft"]["publication"]["created"],1);
    let waiting=&out["draft"]["waiting"]["items"][0];
    assert_eq!(waiting["phase_reference"]["phase"],1);
    assert_eq!(waiting["phase_reference"]["generation"],1);
    let awaiting=&out["awaiting"]["team"]["plan"]["draft"];
    assert_eq!(awaiting["accept_available"],true);
    assert!(awaiting["live"]["nodes"].as_array().unwrap().iter().all(|n|n["state"]=="accepted"));
    assert_eq!(out["accepted"]["team"]["plan"]["draft"]["phase"]["state"],"done");
    assert_eq!(out,serde_json::from_str::<Value>(include_str!("../../../../../../src/ui/teams/fixtures/phase-publication.json")).unwrap());
}

#[test]
fn published_missing_or_incomplete_live_scopes_never_reuse_the_frozen_draft_metrics() {
    let base=cases()["published"].clone();
    for mode in 0..3 {
        let mut context=base.clone();
        match mode {
            0=>{context["draft"]["draft"].as_object_mut().unwrap().remove("live");}
            1=>{context["draft"]["draft"]["live"]=Value::Null;}
            _=>{context["draft"]["draft"]["live"]["complete"]=false.into();context["draft"]["draft"]["live"]["nodes"].as_array_mut().unwrap().truncate(1);}
        }
        let view=wire_from(&context);
        let draft=&view["plan"]["draft"];
        assert!(draft["metrics"]["critical_path_minutes"].is_null(),"{mode}");
        assert!(draft["metrics"]["average_parallelism_tenths"].is_null(),"{mode}");
        assert_eq!(draft["graph"]["complete"],false);
        assert_eq!(draft["accept_available"],false);
    }
}

#[test]
fn malformed_live_scopes_are_unavailable_in_the_actual_model_read_path() {
    let base=cases()["published"].clone();
    for fault in 0..7 {
        let mut context=base.clone();let live=&mut context["draft"]["draft"]["live"];
        match fault {
            0=>live["revision"]=9999.into(),
            1=>live["phase"]=2.into(),
            2=>live["nodes"][0]["phase"]=2.into(),
            3=>live["nodes"][1]["id"]=live["nodes"][0]["id"].clone(),
            4=>{live["nodes"].as_array_mut().unwrap().pop();}
            5=>live["nodes"][0]["future_runtime"]=true.into(),
            _=>live["nodes"]=serde_json::json!([]),
        }
        let out=wire_from(&context);
        assert!(!out["plan"]["draft_failure"].is_null(),"fault {fault}");
        assert!(out["plan"]["draft"].is_null(),"fault {fault}");
    }
}

#[test]
#[ignore = "writes only the UI publication fixture through real core/model/batch"]
fn regenerate_publication_window_fixture() {
    std::fs::write(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/ui/teams/fixtures/phase-publication.json"),serde_json::to_string_pretty(&produced()).unwrap()+"\n").unwrap();
}
