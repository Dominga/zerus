use super::*;

fn completion(field: Option<(&str, Value)>) -> Value {
    let mut item = json!({"type":"CommandExecution","id":"call","command":"synthetic command",
        "status":"completed","exit_code":0});
    if let Some((name, value)) = field {
        item[name] = value;
    }
    json!({"timestamp":"2026-11-14T00:00:00Z","payload":{"type":"item_completed","item":item}})
}

#[test]
fn native_completion_metadata_keeps_output_absent_without_changing_the_projection() {
    let r = record("codex");
    let key = id(&r, "main", "call");
    let mut empty = Jobs::new();
    native::codex(
        &r,
        &[completion(Some(("aggregated_output", json!(""))))],
        &mut empty,
    );
    let baseline = projection(&empty, vec![], false);
    for field in [None, Some(Value::Null), Some(json!(0)), Some(json!({}))] {
        let mut jobs = Jobs::new();
        native::codex(
            &r,
            &[completion(field.map(|value| ("aggregated_output", value)))],
            &mut jobs,
        );
        let job = &jobs[&key];
        assert!(
            job.get("output").is_none(),
            "metadata is not output evidence"
        );
        assert_eq!(job["status"], "completed");
        assert_eq!(job["exit_code"], 0);
        assert_eq!(job["observed"], false);
        let projected = projection(&jobs, vec![], false);
        for field in [
            "items",
            "active_count",
            "live_count",
            "unknown_count",
            "complete",
            "notes",
        ] {
            assert_eq!(projected[field], baseline[field], "{field}");
        }
    }
}

#[test]
fn native_explicit_empty_and_nonempty_output_survive_later_metadata_only_rows() {
    let r = record("codex");
    let key = id(&r, "main", "call");
    for field in ["aggregated_output", "aggregatedOutput"] {
        for output in ["", "retained native output"] {
            let mut jobs = Jobs::new();
            native::codex(
                &r,
                &[completion(Some((field, json!(output)))), completion(None)],
                &mut jobs,
            );
            assert_eq!(jobs[&key]["output"].as_str(), Some(output));
            assert_eq!(jobs[&key]["status"], "completed");
            assert_eq!(jobs[&key]["output_truncated"], false);
        }
    }
}

#[test]
fn missing_function_result_does_not_turn_a_native_call_into_empty_output() {
    let r = record("codex");
    let key = id(&r, "main", "call");
    for missing in [Value::Null, json!(0), json!({"exit_code":0})] {
        let mut jobs = Jobs::new();
        let row = |payload| json!({"timestamp":"2026-11-14T00:00:00Z","payload":payload});
        native::codex(
            &r,
            &[
                row(
                    json!({"type":"function_call","name":"exec_command","call_id":"call","arguments":"{\"cmd\":\"synthetic command\"}"}),
                ),
                row(json!({"type":"function_call_output","call_id":"call","output":missing})),
            ],
            &mut jobs,
        );
        assert!(jobs[&key].get("output").is_none());
        native::codex(
            &r,
            &[row(
                json!({"type":"function_call_output","call_id":"call","output":""}),
            )],
            &mut jobs,
        );
        assert_eq!(jobs[&key]["output"].as_str(), Some(""));
    }
}

#[test]
fn explicit_empty_provider_results_and_existing_hook_output_remain_available() {
    let r = record("codex");
    for response in [
        json!(""),
        json!([]),
        json!({"stdout":""}),
        json!({"stderr":""}),
        json!({"output":""}),
        json!({"content":[]}),
        json!({"structuredContent":{"output":""}}),
    ] {
        let mut job = new_job(&r, "main", "call", "synthetic command", 1.);
        job.as_object_mut().unwrap().remove("output");
        result(&mut job, &response, false, 2.);
        assert_eq!(job["output"].as_str(), Some(""), "{response}");
    }
    for output in ["", "retained hook output"] {
        let mut job = new_job(&r, "main", "call", "synthetic command", 1.);
        job["output"] = json!(output);
        result(&mut job, &json!({"exit_code":0}), false, 2.);
        assert_eq!(job["output"].as_str(), Some(output));
        assert_eq!(job["status"], "completed");
    }
}
