use super::*;

fn projections(uncached: u64, output: u64, read: u64, write: u64) -> Value {
    json!({"tokenUsage":{"uncachedInputTokens":uncached,"outputTokens":output,"cacheReadTokens":read,"cacheWriteTokens":write}})
}

fn record_of(name: &str) -> Value {
    json!({"agent":"dsh","name":name,"conversation_id":"session-A"})
}

fn tokens_of(result: &Value) -> Value {
    result["providers"][0]["models"][0]["tokens"].clone()
}

#[test]
fn a_dsh_window_is_the_difference_of_the_counters_at_its_two_ends() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        let name = "dsh/p/member";
        record(name, "session-A", 1000.0, &projections(100, 50, 1000, 10), true).unwrap();
        record(name, "session-A", 1100.0, &projections(400, 150, 5000, 30), true).unwrap();
        record(name, "session-A", 1300.0, &projections(900, 350, 9000, 30), true).unwrap();
        let result = usage::span_many(&record_of(name), &[(1000.0, 1100.0)]);
        assert_eq!(result["status"], "ok", "{result}");
        assert_eq!(result["source"], "dsh");
        let tokens = tokens_of(&result);
        assert_eq!(
            (tokens["uncached_input"].clone(), tokens["output"].clone(), tokens["cache_read"].clone(), tokens["cache_write"].clone()),
            (json!(300), json!(100), json!(4000), json!(20)),
            "{result}"
        );
        // The window's input is the three input buckets; its total input plus output.
        assert_eq!((tokens["input"].clone(), tokens["total"].clone()), (json!(4320), json!(4420)));
        // Two windows of one part add up; the gap between them is not counted.
        let two = usage::span_many(&record_of(name), &[(1000.0, 1100.0), (1100.0, 1300.0)]);
        assert_eq!(tokens_of(&two)["uncached_input"], 800, "{two}");
        assert_eq!(tokens_of(&two)["output"], 300, "{two}");
    });
}

#[test]
fn a_missing_reading_stays_unknown_never_zero() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        let name = "dsh/p/member";
        // No sample at all.
        let none = usage::span_many(&record_of(name), &[(1000.0, 1100.0)]);
        assert_eq!(none["status"], "unknown", "{none}");
        assert!(none.get("providers").is_none() && !none.to_string().contains("\"total\":0"), "{none}");
        record(name, "session-A", 1000.0, &projections(100, 50, 0, 0), true).unwrap();
        record(name, "session-A", 1100.0, &projections(400, 150, 0, 0), true).unwrap();
        // The window's end is long after the last reading: unknown.
        let late = usage::span_many(&record_of(name), &[(1000.0, 5000.0)]);
        assert_eq!(late["status"], "unknown", "{late}");
        // A window that starts before any reading too.
        let early = usage::span_many(&record_of(name), &[(100.0, 1100.0)]);
        assert_eq!(early["status"], "unknown", "{early}");
        // One window of two unknown makes the whole span unknown.
        let half = usage::span_many(&record_of(name), &[(1000.0, 1100.0), (3000.0, 4000.0)]);
        assert_eq!(half["status"], "unknown", "{half}");
        // Another conversation's readings are not this session's.
        let other = usage::span_many(&json!({"agent":"dsh","name":name,"conversation_id":"session-B"}), &[(1000.0, 1100.0)]);
        assert_eq!(other["status"], "unknown", "{other}");
        // A projection without counters records nothing: the gap stays unknown.
        assert!(!record(name, "session-A", 1200.0, &json!({"tokenUsage":{}}), true).unwrap());
        assert!(!record(name, "session-A", 1200.0, &json!({}), true).unwrap());
        assert_eq!(load(name).len(), 2);
        assert_eq!(usage::span_many(&record_of(name), &[(1100.0, 1200.0)])["status"], "unknown");
    });
}

#[test]
fn counters_that_went_backwards_make_the_window_unknown() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        let name = "dsh/p/member";
        record(name, "session-A", 1000.0, &projections(400, 150, 0, 0), true).unwrap();
        record(name, "session-A", 1100.0, &projections(100, 50, 0, 0), true).unwrap();
        let result = usage::span_many(&record_of(name), &[(1000.0, 1100.0)]);
        assert_eq!(result["status"], "unknown", "{result}");
    });
}

#[test]
fn an_unchanged_counter_is_read_across_a_long_idle_gap() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        let name = "dsh/p/member";
        for at in [1000.0, 2000.0, 3000.0, 4000.0] {
            record(name, "session-A", at, &projections(100, 50, 0, 0), true).unwrap();
        }
        // An idle run keeps only its first and last sample, yet a window inside
        // it is exactly zero tokens: both ends are known.
        let samples = load(name);
        assert_eq!(samples.iter().map(|sample| sample.at).collect::<Vec<_>>(), [1000.0, 4000.0]);
        let result = usage::span_many(&record_of(name), &[(2500.0, 3500.0)]);
        assert_eq!(result["status"], "ok", "{result}");
        assert_eq!(tokens_of(&result)["total"], 0);
        // Work after the run: a window starting inside the idle run reads its
        // start from the plateau, not from the next sample.
        record(name, "session-A", 4100.0, &projections(300, 90, 0, 0), true).unwrap();
        let work = usage::span_many(&record_of(name), &[(3500.0, 4100.0)]);
        assert_eq!(tokens_of(&work)["uncached_input"], 200, "{work}");
    });
}

#[test]
fn samples_are_bounded_and_a_changing_counter_is_not_recorded_closer_than_the_gap() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        let name = "dsh/p/member";
        assert!(record(name, "session-A", 1000.0, &projections(1, 1, 0, 0), false).unwrap());
        // Two seconds later and changed: not recorded unless forced.
        assert!(!record(name, "session-A", 1002.0, &projections(2, 2, 0, 0), false).unwrap());
        assert_eq!(load(name).len(), 1);
        assert!(record(name, "session-A", 1002.0, &projections(2, 2, 0, 0), true).unwrap());
        assert_eq!(load(name).len(), 2);
        // A sample not after the last is ignored.
        assert!(!record(name, "session-A", 1001.0, &projections(3, 3, 0, 0), true).unwrap());
    });
    let mut samples = Vec::new();
    for index in 0..(MAX_SAMPLES as u64 + 500) {
        push(&mut samples, Sample { at: 1000.0 + index as f64 * 20.0, conversation: "c".into(), counters: [index + 1, 0, 0, 0] }, true);
    }
    assert!(samples.len() <= MAX_SAMPLES, "{}", samples.len());
    // The newest stay as they came, the oldest remain (thinned), in order.
    assert_eq!(samples.last().unwrap().counters[0], MAX_SAMPLES as u64 + 500);
    assert_eq!(samples.first().unwrap().at, 1000.0);
    assert!(samples.windows(2).all(|pair| pair[0].at < pair[1].at));
}

#[test]
fn a_dsh_record_without_a_name_is_unavailable() {
    let result = usage::span_many(&json!({"agent":"dsh","conversation_id":"session-A"}), &[(1000.0, 1100.0)]);
    assert_eq!(result["status"], "unknown", "{result}");
    assert!(result["reason"].as_str().unwrap().contains("Native usage unavailable"), "{result}");
}

fn age(file: &std::path::Path, days: u64) {
    let handle = fs::OpenOptions::new().write(true).open(file).unwrap();
    handle.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(days * 24 * 3600)).unwrap();
}

#[test]
fn a_reading_that_records_nothing_writes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        let name = "dsh/p/member";
        assert!(record(name, "session-A", 1000.0, &projections(1, 1, 0, 0), false).unwrap());
        // The file is an hour old; any rewrite would make it new again.
        age(&path(name), 1);
        let before = fs::metadata(path(name)).unwrap().modified().unwrap();
        let bytes = fs::read(path(name)).unwrap();
        // Inspected every second: unchanged, changed, and not after the last.
        for second in 1..=5 {
            assert!(!record(name, "session-A", 1000.0 + second as f64, &projections(1, 1, 0, 0), false).unwrap());
        }
        assert!(!record(name, "session-A", 1006.0, &projections(2, 2, 0, 0), false).unwrap());
        assert!(!record(name, "session-A", 999.0, &projections(3, 3, 0, 0), true).unwrap());
        assert_eq!(fs::metadata(path(name)).unwrap().modified().unwrap(), before, "the file was rewritten");
        assert_eq!(fs::read(path(name)).unwrap(), bytes);
        // A reading MIN_GAP later is recorded and written.
        assert!(record(name, "session-A", 1000.0 + MIN_GAP, &projections(2, 2, 0, 0), false).unwrap());
        assert!(fs::metadata(path(name)).unwrap().modified().unwrap() > before);
        assert_eq!(load(name).len(), 2);
    });
}

#[test]
fn a_forgotten_session_takes_its_samples_with_it() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        record("dsh/p/gone", "session-A", 1000.0, &projections(1, 1, 0, 0), true).unwrap();
        record("dsh/p/stays", "session-B", 1000.0, &projections(1, 1, 0, 0), true).unwrap();
        assert!(path("dsh/p/gone").exists() && path("dsh/p/gone").with_extension("lock").exists());
        forget("dsh/p/gone");
        assert!(!path("dsh/p/gone").exists() && !path("dsh/p/gone").with_extension("lock").exists());
        assert!(path("dsh/p/stays").exists());
    });
}

#[test]
fn a_renamed_session_keeps_its_samples_under_the_new_name() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        record("dsh/p/old", "session-A", 1000.0, &projections(100, 50, 0, 0), true).unwrap();
        record("dsh/p/old", "session-A", 1100.0, &projections(400, 150, 0, 0), true).unwrap();
        rename("dsh/p/old", "dsh/p/new").unwrap();
        assert!(!path("dsh/p/old").exists());
        let result = usage::span_many(&record_of("dsh/p/new"), &[(1000.0, 1100.0)]);
        assert_eq!(tokens_of(&result)["uncached_input"], 300, "{result}");
        // A session without samples renames without creating a file.
        rename("dsh/p/never", "dsh/p/other").unwrap();
        assert!(!path("dsh/p/other").exists());
    });
}

#[test]
fn old_sample_files_are_pruned_in_bounded_slices_and_fresh_ones_stay() {
    let temp = tempfile::tempdir().unwrap();
    with_test_state_root(temp.path(), || {
        let folder = root().join("usage-samples");
        // Five sessions untouched for 40 days, one fresh; each with its lock file.
        for index in 0..5 {
            let name = format!("dsh/p/old{index}");
            record(&name, "session-A", 1000.0, &projections(1, 1, 0, 0), true).unwrap();
            age(&path(&name), 40);
            age(&path(&name).with_extension("lock"), 40);
        }
        record("dsh/p/fresh", "session-A", 1000.0, &projections(1, 1, 0, 0), true).unwrap();
        let count = || fs::read_dir(&folder).unwrap().count();
        assert_eq!(count(), 12);
        // A pass looks at no more than its limit of entries.
        let before = count();
        let mut offset = prune(MAX_AGE, 0, 3);
        assert!(count() >= before - 3, "one pass removed more than it looked at: {} left", count());
        let mut passes = 1;
        while passes < 20 && count() > 2 {
            offset = prune(MAX_AGE, offset, 3);
            passes += 1;
        }
        // Only the fresh session's two files are left; a pass over the end restarts.
        assert_eq!(count(), 2, "{:?}", fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>());
        assert!(path("dsh/p/fresh").exists() && path("dsh/p/fresh").with_extension("lock").exists());
        assert_eq!(prune(MAX_AGE, offset, 100), 0);
        // An orphaned lock file (its samples gone) is removed once old.
        fs::write(path("dsh/p/orphan").with_extension("lock"), b"").unwrap();
        age(&path("dsh/p/orphan").with_extension("lock"), 40);
        prune(MAX_AGE, 0, 100);
        assert!(!path("dsh/p/orphan").with_extension("lock").exists());
        // No folder: nothing to do.
        assert_eq!(prune(MAX_AGE, 5, 100), 0);
    });
}
