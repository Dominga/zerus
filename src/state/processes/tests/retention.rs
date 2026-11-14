use super::*;

fn finished(record: &Value, call: &str, at: f64) -> Value {
    let mut job = new_job(record, "main", call, "synthetic build", at);
    result(
        &mut job,
        &json!({"exit_code":0,"output":"x".repeat(8192)}),
        false,
        at,
    );
    job
}

#[test]
fn large_legacy_record_trims_settled_history_on_write_and_keeps_unsettled_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("record.json");
    let mut record = record("codex");
    let mut jobs = Jobs::new();
    for i in 0..256 {
        let job = finished(&record, &format!("finished-{i}"), i as f64);
        jobs.insert(string(&job, "id").into(), job);
    }
    let mut retained = Jobs::new();
    for (i, status) in [
        "starting",
        "approval",
        "running",
        "stopping",
        "unknown",
        "future-status",
    ]
    .iter()
    .enumerate()
    {
        let mut job = new_job(&record, "main", &format!("current-{i}"), "live command", 0.);
        job["status"] = json!(status);
        job["background"] = json!(true);
        job["native_id"] = json!(format!("native-{i}"));
        job["os_pid"] = json!(12345 + i);
        job["process_birth"] = json!("synthetic-birth");
        job["output"] = json!("live tail");
        if *status == "unknown" {
            job["run_id"] = json!("previous-run");
        }
        retained.insert(string(&job, "id").into(), job);
    }
    let pending = finished(&record, "pending-hook", 0.);
    record["active_tools"] = json!({"pending-hook":{"tool":"exec_command"}});
    retained.insert(string(&pending, "id").into(), pending);
    jobs.extend(retained.clone());
    record["shell_jobs"] = json!(jobs);
    let original = record.to_string();
    assert!(original.len() > 2 << 20);
    std::fs::write(&path, &original).unwrap();
    let mut loaded: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let before = projection(&jobs, vec![], false);
    atomic_record(&path, &mut loaded).unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let saved_jobs: Jobs = serde_json::from_value(saved["shell_jobs"].clone()).unwrap();
    assert!(
        saved_jobs.len() <= retained.len() + 16,
        "settled history must be bounded"
    );
    assert!(saved["shell_jobs"].to_string().len() < 128 << 10);
    for (id, job) in &retained {
        assert_eq!(
            &saved_jobs[id], job,
            "exact current/unsettled evidence: {id}"
        );
    }
    for i in 240..256 {
        let job = &saved_jobs[&id(&record, "main", &format!("finished-{i}"))];
        assert_eq!(job["output"], "x".repeat(1024));
        assert_eq!(job["output_truncated"], true);
        assert!(
            saved_jobs.contains_key(&id(&record, "main", &format!("finished-{i}"))),
            "retain newest settled jobs"
        );
    }
    let after = projection(&saved_jobs, vec![], false);
    for field in ["active_count", "unknown_count", "live_count", "complete"] {
        assert_eq!(before[field], after[field], "{field}");
    }
    assert_eq!(loaded, saved);
    atomic_record(&path, &mut loaded).unwrap();
    assert_eq!(loaded, saved, "compaction is idempotent");
    loaded["archive_id"] = json!(uuid::Uuid::new_v4().to_string());
    loaded["state"] = json!("archived");
    atomic_record(&tmp.path().join("archive.json"), &mut loaded).unwrap();
    for (id, job) in retained {
        assert_eq!(loaded["shell_jobs"][id], job);
    }
}

#[test]
fn maximum_unresolved_jobs_and_history_migrate_below_the_record_read_bound() {
    let mut record = record("codex");
    let mut jobs = Jobs::new();
    for i in 0..MAX_JOBS {
        let mut job = new_job(
            &record,
            "main",
            &format!("current-{i}"),
            "live command",
            i as f64,
        );
        job["output"] = json!("x".repeat(OUTPUT_BYTES));
        jobs.insert(string(&job, "id").into(), job);
    }
    let current = jobs.clone();
    for i in 0..256 {
        let job = finished(&record, &format!("settled-{i}"), i as f64);
        jobs.insert(string(&job, "id").into(), job);
    }
    record["shell_jobs"] = json!(jobs);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("record.json");
    atomic_record(&path, &mut record).unwrap();
    for (id, job) in current {
        assert_eq!(record["shell_jobs"][id], job);
    }
    assert!(record.to_string().len() < 16 << 20);
    assert!(std::fs::metadata(path).unwrap().len() < 16 << 20);
    assert!(record["shell_jobs"].as_object().unwrap().len() <= MAX_JOBS + 16);
}

#[test]
fn measure_synthetic_hook_history_growth() {
    let mut record = record("claude");
    for i in 1..=512 {
        observe(
            &mut record,
            &hook(
                "PostToolUse",
                &format!("call-{i}"),
                json!({"exit_code":0,"output":"x".repeat(8192)}),
            ),
        );
        if [1, 16, 64, 128, 256, 512].contains(&i) {
            let copy: Value = serde_json::from_str(&record.to_string()).unwrap();
            println!(
                "calls={i} jobs={} record_bytes={} shell_jobs_bytes={}",
                copy["shell_jobs"].as_object().unwrap().len(),
                copy.to_string().len(),
                copy["shell_jobs"].to_string().len()
            );
        }
    }
    assert!(record["shell_jobs"].as_object().unwrap().len() <= 16);
    assert!(record["shell_jobs"].to_string().len() <= 64 << 10);
}

#[test]
fn settled_history_has_a_byte_budget_even_with_oversized_legacy_metadata() {
    let mut record = record("codex");
    let mut jobs = Jobs::new();
    for i in 0..16 {
        let mut job = finished(&record, &format!("finished-{i}"), i as f64);
        job["cwd"] = json!("synthetic/".repeat(8192));
        jobs.insert(string(&job, "id").into(), job);
    }
    record["shell_jobs"] = json!(jobs);
    compact(&mut record);
    assert!(
        record["shell_jobs"].to_string().len() <= 64 << 10,
        "count alone does not bound bytes"
    );
}
