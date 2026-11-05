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

/// The scan that stands in for an unreadable index reads at most this many
/// records, within [`SCAN_TIME`].
const SCAN_RECORDS: usize = 2_000;
const SCAN_TIME: Duration = Duration::from_secs(2);
/// Index files are small; anything larger is not one of ours.
const INDEX_BYTES: u64 = 1024 * 1024;
/// The completeness marker, inside `labels/` so a hand deletion of the index
/// removes it too (#1430 review). While it is there, a label without an index
/// file has no session in the ID layout; without it, that file is incomplete.
const MARKER: &str = ".complete";

fn marker() -> PathBuf {
    root().join("labels").join(MARKER)
}

fn path(label: &str) -> PathBuf {
    root()
        .join("labels")
        .join(format!("{:x}.json", Sha256::digest(label.as_bytes())))
}

/// The IDs listed under `label`; None when that cannot be proven. A missing
/// file is empty only while the completeness marker is there; otherwise it
/// may have been deleted by hand, and the caller scans instead.
fn listed(label: &str) -> Option<Vec<String>> {
    let path = path(label);
    let file =
        match crate::platform::never_block(std::fs::OpenOptions::new().read(true)).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return marker().is_file().then(Vec::new);
            }
            Err(_) => return None,
        };
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    use std::io::Read;
    file.take(INDEX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > INDEX_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    if value["version"] != 1 || string(&value, "label") != label {
        return None;
    }
    value["ids"]
        .as_array()?
        .iter()
        .map(|id| {
            id.as_str()
                .filter(|id| session_id::canonical(id))
                .map(str::to_owned)
        })
        .collect()
}

fn save(label: &str, ids: &[String]) -> Result<()> {
    let path = path(label);
    if ids.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        };
    }
    atomic(
        &path,
        &format!("{}\n", json!({"version": 1, "label": label, "ids": ids})),
    )
}

/// Lists `id` under `label`, durably, before its record takes the label.
/// Under the state writer lock. An index that cannot be read is rebuilt from
/// a scan first; one that still cannot be is an error, so the record write
/// that depends on it refuses.
pub(super) fn add(label: &str, id: &str) -> Result<()> {
    let mut ids = match listed(label) {
        Some(ids) => ids,
        None => scan(label)?,
    };
    if !ids.iter().any(|known| known == id) {
        ids.push(id.to_owned());
        ids.sort();
    }
    save(label, &ids)?;
    mark_complete();
    Ok(())
}

/// Writes the completeness marker once the index is known complete: every
/// session in `sessions/` is listed under its label, an unlisted one being
/// added first. Under the state writer lock. A scan beyond its bounds, or any
/// record that cannot be read, leaves the marker out, so readers keep
/// scanning or refusing. Best effort: the index stays correct without it.
pub(super) fn mark_complete() {
    if marker().is_file() {
        return;
    }
    let complete = (|| -> Result<bool> {
        let dir = root().join("sessions");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error.to_string()),
        };
        let deadline = Instant::now() + SCAN_TIME;
        for (count, entry) in entries.enumerate() {
            if count >= SCAN_RECORDS || Instant::now() >= deadline {
                return Ok(false);
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
            let record = storage::read_id(&id)?;
            let label = string(&record, "name").to_owned();
            if !holds(&record, &id, &label) {
                continue;
            }
            match std::fs::read(path(&label)) {
                Ok(_) => match listed(&label) {
                    Some(ids) if ids.contains(&id) => {}
                    Some(mut ids) => {
                        ids.push(id);
                        ids.sort();
                        save(&label, &ids)?;
                    }
                    None => return Ok(false),
                },
                Err(error) if error.kind() == io::ErrorKind::NotFound => save(&label, &[id])?,
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(true)
    })();
    if matches!(complete, Ok(true)) {
        let _ = atomic(&marker(), "{\"version\":1}\n");
    }
}

/// Removes `id` from `label`'s entry once its record has left the label.
/// Under the state writer lock. A failure only leaves an extra entry.
pub(super) fn remove(label: &str, id: &str) -> Result<()> {
    let Some(mut ids) = listed(label) else {
        return save(label, &scan(label)?);
    };
    ids.retain(|known| known != id);
    save(label, &ids)?;
    mark_complete();
    Ok(())
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

/// The sessions in the ID layout under `label`, read from `sessions/` within
/// the scan's bounds. An error when the bounds cannot cover it.
fn scan(label: &str) -> Result<Vec<String>> {
    let dir = root().join("sessions");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let deadline = Instant::now() + SCAN_TIME;
    let mut ids = Vec::new();
    for (count, entry) in entries.enumerate() {
        if count >= SCAN_RECORDS || Instant::now() >= deadline {
            return Err(format!(
                "{label}: the sessions that share this label could not all be read; choose one with --session-id"
            ));
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
        if verified(&id, label)?.is_some() {
            ids.push(id);
        }
    }
    ids.sort();
    Ok(ids)
}

/// The record of `id` when it is a live or saved session under `label`; None
/// when it is gone or left the label (a stale entry). A record that is there
/// but cannot be read, or that another session's file replaced, is never
/// taken as absent: the set cannot be proven complete, so it is an error.
fn verified(id: &str, label: &str) -> Result<Option<Value>> {
    let Some(path) = storage::id_record_path(id) else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
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

/// The verified candidates of `label`, read without the lock. An unreadable
/// index falls back to the bounded scan; when that cannot prove the set
/// complete it is an error, never the first holder alone.
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

        // Files owned by name are keyed by the session ID in the ID layout.
        assert_eq!(storage::owner_key(alone), alone_id);
        assert_eq!(
            storage::owner_key(shared).len(),
            64,
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
            search_index::id_layout_path(&root(), &identity),
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

        // The completeness marker: a missing index file is empty only while it
        // is there; deleting labels/ by hand removes it, and readers scan.
        mark_complete();
        assert!(marker().is_file());
        assert_eq!(listed("claude/example/free"), Some(Vec::new()));
        std::fs::remove_dir_all(root().join("labels")).unwrap();
        assert_eq!(listed(alone), None);
        assert_eq!(
            candidates(alone).unwrap().by_id.len(),
            1,
            "the scan finds it"
        );
        // Forget in place, too.
        let current = storage::read_id(&alone_id).unwrap();
        storage::remove_record(&current).unwrap();
        assert!(!storage::id_record_path(&alone_id).unwrap().exists());
        assert_eq!(std::fs::read(&first_path).unwrap(), first_bytes);
    }
}
