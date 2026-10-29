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

/// C1: where a claimed run waits while its runner executes it:
/// `<request_id>.running.json`, naming the request, when it started and its bound.
pub(crate) fn running_path(folder: &Path, request_id: &str) -> PathBuf {
    dir(folder).join(format!("{request_id}.running.json"))
}

/// Writes the claim for a run the way a request is written: beside its name and
/// renamed, so a reader never sees half of it.
pub(crate) fn write_running(folder: &Path, request_id: &str, marker: &Value) -> anyhow::Result<PathBuf> {
    ensure!(super::store::canonical_uuid(request_id), "An outbox request needs a canonical request_id");
    let dir = dir(folder);
    fs::create_dir_all(&dir)?;
    let temp = dir.join(format!("{request_id}.running.tmp"));
    fs::write(&temp, serde_json::to_vec(marker)?)?;
    let path = running_path(folder, request_id);
    fs::rename(&temp, &path)?;
    Ok(path)
}

/// The claim of one run, when the runner still holds it.
pub(crate) fn running(folder: &Path, request_id: &str) -> Option<Value> {
    fs::read(running_path(folder, request_id)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// The claim a run's runner holds: the request plus when it started and its
/// bound; None when there is no such file or it is unusable.
pub(crate) fn read_marker(path: &Path) -> Option<Value> {
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > REQUEST_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(REQUEST_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > REQUEST_BYTES {
        return None;
    }
    serde_json::from_slice::<Value>(&bytes).ok().filter(Value::is_object)
}

/// Every claim left in a folder, at most `SCAN_LIMIT` of them (C1's stale runs).
pub(crate) fn running_all(folder: &Path) -> Vec<(PathBuf, Value)> {
    let Ok(entries) = fs::read_dir(dir(folder)) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in entries.take(SCAN_LIMIT).flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".running.json") else { continue };
        if !super::store::canonical_uuid(id) {
            continue;
        }
        let path = entry.path();
        if let Some(value) = read_marker(&path) {
            found.push((path, value));
        }
    }
    found.sort_by(|left, right| left.0.file_name().cmp(&right.0.file_name()));
    found
}

/// Records a run's outcome and drops its claim: the member's poll reads the
/// first, and a claimed run is answered exactly once.
pub(crate) fn settle_running(folder: &Path, request_id: &str, outcome: &Value) -> anyhow::Result<()> {
    fs::write(result_path(folder, request_id), serde_json::to_vec(outcome)?)?;
    let _ = fs::remove_file(running_path(folder, request_id));
    Ok(())
}

/// N4: a request with this id still queued, for a member asking again.
pub(crate) fn request(folder: &Path, request_id: &str) -> Option<Value> {
    read_request(&request_path(folder, request_id), &format!("{request_id}.json")).ok()
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

/// The largest request file the worker reads: the CLI's own request limit.
pub(crate) const REQUEST_BYTES: u64 = 256 * 1024;

/// How many outbox entries one tick looks at (U20): a folder flooded with
/// files costs a bounded scan, never a walk of all of them.
pub(crate) const SCAN_LIMIT: usize = 1000;

/// How many outcomes stay for a member's late poll, and for how long; the
/// rejected requests kept for a look are bounded the same way.
pub(crate) const RESULTS_KEPT: usize = 100;
pub(crate) const RESULTS_AGE: Duration = Duration::from_secs(24 * 3600);

/// How many request files one tick opens, usable or not.
pub(crate) const READS_PER_TICK: usize = 100;

/// A half-written request older than this was abandoned by its writer.
const TEMP_AGE: Duration = Duration::from_secs(3600);

/// Where the worker moves a file it cannot use, so it never comes back.
pub(crate) fn rejected_dir(folder: &Path) -> PathBuf {
    dir(folder).join("rejected")
}

/// The requests waiting for the worker, oldest first, at most `limit`, from at
/// most `SCAN_LIMIT` entries. Every file the worker cannot use - not a regular
/// file, over `REQUEST_BYTES`, unreadable, not JSON, or not named by its own
/// canonical request_id - is answered with an error result when its id is
/// known and moved to `rejected/`, so it never holds the queue. Old outcomes
/// and abandoned half-written requests are pruned on the way.
pub(crate) fn pending(folder: &Path, limit: usize) -> anyhow::Result<Vec<(PathBuf, Value)>> {
    let outbox = dir(folder);
    let Ok(entries) = fs::read_dir(&outbox) else { return Ok(Vec::new()) };
    let now = SystemTime::now();
    let age = |modified: SystemTime| now.duration_since(modified).unwrap_or_default();
    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut results: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut rejected_any = false;
    for entry in entries.take(SCAN_LIMIT).flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // The metadata of the entry itself: a link is never followed.
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        let modified = meta.modified().unwrap_or(now);
        if name == "rejected" && meta.is_dir() {
            continue;
        }
        if !meta.is_file() {
            reject(folder, &path, &name, "An outbox request is a regular file");
            rejected_any = true;
        } else if name.ends_with(".result.json") {
            results.push((modified, path));
        } else if name.ends_with(".running.json") {
            // C1/N11: a claimed run is never a request, and its claim is never
            // pruned here: the runner (or the stale-claim check) settles it.
            // A live claim dropped by this scan would let the same id run twice.
            continue;
        } else if name.ends_with(".tmp") {
            if age(modified) > TEMP_AGE {
                let _ = fs::remove_file(&path);
            }
        } else if name.ends_with(".json") {
            candidates.push((modified, path));
        } else {
            reject(folder, &path, &name, "Not an outbox request");
            rejected_any = true;
        }
    }
    prune(&mut results, age);
    candidates.sort();
    let mut requests = Vec::new();
    // At most READS_PER_TICK files are opened: unusable ones are answered and
    // moved away, so the next tick reaches the requests behind them.
    for (_, path) in candidates.into_iter().take(READS_PER_TICK) {
        if requests.len() >= limit {
            break;
        }
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        match read_request(&path, &name) {
            Ok(value) => requests.push((path, value)),
            Err(error) => {
                reject(folder, &path, &name, &format!("{error:#}"));
                rejected_any = true;
            }
        }
    }
    if rejected_any {
        prune_rejected(folder, age);
    }
    Ok(requests)
}

/// One request file, read only when it is a regular file of at most
/// `REQUEST_BYTES`, and only that much of it.
fn read_request(path: &Path, name: &str) -> anyhow::Result<Value> {
    use std::io::Read;
    let file = fs::File::open(path).context("The request cannot be read")?;
    let meta = file.metadata().context("The request cannot be read")?;
    ensure!(meta.is_file(), "An outbox request is a regular file");
    ensure!(meta.len() <= REQUEST_BYTES, "An outbox request is at most {} KiB", REQUEST_BYTES / 1024);
    let mut bytes = Vec::new();
    file.take(REQUEST_BYTES + 1).read_to_end(&mut bytes).context("The request cannot be read")?;
    ensure!(bytes.len() as u64 <= REQUEST_BYTES, "An outbox request is at most {} KiB", REQUEST_BYTES / 1024);
    let value: Value = serde_json::from_slice(&bytes).context("The request is not valid JSON")?;
    ensure!(value.is_object(), "An outbox request is a JSON object");
    // The worker settles a request by its id, so the file must carry it.
    let id = value["request_id"].as_str().unwrap_or_default();
    ensure!(
        super::store::canonical_uuid(id) && name == format!("{id}.json"),
        "An outbox request is named by its canonical request_id"
    );
    Ok(value)
}

/// The request id a file name carries, when it is one.
fn named_id(name: &str) -> Option<&str> {
    let id = name.strip_suffix(".json")?;
    super::store::canonical_uuid(id).then_some(id)
}

/// Answers a file the worker cannot use and moves it out of the queue: to
/// `rejected/`, or away when even that fails.
fn reject(folder: &Path, path: &Path, name: &str, error: &str) {
    if let Some(id) = named_id(name) {
        let outcome = json!({"status":"refused","request_id":id,"error":error});
        if let Ok(bytes) = serde_json::to_vec(&outcome) {
            let _ = fs::write(result_path(folder, id), bytes);
        }
    }
    let rejected = rejected_dir(folder);
    let moved = fs::create_dir_all(&rejected).is_ok() && fs::rename(path, rejected.join(name)).is_ok();
    if !moved {
        // A directory is removed only when empty; anything else goes.
        let _ = fs::remove_file(path).or_else(|_| fs::remove_dir(path));
    }
}

/// Keeps the `RESULTS_KEPT` newest files of a list, none older than
/// `RESULTS_AGE`.
fn prune(files: &mut [(SystemTime, PathBuf)], age: impl Fn(SystemTime) -> Duration) {
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (index, (modified, path)) in files.iter().enumerate() {
        if index >= RESULTS_KEPT || age(*modified) > RESULTS_AGE {
            let _ = fs::remove_file(path).or_else(|_| fs::remove_dir_all(path));
        }
    }
}

/// Bounds `rejected/` like the outcomes, from at most `SCAN_LIMIT` entries.
fn prune_rejected(folder: &Path, age: impl Fn(SystemTime) -> Duration) {
    let Ok(entries) = fs::read_dir(rejected_dir(folder)) else { return };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .take(SCAN_LIMIT)
        .flatten()
        .filter_map(|entry| {
            let meta = fs::symlink_metadata(entry.path()).ok()?;
            Some((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), entry.path()))
        })
        .collect();
    prune(&mut files, age);
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

    /// N11: a live run's claim is never pruned by the tick's scan. Dropping
    /// one let the same request id be claimed again and run twice.
    #[test]
    fn a_live_claim_is_never_pruned_from_the_outbox() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path();
        for n in 0..150u32 {
            let id = format!("{n:08x}-1111-4111-8111-111111111111");
            write_running(folder, &id, &json!({"request_id":id,"started":1.0,"timeout_secs":1800})).unwrap();
        }
        assert!(pending(folder, PER_TICK).unwrap().is_empty(), "a claim is not a request");
        assert_eq!(running_all(folder).len(), 150, "every live claim stays");
    }

    #[test]
    fn a_request_without_a_canonical_id_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(push(dir.path(), &json!({"op":"report","request_id":"not-a-uuid"})).is_err());
        assert!(pending(dir.path(), PER_TICK).unwrap().is_empty());
    }

    fn aged(path: &Path, seconds: u64) {
        let at = SystemTime::now() - Duration::from_secs(seconds);
        fs::File::options().write(true).open(path).unwrap().set_modified(at).unwrap();
    }

    #[test]
    fn an_oversized_misnamed_or_non_file_request_is_answered_and_moved_away() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path();
        let outbox = super::dir(folder);
        fs::create_dir_all(&outbox).unwrap();
        // Over the CLI's 256 KiB request limit: refused by its size.
        let big = "55555555-5555-4555-8555-555555555555";
        fs::write(outbox.join(format!("{big}.json")), vec![b' '; REQUEST_BYTES as usize + 1]).unwrap();
        // Valid JSON under another request's name: it could never be settled.
        let misnamed = "66666666-6666-4666-8666-666666666666";
        fs::write(outbox.join(format!("{misnamed}.json")), serde_json::to_vec(&request("77777777-7777-4777-8777-777777777777", "report")).unwrap()).unwrap();
        // A directory is not a request, and neither is a stray file.
        let folder_id = "88888888-8888-4888-8888-888888888888";
        fs::create_dir_all(outbox.join(format!("{folder_id}.json")).join("inside")).unwrap();
        fs::write(outbox.join("notes.txt"), b"hello").unwrap();
        // A link is never followed: the worker runs unsandboxed.
        let target = folder.join("secret.json");
        fs::write(&target, serde_json::to_vec(&request("99999999-9999-4999-8999-999999999999", "report")).unwrap()).unwrap();
        let link = outbox.join("99999999-9999-4999-8999-999999999999.json");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        let valid = request("11111111-1111-4111-8111-111111111111", "report");
        push(folder, &valid).unwrap();
        let queued = pending(folder, PER_TICK).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].1["request_id"], "11111111-1111-4111-8111-111111111111");
        let error = |id: &str| result(folder, id).unwrap()["error"].as_str().unwrap_or_default().to_owned();
        assert!(error(big).contains("at most 256 KiB"), "{}", error(big));
        assert!(error(misnamed).contains("named by its canonical request_id"), "{}", error(misnamed));
        assert!(error(folder_id).contains("regular file"), "{}", error(folder_id));
        let rejected = rejected_dir(folder);
        for name in [format!("{big}.json"), format!("{misnamed}.json"), format!("{folder_id}.json"), "notes.txt".into()] {
            assert!(!outbox.join(&name).exists() && rejected.join(&name).exists(), "{name}");
        }
        if linked {
            // The link itself moved; its target was never read or touched.
            assert!(!link.exists() && fs::symlink_metadata(rejected.join("99999999-9999-4999-8999-999999999999.json")).is_ok());
            assert!(target.exists());
        }
        // Nothing comes back on the next tick.
        assert_eq!(pending(folder, PER_TICK).unwrap().len(), 1);
    }

    #[test]
    fn a_flooded_outbox_is_scanned_in_bounded_slices() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path();
        let outbox = super::dir(folder);
        fs::create_dir_all(&outbox).unwrap();
        for n in 0..1500 {
            fs::write(outbox.join(format!("junk-{n}.txt")), b"x").unwrap();
        }
        let valid = request("11111111-1111-4111-8111-111111111111", "report");
        push(folder, &valid).unwrap();
        let live = || fs::read_dir(&outbox).unwrap().filter(|entry| entry.as_ref().unwrap().file_name() != "rejected").count();
        // One tick looks at SCAN_LIMIT entries at most.
        let first = pending(folder, PER_TICK).unwrap();
        assert!(live() >= 1501 - SCAN_LIMIT, "{}", live());
        let mut found = !first.is_empty();
        for _ in 0..2 {
            found |= !pending(folder, PER_TICK).unwrap().is_empty();
        }
        assert!(found, "the valid request was reached");
        assert_eq!(live(), 1, "only the valid request is left");
        // The rejected files are bounded too.
        assert!(fs::read_dir(rejected_dir(folder)).unwrap().count() <= RESULTS_KEPT);
    }

    #[test]
    fn outcomes_are_kept_for_a_bounded_time_and_count() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path();
        fs::create_dir_all(super::dir(folder)).unwrap();
        for n in 0..120u32 {
            let id = format!("{n:08x}-1111-4111-8111-111111111111");
            settle(folder, &json!({"request_id":id}), &json!({"status":"applied"})).unwrap();
            // The first ones are the oldest.
            aged(&result_path(folder, &id), 1000 - n as u64);
        }
        let stale = "aaaaaaaa-1111-4111-8111-111111111111";
        settle(folder, &json!({"request_id":stale}), &json!({"status":"applied"})).unwrap();
        aged(&result_path(folder, stale), 25 * 3600);
        // A half-written request is dropped only once it is abandoned.
        let fresh = super::dir(folder).join("bbbbbbbb-1111-4111-8111-111111111111.tmp");
        let abandoned = super::dir(folder).join("cccccccc-1111-4111-8111-111111111111.tmp");
        fs::write(&fresh, b"{").unwrap();
        fs::write(&abandoned, b"{").unwrap();
        aged(&abandoned, 2 * 3600);
        assert!(pending(folder, PER_TICK).unwrap().is_empty());
        let results = fs::read_dir(super::dir(folder)).unwrap().filter(|entry| entry.as_ref().unwrap().file_name().to_string_lossy().ends_with(".result.json")).count();
        assert_eq!(results, RESULTS_KEPT);
        assert!(result(folder, stale).is_none());
        // The newest stay, the oldest went.
        assert!(result(folder, "00000077-1111-4111-8111-111111111111").is_some());
        assert!(result(folder, "00000000-1111-4111-8111-111111111111").is_none());
        assert!(fresh.exists() && !abandoned.exists());
    }
}
