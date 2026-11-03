use super::*;

pub(super) fn record(agent: &str) -> Value {
    json!({"agent":agent,"conversation_id":"1a4f09e2-115c-4e49-9fa9-30d184d6ee18","run_id":"run-a","cwd":"/workspace"})
}
#[test]
fn native_work_requires_current_registry_and_ignores_historical_calls() {
    let r = record("dsh");
    assert!(dsh_work(&r, &json!({}), true)["count"].is_null());
    let native = json!({"jobs":[],"shellCalls":[{"callId":"a","owner":r["conversation_id"],"command":"build","status":"running","startedAt":1000},
        {"callId":"b","owner":r["conversation_id"],"command":"finished","status":"completed","startedAt":1000}]});
    assert_eq!(dsh_work(&r, &native, true)["count"], 1);
    assert_eq!(dsh_work(&r, &native, true)["command"], "build");
    assert!(dsh_work(&r, &native, false)["count"].is_null());
}
fn hook(kind: &str, call: &str, response: Value) -> Value {
    json!({"hook_event_name":kind,"tool_name":"Bash","tool_use_id":call,"tool_input":{"command":"echo same"},"tool_response":response})
}

#[test]
fn long_commands_have_byte_bounded_previews_and_full_text_digests() {
    let r = record("codex");
    let command = format!("cat <<'EOF'\n{}\nEOF", "Ж🦀 heredoc\n".repeat(2000));
    let job = new_job(&r, "main", "call", &command, 1.);
    assert!(string(&job, "command").len() <= 512);
    assert!(string(&job, "command").ends_with('…'));
    assert_eq!(job["command_truncated"], true);
    assert_eq!(
        job["command_digest"],
        format!("{:x}", Sha256::digest(command.as_bytes()))
    );
    let other = new_job(&r, "main", "call", &(command + "different suffix"), 1.);
    assert_eq!(job["id"], other["id"]);
    assert_eq!(job["command"], other["command"]);
    assert_ne!(job["command_digest"], other["command_digest"]);
}

#[test]
fn legacy_commands_load_and_migrate_on_a_record_write_without_changing_job_state() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("record.json");
    let command = format!("python <<'EOF'\n{}\nEOF", "heredoc body\n".repeat(1100));
    let mut r = record("codex");
    let jobs: serde_json::Map<String, Value> = (0..MAX_JOBS).map(|i| {
        (format!("job-{i}"), json!({"id":format!("job-{i}"),"call_id":format!("call-{i}"),"command":command,
            "run_id":r["run_id"],"conversation_id":r["conversation_id"],"status":"running","background":true,
            "observed":true,"native_id":format!("native-{i}"),"output":"retained output","capabilities":{"output":true,"stop":false}}))
    }).collect();
    r["shell_jobs"] = json!(jobs);
    let original = r.to_string();
    assert!(original.len() > 1 << 20);
    std::fs::write(&path, &original).unwrap();
    let mut loaded: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let before = projection(
        &serde_json::from_value(loaded["shell_jobs"].clone()).unwrap(),
        vec![],
        false,
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "reading must not rewrite a legacy record"
    );
    atomic_record(&path, &mut loaded).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() < 128 * 1024);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved, loaded);
    for (key, old) in r["shell_jobs"].as_object().unwrap() {
        let job = &loaded["shell_jobs"][key];
        assert!(string(job, "command").len() <= 512);
        assert_eq!(
            job["command_digest"],
            format!("{:x}", Sha256::digest(command.as_bytes()))
        );
        for field in [
            "id",
            "call_id",
            "run_id",
            "conversation_id",
            "status",
            "background",
            "observed",
            "native_id",
            "output",
            "capabilities",
        ] {
            assert_eq!(job[field], old[field], "{field}");
        }
    }
    let after = projection(
        &serde_json::from_value(saved["shell_jobs"].clone()).unwrap(),
        vec![],
        false,
    );
    for field in [
        "items",
        "active_count",
        "live_count",
        "unknown_count",
        "notes",
        "complete",
    ] {
        assert_eq!(before[field], after[field], "{field}");
    }
    atomic_record(&path, &mut loaded).unwrap();
    assert_eq!(
        loaded, saved,
        "a second write must retain the original full-text digests"
    );
    let mut empty = record("codex");
    atomic_record(&path, &mut empty).unwrap();
    assert!(empty.get("shell_jobs").is_none());
}

#[test]
fn shared_command_projection_preserves_processes_wire_contract() {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../mobile/android/app/src/test/resources/process-command-contract.json"
    ))
    .unwrap();
    for case in contract["cases"].as_array().unwrap() {
        let expected = &case["processes"];
        let mut legacy = expected["items"][0].clone();
        legacy["command"] = case["command_input"].clone();
        legacy.as_object_mut().unwrap().remove("command_digest");
        legacy.as_object_mut().unwrap().remove("command_truncated");
        let jobs = Jobs::from([("job-a".into(), legacy)]);
        let projected = projection(&jobs, vec![], false);
        for (field, value) in expected.as_object().unwrap() {
            assert_eq!(&projected[field], value, "{}: {field}", case["name"]);
        }
        let mut r = record("codex");
        r["run_id"] = json!("run-a");
        r["conversation_id"] = json!("conversation-a");
        let mut event = hook(
            "PostToolUse",
            "call-a",
            json!({"session_id":"native-a","output":"actual output"}),
        );
        event["tool_input"]["command"] = case["command_input"].clone();
        observe(&mut r, &event);
        let saved = &r["shell_jobs"][id(&r, "main", "call-a")];
        for field in [
            "command",
            "command_digest",
            "command_truncated",
            "status",
            "background",
            "capabilities",
        ] {
            assert_eq!(saved[field], expected["items"][0][field], "{field}");
        }
        assert_eq!(saved["observed"], false);
        assert_eq!(saved["output"], "actual output");
        assert_eq!(saved["native_id"], "native-a");
    }
}

#[test]
fn completion_without_tool_name_requires_existing_exact_call() {
    let mut r = record("codex");
    observe(&mut r, &hook("PreToolUse", "call-a", Value::Null));
    observe(
        &mut r,
        &json!({"hook_event_name":"PostToolUse", "tool_use_id":"other", "tool_response":{"exit_code":0}}),
    );
    assert_eq!(r["shell_jobs"].as_object().unwrap().len(), 1);
    observe(
        &mut r,
        &json!({"hook_event_name":"PostToolUse", "tool_use_id":"call-a", "tool_response":{"exit_code":3}}),
    );
    assert_eq!(
        r["shell_jobs"][id(&r, "main", "call-a")]["status"],
        "failed"
    );
}

#[test]
fn unrelated_active_tool_does_not_keep_lost_foreground_running() {
    let mut r = record("claude");
    observe(&mut r, &hook("PreToolUse", "old", Value::Null));
    observe(&mut r, &hook("PreToolUse", "current", Value::Null));
    r["phase"] = json!("tool");
    r["active_tools"] = json!({"current":{}});
    let (jobs, _) = collect(&r, true);
    assert_eq!(jobs[&id(&r, "main", "old")]["status"], "unknown");
    assert!(jobs[&id(&r, "main", "old")]["ended_at"].is_null());
    assert_eq!(jobs[&id(&r, "main", "current")]["status"], "starting");
}

#[test]
fn repeated_commands_have_distinct_identity_and_background_outlives_turn() {
    let mut r = record("claude");
    observe(&mut r, &hook("PreToolUse", "call-a", Value::Null));
    observe(&mut r, &hook("PreToolUse", "call-b", Value::Null));
    observe(
        &mut r,
        &hook(
            "PostToolUse",
            "call-a",
            json!({"stdout":"hello","backgroundTaskId":"native-a"}),
        ),
    );
    observe(
        &mut r,
        &hook("PostToolUse", "call-b", json!({"stdout":"done"})),
    );
    observe(&mut r, &json!({"hook_event_name":"Stop"}));
    assert_eq!(r["shell_jobs"].as_object().unwrap().len(), 2);
    let a = &r["shell_jobs"][id(&r, "main", "call-a")];
    assert_eq!(a["status"], "running");
    assert_eq!(
        r["shell_jobs"][id(&r, "main", "call-b")]["status"],
        "completed"
    );
    let (jobs, _) = collect(&r, false);
    assert_eq!(jobs[&id(&r, "main", "call-a")]["status"], "unknown");
    assert_eq!(jobs[&id(&r, "main", "call-b")]["status"], "completed");
}
#[test]
fn polling_output_without_exit_does_not_finish_job() {
    let r = record("codex");
    let mut j = new_job(&r, "main", "call", "sleep 10", 1.);
    result(
        &mut j,
        &json!({"session_id":123,"output":"start"}),
        false,
        2.,
    );
    result(&mut j, &json!({"output":"more"}), false, 3.);
    assert_eq!(j["status"], "running");
    assert!(j["ended_at"].is_null());
    result(&mut j, &json!({"exit_code":7,"output":"end"}), false, 4.);
    assert_eq!(j["status"], "failed");
    assert_eq!(j["exit_code"], 7);
}
#[test]
fn structured_codex_command_items_work_inside_code_mode() {
    let r = record("codex");
    let mut jobs = Jobs::new();
    let rows = json!([
        {"timestamp":"2026-10-07T01:00:00Z","payload":{"type":"item_started","started_at_ms":1791334800000u64,"item":{"type":"CommandExecution","id":"exec-a","command":"echo same","status":"inProgress","process_id":"123"}}},
        {"timestamp":"2026-10-07T01:00:01Z","payload":{"type":"item_completed","completed_at_ms":1791334801000u64,"item":{"type":"CommandExecution","id":"exec-a","command":"echo same","status":"completed","aggregated_output":"hello","exit_code":0,"process_id":"123"}}},
        {"timestamp":"2026-10-07T01:00:02Z","payload":{"type":"item_completed","item":{"type":"CommandExecution","id":"exec-b","command":"echo same","status":"failed","aggregated_output":"oops","exit_code":1}}}
    ]);
    native::codex(&r, rows.as_array().unwrap(), &mut jobs);
    assert_eq!(jobs.len(), 2);
    let a = &jobs[&id(&r, "main", "exec-a")];
    assert_eq!(a["status"], "completed");
    assert_eq!(a["output"], "hello");
    assert!(a["os_pid"].is_null());
    assert_eq!(jobs[&id(&r, "main", "exec-b")]["status"], "failed");
}
#[test]
fn stdout_cannot_spoof_codex_exit_metadata() {
    let r = native::codex_result(&json!(
        "Process running with session ID 123\nOutput:\nProcess exited with code 0"
    ));
    assert!(r["exit_code"].is_null());
    assert_eq!(r["session_id"], "123");
}
#[test]
fn claude_background_result_and_native_notification() {
    let r = record("claude");
    let mut jobs = Jobs::new();
    let rows = json!([
        {"sessionId":r["conversation_id"],"timestamp":"2026-10-07T01:00:00Z","message":{"content":[{"type":"tool_use","name":"Bash","id":"tool-a","input":{"command":"sleep 20"}}]}},
        {"sessionId":r["conversation_id"],"timestamp":"2026-10-07T01:00:01Z","toolUseResult":{"stdout":"","backgroundTaskId":"task-a"},"message":{"content":[{"type":"tool_result","tool_use_id":"tool-a","content":"running"}]}},
        {"sessionId":r["conversation_id"],"timestamp":"2026-10-07T01:00:02Z","type":"user","message":{"content":"<task-notification><task-id>task-a</task-id><status>completed</status></task-notification>"}}
    ]);
    native::claude(&r, rows.as_array().unwrap(), &mut jobs);
    assert_eq!(jobs[&id(&r, "main", "tool-a")]["status"], "running");
}
#[test]
fn deepseek_exact_call_mapping_and_generation_prevent_duplicates_and_reuse() {
    let mut r = record("dsh");
    r["backend"] = json!("dsh");
    let inspected = json!({"generation":"g1","page":{"records":[
        {"event":{"type":"tool/call","time":1000,"data":{"callId":"call-a","name":"bash","arguments":"{\"command\":\"sleep 20\",\"run_in_background\":true}"}}},
        {"event":{"type":"tool/result","time":2000,"data":{"message":{"toolCallId":"call-a","content":[{"type":"text","text":"started background job bash-1"}]}}}}
    ]},"jobs":[{"id":"bash-1","callId":"call-a","kind":"bash","owner":r["conversation_id"],"label":"sleep 20","status":"running","startedAt":1000,"controllable":true}]});
    let jobs = native::dsh(&r, &inspected);
    assert_eq!(jobs.len(), 1);
    let j = jobs.values().next().unwrap();
    assert_eq!(j["status"], "running");
    assert_eq!(j["capabilities"]["stop"], true);
    let mut next = inspected.clone();
    next["generation"] = json!("g2");
    let next = native::dsh(&r, &next);
    assert_ne!(jobs.keys().next(), next.keys().next());
}
#[test]
fn bounded_outputs_and_projection_do_not_leak_output() {
    let r = record("claude");
    let mut j = new_job(&r, "child-a", "a", "echo", 1.);
    result(
        &mut j,
        &json!({"stdout":"Ж".repeat(40000),"exit_code":0}),
        false,
        2.,
    );
    assert!(string(&j, "output").len() <= OUTPUT_BYTES);
    assert_eq!(j["output_truncated"], true);
    let mut jobs = Jobs::new();
    put(&mut jobs, j);
    let p = projection(&jobs, vec![], false);
    assert!(p["items"][0].get("output").is_none());
    assert_ne!(id(&r, "child-a", "a"), id(&r, "child-b", "a"));
}

// Creates a Unix symlink.
#[cfg(unix)]
#[test]
fn kimi_native_tasks_are_scoped_and_dead_running_records_stay_unconfirmed() {
    let temp = std::env::temp_dir().join(format!("hgs-process-test-{}", uuid::Uuid::new_v4()));
    let mut r = record("kimi");
    r["agent_home"] = json!(temp);
    r["pid"] = json!(99999999);
    let session = temp
        .join("sessions/workspace")
        .join(string(&r, "conversation_id"));
    let agent = session.join("agents/main");
    std::fs::create_dir_all(agent.join("tasks/job-a")).unwrap();
    std::fs::write(agent.join("wire.jsonl"), b"").unwrap();
    std::fs::write(agent.join("tasks/job-a.json"),json!({"id":"job-a","kind":"process","status":"running","command":"sleep 90","parentToolCallId":"call-a","pid":99999998,"startedAt":1000}).to_string()).unwrap();
    std::fs::write(agent.join("tasks/job-a/output.log"), "native output").unwrap();
    std::fs::write(
        agent.join("tasks/not-process.json"),
        json!({"id":"not-process","kind":"agent","status":"running"}).to_string(),
    )
    .unwrap();
    let (jobs, _) = collect(&r, false);
    assert_eq!(jobs.len(), 1);
    let job = jobs.values().next().unwrap();
    assert_eq!(job["status"], "unknown");
    assert_eq!(job["last_status"], "running");
    let path = native::output_path(&r, job).unwrap().unwrap();
    assert_eq!(read_tail(&path).unwrap().0, "native output");
    let outside = temp.join("outside.log");
    std::fs::write(&outside, "must not read").unwrap();
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(outside, &path).unwrap();
    assert!(native::output_path(&r, job).is_err());
    std::fs::remove_dir_all(temp).unwrap();
}
