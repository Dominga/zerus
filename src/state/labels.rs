//! The label index (zerus-ucck P3a): `labels/<sha256(label)>.json` lists the
//! sessions stored in the ID layout (`sessions/<id>.json`) under a label. A
//! label's candidates are its first holder, at the label's own path, plus
//! these sessions.
//!
//! The index is always a superset. A writer makes a session's entry durable
//! before its record takes the label, and a record write refuses when that
//! failed; the record leaves the label before its entry is removed. A durable
//! record therefore always has an entry, so a crash or a lock-free reader can
//! only see an extra entry, which verification drops, never miss one.
//!
//! An index that cannot be read never lets a reader settle for the label's
//! first holder: a bounded scan of `sessions/` decides instead, and when even
//! that cannot prove the set complete, resolution refuses. Entries route; they
//! grant nothing: every candidate is verified against its own record.
use super::*;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

/// The scan that stands in for an index that cannot be proven reads at most
/// this many records, within [`SCAN_TIME`], checked before every read and when
/// the scan settles.
const SCAN_RECORDS: usize = 2_000;
const SCAN_TIME: Duration = Duration::from_secs(2);
/// Index files are small; anything larger is not one of ours.
const INDEX_BYTES: u64 = 1024 * 1024;
/// The manifest, inside `labels/` so a hand deletion of the index removes it
/// too: `{"version":1,"labels":{<sha256(label)>:<sha256 of that label's
/// file>}}`, one entry for every label that has a session in the ID layout
/// (#1437 review). It proves, for one label at a time, that the label has no
/// such session (no entry) or exactly the sessions its file lists (the file
/// hashes to its entry).
const MANIFEST: &str = ".complete";
/// The manifest's bound: about 120 bytes a label.
const MANIFEST_BYTES: u64 = 16 << 20;

type Manifest = BTreeMap<String, String>;

fn manifest_path() -> PathBuf {
    manifest_in(&root())
}

fn manifest_in(state: &Path) -> PathBuf {
    state.join("labels").join(MANIFEST)
}

fn key(label: &str) -> String {
    format!("{:x}", Sha256::digest(label.as_bytes()))
}

fn path(label: &str) -> PathBuf {
    path_in(&root(), label)
}

fn path_in(state: &Path, label: &str) -> PathBuf {
    state.join("labels").join(format!("{}.json", key(label)))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A small private file's bytes: never through a final link, never blocking
/// on a FIFO, only a regular file, at most `limit` bytes. `Ok(None)` only when
/// it verifiably does not exist; anything else there that cannot be read is
/// an error, never absence.
pub(super) fn read_file(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let file = match crate::platform::no_follow(std::fs::OpenOptions::new().read(true)).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("too large"));
    }
    Ok(Some(bytes))
}

/// The manifest; None when it is absent, unreadable or not well formed.
fn manifest() -> Option<Manifest> {
    read_manifest(&root())
}

fn read_manifest(state: &Path) -> Option<Manifest> {
    parse_manifest(&read_file(&manifest_in(state), MANIFEST_BYTES).ok()??)
}

/// The manifest in `bytes`; None when it is not well formed.
fn parse_manifest(bytes: &[u8]) -> Option<Manifest> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    if value["version"] != 1 {
        return None;
    }
    value["labels"]
        .as_object()?
        .iter()
        .map(|(key, digest)| digest.as_str().map(|digest| (key.clone(), digest.to_owned())))
        .collect()
}

fn write_manifest(manifest: &Manifest) -> Result<()> {
    atomic(
        &manifest_path(),
        &format!("{}\n", json!({"version": 1, "labels": manifest})),
    )
}

/// The IDs a label file lists: its own label, one or more canonical IDs.
fn parse_index(label: &str, bytes: &[u8]) -> Option<Vec<String>> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    if value["version"] != 1 || string(&value, "label") != label {
        return None;
    }
    let ids = value["ids"]
        .as_array()?
        .iter()
        .map(|id| id.as_str().filter(|id| session_id::canonical(id)).map(str::to_owned))
        .collect::<Option<Vec<String>>>()?;
    (!ids.is_empty()).then_some(ids)
}

/// The IDs under `label`, when the manifest proves them: none when it lists
/// no entry for the label, else exactly what the label's file lists, which
/// must be there, readable, well formed and hash to the manifest's entry.
/// None when anything is missing, changed or unreadable: the caller then scans
/// or refuses, and never settles for the label's first holder or for a subset.
fn listed(label: &str) -> Option<Vec<String>> {
    listed_in(&root(), label)
}

/// [`listed`], under the state root `state`.
pub(super) fn listed_in(state: &Path, label: &str) -> Option<Vec<String>> {
    let manifest = read_file(&manifest_in(state), MANIFEST_BYTES).ok()??;
    prove(label, &manifest, || read_file(&path_in(state, label), INDEX_BYTES).ok().flatten())
}

/// The IDs the label index proves for `label`, given the manifest's bytes
/// and a reader of the label's file, which runs only when the manifest lists
/// the label. None when the manifest lists no entry for the label; else exactly
/// what the label's file lists, which must be there, hash to the entry and
/// list one or more canonical IDs. None, never fewer IDs, when anything is
/// malformed, missing or changed (#1437 review). Every reader of the index,
/// the native ones included, decides through this (`prove_label_index`).
pub(crate) fn prove(
    label: &str,
    manifest: &[u8],
    file: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<Vec<String>> {
    let manifest = parse_manifest(manifest)?;
    let Some(expected) = manifest.get(&key(label)) else {
        return Some(Vec::new());
    };
    let bytes = file()?;
    if digest(&bytes) != *expected {
        return None;
    }
    parse_index(label, &bytes)
}

/// Writes `label`'s file and then its manifest entry; the last session's
/// removal takes the entry out first, then the file (#1437 review). Under the
/// state writer lock, with a valid manifest ([`ensure_manifest`]). A reader
/// between the two steps sees a file that does not match its entry and scans.
fn save(label: &str, ids: &[String]) -> Result<()> {
    let mut manifest = manifest().ok_or("the label index has no valid manifest")?;
    let path = path(label);
    if ids.is_empty() {
        if manifest.remove(&key(label)).is_some() {
            write_manifest(&manifest)?;
        }
        storage::cut_write()?;
        return match std::fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        };
    }
    let text = format!("{}\n", json!({"version": 1, "label": label, "ids": ids}));
    atomic(&path, &text)?;
    manifest.insert(key(label), digest(text.as_bytes()));
    write_manifest(&manifest)
}

/// Lists `id` under `label`, durably, before its record takes the label.
/// Under the state writer lock. An index that cannot be proven is rebuilt
/// from a scan first; one that still cannot be is an error, so the record
/// write that depends on it refuses.
pub(super) fn add(label: &str, id: &str) -> Result<()> {
    ensure_manifest()?;
    let mut ids = match listed(label) {
        Some(ids) => ids,
        None => scan(label)?,
    };
    if !ids.iter().any(|known| known == id) {
        ids.push(id.to_owned());
        ids.sort();
    }
    save(label, &ids)
}

/// Removes `id` from `label`'s entry once its record has left the label.
/// Under the state writer lock. A failure only leaves an extra entry.
pub(super) fn remove(label: &str, id: &str) -> Result<()> {
    ensure_manifest()?;
    let mut ids = match listed(label) {
        Some(ids) => ids,
        None => scan(label)?,
    };
    ids.retain(|known| known != id);
    save(label, &ids)
}

/// Makes the manifest valid, under the state writer lock: a valid one is
/// kept; otherwise it is rebuilt from a bounded enumeration of `sessions/`,
/// every session listed under its label, together with every IDs a label
/// file already lists (so an entry written ahead of its record is kept). A
/// record that cannot be read, or an enumeration beyond its bounds, is an
/// error and writes no manifest: readers keep scanning or refusing.
pub(super) fn ensure_manifest() -> Result<()> {
    if manifest().is_some() {
        return Ok(());
    }
    let deadline = Instant::now() + SCAN_TIME;
    let mut found: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for (id, record) in enumerate(deadline)? {
        let label = string(&record, "name").to_owned();
        found.entry(key(&label)).or_insert_with(|| (label, Vec::new())).1.push(id);
    }
    let folder = root().join("labels");
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for (count, entry) in entries.enumerate() {
            if count >= SCAN_RECORDS || Instant::now() >= deadline {
                return Err("the label index is too large to rebuild here".into());
            }
            let path = entry.map_err(|error| error.to_string())?.path();
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if path.extension().is_none_or(|extension| extension != "json") || stem.len() != 64 {
                continue;
            }
            let Some(bytes) = read_file(&path, INDEX_BYTES).map_err(|error| error.to_string())? else {
                continue;
            };
            let Some(label) = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .map(|value| string(&value, "label").to_owned())
                .filter(|label| key(label) == stem)
            else {
                continue;
            };
            if let Some(ids) = parse_index(&label, &bytes) {
                found.entry(stem.to_owned()).or_insert_with(|| (label, Vec::new())).1.extend(ids);
            }
        }
    }
    let mut manifest = Manifest::new();
    for (key, (label, mut ids)) in found {
        ids.sort();
        ids.dedup();
        let text = format!("{}\n", json!({"version": 1, "label": label, "ids": ids}));
        atomic(&path(&label), &text)?;
        manifest.insert(key, digest(text.as_bytes()));
    }
    write_manifest(&manifest)
}

/// Rebuilds a manifest that cannot be read, from label resolution only (a
/// command, never a hook), when the writer lock is free at once, as `ids/`
/// is repaired (P2a). Until then every reader scans or refuses.
pub(super) fn repair() {
    // Without `sessions/` nothing is in the ID layout: nothing to prove, and
    // resolution then writes nothing.
    if root().join("sessions").is_dir() && manifest().is_none() {
        if let Ok(Some(_guard)) = storage::writer_until(Instant::now()) {
            let _ = ensure_manifest();
        }
    }
}

/// Every live or saved session in `sessions/`, read within the bounds and
/// `deadline`; an error when they cannot cover it, or when a record there
/// cannot be read.
fn enumerate(deadline: Instant) -> Result<Vec<(String, Value)>> {
    let dir = root().join("sessions");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let late = || "the sessions in the ID layout could not all be read in time; choose one with --session-id".to_owned();
    let mut found = Vec::new();
    for (count, entry) in entries.enumerate() {
        if count >= SCAN_RECORDS || Instant::now() >= deadline {
            return Err(late());
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let Some(id) = entry
            .path()
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|id| session_id::canonical(id))
            .map(str::to_owned)
        else {
            continue;
        };
        let record = storage::read_id(&id).map_err(|error| {
            format!("a session in the ID layout could not be read ({error}); choose one with --session-id")
        })?;
        if string(&record, "archive_id").is_empty() && record["rename_shadow"] != true {
            found.push((id, record));
        }
    }
    if Instant::now() >= deadline {
        return Err(late());
    }
    Ok(found)
}

/// Whether `record`, read from the ID layout, is the live or saved session
/// `id` under `label`.
fn holds(record: &Value, id: &str, label: &str) -> bool {
    string(record, "session_id") == id
        && string(record, "name") == label
        && storage::in_id_layout(record)
        && string(record, "archive_id").is_empty()
        && record["rename_shadow"] != true
}

/// The sessions in the ID layout under `label`, by a bounded scan of
/// `sessions/`. An error when the bounds cannot cover it, or when a record
/// there cannot be read: it may be this label's.
fn scan(label: &str) -> Result<Vec<String>> {
    let deadline = Instant::now() + SCAN_TIME;
    let mut ids: Vec<String> = enumerate(deadline)
        .map_err(|error| format!("{label}: {error}"))?
        .into_iter()
        .filter(|(id, record)| holds(record, id, label))
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    Ok(ids)
}

/// The record of `id` when it is a live or saved session under `label`; None
/// only when its file verifiably does not exist, or its record left the label
/// (a stale entry). Anything present there that cannot be read, a link or a
/// FIFO included, is never taken as absent: the set cannot be proven
/// complete, so it is an error (#1437 review).
fn verified(id: &str, label: &str) -> Result<Option<Value>> {
    let Some(path) = storage::id_record_path(id) else {
        return Ok(None);
    };
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{label}: {error}")),
        Ok(_) => {}
    }
    let record = storage::read_id(id).map_err(|error| {
        format!("{label}: a session that may share this label could not be read ({error}); choose one with --session-id")
    })?;
    Ok(holds(&record, id, label).then_some(record))
}

/// A label's candidates: its first holder, when the label's own path holds a
/// record, and every verified session in the ID layout under it.
pub(super) struct Candidates {
    pub first: bool,
    pub by_id: Vec<Value>,
}

/// The verified candidates of `label`, read without the lock. An index that
/// the manifest cannot prove falls back to the bounded scan; when the scan
/// cannot prove the set complete it is an error, never the first holder alone.
pub(super) fn candidates(label: &str) -> Result<Candidates> {
    let ids = match listed(label) {
        Some(ids) => ids,
        None => scan(label)?,
    };
    let mut by_id = Vec::new();
    for id in &ids {
        if let Some(record) = verified(id, label)? {
            by_id.push(record);
        }
    }
    Ok(Candidates {
        first: storage::label_record_path(label).is_file(),
        by_id,
    })
}

/// The ID layout end to end (zerus-ucck P3a). Sessions in the ID layout are
/// written by hand here, as P3e will write them; P3a reads, writes, renames,
/// archives and forgets them in place and never makes one.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Runs the ignored fixture in a child with a private state root and no
    /// tmux server, so nothing here touches real sessions.
    #[test]
    fn the_id_layout_is_read_written_and_retired_in_place() {
        let state = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::labels::tests::id_layout_fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", state.path().join("sessions-root"))
            .env("TMUX_TMPDIR", state.path().join("tmux"))
            .env("ZERUS_ID_LAYOUT_FIXTURE", "1")
            .env_remove("TMUX")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// #1437 review: every reader of the label index outside this module
    /// decides through `prove_label_index`, the same proof label resolution
    /// uses, and never parses an index file's IDs itself. A new reader fails
    /// here until it is listed.
    #[test]
    fn every_label_index_reader_proves_through_the_shared_parser() {
        const READERS: [&str; 2] = ["src/desktop/original_store.rs", "src/state/task_reconcile.rs"];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        let mut stack = vec![root.join("src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_none_or(|name| name != "tests") {
                        stack.push(path);
                    }
                    continue;
                }
                let relative = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                if path.extension().is_none_or(|extension| extension != "rs")
                    || relative == "src/state/labels.rs"
                    || relative.ends_with("/tests.rs")
                {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                // Production code only: an inline test module comes last.
                let production = text
                    .split("#[cfg(test)]\nmod tests")
                    .next()
                    .unwrap()
                    .split("#[cfg(all(test, unix))]\nmod tests")
                    .next()
                    .unwrap();
                if !(production.contains("\"labels\"") && production.contains(".complete")) {
                    continue;
                }
                assert!(production.contains("prove_label_index("), "{relative} reads the label index without the shared proof");
                assert!(
                    production.contains("is_id_layout_record("),
                    "{relative} reads candidates without the shared identity check"
                );
                assert!(!production.contains("\"ids\""), "{relative} parses an index file's IDs itself");
                found.push(relative);
            }
        }
        found.sort();
        assert_eq!(found, READERS, "the label index's readers changed; review the new one");
    }

    /// #1430 review: every source file that lists a binding directory also
    /// covers the ID layout, so a new scanner cannot miss it. A file that names
    /// a binding directory and calls `read_dir` must also name `sessions` (or
    /// use `storage::BINDING_DIRS`), unless listed here with its reason.
    #[test]
    fn binding_dirs_cover_the_id_layout() {
        const ALLOWED: [(&str, &str); 3] = [
            (
                "src/state/dsh.rs",
                "DeepSeek's own bindings; its ID layout comes with P3e",
            ),
            (
                "src/state/opencode.rs",
                "OpenCode's own bindings; its ID layout comes with P3e",
            ),
            (
                "src/state/rename.rs",
                "legacy rename routes, which the ID layout never writes",
            ),
        ];
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        let mut stack = vec![source.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(source.parent().unwrap())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if path.extension().is_none_or(|e| e != "rs")
                    || relative.contains("/tests")
                    || relative.ends_with("tests.rs")
                {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let lists = text.contains("read_dir")
                    && [
                        "\"dsh/bindings\"",
                        "\"opencode/bindings\"",
                        "join(\"bindings\")",
                    ]
                    .iter()
                    .any(|dir| text.contains(dir));
                let covers = text.contains("\"sessions\"")
                    || text.contains("BINDING_DIRS")
                    || text.contains("\"dsh/sessions\"");
                if lists && !covers && !ALLOWED.iter().any(|(file, _)| relative == *file) {
                    missing.push(relative);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "binding scanners without the ID layout: {missing:?}"
        );
    }

    fn record(name: &str, run: &str) -> Value {
        json!({"version":1,"agent":"claude","name":name,"run_id":run,"conversation_id":format!("conversation-{run}"),
               "created":100,"cwd":format!("/tmp/zerus-p3a/{run}"),"launch_dir":format!("/tmp/zerus-p3a/{run}")})
    }

    /// A session in the ID layout, written as P3e writes one: its index entry
    /// durable first, then its record, then its ID index entry.
    fn by_id(name: &str, run: &str) -> (Value, String) {
        let id = uuid::Uuid::new_v4().to_string();
        let mut written = record(name, run);
        written["session_id"] = json!(id);
        written["layout"] = json!("id");
        add(name, &id).unwrap();
        atomic(
            &storage::id_record_path(&id).unwrap(),
            &format!("{written}\n"),
        )
        .unwrap();
        session_index::note(&written, session_index::Kind::IdRecord);
        (written, id)
    }

    fn sorted(mut names: Vec<String>) -> Vec<String> {
        names.sort();
        names
    }

    #[test]
    #[ignore = "run by the_id_layout_is_read_written_and_retired_in_place"]
    fn id_layout_fixture() {
        if std::env::var_os("ZERUS_ID_LAYOUT_FIXTURE").is_none() {
            return;
        }
        std::fs::create_dir_all(root()).unwrap();
        let (shared, alone) = ("claude/example/shared", "claude/example/alone");
        // The label's first holder, at its own path, and two sessions in the ID
        // layout: one sharing that label, one alone under its own.
        let mut first = record(shared, "run-first");
        storage::write(&mut first).unwrap();
        let first_path = storage::label_record_path(shared);
        let first_bytes = std::fs::read(&first_path).unwrap();
        let (_, twin_id) = by_id(shared, "run-twin");
        let (_, alone_id) = by_id(alone, "run-alone");

        // Reads: verified by path, and a name reaches its label's first holder,
        // or its only session in the ID layout; never another session.
        assert_eq!(storage::read_id(&twin_id).unwrap()["run_id"], "run-twin");
        assert_eq!(read(shared).unwrap()["run_id"], "run-first");
        assert_eq!(read(alone).unwrap()["session_id"], alone_id.as_str());
        let mut names = records()
            .unwrap()
            .iter()
            .map(|r| string(r, "run_id").to_owned())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["run-alone", "run-first", "run-twin"]);
        assert_eq!(session_id::names_of(&twin_id).unwrap(), [shared]);
        std::fs::remove_file(root().join("ids").join(format!("{alone_id}.json"))).unwrap();
        assert_eq!(
            session_id::names_of(&alone_id).unwrap(),
            [alone],
            "by scan, then repaired"
        );
        assert_eq!(
            session_index::find(&alone_id).unwrap().kind,
            session_index::Kind::IdRecord
        );
        // A file in the ID layout that carries another ID, or no layout, is refused.
        let forged = uuid::Uuid::new_v4().to_string();
        atomic(
            &storage::id_record_path(&forged).unwrap(),
            &format!(
                "{}\n",
                json!({"version":1,"agent":"claude",
            "name":alone,"run_id":"run-forged","session_id":alone_id,"layout":"id"})
            ),
        )
        .unwrap();
        assert!(storage::read_id(&forged).is_err());
        std::fs::remove_file(storage::id_record_path(&forged).unwrap()).unwrap();
        let bare = uuid::Uuid::new_v4().to_string();
        atomic(
            &storage::id_record_path(&bare).unwrap(),
            &format!(
                "{}\n",
                json!({"version":1,"agent":"claude",
            "name":"claude/example/bare","run_id":"run-bare","session_id":bare})
            ),
        )
        .unwrap();
        let disagree = "layout and location disagree";
        assert!(storage::read_id(&bare).unwrap_err().contains(disagree), "no layout");
        std::fs::remove_file(storage::id_record_path(&bare).unwrap()).unwrap();
        // A record at a label's path that claims the ID layout is refused too.
        let claimed = "claude/example/claimed";
        atomic(
            &storage::label_record_path(claimed),
            &format!(
                "{}\n",
                json!({"version":1,"agent":"claude","name":claimed,"run_id":"run-claimed",
            "session_id":uuid::Uuid::new_v4().to_string(),"layout":"id"})
            ),
        )
        .unwrap();
        assert!(
            read(claimed).unwrap_err().contains(disagree),
            "the ID layout claimed at a label's path"
        );
        std::fs::remove_file(storage::label_record_path(claimed)).unwrap();

        // Writes stay in place; a record whose layout and file disagree is refused.
        let mut again = storage::read_id(&alone_id).unwrap();
        again["activity"] = json!("idle");
        storage::write(&mut again).unwrap();
        assert!(!storage::label_record_path(alone).exists());
        assert_eq!(storage::read_id(&alone_id).unwrap()["activity"], "idle");
        let mut flagless = again.clone();
        flagless.as_object_mut().unwrap().remove("layout");
        assert!(
            storage::write(&mut flagless).is_err(),
            "no layout, yet sessions/<id> exists"
        );
        let mut lost = again.clone();
        lost["session_id"] = json!(uuid::Uuid::new_v4().to_string());
        assert!(
            storage::write(&mut lost).is_err(),
            "layout, yet no sessions/<id>"
        );
        assert!(!storage::label_record_path(alone).exists());

        // Selection and resolution: a selected session in the ID layout is read
        // under its label; a label several sessions share is the ambiguity
        // report; a sole holder in the ID layout is selected.
        session_id::select(shared, &twin_id);
        assert_eq!(read(shared).unwrap()["run_id"], "run-twin");
        session_id::clear();
        let (text, value) = resolve_label(shared).unwrap_err();
        assert!(
            text.starts_with("2 sessions on this computer are named claude/example/shared"),
            "{text}"
        );
        assert_eq!(value.unwrap()["total"], 2);
        resolve_label(alone).unwrap();
        assert_eq!(session_id::selected().unwrap().session_id, alone_id);
        session_id::clear();

        // Hooks find their own record through ids/ without a scan.
        std::env::set_var("HGS_SESSION_ID", &twin_id);
        let scans = session_index::SCANS.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            storage::read_own_run(shared, "run-twin").unwrap().unwrap()["session_id"],
            twin_id.as_str()
        );
        assert_eq!(
            session_index::SCANS.load(std::sync::atomic::Ordering::SeqCst),
            scans
        );
        std::env::remove_var("HGS_SESSION_ID");
        // Another session's run is found by its run, never the first holder.
        assert_eq!(
            storage::read_run(shared, "run-twin").unwrap().unwrap()["session_id"],
            twin_id.as_str()
        );

        // A session's files are keyed by its verified record: the session ID
        // in the ID layout, the label's hash for the first holder.
        assert_eq!(storage::owner_key(&storage::read_id(&alone_id).unwrap()), alone_id);
        assert_eq!(
            storage::owner_key(&read(shared).unwrap()),
            storage::label_key(shared),
            "the first holder keeps its label's hash"
        );

        // Every binding scanner sees the ID layout.
        let folders: Vec<String> = worktree_cleanup::bindings()
            .unwrap()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert!(
            folders.iter().filter(|name| *name == shared).count() == 2,
            "{folders:?}"
        );
        let routed = task_bindings_in(&root()).unwrap();
        assert_eq!(routed.iter().filter(|row| row["name"] == alone).count(), 1);
        let (sources, _) = session_graph::inventory(&root()).unwrap();
        assert!(
            sources
                .iter()
                .any(|(source, _, _)| source == &format!("sessions/{alone_id}.json")),
            "{sources:?}"
        );
        let identity: search_index::Identity = serde_json::from_value(
            json!({"name":alone,"run_id":"run-alone","conversation_id":"conversation-run-alone"}),
        )
        .unwrap();
        assert_eq!(
            search_index::id_layout_path(&root(), &identity, &storage::label_record_path(alone)),
            storage::id_record_path(&alone_id)
        );

        // Uniqueness counts the ID layout, and fails closed when the candidates
        // cannot be proven complete.
        assert!(storage::label_taken(alone) && storage::label_taken(shared));
        assert!(!storage::label_taken("claude/example/free"));
        let index = path(alone);
        let kept = std::fs::read(&index).unwrap();
        std::fs::write(&index, b"not an index").unwrap();
        let unreadable = storage::id_record_path(&alone_id).unwrap();
        let record_bytes = std::fs::read(&unreadable).unwrap();
        std::fs::write(&unreadable, b"{").unwrap();
        assert!(
            candidates(alone).is_err(),
            "a present unreadable candidate is not absence"
        );
        assert!(storage::label_taken(alone));
        std::fs::write(&unreadable, &record_bytes).unwrap();
        std::fs::write(&index, &kept).unwrap();

        // The index stays a superset at every cut of a rename in place:
        // after the new label's entry, after the record, after the old entry.
        let renamed = "claude/example/renamed";
        add(renamed, &twin_id).unwrap();
        assert_eq!(
            candidates(renamed).unwrap().by_id.len(),
            0,
            "an entry before its record is stale"
        );
        assert_eq!(
            candidates(shared).unwrap().by_id.len(),
            1,
            "the old label still holds it"
        );
        remove(renamed, &twin_id).unwrap();
        // The rename itself: only its own record and the index change.
        let routes = root().join("name_routes");
        session_id::select(shared, &twin_id);
        rename::dispatch(&[shared.to_owned(), renamed.to_owned()], false).unwrap();
        session_id::clear();
        assert_eq!(
            std::fs::read(&first_path).unwrap(),
            first_bytes,
            "the first holder is untouched"
        );
        assert!(
            !routes.exists() || std::fs::read_dir(&routes).unwrap().next().is_none(),
            "no route at a label path"
        );
        assert_eq!(storage::read_id(&twin_id).unwrap()["name"], renamed);
        assert_eq!(sorted(listed(renamed).unwrap()), vec![twin_id.clone()]);
        assert!(listed(shared).unwrap().is_empty());
        assert_eq!(read(shared).unwrap()["run_id"], "run-first");

        // Archive in place: the session's own record, its ID index and label
        // index entries go; the first holder stays byte-identical.
        let current = storage::read_id(&twin_id).unwrap();
        archive::save(&current).unwrap();
        assert!(!storage::id_record_path(&twin_id).unwrap().exists());
        assert!(listed(renamed).unwrap().is_empty());
        assert!(session_index::find(&twin_id).is_none());
        assert_eq!(std::fs::read(&first_path).unwrap(), first_bytes);

        // The manifest: a label it does not list has no session in the ID
        // layout. Deleting labels/ by hand removes it; readers then scan, and
        // label resolution rebuilds it when the writer lock is free.
        assert!(manifest().is_some());
        assert_eq!(listed("claude/example/free"), Some(Vec::new()));
        std::fs::remove_dir_all(root().join("labels")).unwrap();
        assert_eq!(listed(alone), None);
        assert_eq!(
            candidates(alone).unwrap().by_id.len(),
            1,
            "the scan finds it"
        );
        repair();
        assert_eq!(listed(alone), Some(vec![alone_id.clone()]));
        // Forget in place, too.
        let current = storage::read_id(&alone_id).unwrap();
        storage::remove_record(&current).unwrap();
        assert!(!storage::id_record_path(&alone_id).unwrap().exists());
        assert_eq!(std::fs::read(&first_path).unwrap(), first_bytes);
    }

    /// #1437 review (desktop-u G1): each control runs in its own child with a
    /// private state root, and a child that outlives the scan's promised
    /// bound by far fails instead of hanging the suite.
    fn control(case: &str) {
        let state = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "state::labels::tests::control_fixture", "--ignored", "--test-threads=1"])
            .env("HGS_STATE_DIR", state.path().join("sessions-root"))
            .env("TMUX_TMPDIR", state.path().join("tmux"))
            .env("ZERUS_ID_LAYOUT_CONTROL", case)
            .env_remove("TMUX")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_millis(3_500);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= until {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("{case}: the bounded scan did not return within 3.5 s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{case}: {stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn a_deleted_label_file_never_selects_the_first_holder() {
        control("deleted");
    }
    #[test]
    fn a_well_formed_but_incomplete_label_file_never_selects_the_first_holder() {
        control("partial");
    }
    #[test]
    fn a_dangling_candidate_is_not_proven_absent() {
        control("dangling");
    }
    #[test]
    fn a_twin_keeps_its_own_recovery_job_and_native_input_lock() {
        control("owned");
    }
    #[test]
    fn a_selected_record_that_vanishes_never_changes_its_route() {
        control("selected");
    }
    #[test]
    fn a_scan_never_blocks_on_a_candidate_fifo() {
        control("fifo");
    }
    /// #1437 review (review-q2): the index stays a superset at every crash
    /// cut of a rename in place, an archive and a forget.
    #[test]
    fn the_label_index_stays_a_superset_at_every_crash_cut() {
        control("cuts");
    }

    /// Every session in the ID layout is a candidate of its label, or that
    /// label's resolution refuses: never its first holder alone.
    fn superset_holds(context: &str) {
        let Ok(entries) = std::fs::read_dir(root().join("sessions")) else {
            return;
        };
        for entry in entries.flatten() {
            let Some(id) = entry
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|id| session_id::canonical(id))
                .map(str::to_owned)
            else {
                continue;
            };
            let Ok(record) = storage::read_id(&id) else {
                continue;
            };
            let label = string(&record, "name");
            if let Ok(found) = candidates(label) {
                assert!(
                    found.by_id.iter().any(|candidate| string(candidate, "session_id") == id),
                    "{context}: {id} is not a candidate of {label}"
                );
            }
        }
    }

    #[test]
    #[ignore = "run by the bounded controls above"]
    fn control_fixture() {
        let Ok(case) = std::env::var("ZERUS_ID_LAYOUT_CONTROL") else {
            return;
        };
        std::fs::create_dir_all(root()).unwrap();
        let label = "claude/example/control";
        let mut first = record(label, "run-first");
        storage::write(&mut first).unwrap();
        let (twin, id) = by_id(label, "run-twin");
        assert!(manifest().is_some_and(|manifest| manifest.contains_key(&key(label))));
        let ambiguous = || {
            let (text, _) = resolve_label(label).unwrap_err();
            assert!(text.starts_with("2 sessions on this computer are named"), "{text}");
        };
        match case.as_str() {
            // Only the label's file goes; the manifest still lists the label,
            // so the reader scans and finds the twin.
            "deleted" => {
                std::fs::remove_file(path(label)).unwrap();
                assert!(manifest().is_some());
                assert_eq!(listed(label), None);
                ambiguous();
            }
            // A well-formed file that no longer matches its manifest entry.
            "partial" => {
                atomic(&path(label), &json!({"version":1,"label":label,"ids":[]}).to_string()).unwrap();
                assert_eq!(listed(label), None);
                ambiguous();
                let other = uuid::Uuid::new_v4().to_string();
                atomic(&path(label), &json!({"version":1,"label":label,"ids":[other]}).to_string()).unwrap();
                assert_eq!(listed(label), None);
                ambiguous();
                // A listed label whose file lists no session is inconsistent,
                // even with a matching entry: no writer makes one.
                let empty = json!({"version":1,"label":label,"ids":[]}).to_string();
                atomic(&path(label), &empty).unwrap();
                let mut proof = manifest().unwrap();
                proof.insert(key(label), digest(empty.as_bytes()));
                write_manifest(&proof).unwrap();
                assert_eq!(listed(label), None);
                ambiguous();
            }
            // A link there, even a dangling one, is present and unreadable.
            "dangling" => {
                let own = storage::id_record_path(&id).unwrap();
                std::fs::remove_file(&own).unwrap();
                std::os::unix::fs::symlink(root().join("missing-target"), &own).unwrap();
                assert!(resolve_label(label).is_err());
                assert!(candidates(label).is_err());
                assert!(storage::label_taken(label));
            }
            // With no selection, a watch or worker that holds the twin's record
            // reaches its ID-keyed job and its own native-input lock.
            "owned" => {
                let job = json!({"id":"twin-job","state":"waiting","name":label,
                    "identity":[twin["run_id"], twin["conversation_id"]]});
                atomic(&root().join("recovery/jobs").join(format!("{id}.json")), &job.to_string()).unwrap();
                let legacy = json!({"id":"first-job","state":"waiting","name":label,
                    "identity":[first["run_id"], first["conversation_id"]]});
                atomic(
                    &root().join("recovery/jobs").join(format!("{}.json", storage::label_key(label))),
                    &legacy.to_string(),
                )
                .unwrap();
                let mut output = json!({"run_id":twin["run_id"],"conversation_id":twin["conversation_id"]});
                recovery::enrich(&twin, &mut output);
                assert_eq!(output["recovery"]["id"], "twin-job");
                let mut output = json!({"run_id":first["run_id"],"conversation_id":first["conversation_id"]});
                recovery::enrich(&read(label).unwrap(), &mut output);
                assert_eq!(output["recovery"]["id"], "first-job");
                let held = startup_choice::driving(&twin).unwrap();
                let own = root().join("native-input").join(format!("{id}.lock"));
                assert!(matches!(lock_until(&own, Instant::now()), Ok(None)), "the twin's lock is held");
                let other = root().join("native-input").join(format!("{}.lock", storage::label_key(label)));
                assert!(matches!(lock_until(&other, Instant::now()), Ok(Some(_))), "never the first holder's");
                drop(held);
                // The scheduler's observation carries the twin's own key.
                let observed = recovery::observe().unwrap();
                let seen = observed.iter().find(|s| s["run_id"] == "run-twin").unwrap();
                assert_eq!(storage::owner_key(seen), id);
                // A rename in place keeps the job's key and gives it the label.
                session_id::select(label, &id);
                rename::dispatch(&[label.to_owned(), "claude/example/renamed".to_owned()], false).unwrap();
                session_id::clear();
                let moved: Value = serde_json::from_slice(
                    &std::fs::read(root().join("recovery/jobs").join(format!("{id}.json"))).unwrap(),
                )
                .unwrap();
                assert_eq!((moved["id"].as_str(), moved["name"].as_str()), (Some("twin-job"), Some("claude/example/renamed")));
            }
            // A selection verified in the ID layout keeps its fixed route when
            // the file vanishes: the read refuses, never the first holder.
            "selected" => {
                session_id::select(label, &id);
                let own = storage::id_record_path(&id).unwrap();
                std::fs::remove_file(&own).unwrap();
                assert_eq!(storage::record_path(label), own);
                assert!(read(label).is_err());
                session_id::clear();
                assert_eq!(read(label).unwrap()["run_id"], "run-first");
            }
            // A FIFO where a record should be: the scan refuses at once.
            "fifo" => {
                std::fs::remove_file(path(label)).unwrap();
                std::fs::remove_file(manifest_path()).unwrap();
                let own = storage::id_record_path(&id).unwrap();
                std::fs::remove_file(&own).unwrap();
                use std::os::unix::ffi::OsStrExt;
                let c = std::ffi::CString::new(own.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
                let started = Instant::now();
                assert!(candidates(label).is_err(), "an unsafe candidate closes the scan");
                assert!(storage::label_taken(label));
                assert!(started.elapsed() < SCAN_TIME);
            }
            // Stop after each write of the operation, as if it crashed there.
            "cuts" => {
                let base = root();
                for op in ["rename", "archive", "forget"] {
                    let mut writes = 0;
                    loop {
                        std::env::set_var("HGS_STATE_DIR", base.join(format!("{op}-{writes}")));
                        std::fs::create_dir_all(root()).unwrap();
                        let mut first = record(label, "run-first");
                        storage::write(&mut first).unwrap();
                        let (_, id) = by_id(label, "run-twin");
                        session_id::select(label, &id);
                        storage::cut::arm(writes);
                        let result = match op {
                            "rename" => rename::dispatch(
                                &[label.to_owned(), "claude/example/moved".to_owned()],
                                false,
                            )
                            .map(drop),
                            "archive" => archive::save(&storage::read_id(&id).unwrap()).map(drop),
                            _ => storage::remove_record(&storage::read_id(&id).unwrap()),
                        };
                        storage::cut::disarm();
                        session_id::clear();
                        superset_holds(&format!("{op} cut after {writes} writes"));
                        if result.is_ok() {
                            break;
                        }
                        writes += 1;
                        assert!(writes < 64, "{op} never completed");
                    }
                    assert!(writes >= 2, "{op} completed after only {writes} writes");
                }
            }
            other => panic!("unknown control {other}"),
        }
    }
}
