//! Optional bounded reads beside the unchanged legacy `task get`: one
//! chronological page of a task's history. Pages come from the task index,
//! which keeps the histories of the most recently read tasks, or from the
//! task's document when the index cannot answer; both give the same page.
//! Cursors bind the project, task and immutable history entry.
use super::*;
use sha2::{Digest, Sha256};

const MAX_PAGE: usize = 80;

pub(super) fn limit(args: &[String]) -> Result<Option<usize>> {
    let limit = option(args, "--history-limit")?
        .map(|s| {
            s.parse::<usize>()
                .ok()
                .filter(|n| (1..=MAX_PAGE).contains(n))
                .ok_or_else(|| fail("history limit must be between 1 and 80"))
        })
        .transpose()?;
    if limit.is_none() && option(args, "--history-before")?.is_some() {
        return Err(fail("--history-before requires --history-limit"));
    }
    if limit.is_none() && option(args, "--attachments")?.is_some() {
        return Err(fail("--attachments requires --history-limit"));
    }
    Ok(limit)
}

/// `--attachments metadata`: a page names each file (its name, type,
/// reference, size and SHA-256) instead of carrying it, and names each entry;
/// `hgs task attachment` returns one file.
pub(super) fn summary(args: &[String]) -> Result<bool> {
    match option(args, "--attachments")? {
        None => Ok(false),
        Some("metadata") => Ok(true),
        Some(_) => Err(fail("--attachments takes metadata")),
    }
}

/// One history entry, in full and as a summary.
pub(super) struct Entry {
    pub id: String,
    pub at: i64,
    pub payload: Value,
    pub summary: Value,
}

/// A task's history as pages are built from it: the task without its log,
/// in full and as a summary, and its entries in their canonical order.
pub(super) struct Read {
    pub header: Value,
    pub summary: Value,
    pub entries: Vec<Entry>,
}

/// `value` with each attachment's metadata instead of its file.
fn summarized(value: &Value) -> Result<Value> {
    let mut out = value.clone();
    if let Some(files) = value.get("attachments") {
        let files: Vec<TaskAttachment> = serde_json::from_value(files.clone()).map_err(fail)?;
        let described = files
            .iter()
            .map(TaskAttachment::metadata)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(fail)?;
        out["attachments"] = json!(described);
    }
    Ok(out)
}

/// Reads a task's history from its document.
pub(super) fn materialize(task: &TaskDoc) -> Result<Read> {
    let (mut view, ids) = task.view_with_history_ids().map_err(fail)?;
    let dispatch = view.execution_id();
    let mut started = false;
    for entry in &view.log {
        if entry.kind == "sent" {
            started = false;
        } else if entry.kind == "started" {
            started = true;
        }
    }
    let log = std::mem::take(&mut view.log);
    let mut header = serde_json::to_value(view).map_err(fail)?;
    header["dispatch_id"] = json!(dispatch);
    header["start_confirmed"] = json!(started);
    header.as_object_mut().expect("task").remove("log");
    let summary = summarized(&header)?;
    let phases = task.attempt_phases().map_err(fail)?;
    let entries = ids
        .into_iter()
        .zip(log)
        .map(|(id, entry)| {
            let mut payload = serde_json::to_value(&entry).map_err(fail)?;
            // An entry whose request names an attempt carries that attempt's
            // own origin and phase (A4): a reopen's note is a change request
            // whose delivery its attempt alone shows. Additive; an entry
            // without an attempt is unchanged.
            if let Some((origin, phase)) = entry.request_id.as_deref().and_then(|r| phases.get(r)) {
                payload["attempt"] = json!({"origin": origin, "phase": phase});
            }
            Ok(Entry {
                id,
                at: entry.at,
                summary: summarized(&payload)?,
                payload,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Read { header, summary, entries })
}

/// The page's reply: the task with one page of its log.
pub(super) fn finish(mut task: Value, entries: Vec<Value>, total: usize, next: Option<String>) -> Value {
    task["log"] = json!(entries);
    task["log_total"] = json!(total);
    task["history_next"] = json!(next);
    task["history_schema"] = json!(1);
    task
}

pub(super) fn scope(project: &str, uuid: &str) -> String {
    format!("{:x}:", Sha256::digest(format!("{project}\n{uuid}")))
}

/// The entry ID a cursor or entry reference names, for this task only.
fn entry_id<'a>(prefix: &str, cursor: &'a str) -> Result<&'a str> {
    cursor
        .strip_prefix(prefix)
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or_else(|| fail("task history cursor does not match this task"))
}

fn gone() -> Error {
    fail("History changed; return to Latest to refresh.")
}

pub(super) fn page(store: &ProjectStore, uuid: &str, limit: usize, before: Option<&str>, summary: bool) -> Result<Value> {
    if !zerus_replica::ids::is_uuid(uuid) || !(1..=MAX_PAGE).contains(&limit) {
        return Err(fail("invalid task history request"));
    }
    let prefix = scope(store.project_id(), uuid);
    let before = before.map(|cursor| entry_id(&prefix, cursor)).transpose()?;
    if let Some(mut index) = index::Index::open(store) {
        match index.history(uuid, limit, before, summary, &prefix) {
            Ok(Some(page)) => return Ok(page),
            Ok(None) => return Err(gone()),
            Err(failure) => index.discard(&failure),
        }
    }
    // The same page from the document, when the index cannot answer.
    let read = materialize(&store.load(uuid).map_err(fail)?)?;
    let end = match before {
        Some(id) => read.entries.iter().position(|e| e.id == id).ok_or_else(gone)?,
        None => read.entries.len(),
    };
    let start = end.saturating_sub(limit);
    let next = (start > 0).then(|| format!("{prefix}{}", read.entries[start].id));
    let entries = read.entries[start..end]
        .iter()
        .map(|entry| {
            if summary {
                let mut shown = entry.summary.clone();
                shown["id"] = json!(format!("{prefix}{}", entry.id));
                shown
            } else {
                entry.payload.clone()
            }
        })
        .collect();
    let total = read.entries.len();
    Ok(finish(if summary { read.summary } else { read.header }, entries, total, next))
}

/// `hgs task attachment REF --file N [--entry ID]`: one file of a task, or of
/// one of its history entries (an `id` from a summary page), with its bytes.
pub(super) fn attachment(config: &Config, args: &[String]) -> Result<Value> {
    let machines = machines(config)?;
    let who = actor(&machines.here, args, false)?;
    let r = reference(config, args, &who)?;
    let number = option(args, "--file")?
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .ok_or_else(|| fail("--file is a number from 1"))?;
    let entry = option(args, "--entry")?;
    let store = store(config, &r.project.0)?;
    let task = find(&store, &r.task)?;
    let (view, ids) = task.view_with_history_ids().map_err(fail)?;
    let files = match entry {
        None => &view.attachments,
        Some(cursor) => {
            let id = entry_id(&scope(store.project_id(), task.uuid()), cursor)?;
            let at = ids.iter().position(|known| known == id).ok_or_else(gone)?;
            &view.log[at].attachments
        }
    };
    let file = files.get(number - 1).ok_or_else(|| fail(format!("there is no file {number}")))?;
    let bytes = file.bytes().map_err(fail)?;
    Ok(json!({
        "project": {"id": r.project.0, "name": r.project.1},
        "task": task.uuid(),
        "entry": entry,
        "file": number,
        "name": file.name,
        "mime": file.mime,
        "reference": file.reference,
        "size": bytes.len(),
        "sha256": format!("{:x}", Sha256::digest(&bytes)),
        "data_base64": file.data_base64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use zerus_replica::{NewTask, SessionKey};

    fn task() -> TaskDoc {
        TaskDoc::create(NewTask {
            project: "example",
            id: "T-abcdef",
            text: "Description",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "U",
            now: 100,
        })
        .unwrap()
    }

    fn file(name: &str, text: &str) -> TaskAttachment {
        use base64::Engine;
        TaskAttachment {
            name: name.into(),
            mime: "text/plain".into(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(text),
            reference: String::new(),
        }
    }

    /// Every page of `uuid`, oldest first.
    fn all(store: &ProjectStore, uuid: &str, limit: usize, summary: bool) -> Vec<Value> {
        let mut collected = Vec::new();
        let mut before = None;
        loop {
            let p = page(store, uuid, limit, before.as_deref(), summary).unwrap();
            assert!(p["log"].as_array().unwrap().len() <= limit);
            assert_eq!(p["history_schema"], json!(1));
            let mut older = p["log"].as_array().unwrap().clone();
            older.extend(collected);
            collected = older;
            before = p["history_next"].as_str().map(str::to_owned);
            if before.is_none() {
                return collected;
            }
        }
    }

    #[test]
    fn pages_preserve_canonical_order() {
        let temp = crate::host::staging::private_tempdir();
        let root = temp.path().join("replica");
        crate::platform::owner_only_dir(&root).unwrap();
        let store = ProjectStore::open(&root, "example").unwrap();
        let mut task = task();
        // Equal times retain the canonical Loro order; merged earlier times
        // need chronological order rather than raw append positions.
        for (at, text) in [(300, "first"), (300, "second"), (200, "earlier")] {
            task.comment(text, &Actor::Person, at).unwrap();
        }
        store.save(&mut task).unwrap();
        let expected = serde_json::to_value(task.view().unwrap().log).unwrap();
        assert_eq!(json!(all(&store, task.uuid(), 2, false)), expected);
    }

    #[test]
    fn cursor_survives_append_compaction_and_eviction() {
        let temp = crate::host::staging::private_tempdir();
        let root = temp.path().join("replica");
        crate::platform::owner_only_dir(&root).unwrap();
        let store = ProjectStore::open(&root, "example").unwrap();
        let mut a = task();
        for n in 1..=6 {
            a.comment(&format!("note {n}"), &Actor::Person, 100 + n).unwrap();
        }
        store.save(&mut a).unwrap();
        let latest = page(&store, a.uuid(), 2, None, false).unwrap();
        let cursor = latest["history_next"].as_str().unwrap().to_owned();
        let old = page(&store, a.uuid(), 2, Some(&cursor), false).unwrap();
        a.comment("new", &Actor::Person, 200).unwrap();
        store.save(&mut a).unwrap();
        assert_eq!(page(&store, a.uuid(), 2, Some(&cursor), false).unwrap()["log"], old["log"]);
        let executor = SessionKey {
            machine: "example-computer".into(),
            session: "codex/example/worker".into(),
            session_id: "run_id:example".into(),
        };
        a.record_send(&executor, "dispatch", &Actor::Person, 201).unwrap();
        a.record_report("Result", &Actor::Agent(executor), 202).unwrap();
        a.set_stage(Stage::Done, &Actor::Person, 203).unwrap();
        store.save(&mut a).unwrap(); // Done folds the updates into the snapshot.
        assert_eq!(page(&store, a.uuid(), 2, Some(&cursor), false).unwrap()["log"], old["log"]);
        // Reading sixteen other tasks evicts its history, not the cursor.
        for _ in 0..16 {
            let mut other = task();
            store.save(&mut other).unwrap();
            page(&store, other.uuid(), 1, None, false).unwrap();
            assert!(page(&store, other.uuid(), 2, Some(&cursor), false).is_err());
        }
        index::MATERIALIZED.with(|n| n.set(0));
        assert_eq!(page(&store, a.uuid(), 2, Some(&cursor), false).unwrap()["log"], old["log"]);
        assert_eq!(index::MATERIALIZED.with(Cell::get), 1, "read again after eviction");
    }

    /// The inspector's page and a write guard's one-entry read alternate
    /// between tasks without reading either document again.
    #[test]
    fn alternating_tasks_keep_their_histories() {
        let temp = crate::host::staging::private_tempdir();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let (mut a, mut b) = (task(), task());
        store.save(&mut a).unwrap();
        store.save(&mut b).unwrap();
        index::MATERIALIZED.with(|n| n.set(0));
        for _ in 0..3 {
            page(&store, a.uuid(), 80, None, false).unwrap();
            page(&store, b.uuid(), 1, None, false).unwrap();
        }
        assert_eq!(index::MATERIALIZED.with(Cell::get), 2);
        // A change reads that task again, once.
        a.comment("note", &Actor::Person, 300).unwrap();
        store.save(&mut a).unwrap();
        page(&store, a.uuid(), 80, None, false).unwrap();
        page(&store, a.uuid(), 80, None, false).unwrap();
        page(&store, b.uuid(), 1, None, false).unwrap();
        assert_eq!(index::MATERIALIZED.with(Cell::get), 3);
    }

    #[cfg(windows)]
    #[test]
    fn a_new_store_and_history_index_inherit_only_the_owner() {
        use crate::platform;
        use std::fs;
        let temp = crate::host::staging::private_tempdir();
        let state = temp.path().join("state");
        fs::create_dir(&state).unwrap();
        assert!(std::process::Command::new("icacls").arg(&state)
            .args(["/grant", "*S-1-5-11:(OI)(CI)M", "/Q"])
            .stdout(std::process::Stdio::null()).status().unwrap().success());
        let config = Config {
            home: temp.path().to_string_lossy().into_owned(),
            dir: temp.path().join("config"), state,
            selves: vec![], peers: vec![], base_peers: vec![],
            peers_from_environment: false, machines: Default::default(),
            tab: false, tab_colors: String::new(),
        };
        let store = super::super::store(&config, "example").unwrap();
        let mut task = task();
        store.save(&mut task).unwrap();
        let dir = store_dir(&config, "example");
        for _ in 0..2 {
            page(&store, task.uuid(), 2, None, false).unwrap();
        }
        assert!(platform::verify_private(&dir, platform::Private::Directory).unwrap());
        assert!(platform::verify_private(&dir.join("task-index-v2.sqlite3"), platform::Private::File).unwrap());
        assert!(!platform::verify_private(&config.state, platform::Private::Directory).unwrap(),
            "the existing parent keeps its foreign grant");
    }

    #[test]
    fn summary_pages_name_files_and_entries() {
        let temp = crate::host::staging::private_tempdir();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let mut a = task();
        a.set_attachments(&[file("spec.txt", "the spec")], &Actor::Person, 101).unwrap();
        a.comment_with_attachments("see", &[file("log.txt", "a log")], &Actor::Person, 102).unwrap();
        store.save(&mut a).unwrap();
        let full = page(&store, a.uuid(), 80, None, false).unwrap();
        let summary = page(&store, a.uuid(), 80, None, true).unwrap();
        let sha = |text: &str| format!("{:x}", Sha256::digest(text.as_bytes()));
        assert_eq!(summary["attachments"], json!([{"name":"spec.txt","mime":"text/plain","reference":"","size":8,"sha256":sha("the spec")}]));
        assert!(full["attachments"][0]["data_base64"].is_string());
        let log = summary["log"].as_array().unwrap();
        let commented = log.iter().find(|e| e["text"] == json!("see")).unwrap();
        assert_eq!(commented["attachments"], json!([{"name":"log.txt","mime":"text/plain","reference":"","size":5,"sha256":sha("a log")}]));
        assert!(!summary.to_string().contains("data_base64"));
        // Each summary entry names itself as a cursor would; full pages do not.
        let prefix = scope("example", a.uuid());
        assert!(log.iter().all(|e| e["id"].as_str().is_some_and(|id| id.starts_with(&prefix))));
        assert!(full["log"].as_array().unwrap().iter().all(|e| e.get("id").is_none()));
        // Apart from files and IDs the pages agree.
        let strip = |mut v: Value| {
            v.as_object_mut().unwrap().remove("attachments");
            v.as_object_mut().unwrap().remove("id");
            v
        };
        let entries = |p: &Value| p["log"].as_array().unwrap().iter().cloned().map(strip).collect::<Vec<_>>();
        assert_eq!(entries(&full), entries(&summary));
        assert_eq!(strip(full.clone()).get("log_total"), strip(summary.clone()).get("log_total"));
    }

    /// A read never fails because of the index: one that cannot be used
    /// gives the same pages from the document.
    #[cfg(unix)]
    #[test]
    fn the_document_gives_the_same_pages_as_the_index() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let temp = crate::host::staging::private_tempdir();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let mut a = task();
        a.set_attachments(&[file("spec.txt", "the spec")], &Actor::Person, 101).unwrap();
        for n in 1..=7 {
            a.comment(&format!("note {n}"), &Actor::Person, 100 + n).unwrap();
        }
        store.save(&mut a).unwrap();
        let indexed: Vec<_> = [false, true].iter().map(|s| all(&store, a.uuid(), 3, *s)).collect();
        let latest = page(&store, a.uuid(), 3, None, true).unwrap();
        // The folder is not private: the index cannot be used at all.
        fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o755)).unwrap();
        let read: Vec<_> = [false, true].iter().map(|s| all(&store, a.uuid(), 3, *s)).collect();
        assert_eq!(read, indexed);
        assert_eq!(page(&store, a.uuid(), 3, None, true).unwrap(), latest);
        fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(page(&store, a.uuid(), 3, None, true).unwrap(), latest);
    }

    /// An entry whose request names an attempt carries that attempt's own
    /// origin and phase (A4), from the index and from the document alike: a
    /// reopen's note is a change request whose delivery only its attempt
    /// shows. A settled receipt changes the task, so the cached page follows.
    #[cfg(unix)]
    #[test]
    fn an_entry_naming_an_attempt_carries_its_own_phase() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use zerus_replica::execution::{NativeRequest, Origin, Receipt};
        use zerus_replica::{Settlement, TaskDelivery};
        const SEND: &str = "11111111-1111-4111-8111-111111111111";
        const REOPEN: &str = "22222222-2222-4222-8222-222222222222";
        let temp = crate::host::staging::private_tempdir();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let to = SessionKey {
            machine: "m1".into(),
            session: "codex/api".into(),
            session_id: "launch_id:api".into(),
        };
        let native = || NativeRequest {
            machine: "m1".into(),
            name: "codex/api".into(),
            run_id: "run-api".into(),
            conversation_id: "conversation-api".into(),
            digest: "ab".repeat(32),
        };
        let receipt = |id: &str| Receipt {
            request_id: id.into(),
            name: "codex/api".into(),
            run_id: "run-api".into(),
            conversation_id: "conversation-api".into(),
            first_message: false,
            digest: "ab".repeat(32),
            submitted: true,
        };
        let mut a = task();
        a.comment("plain note", &Actor::Person, 101).unwrap();
        a.reserve_attempt(SEND, Origin::Send, &to, native(), &Actor::Person, 102)
            .unwrap();
        a.settle_attempt(
            SEND,
            Settlement::Receipt(&receipt(SEND)),
            &Actor::Person,
            103,
        )
        .unwrap();
        a.report_attempt(SEND, "Done", zerus_replica::execution::ReportKind::Ready, None, &Actor::Agent(to.clone()), 104)
            .unwrap();
        let view = a.view().unwrap();
        assert_eq!(view.stage, Stage::Review);
        let note = TaskDelivery {
            mode: "note".into(),
            state: "note".into(),
            stage: "review".into(),
            stage_at: view.stage_at,
            dispatch_id: SEND.into(),
            submitted_id: String::new(),
            run_id: String::new(),
            conversation_id: String::new(),
            prompt: String::new(),
            error: String::new(),
        };
        a.reserve_message(
            REOPEN,
            "Please change it",
            &[],
            &note,
            None,
            (&Actor::Person, 105),
        )
        .unwrap();
        a.reserve_attempt(REOPEN, Origin::Reopen, &to, native(), &Actor::Person, 105)
            .unwrap();
        store.save(&mut a).unwrap();
        let shown = |pages: &[Value], request: &str| {
            let named: Vec<_> = pages
                .iter()
                .filter(|e| e["request_id"] == request)
                .collect();
            assert!(!named.is_empty(), "{request}: {pages:#?}");
            named
                .iter()
                .map(|e| e.get("attempt").cloned())
                .collect::<Vec<_>>()
        };
        let reopen = |phase: &str| vec![Some(json!({"origin": "reopen", "phase": phase}))];
        for summary in [false, true] {
            let pages = all(&store, a.uuid(), 3, summary);
            assert_eq!(shown(&pages, REOPEN), reopen("unknown"));
            assert!(shown(&pages, SEND)
                .iter()
                .all(|a| *a == Some(json!({"origin": "send", "phase": "ready"}))));
            let plain: Vec<_> = pages.iter().filter(|e| e["text"] == "plain note").collect();
            assert!(plain.len() == 1 && plain[0].get("attempt").is_none());
        }
        // Its own receipt settles it; the cached history is read again.
        a.settle_attempt(
            REOPEN,
            Settlement::Receipt(&receipt(REOPEN)),
            &Actor::Person,
            106,
        )
        .unwrap();
        store.save(&mut a).unwrap();
        let indexed = all(&store, a.uuid(), 3, false);
        assert_eq!(shown(&indexed, REOPEN)[0], reopen("submitted")[0]);
        // The document gives the same pages.
        fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(all(&store, a.uuid(), 3, false), indexed);
        fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    }

    /// A cached history of another shape is never served: the page comes
    /// from the document instead.
    #[test]
    fn a_cached_history_of_another_shape_is_never_served() {
        let temp = crate::host::staging::private_tempdir();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let mut a = task();
        a.comment("note", &Actor::Person, 101).unwrap();
        store.save(&mut a).unwrap();
        let good = page(&store, a.uuid(), 80, None, false).unwrap();
        let path = store.dir().join("task-index-v2.sqlite3");
        let header = |field: &str, value: Value| {
            let mut h = good.clone();
            for key in ["log", "log_total", "history_next", "history_schema", "ref"] {
                h.as_object_mut().unwrap().remove(key);
            }
            h[field] = value;
            h.to_string()
        };
        let entry = |value: Value| {
            let mut e = good["log"][0].clone();
            e["at"] = value;
            e.to_string()
        };
        for (table, column, raw) in [
            ("histories", "header", "true".to_owned()),
            ("histories", "header", "[1]".to_owned()),
            ("histories", "header", "{\"uuid\": \"another\"}".to_owned()),
            ("histories", "header", header("id", json!("T-zzzzzz"))),
            ("histories", "header", header("id", json!(7))),
            ("histories", "header", header("uuid", json!(zerus_replica::ids::new_uuid()))),
            ("entries", "payload", "\"text\"".to_owned()),
            ("entries", "payload", "7".to_owned()),
            ("entries", "payload", entry(json!(1))),
            ("entries", "payload", entry(json!("later"))),
        ] {
            page(&store, a.uuid(), 80, None, false).unwrap();
            {
                let db = rusqlite::Connection::open(&path).unwrap();
                db.execute(&format!("UPDATE {table} SET {column}=?1"), [&raw]).unwrap();
            }
            assert_eq!(page(&store, a.uuid(), 80, None, false).unwrap(), good, "{table}.{column} = {raw}");
        }
    }

    #[test]
    fn invalid_limits_cursors_and_modes_are_refused() {
        for raw in ["0", "81", "-1", "garbage"] {
            assert!(limit(&["--history-limit".into(), raw.into()]).is_err());
        }
        assert_eq!(limit(&[]).unwrap(), None);
        assert_eq!(limit(&["--history-limit".into(), "80".into()]).unwrap(), Some(80));
        assert!(limit(&["--history-before".into(), "cursor".into()]).is_err());
        assert!(limit(&["--attachments".into(), "metadata".into()]).is_err());
        assert!(summary(&["--attachments".into(), "files".into()]).is_err());
        assert!(summary(&["--attachments".into(), "metadata".into()]).unwrap());
        let temp = crate::host::staging::private_tempdir();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let (mut a, mut b) = (task(), task());
        a.comment("note", &Actor::Person, 101).unwrap();
        a.comment("note", &Actor::Person, 102).unwrap();
        store.save(&mut a).unwrap();
        store.save(&mut b).unwrap();
        let cursor = page(&store, a.uuid(), 1, None, false).unwrap()["history_next"].as_str().unwrap().to_owned();
        let refused = page(&store, b.uuid(), 1, Some(&cursor), false).unwrap_err().message;
        assert!(refused.contains("cursor does not match this task"), "{refused}");
        let gone = format!("{}{}", scope("example", a.uuid()), "999@1");
        let refused = page(&store, a.uuid(), 1, Some(&gone), false).unwrap_err().message;
        assert!(refused.contains("History changed"), "{refused}");
    }
}
