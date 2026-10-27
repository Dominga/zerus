//! U20: a member whose sandbox blocks the team store queues its report, ask
//! and commit requests in its own folder, where it may write; the worker reads
//! them there and applies each with the same identity checks as a direct write.
//! The folder proves the member, and the request must name that member and one
//! of its own attempts.
use super::super::*;
use anyhow::{ensure, Context};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How many requests one tick takes from one member (U20): bounded work for a
/// member that queued a burst while the worker was away.
pub(crate) const PER_TICK: usize = 20;

/// The member's outbox: `<member folder>/.zerus-team/outbox`.
pub(crate) fn dir(folder: &Path) -> PathBuf {
    folder.join(".zerus-team").join("outbox")
}

/// Where a request waits: `<request_id>.json`, the id the member reuses when it
/// polls for the outcome.
pub(crate) fn request_path(folder: &Path, request_id: &str) -> PathBuf {
    dir(folder).join(format!("{request_id}.json"))
}

/// Where the worker leaves the outcome: `<request_id>.result.json`.
pub(crate) fn result_path(folder: &Path, request_id: &str) -> PathBuf {
    dir(folder).join(format!("{request_id}.result.json"))
}

/// Queues one request for the worker. The file is written beside its name and
/// renamed, so the worker never reads half a request.
pub(crate) fn push(folder: &Path, request: &Value) -> anyhow::Result<PathBuf> {
    let id = request["request_id"].as_str().unwrap_or_default();
    ensure!(super::store::canonical_uuid(id), "An outbox request needs a canonical request_id");
    let dir = dir(folder);
    fs::create_dir_all(&dir)?;
    let temp = dir.join(format!("{id}.tmp"));
    fs::write(&temp, serde_json::to_vec(request)?)?;
    let path = request_path(folder, id);
    fs::rename(&temp, &path)?;
    Ok(path)
}

/// The requests waiting for the worker, oldest first, at most `limit`. A result
/// file and a half-written request are not requests.
pub(crate) fn pending(folder: &Path, limit: usize) -> anyhow::Result<Vec<(PathBuf, Value)>> {
    let Ok(entries) = fs::read_dir(dir(folder)) else { return Ok(Vec::new()) };
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if !name.ends_with(".json") || name.ends_with(".result.json") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else { continue };
        found.push((modified, path));
    }
    found.sort();
    let mut requests = Vec::new();
    for (_, path) in found.into_iter().take(limit) {
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
        requests.push((path, value));
    }
    Ok(requests)
}

/// The outcome the worker wrote, when it has.
pub(crate) fn result(folder: &Path, request_id: &str) -> Option<Value> {
    fs::read(result_path(folder, request_id)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// Records the outcome, then removes the request: the member's `--wait` reads
/// the first, and a worker that writes and then dies does not apply it twice
/// (the store's own receipt is the second guard).
pub(crate) fn settle(folder: &Path, request: &Value, outcome: &Value) -> anyhow::Result<()> {
    let id = request["request_id"].as_str().context("An outbox request names its request_id")?;
    fs::write(result_path(folder, id), serde_json::to_vec(outcome)?)?;
    let _ = fs::remove_file(request_path(folder, id));
    Ok(())
}

/// Waits for the worker's outcome, at most `timeout` seconds (`hgs team report
/// --wait` polls this). None leaves the request queued.
pub(crate) fn wait(folder: &Path, request_id: &str, timeout: f64) -> Option<Value> {
    let until = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
    loop {
        if let Some(result) = result(folder, request_id) {
            return Some(result);
        }
        if Instant::now() >= until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, op: &str) -> Value {
        json!({"op":op,"request_id":id,"team":"t","member":"dev","attempt":"A1","input":{"text":"Done"}})
    }

    #[test]
    fn a_queued_request_waits_for_the_worker_and_its_outcome_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path();
        let first = request("11111111-1111-4111-8111-111111111111", "report");
        let second = request("22222222-2222-4222-8222-222222222222", "ask");
        push(folder, &first).unwrap();
        push(folder, &second).unwrap();
        // Oldest first, bounded by the tick's budget.
        let queued: Vec<String> = pending(folder, PER_TICK).unwrap().into_iter().map(|(_, value)| value["op"].as_str().unwrap_or_default().to_owned()).collect();
        assert_eq!(queued, ["report", "ask"]);
        assert_eq!(pending(folder, 1).unwrap().len(), 1);
        // A result file is not a request.
        settle(folder, &first, &json!({"status":"applied"})).unwrap();
        let left: Vec<String> = pending(folder, PER_TICK).unwrap().into_iter().map(|(_, value)| value["request_id"].as_str().unwrap_or_default().to_owned()).collect();
        assert_eq!(left, ["22222222-2222-4222-8222-222222222222"]);
        assert_eq!(result(folder, "11111111-1111-4111-8111-111111111111").unwrap(), json!({"status":"applied"}));
        // The request file is gone; a late poll still reads the outcome.
        assert!(!request_path(folder, "11111111-1111-4111-8111-111111111111").exists());
        assert!(wait(folder, "11111111-1111-4111-8111-111111111111", 0.0).unwrap()["status"] == "applied");
        assert!(wait(folder, "22222222-2222-4222-8222-222222222222", 0.0).is_none());
    }

    #[test]
    fn a_request_without_a_canonical_id_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(push(dir.path(), &json!({"op":"report","request_id":"not-a-uuid"})).is_err());
        assert!(pending(dir.path(), PER_TICK).unwrap().is_empty());
    }
}
