//! Bounded, disposable routing metadata for task LISTS only. Canonical JSON
//! stays authoritative; write commands and caller proof never consult this.
//! Every cache entry is tied to its file identity and every lookup verifies a
//! complete before/after path inventory. Like the existing record-by-record
//! reader, rows are canonical record snapshots, not a global writer lock.
//! Partial discovery never proves absence.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::time::{Duration, Instant};

const DIRECTORY: &str = "task-routing-v1";
const CACHE: &str = "task-routing-v1/metadata.json";
const CACHE_BYTES: u64 = 32 << 20;
const ROW_BYTES: usize = 16 << 10;
const FILES: usize = 100_000;
const INCOMPLETE: &str =
    "Task session routing is incomplete; its metadata is updating; retry the list";
const UNAVAILABLE: &str =
    "Task session routing is unavailable; no session references were resolved";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: i64,
    nanos: i64,
    changed: i64,
    changed_nanos: i64,
}

fn stamp(path: &Path) -> Result<Stamp> {
    let metadata = fs::symlink_metadata(path).map_err(|_| UNAVAILABLE)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(UNAVAILABLE.into());
    }
    let (id, metadata) = crate::platform::FileIdentity::at(path).map_err(|_| UNAVAILABLE)?;
    #[cfg(unix)]
    let (changed, changed_nanos) = {
        use std::os::unix::fs::MetadataExt;
        (metadata.ctime(), metadata.ctime_nsec())
    };
    #[cfg(windows)]
    let (changed, changed_nanos) = {
        let file = crate::platform::no_follow(fs::OpenOptions::new().read(true))
            .open(path)
            .map_err(|_| UNAVAILABLE)?;
        windows_change(&file)?
    };
    Ok(Stamp {
        device: id.device,
        inode: id.inode,
        bytes: metadata.len(),
        modified: id.modified,
        nanos: id.modified_nanos,
        changed,
        changed_nanos,
    })
}

#[cfg(windows)]
fn windows_change(file: &fs::File) -> Result<(i64, i64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileBasicInfo, GetFileInformationByHandleEx, FILE_BASIC_INFO,
    };
    let mut info: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as _,
            FileBasicInfo,
            &mut info as *mut _ as _,
            std::mem::size_of_val(&info) as u32,
        )
    } == 0
    {
        return Err(UNAVAILABLE.into());
    }
    Ok((info.ChangeTime, 0))
}

fn opened_stamp(file: &fs::File) -> Result<Stamp> {
    let (id, metadata) =
        crate::platform::FileIdentity::with_metadata(file).map_err(|_| UNAVAILABLE)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(UNAVAILABLE.into());
    }
    #[cfg(unix)]
    let (changed, changed_nanos) = {
        use std::os::unix::fs::MetadataExt;
        (metadata.ctime(), metadata.ctime_nsec())
    };
    #[cfg(windows)]
    let (changed, changed_nanos) = windows_change(file)?;
    Ok(Stamp {
        device: id.device,
        inode: id.inode,
        bytes: metadata.len(),
        modified: id.modified,
        nanos: id.modified_nanos,
        changed,
        changed_nanos,
    })
}

/// The same namespaces and filename rules as `task_bindings_in`, with a
/// count/time bound and explicit errors instead of a silently partial set.
fn inventory(root: &Path, until: Instant) -> Result<BTreeMap<String, Stamp>> {
    let mut found = BTreeMap::new();
    let mut count = 0;
    for namespace in storage::BINDING_DIRS {
        let folder = root.join(namespace);
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(UNAVAILABLE.into()),
        };
        for entry in entries {
            count += 1;
            if count > FILES || Instant::now() >= until {
                return Err(INCOMPLETE.into());
            }
            let entry = entry.map_err(|_| UNAVAILABLE)?;
            let path = entry.path();
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let named = match namespace {
                "dsh/bindings" | "opencode/bindings" => {
                    stem.len() == 64 && stem.bytes().all(|c| c.is_ascii_hexdigit())
                }
                _ => storage::binding_stem(namespace, stem),
            };
            if path.extension().is_none_or(|s| s != "json") || !named {
                continue;
            }
            let source = path
                .strip_prefix(root)
                .map_err(|_| UNAVAILABLE)?
                .to_str()
                .ok_or(UNAVAILABLE)?
                .to_owned();
            found.insert(source, stamp(&path)?);
        }
    }
    Ok(found)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    stamp: Stamp,
    row: Option<Value>,
    digest: String,
}
impl Entry {
    fn digest(source: &str, stamp: &Stamp, row: &Option<Value>) -> Result<String> {
        let bytes = serde_json::to_vec(&(source, stamp, row)).map_err(|_| UNAVAILABLE)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    fn valid(&self, source: &str, stamp: &Stamp) -> bool {
        self.stamp == *stamp
            && Self::digest(source, stamp, &self.row).ok().as_deref() == Some(&self.digest)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cache {
    schema: u8,
    entries: BTreeMap<String, Entry>,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            schema: 1,
            entries: BTreeMap::new(),
        }
    }
}

/// Read only metadata fields, with the canonical record's existing 16 MiB
/// cap. Job commands and output are skipped by the parser and never cached.
fn binding(path: &Path, limit: u64) -> Result<(Option<Value>, Stamp, usize)> {
    let mut file = crate::platform::no_follow(fs::OpenOptions::new().read(true))
        .open(path)
        .map_err(|_| UNAVAILABLE)?;
    let before = opened_stamp(&file)?;
    if before.bytes > session_graph::MAX_RECORD {
        return Err(UNAVAILABLE.into());
    }
    if before.bytes > limit {
        return Err(INCOMPLETE.into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UNAVAILABLE)?;
    #[cfg(test)]
    READ_BYTES.with(|count| count.set(count.get() + bytes.len()));
    #[cfg(test)]
    AFTER_READ.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    if bytes.len() as u64 > limit || opened_stamp(&file)? != before || stamp(path)? != before {
        return Err(INCOMPLETE.into());
    }
    let row = projection(&bytes)?;
    Ok((row, before, bytes.len()))
}

fn projection(bytes: &[u8]) -> Result<Option<Value>> {
    let TaskBinding(mut row) = serde_json::from_slice(bytes).map_err(|_| UNAVAILABLE)?;
    if row.get("rename_shadow") == Some(&Value::Bool(true))
        || row
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || row
            .get("run_id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Ok(None);
    }
    // Only the retained original label is relevant, never any nested payload.
    if let Some(shadow) = row.get_mut("legacy_shadow") {
        *shadow = json!({"name": shadow["name"].as_str()});
    }
    let row = Value::Object(row);
    if row.to_string().len() > ROW_BYTES {
        return Err(UNAVAILABLE.into());
    }
    Ok(Some(row))
}

struct Budget {
    records: usize,
    bytes: u64,
    time: Duration,
}
impl Budget {
    fn foreground() -> Self {
        Self {
            records: 256,
            bytes: 64 << 20,
            time: Duration::from_secs(2),
        }
    }
}

/// Auxiliary cache locks never take the state writer or a provider's lock.
fn guard(root: &Path, name: &str) -> Result<Option<storage::Guard>> {
    use fs2::FileExt;
    // Cache refreshes must not churn the canonical binding directory and
    // trigger another saved-session inventory in a concurrent catalog watch.
    let directory = root.join(DIRECTORY);
    private_dir(&directory)?;
    let metadata = fs::symlink_metadata(&directory).map_err(|_| UNAVAILABLE)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(UNAVAILABLE.into());
    }
    if !crate::platform::verify_private(&directory, crate::platform::Private::Directory)
        .map_err(|_| UNAVAILABLE)?
    {
        return Err(UNAVAILABLE.into());
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    crate::platform::private_open(&mut options);
    crate::platform::no_follow(&mut options);
    let file = options
        .open(directory.join(name))
        .map_err(|_| UNAVAILABLE)?;
    if !file.metadata().map_err(|_| UNAVAILABLE)?.is_file() {
        return Err(UNAVAILABLE.into());
    }
    if !crate::platform::verify_private_file(&file).map_err(|_| UNAVAILABLE)? {
        return Err(UNAVAILABLE.into());
    }
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(storage::Guard::held(file))),
        Err(e) if crate::platform::lock_contended(&e) => Ok(None),
        Err(_) => Err(UNAVAILABLE.into()),
    }
}

/// Refresh a bounded slice, retaining safe partial progress for the worker
/// or a later command. A complete inventory is required even for a cache miss.
fn refresh(root: &Path, budget: Budget) -> Result<Option<Cache>> {
    let _guard = guard(root, "task-routing-v1.lock")?.ok_or(INCOMPLETE)?;
    let until = Instant::now() + budget.time;
    let before = inventory(root, until)?;
    let raw = labels::read_file(&root.join(CACHE), CACHE_BYTES).map_err(|_| UNAVAILABLE)?;
    let mut cache: Cache = raw
        .as_deref()
        .and_then(|b| serde_json::from_slice(b).ok())
        .filter(|c: &Cache| c.schema == 1 && c.entries.len() <= FILES)
        .unwrap_or_default();
    cache.entries.retain(|source, entry| {
        before
            .get(source)
            .is_some_and(|stamp| entry.valid(source, stamp))
    });
    let mut count = 0;
    let mut bytes = 0;
    for (source, expected) in &before {
        if cache.entries.contains_key(source) {
            continue;
        }
        if expected.bytes > session_graph::MAX_RECORD {
            return Err(UNAVAILABLE.into());
        }
        if count >= budget.records
            || bytes + expected.bytes > budget.bytes
            || Instant::now() >= until
        {
            break;
        }
        count += 1;
        let path = root.join(source);
        let (row, observed, consumed) =
            match binding(&path, (budget.bytes - bytes).min(session_graph::MAX_RECORD)) {
                Ok(value) => value,
                Err(error) if error == INCOMPLETE => break,
                Err(error) => return Err(error),
            };
        bytes += consumed as u64;
        let digest = Entry::digest(source, &observed, &row)?;
        cache.entries.insert(
            source.clone(),
            Entry {
                stamp: observed,
                row,
                digest,
            },
        );
    }
    #[cfg(test)]
    BEFORE_VERIFY.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    // Require complete namespace coverage. Existing records may continue to
    // change after their observed snapshot, just as in task_bindings_in; a
    // later lookup must validate their versions again. A global stamp-equality
    // fence would starve lists while unrelated live jobs update their output.
    let after = inventory(root, Instant::now() + Duration::from_secs(2))?;
    // Retain verified record snapshots even when a new source arrived. They
    // are safe progress for the next slice, never complete lookup evidence.
    let namespace_complete = after.keys().eq(before.keys());
    let complete = namespace_complete && cache.entries.len() == before.len();
    let encoded = serde_json::to_vec(&cache).map_err(|_| UNAVAILABLE)?;
    if encoded.len() as u64 > CACHE_BYTES {
        return Err(INCOMPLETE.into());
    }
    if raw.as_deref() != Some(encoded.as_slice()) {
        atomic(
            &root.join(CACHE),
            std::str::from_utf8(&encoded).map_err(|_| UNAVAILABLE)?,
        )?;
    }
    Ok(complete.then_some(cache))
}

fn identity(row: &Value) -> String {
    for key in ["launch_id", "run_id"] {
        if let Some(id) = row[key].as_str().filter(|id| !id.is_empty()) {
            return format!("{key}:{id}");
        }
    }
    String::new()
}

/// Only metadata needed by the selected task rows. A name-only reference
/// includes aliases too, so Machines::resolve retains current-name precedence
/// and ambiguity handling. Unknown immutable IDs never adopt a reused name.
pub(crate) fn rows(root: &Path, wanted: &BTreeSet<(String, String)>) -> Result<Vec<Value>> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let root = crate::platform::canonical(root).map_err(|_| UNAVAILABLE)?;
    let cache = match refresh(&root, Budget::foreground()) {
        Ok(Some(cache)) => cache,
        Ok(None) => {
            request(&root);
            return Err(INCOMPLETE.into());
        }
        Err(error) => {
            if error == INCOMPLETE {
                request(&root);
            }
            return Err(error);
        }
    };
    let mut ids: BTreeMap<_, u8> = wanted
        .iter()
        .filter(|(_, id)| !id.is_empty())
        .map(|(_, id)| (id.as_str(), 0))
        .collect();
    let mut names: BTreeMap<_, [u8; 2]> = wanted
        .iter()
        .filter(|(_, id)| id.is_empty())
        .map(|(name, _)| (name.as_str(), [0, 0]))
        .collect();
    let keep = |count: &mut u8| {
        if *count >= 2 {
            false
        } else {
            *count += 1;
            true
        }
    };
    let mut selected = Vec::new();
    for row in cache.entries.into_values().filter_map(|entry| entry.row) {
        // Two distinct sources already prove ambiguity. Keep exact-name and
        // alias evidence separately, preserving the resolver's precedence.
        let mut needed = ids.get_mut(identity(&row).as_str()).is_some_and(keep);
        if let Some(count) = names.get_mut(string(&row, "name")) {
            needed |= keep(&mut count[0]);
        }
        let aliases: BTreeSet<_> = [
            row["journal_name"].as_str(),
            row["launch_label"].as_str(),
            row["legacy_shadow"]["name"].as_str(),
        ]
        .into_iter()
        .flatten()
        .collect();
        for alias in aliases {
            if let Some(count) = names.get_mut(alias) {
                needed |= keep(&mut count[1]);
            }
        }
        if needed {
            selected.push(row);
        }
    }
    Ok(selected)
}

#[cfg(test)]
thread_local! {
    static READ_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static AFTER_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static BEFORE_VERIFY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

// A libtest image is not a CLI worker; native worker coverage uses the CLI.
#[cfg(test)]
fn request(_root: &Path) {}

#[cfg(not(test))]
fn request(root: &Path) {
    use std::process::{Command, Stdio};
    let launch = (|| -> Result<()> {
        let Some(_launch) = guard(root, "task-routing-launch.lock")? else {
            return Ok(());
        };
        let Some(probe) = guard(root, "task-routing-worker.lock")? else {
            return Ok(());
        };
        let attempt = root.join(DIRECTORY).join("attempt.json");
        let last = labels::read_file(&attempt, 4096)
            .map_err(|_| UNAVAILABLE)?
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v["at"].as_f64())
            .unwrap_or(0.0);
        if (0.0..5.0).contains(&(now() - last)) {
            return Ok(());
        }
        atomic(&attempt, &json!({"at":now()}).to_string())?;
        let mut command = Command::new(std::env::current_exe().map_err(|_| UNAVAILABLE)?);
        for (key, _) in
            std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("HGS_"))
        {
            command.env_remove(key);
        }
        command
            .args(["__state", "task-binding-index-worker"])
            .env("HGS_STATE_DIR", root)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        drop(probe);
        let mut child = crate::platform::spawn_detached(&mut command).map_err(|_| UNAVAILABLE)?;
        // A service/watch can call the list too: do not leave an exited helper
        // as its zombie until that long-lived process exits.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    })();
    let _ = launch;
}

/// One coalesced, finite metadata-only worker. No native provider, session
/// writer, caller authority, task document or input operation is entered.
pub(super) fn worker() -> Result<i32> {
    let root = crate::platform::canonical(root()).map_err(|_| UNAVAILABLE)?;
    let Some(_guard) = guard(&root, "task-routing-worker.lock")? else {
        return Ok(0);
    };
    let until = Instant::now() + Duration::from_secs(60);
    // At most 8 GiB of canonical input and 16 cache slices per worker, even
    // when files churn continuously; the original 60-second bound also holds.
    for _ in 0..16 {
        if Instant::now() >= until {
            break;
        }
        let budget = Budget {
            records: 4096,
            bytes: 512 << 20,
            time: Duration::from_secs(5),
        };
        match refresh(&root, budget) {
            Ok(Some(_)) => return Ok(0),
            Ok(None) => {}
            Err(_) => return Ok(1),
        }
    }
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(root: &Path, name: &str, launch: &str) -> PathBuf {
        let path = root.join(format!("{:x}.json", Sha256::digest(name.as_bytes())));
        fs::write(
            &path,
            json!({"name":name,"run_id":format!("run-{launch}"),"launch_id":launch,
            "shell_jobs":{"unknown":{"status":"unknown","output":"x".repeat(64 << 10)}}})
            .to_string(),
        )
        .unwrap();
        path
    }
    fn wanted(name: &str, id: &str) -> BTreeSet<(String, String)> {
        BTreeSet::from([(name.into(), id.into())])
    }
    fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
        READ_BYTES.with(|n| n.set(0));
        let result = f();
        (result, READ_BYTES.with(std::cell::Cell::get))
    }

    #[test]
    fn warm_pages_read_no_unrelated_bodies_and_return_only_requested_bindings() {
        let temp = tempfile::tempdir().unwrap();
        for n in 0..200 {
            saved(
                temp.path(),
                &format!("codex/unrelated/{n}"),
                &format!("unrelated-{n}"),
            );
        }
        saved(temp.path(), "codex/selected", "selected");
        let keys = wanted("codex/old-label", "launch_id:selected");
        let (first, bytes) = counted(|| rows(temp.path(), &keys).unwrap());
        assert_eq!(first.len(), 1);
        assert!(bytes > 10 << 20);
        let (again, bytes) = counted(|| rows(temp.path(), &keys).unwrap());
        assert_eq!(again, first);
        assert_eq!(bytes, 0, "warm routing must not reread command output");
        let (empty, bytes) = counted(|| rows(temp.path(), &BTreeSet::new()).unwrap());
        assert!(empty.is_empty());
        assert_eq!(bytes, 0);
        assert_eq!(first[0]["name"], "codex/selected");
        assert!(first[0].get("shell_jobs").is_none());
    }

    #[test]
    fn added_or_removed_bindings_never_turn_stale_coverage_into_absence() {
        let temp = tempfile::tempdir().unwrap();
        let first = saved(temp.path(), "codex/first", "shared");
        let keys = wanted("codex/old", "launch_id:shared");
        assert_eq!(rows(temp.path(), &keys).unwrap().len(), 1);
        let second = saved(temp.path(), "codex/second", "shared");
        assert_eq!(
            rows(temp.path(), &keys).unwrap().len(),
            2,
            "both claimants must reach ambiguity checks"
        );
        fs::remove_file(first).unwrap();
        assert_eq!(rows(temp.path(), &keys).unwrap()[0]["name"], "codex/second");
        fs::remove_file(second).unwrap();
        assert!(rows(temp.path(), &keys).unwrap().is_empty());
    }

    #[test]
    fn a_binding_arriving_during_the_inventory_never_becomes_a_canonical_miss() {
        let temp = tempfile::tempdir().unwrap();
        saved(temp.path(), "codex/first", "shared");
        let keys = wanted("codex/old", "launch_id:shared");
        assert_eq!(rows(temp.path(), &keys).unwrap().len(), 1);
        let root = temp.path().to_path_buf();
        BEFORE_VERIFY.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                saved(&root, "codex/arrived", "shared");
            }))
        });
        assert_eq!(rows(temp.path(), &keys).unwrap_err(), INCOMPLETE);
        assert_eq!(rows(temp.path(), &keys).unwrap().len(), 2);
    }

    #[test]
    fn namespace_churn_retains_safe_progress_without_publishing_partial_coverage() {
        let temp = tempfile::tempdir().unwrap();
        saved(temp.path(), "codex/first", "first");
        saved(temp.path(), "codex/second", "second");
        for n in 0..4 {
            let root = temp.path().to_path_buf();
            BEFORE_VERIFY.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    saved(
                        &root,
                        &format!("codex/arriving-{n}"),
                        &format!("arriving-{n}"),
                    );
                }))
            });
            let partial = refresh(
                temp.path(),
                Budget {
                    records: 1,
                    bytes: 1 << 20,
                    time: Duration::from_secs(2),
                },
            )
            .unwrap();
            assert!(
                partial.is_none(),
                "a changed namespace never proves coverage"
            );
            let cache: Cache =
                serde_json::from_slice(&fs::read(temp.path().join(CACHE)).unwrap()).unwrap();
            assert_eq!(
                cache.entries.len(),
                n + 1,
                "safe progress must survive churn"
            );
        }
        assert_eq!(
            rows(temp.path(), &wanted("codex/first", "launch_id:first"))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn an_in_place_change_during_a_body_read_keeps_the_reference_incomplete() {
        changed_during_read(false);
    }

    #[test]
    fn replacing_the_path_during_a_body_read_keeps_the_reference_incomplete() {
        changed_during_read(true);
    }

    fn changed_during_read(replace: bool) {
        let temp = tempfile::tempdir().unwrap();
        let path = saved(temp.path(), "codex/initial", "kept");
        let changed = path.clone();
        AFTER_READ.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let next = json!({"name":"codex/changed","run_id":"run-kept","launch_id":"kept"})
                    .to_string();
                if replace {
                    let next_path = changed.with_extension("replacement");
                    fs::write(&next_path, next).unwrap();
                    fs::rename(next_path, changed).unwrap();
                } else {
                    fs::write(changed, next).unwrap();
                }
            }))
        });
        let keys = wanted("codex/initial", "launch_id:kept");
        assert_eq!(rows(temp.path(), &keys).unwrap_err(), INCOMPLETE);
        assert_eq!(
            rows(temp.path(), &keys).unwrap()[0]["name"],
            "codex/changed"
        );
    }

    #[test]
    fn later_job_output_does_not_starve_a_read_only_record_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        saved(temp.path(), "codex/selected", "selected");
        let busy = saved(temp.path(), "codex/busy", "busy");
        let keys = wanted("codex/selected", "launch_id:selected");
        let expected = rows(temp.path(), &keys).unwrap();
        for n in 0..3 {
            let busy = busy.clone();
            BEFORE_VERIFY.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    let mut record: Value =
                        serde_json::from_slice(&fs::read(&busy).unwrap()).unwrap();
                    record["shell_jobs"]["unknown"]["output"] = json!(format!("Output {n}"));
                    fs::write(busy, record.to_string()).unwrap();
                }))
            });
            assert_eq!(rows(temp.path(), &keys).unwrap(), expected);
        }
        assert_eq!(rows(temp.path(), &keys).unwrap(), expected);
    }

    #[test]
    fn only_two_claimants_per_selector_are_needed_and_aliases_are_deduplicated_per_source() {
        let temp = tempfile::tempdir().unwrap();
        for n in 0..20 {
            let path = saved(temp.path(), &format!("codex/claimant-{n}"), "shared");
            let mut row: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            row["journal_name"] = json!("codex/original");
            row["launch_label"] = json!("codex/original");
            row["legacy_shadow"] = json!({"name":"codex/original"});
            fs::write(path, row.to_string()).unwrap();
        }
        assert_eq!(
            rows(temp.path(), &wanted("codex/old", "launch_id:shared"))
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            rows(temp.path(), &wanted("codex/original", ""))
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn refreshing_metadata_does_not_change_the_binding_directory_or_any_canonical_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let path = saved(temp.path(), "codex/selected", "selected");
        let keys = wanted("codex/selected", "launch_id:selected");
        rows(temp.path(), &keys).unwrap();
        let before = fs::metadata(temp.path()).unwrap().modified().unwrap();
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes).unwrap(); // same routing, newer source stamp
        rows(temp.path(), &keys).unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(
            fs::metadata(temp.path()).unwrap().modified().unwrap(),
            before
        );
    }

    #[test]
    fn an_in_place_rewrite_that_keeps_length_and_mtime_invalidates_cached_identity() {
        let temp = tempfile::tempdir().unwrap();
        let path = saved(temp.path(), "codex/initial", "kept");
        let keys = wanted("codex/initial", "launch_id:kept");
        assert_eq!(
            rows(temp.path(), &keys).unwrap()[0]["name"],
            "codex/initial"
        );
        let before = fs::metadata(&path).unwrap();
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["name"] = json!("codex/renamed");
        fs::write(&path, record.to_string()).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
            .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), before.len());
        assert_eq!(
            rows(temp.path(), &keys).unwrap()[0]["name"],
            "codex/renamed"
        );
    }

    #[test]
    fn corrupted_metadata_and_missing_cache_entries_are_rebuilt_from_canonical_files() {
        let temp = tempfile::tempdir().unwrap();
        saved(temp.path(), "codex/original", "kept");
        let keys = wanted("codex/original", "launch_id:kept");
        let expected = rows(temp.path(), &keys).unwrap();
        let cache_path = temp.path().join(CACHE);
        let mut cache: Value = serde_json::from_slice(&fs::read(&cache_path).unwrap()).unwrap();
        let entries = cache["entries"].as_object_mut().unwrap();
        entries.values_mut().next().unwrap()["row"]["name"] = json!("codex/forged");
        fs::write(&cache_path, cache.to_string()).unwrap();
        assert_eq!(rows(temp.path(), &keys).unwrap(), expected);
        fs::write(&cache_path, b"{broken").unwrap();
        assert_eq!(rows(temp.path(), &keys).unwrap(), expected);
        fs::remove_file(&cache_path).unwrap();
        assert_eq!(rows(temp.path(), &keys).unwrap(), expected);
    }

    #[test]
    fn partial_progress_corrupt_sources_and_a_busy_cache_never_resolve_references() {
        let temp = tempfile::tempdir().unwrap();
        let path = saved(temp.path(), "codex/one", "one");
        saved(temp.path(), "codex/two", "two");
        let tiny = Budget {
            records: 1,
            bytes: 1 << 20,
            time: Duration::from_secs(2),
        };
        assert!(refresh(temp.path(), tiny).unwrap().is_none());
        let cache: Cache =
            serde_json::from_slice(&fs::read(temp.path().join(CACHE)).unwrap()).unwrap();
        assert_eq!(cache.entries.len(), 1);
        let held = guard(temp.path(), "task-routing-v1.lock").unwrap().unwrap();
        assert_eq!(
            rows(temp.path(), &wanted("codex/one", "launch_id:one")).unwrap_err(),
            INCOMPLETE
        );
        drop(held);
        fs::write(path, b"{broken").unwrap();
        assert_eq!(
            rows(temp.path(), &wanted("codex/one", "launch_id:one")).unwrap_err(),
            UNAVAILABLE
        );
    }

    #[test]
    fn an_oversized_record_is_unavailable_without_reading_its_body() {
        let temp = tempfile::tempdir().unwrap();
        let path = saved(temp.path(), "codex/oversized", "oversized");
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_len(128 << 20)
            .unwrap();
        let (result, bytes) = counted(|| {
            rows(
                temp.path(),
                &wanted("codex/oversized", "launch_id:oversized"),
            )
        });
        assert_eq!(result.unwrap_err(), UNAVAILABLE);
        assert_eq!(bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn cache_links_and_canonical_binding_links_are_never_followed() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let path = saved(temp.path(), "codex/selected", "selected");
        let keys = wanted("codex/selected", "launch_id:selected");
        rows(temp.path(), &keys).unwrap();
        let canonical = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let outside = other.path().join("untouched.json");
        fs::write(&outside, &canonical).unwrap();
        symlink(&outside, &path).unwrap();
        let (result, bytes) = counted(|| rows(temp.path(), &keys));
        assert_eq!(result.unwrap_err(), UNAVAILABLE);
        assert_eq!(bytes, 0);
        assert_eq!(fs::read(&outside).unwrap(), canonical);
        fs::remove_file(&path).unwrap();
        fs::write(path, &canonical).unwrap();
        let cache = temp.path().join(CACHE);
        fs::remove_file(&cache).unwrap();
        symlink(&outside, cache).unwrap();
        assert_eq!(rows(temp.path(), &keys).unwrap_err(), UNAVAILABLE);
        assert_eq!(fs::read(outside).unwrap(), canonical);
    }

    #[test]
    fn aliases_shadows_and_all_binding_namespaces_keep_the_existing_resolution_contract() {
        let temp = tempfile::tempdir().unwrap();
        for (n, namespace) in storage::BINDING_DIRS.iter().enumerate() {
            fs::create_dir_all(temp.path().join(namespace)).unwrap();
            let id = uuid::Uuid::from_u128(n as u128 + 1).to_string();
            let stem = if namespace.ends_with("sessions") {
                id
            } else {
                format!("{n:064x}")
            };
            let row = json!({"name":format!("codex/current-{n}"),"run_id":format!("run-{n}"),
                "launch_label":format!("codex/launched-{n}"),"journal_name":format!("codex/journal-{n}"),
                "legacy_shadow":{"name":format!("codex/old-{n}")}});
            fs::write(
                temp.path().join(namespace).join(format!("{stem}.json")),
                row.to_string(),
            )
            .unwrap();
        }
        let path = saved(temp.path(), "codex/shadow", "shadow");
        let mut shadow: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        shadow["rename_shadow"] = json!(true);
        fs::write(path, shadow.to_string()).unwrap();
        for n in 0..storage::BINDING_DIRS.len() {
            for prefix in ["launched", "journal", "old"] {
                let found = rows(temp.path(), &wanted(&format!("codex/{prefix}-{n}"), "")).unwrap();
                assert_eq!(found.len(), 1);
                assert_eq!(found[0]["name"], format!("codex/current-{n}"));
            }
        }
        assert!(
            rows(temp.path(), &wanted("codex/shadow", "launch_id:shadow"))
                .unwrap()
                .is_empty()
        );
    }
}
