//! Task 121: Kimi's `SessionHeartbeat` hook, made a fast append. The heartbeat
//! only refreshes the session's last activity, yet its hook ran the whole
//! delivery path — the state lock with its recovery scans, the terminal read,
//! the session record write — and Kimi stops a hook after 5 s. On a loaded
//! Windows machine a delivery holding the lock outlived that, the hook timed
//! out, and the starved message stayed in the prompt. The hook now writes one
//! small file for its run without a lock; every record read folds its time into
//! `last_event_at` and `updated`, exactly what the old write did.
use super::{now, storage};
use serde_json::{json, Value};
use std::path::PathBuf;
/// Where one run's heartbeat lives: a bounded, one-line file per run.
fn path(run: &str) -> PathBuf {
    storage::root().join("heartbeats").join(format!("{run}.json"))
}

/// Whether a run id is safe as a file name (the hook's environment is the only
/// source, but a file name is still built from it).
fn valid(run: &str) -> bool {
    !run.is_empty()
        && run.len() <= 128
        && run.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The run's heartbeat at `at`: one atomic file replace, never the state lock.
/// Best effort: a heartbeat that cannot be written only leaves the old one.
pub(crate) fn beat(run: &str, at: f64) {
    if !valid(run) || !at.is_finite() || at <= 0.0 {
        return;
    }
    let path = path(run);
    if let Some(folder) = path.parent() {
        let _ = storage::private_dir(folder);
    }
    let _ = storage::atomic_private(&path, &json!({"run_id": run, "at": at}).to_string());
}

/// Folds a run's heartbeat into a record being read: a newer beat advances
/// `last_event_at` and `updated`, as the hook's write did; an older one never
/// moves them back, and another run's beat never touches this record.
pub(crate) fn apply(record: &mut Value) {
    let run = record["run_id"].as_str().unwrap_or_default();
    if !valid(run) {
        return;
    }
    let Ok(bytes) = std::fs::read(path(run)) else { return; };
    let Ok(beat) = serde_json::from_slice::<Value>(&bytes) else { return; };
    if beat["run_id"].as_str() != Some(run) {
        return;
    }
    let Some(at) = beat["at"].as_f64().filter(|at| at.is_finite() && *at > 0.0) else { return; };
    if record["last_event_at"].as_f64().is_none_or(|last| at > last) {
        record["last_event_at"] = json!(at);
    }
    if record["updated"].as_f64().is_none_or(|updated| at > updated) {
        record["updated"] = json!(at);
    }
}

/// The hook's heartbeat: the time it arrived, folded on the next read.
pub(crate) fn observed(run: &str) -> f64 {
    let at = now();
    beat(run, at);
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A heartbeat advances the record's activity, never lowers it, and never
    /// touches another run's record.
    #[test]
    fn a_beat_is_folded_into_the_run_it_names() {
        let dir = std::env::temp_dir().join(format!("zerus-heartbeat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        storage::set_test_root(Some(dir.clone()));
        let mut record = json!({"run_id":"run-a","last_event_at":10.0,"updated":10.0});
        beat("run-a", 20.0);
        apply(&mut record);
        assert_eq!((record["last_event_at"].as_f64(), record["updated"].as_f64()), (Some(20.0), Some(20.0)));
        // An older beat keeps the newer event.
        let mut newer = json!({"run_id":"run-a","last_event_at":30.0,"updated":30.0});
        apply(&mut newer);
        assert_eq!(newer["last_event_at"].as_f64(), Some(30.0));
        // Another run's beat is not this record's.
        let mut other = json!({"run_id":"run-b","last_event_at":5.0});
        apply(&mut other);
        assert_eq!(other["last_event_at"].as_f64(), Some(5.0));
        // A name that is not a plain run id writes no file.
        beat("../escape", 40.0);
        let files: Vec<String> = std::fs::read_dir(dir.join("heartbeats"))
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect();
        assert_eq!(files, ["run-a.json"]);
        storage::set_test_root(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
