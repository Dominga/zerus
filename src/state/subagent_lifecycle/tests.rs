use super::*;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::process::{Child, Command, Stdio};

const ID: &str = "external:codex:22222222-2222-4222-8222-222222222222";
#[cfg(unix)]
struct Owned(Child);
#[cfg(unix)]
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
#[cfg(unix)]
fn sleeper() -> Owned {
    Owned(
        Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn record() -> Value {
    json!({"agent":"claude","name":"claude/example/parent","pid":std::process::id(),
        "process_start":process_start(std::process::id()),"run_id":"run","conversation_id":"parent",
        "main_done":true,"activity":"busy","phase":"working","active_tools":{},
        "subagents":{ID:{"external":true,"provider":"codex","conversation_id":"child","state":"working",
            "started":1.0,"updated":2.0,"reply_id":"original-reply","attention_id":"original-attention"}}})
}
fn request() -> Retire {
    Retire {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_run_id: "run".into(),
        expected_conversation_id: "parent".into(),
        agent_id: ID.into(),
        expected_updated: 2.0,
    }
}

#[test]
#[cfg(unix)]
fn killed_original_without_stop_leaves_working_and_its_jobs_are_unknown() {
    let mut child = sleeper();
    let mut record = record();
    record["subagents"][ID]["original_process"] = capture(&record, child.0.id()).unwrap();
    record["shell_jobs"] = json!({"job":{"id":"job","owner":ID,"run_id":"run","status":"running",
        "source":"hooks","background":true,"observed":true,"command":"private fake work"}});
    reconcile(&mut record);
    assert!(telemetry::work_remains(&record));
    assert_eq!(processes::snapshot(&record, true)["active_count"], 1);
    // Only our original Child is killed/reaped. No provider completion exists.
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["state"], "ended");
    assert_eq!(
        record["subagents"][ID]["completion_source"],
        "original_process_exit"
    );
    assert_eq!(record["subagents"][ID]["reply_id"], "original-reply");
    assert_eq!(
        record["subagents"][ID]["attention_id"],
        "original-attention"
    );
    assert!(!telemetry::work_remains(&record));
    assert_eq!(record["activity"], "idle");
    let summary = telemetry::summary(
        &record,
        &json!({"runtime_state":"live","activity":"idle","phase":"idle"}),
        "",
        "",
    );
    assert_eq!(summary["subagent_active_count"], 0);
    assert_eq!(
        summary["subagent_completed_count"], 0,
        "no invented successful completion"
    );
    assert_eq!(processes::snapshot(&record, true)["active_count"], 0);
    let jobs = processes::snapshot(&record, true);
    assert_eq!(
        jobs["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["id"] == "job")
            .unwrap()["status"],
        "unknown"
    );
}

#[test]
#[cfg(unix)]
fn live_original_cannot_be_retired_even_when_the_parent_is_offline() {
    let child = sleeper();
    let mut record = record();
    record["subagents"][ID]["original_process"] = capture(&record, child.0.id()).unwrap();
    assert!(retire_record(&mut record, request())
        .unwrap_err()
        .contains("still live"));
    record["process_start"] = json!("different parent birth");
    reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["display_state"], "unknown");
    assert!(!telemetry::work_remains(&record));
    assert!(retire_record(&mut record, request())
        .unwrap_err()
        .contains("still live"));
}

#[test]
#[cfg(unix)]
fn a_reused_pid_or_wrong_run_never_adopts_the_new_live_process() {
    let child = sleeper();
    let mut record = record();
    record["subagents"][ID]["original_process"] = capture(&record, child.0.id()).unwrap();
    record["subagents"][ID]["original_process"]["birth"] = json!("old birth");
    reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["state"], "ended");
    record["subagents"][ID]["state"] = json!("working");
    record["subagents"][ID]["original_process"] = capture(&record, child.0.id()).unwrap();
    record["run_id"] = json!("new-run");
    reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["display_state"], "unknown");
    assert!(!telemetry::work_remains(&record));
}

#[test]
fn legacy_unknown_is_not_working_and_explicit_retirement_is_exact_and_idempotent() {
    let mut record = record();
    reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["display_state"], "unknown");
    assert!(!telemetry::work_remains(&record));
    let request = request();
    let saved = serde_json::to_vec(&request).unwrap();
    let (receipt, changed) = retire_record(&mut record, request).unwrap();
    assert!(changed);
    assert_eq!(record["subagents"][ID]["state"], "ended");
    assert_eq!(receipt["native_completion"], false);
    let (again, changed) =
        retire_record(&mut record, serde_json::from_slice(&saved).unwrap()).unwrap();
    assert_eq!(again, receipt);
    assert!(!changed);
    let mut conflict: Retire = serde_json::from_slice(&saved).unwrap();
    conflict.expected_updated += 1.0;
    assert!(retire_record(&mut record, conflict)
        .unwrap_err()
        .contains("identity reused"));
    assert_eq!(record["subagents"][ID]["reply_id"], "original-reply");
}

#[test]
#[cfg(unix)]
fn a_separately_bound_live_job_outlasts_its_provider_and_only_its_own_exit_ends_work() {
    let mut provider = sleeper();
    let mut job = sleeper();
    let mut record = record();
    record["subagents"][ID]["original_process"] = capture(&record, provider.0.id()).unwrap();
    let birth = crate::platform::process_identity(job.0.id()).unwrap().birth;
    record["shell_jobs"] = json!({"job":{"id":"job","owner":ID,"run_id":"run","status":"running",
        "source":"hooks","background":true,"observed":true,"os_pid":job.0.id(),"process_birth":birth}});
    provider.0.kill().unwrap();
    provider.0.wait().unwrap();
    reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["state"], "ended");
    assert_eq!(processes::snapshot(&record, true)["active_count"], 1);
    job.0.kill().unwrap();
    job.0.wait().unwrap();
    assert_eq!(processes::snapshot(&record, true)["active_count"], 0);
}

#[test]
#[cfg(unix)]
fn a_current_kimi_native_task_keeps_its_exact_process_binding_until_exit() {
    let mut child = sleeper();
    let root = tempfile::tempdir().unwrap();
    let mut record = record();
    record["agent"] = json!("kimi");
    record["conversation_id"] = json!("11111111-1111-4111-8111-111111111111");
    record["agent_home"] = json!(root.path());
    let native = root
        .path()
        .join("sessions/workspace")
        .join(string(&record, "conversation_id"))
        .join("agents/main");
    fs::create_dir_all(native.join("tasks")).unwrap();
    fs::write(native.join("wire.jsonl"), b"").unwrap();
    let identity = crate::platform::process_identity(child.0.id()).unwrap();
    fs::write(
        native.join("tasks/job-a.json"),
        json!({"id":"job-a","kind":"process",
        "status":"running","command":"private fake work","pid":child.0.id(),
        "startedAt":identity.started*1000.})
        .to_string(),
    )
    .unwrap();
    let snapshot = processes::snapshot(&record, true);
    let task = snapshot["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["native_id"] == "job-a")
        .unwrap();
    assert_eq!(task["status"], "running");
    assert_eq!(task["os_pid"], child.0.id());
    assert_eq!(task["process_birth"], identity.birth);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let snapshot = processes::snapshot(&record, true);
    assert_eq!(
        snapshot["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["native_id"] == "job-a")
            .unwrap()["status"],
        "unknown"
    );
}

#[test]
fn malformed_original_and_receipts_never_become_live_or_replayed() {
    let mut record = record();
    record["subagents"][ID]["original_process"] = json!({"pid":std::process::id(),
        "birth":"1".repeat(201),"run_id":"run","conversation_id":"parent",
        "parent_pid":std::process::id(),"parent_birth":"1","parent_start":"1"});
    reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["display_state"], "unknown");
    let request = request();
    record["subagent_retirements"] = json!({request.request_id.clone():{
        "original":serde_json::to_value(&request).unwrap(),"result":{"accepted":true}}});
    let before = record.clone();
    assert_eq!(
        retire_record(&mut record, request).unwrap_err(),
        "Invalid subagent retirement receipt"
    );
    assert_eq!(record, before);
}

#[test]
#[cfg(unix)]
fn process_availability_recovery_preserves_a_provider_unknown_and_only_clears_our_projection() {
    let child = sleeper();
    let mut record = record();
    record["subagents"][ID]["original_process"] = capture(&record, child.0.id()).unwrap();
    record["subagents"][ID]["display_state"] = json!("unknown");
    reconcile(&mut record);
    assert_eq!(record["subagents"][ID]["display_state"], "unknown");
    assert!(!telemetry::work_remains(&record));
    record["subagents"][ID]["process_unknown"] = json!(true);
    reconcile(&mut record);
    assert!(record["subagents"][ID]["display_state"].is_null());
    assert!(telemetry::work_remains(&record));
    assert_eq!(record["activity"], "busy");
}

#[test]
fn native_current_work_refuses_retirement_but_unknown_stale_individual_records_allow_it() {
    let mut record = record();
    record["subagents"][ID]["external"] = json!(false);
    assert!(retire_record(&mut record, request())
        .unwrap_err()
        .contains("current work"));
    record["subagents"][ID]["display_state"] = json!("unknown");
    assert!(retire_record(&mut record, request()).unwrap().1);
}

#[test]
fn wrong_scope_changed_child_and_full_receipts_refuse_without_a_change() {
    for kind in ["run", "conversation", "child", "updated", "full"] {
        let mut record = record();
        let mut request = request();
        match kind {
            "run" => request.expected_run_id = "other".into(),
            "conversation" => request.expected_conversation_id = "other".into(),
            "child" => request.agent_id = "other".into(),
            "updated" => request.expected_updated += 1.0,
            _ => {
                record["subagent_retirements"] = json!((0..CAP)
                    .map(|i| (i.to_string(), json!({})))
                    .collect::<serde_json::Map<_, _>>())
            }
        }
        let before = record.clone();
        assert!(retire_record(&mut record, request).is_err());
        assert_eq!(record, before);
    }
    assert!(retire("unused", &vec![b' '; 8193]).is_err());
    assert!(serde_json::from_slice::<Retire>(br#"{"unexpected":true}"#).is_err());
}
