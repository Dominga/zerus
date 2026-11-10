//! The draft store on a real disk, in a private scratch folder per test.
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use zerus_client_core::attachments::{Attachment, Draft};
use zerus_client_core::drafts::{
    import_at_start, import_once, import_sources, read_draft, read_index, Cursor,
    DraftStore, ImportConflict, ImportReport, ImportSource, IndexPage, Loaded, Page, StoredDraft,
    Summary, ALREADY_EXISTS, IMPORT_MARKER, INTERRUPTED, NOT_RESTORED, UNCONFIRMED,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "zerus-client-core-drafts-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const KEY: &str = "studio\nclaude/sample-project/review";
/// sha256 of KEY.
fn folder(root: &Path, key: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(key.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    root.join(hex)
}

fn draft(text: &str, files: &[(&str, &[u8])]) -> StoredDraft {
    StoredDraft {
        draft: Draft {
            text: text.into(),
            attachments: files
                .iter()
                .enumerate()
                .map(|(index, (name, bytes))| Attachment {
                    name: (*name).into(),
                    mime: "text/plain".into(),
                    bytes: (*bytes).into(),
                    reference: format!("[File #{}]", index + 1),
                })
                .collect(),
            next_attachment_number: files.len() as u32 + 1,
        },
        position: 3,
        anchor: 1,
        ..StoredDraft::default()
    }
}

fn wait<T: Send + 'static>(start: impl FnOnce(Box<dyn FnOnce(T) + Send>)) -> T {
    let (sender, receiver) = mpsc::channel();
    start(Box::new(move |value| sender.send(value).unwrap()));
    receiver.recv_timeout(Duration::from_secs(10)).unwrap()
}

fn save(store: &DraftStore, key: &str, value: StoredDraft) -> Result<(), String> {
    wait(|done| store.save_durable(key, value, done))
}

fn load(store: &DraftStore, key: &str) -> Loaded {
    wait(|done| store.load(key, done))
}

fn page(store: &DraftStore, limit: usize, before: Option<Cursor>) -> Page {
    wait(|done| store.index(IndexPage { limit, before }, done))
}

fn index(store: &DraftStore) -> std::collections::BTreeMap<String, Summary> {
    page(store, 200, None).items.into_iter().collect()
}

#[test]
fn clearing_a_submitted_snapshot_preserves_edits_on_either_side_of_it() {
    let scratch = Scratch::new("conditional-clear");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let key = "task-comment\nexample\n11111111-1111-4111-8111-111111111111";
    let submitted = draft("Submitted", &[("a.txt", b"original")]);
    save(&store, key, submitted.clone()).unwrap();
    let newer = draft("New context", &[("a.txt", b"original")]);
    save(&store, key, newer.clone()).unwrap();
    assert!(!wait(|done| store.clear_if_unchanged(key, submitted.draft.clone(), done)).unwrap());
    assert_eq!(load(&store, key).draft.draft, newer.draft);
    // An edit queued after the clear must win. Splitting the comparison and
    // clearing into separate writer jobs would put the clear after this edit.
    let (sender, receiver) = mpsc::channel();
    store.clear_if_unchanged(key, newer.draft.clone(), move |r| sender.send(r).unwrap());
    let after = draft("Typed later", &[("b.txt", b"later")]);
    store.save_durable(key, after.clone(), |r| r.unwrap());
    assert!(receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap());
    store.flush();
    assert_eq!(load(&store, key).draft.draft, after.draft);
}

#[test]
fn conditional_clear_checks_attachment_bytes_and_resets_delivery_flags() {
    let scratch = Scratch::new("clear-media");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let mut stored = draft("Same text", &[("a.txt", b"changed")]);
    stored.sending = true;
    stored.uncertain = true;
    save(&store, KEY, stored.clone()).unwrap();
    assert!(!wait(|done| store.clear_if_unchanged(
        KEY,
        draft("Same text", &[("a.txt", b"original")]).draft,
        done
    ))
    .unwrap());
    assert!(wait(|done| store.clear_if_unchanged(KEY, stored.draft, done)).unwrap());
    let empty = load(&store, KEY).draft;
    assert!(empty.draft.text.is_empty() && empty.draft.attachments.is_empty());
    assert!(!empty.sending && !empty.uncertain);
}

#[test]
fn a_draft_round_trips_in_qt_format() {
    let scratch = Scratch::new("round");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let mut value = draft("hello [File #1]", &[("a.txt", b"hello")]);
    value.label = "Release notes".into();
    value.form_state = json!({"title": "Ship it"}).as_object().unwrap().clone();
    save(&store, KEY, value.clone()).unwrap();
    assert_eq!(
        load(&store, KEY),
        Loaded {
            draft: value,
            storage_error: None,
            notice: None
        }
    );
    let dir = folder(store.root(), KEY);
    let manifest: Value =
        serde_json::from_slice(&fs::read(dir.join("draft.json")).unwrap()).unwrap();
    assert_eq!(
        manifest,
        json!({"version": 1, "key": KEY, "text": "hello [File #1]", "position": 3, "anchor": 1,
               "label": "Release notes", "form_state": {"title": "Ship it"}, "next_attachment_number": 2,
               "sending": false, "uncertain": false,
               "attachments": [{"name": "a.txt", "mime": "text/plain", "reference": "[File #1]",
                                "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
                                "bytes": 5}]})
    );
    assert_eq!(
        fs::read(dir.join("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824.bin"))
            .unwrap(),
        b"hello"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(store.root()), 0o700);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("draft.json")), 0o600);
    }
    // A missing draft is empty, without an error.
    assert_eq!(
        load(&store, "studio\nclaude/sample-project/other"),
        Loaded::default()
    );
}

#[test]
fn edits_coalesce_but_never_jump_ahead_of_a_durable_save() {
    let scratch = Scratch::new("order");
    let store = DraftStore::open(scratch.0.join("drafts"));
    // Hold the writer so everything below queues up.
    let (release, held) = mpsc::channel::<()>();
    store.index(
        IndexPage {
            limit: 1,
            before: None,
        },
        move |_| {
            let _ = held.recv();
        },
    );
    let (sender, written) = mpsc::channel();
    for (text, durable) in [
        ("one", false),
        ("two", true),
        ("three", false),
        ("four", false),
    ] {
        let sender = sender.clone();
        let done = move |result: Result<(), String>| sender.send((text, result.is_ok())).unwrap();
        if durable {
            store.save_durable(KEY, draft(text, &[]), done);
        } else {
            store.save(KEY, draft(text, &[]), done);
        }
    }
    release.send(()).unwrap();
    let order: Vec<_> = (0..3)
        .map(|_| written.recv_timeout(Duration::from_secs(10)).unwrap())
        .collect();
    // "one" ran before the durable "two"; "four" replaced "three" behind it.
    assert_eq!(order, [("one", true), ("two", true), ("four", true)]);
    assert_eq!(load(&store, KEY).draft.draft.text, "four");
}

#[test]
fn a_crash_between_blob_and_manifest_keeps_the_last_manifest() {
    let scratch = Scratch::new("crash");
    let store = DraftStore::open(scratch.0.join("drafts"));
    save(&store, KEY, draft("first", &[("a.txt", b"hello")])).unwrap();
    let dir = folder(store.root(), KEY);
    // A blob written for a newer draft whose manifest never landed.
    fs::write(dir.join(format!("{}.bin", "ab".repeat(32))), b"orphan").unwrap();
    let loaded = load(&store, KEY);
    assert_eq!(loaded.draft.draft.text, "first");
    assert_eq!(loaded.draft.draft.attachments.len(), 1);
    // The next save removes what the manifest does not name.
    save(&store, KEY, draft("second", &[])).unwrap();
    let left: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left, ["draft.json"]);
}

#[test]
fn damaged_drafts_restore_what_they_can_and_interrupted_sends_warn() {
    let scratch = Scratch::new("damage");
    let store = DraftStore::open(scratch.0.join("drafts"));
    save(
        &store,
        KEY,
        draft("text", &[("a.txt", b"hello"), ("b.txt", b"world")]),
    )
    .unwrap();
    let dir = folder(store.root(), KEY);
    let blob = dir.join("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824.bin");
    fs::write(&blob, b"jello").unwrap();
    let loaded = load(&store, KEY);
    assert_eq!(loaded.storage_error.as_deref(), Some(NOT_RESTORED));
    assert_eq!(loaded.draft.draft.text, "text");
    let names: Vec<_> = loaded
        .draft
        .draft
        .attachments
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(names, ["b.txt"]);
    assert!(blob.exists(), "files stay on disk");
    // A manifest for another key, or of another version, restores nothing.
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(dir.join("draft.json")).unwrap()).unwrap();
    manifest["version"] = json!(2);
    fs::write(dir.join("draft.json"), manifest.to_string()).unwrap();
    assert_eq!(
        load(&store, KEY),
        Loaded {
            storage_error: Some(NOT_RESTORED.into()),
            ..Loaded::default()
        }
    );
    // A draft saved while sending may have been delivered.
    let mut sending = draft("in flight", &[]);
    sending.sending = true;
    save(&store, KEY, sending).unwrap();
    let loaded = load(&store, KEY);
    assert!(loaded.draft.uncertain);
    assert_eq!(loaded.notice.as_deref(), Some(INTERRUPTED));
}

#[test]
fn saved_drafts_lists_nonempty_drafts_of_every_session() {
    let scratch = Scratch::new("index");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let mut ended = draft("\n  \n  first   line \t here\nsecond", &[]);
    ended.label = "Ended session".into();
    save(&store, "\nclaude/p/ended\narchive-7", ended).unwrap();
    save(&store, KEY, draft("", &[("a.txt", b"hello")])).unwrap();
    save(&store, "studio\nclaude/p/empty", draft("   ", &[])).unwrap();
    save(
        &store,
        "studio\nclaude/p/long",
        draft(&"x".repeat(300), &[]),
    )
    .unwrap();
    // A manifest in a folder not named after its key is ignored.
    let stray = store.root().join("not-a-digest");
    fs::create_dir_all(&stray).unwrap();
    fs::copy(
        folder(store.root(), KEY).join("draft.json"),
        stray.join("draft.json"),
    )
    .unwrap();
    let listed = index(&store);
    assert_eq!(
        listed.keys().cloned().collect::<Vec<_>>(),
        ["\nclaude/p/ended\narchive-7", "studio\nclaude/p/long", KEY]
    );
    let ended = &listed["\nclaude/p/ended\narchive-7"];
    assert_eq!(
        (ended.label.as_str(), ended.preview.as_str()),
        ("Ended session", "first line here")
    );
    assert!(ended.modified > 1_700_000_000.0);
    assert_eq!(listed[KEY].attachments, 1);
    assert_eq!(listed["studio\nclaude/p/long"].preview.chars().count(), 200);
}

#[test]
fn a_rename_never_overwrites_and_retires_the_old_key_after() {
    let scratch = Scratch::new("rename");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let (old, new) = ("studio\nclaude/p/old", "studio\nclaude/p/new");
    save(&store, old, draft("moving", &[])).unwrap();
    save(&store, new, draft("already here", &[])).unwrap();
    assert_eq!(
        wait(|done| store.rename(old, new, draft("moving", &[]), done)),
        Err(ALREADY_EXISTS.to_owned())
    );
    assert_eq!(load(&store, old).draft.draft.text, "moving");
    assert_eq!(load(&store, new).draft.draft.text, "already here");
    save(&store, new, StoredDraft::default()).unwrap();
    assert_eq!(
        wait(|done| store.rename(old, new, draft("moving", &[]), done)),
        Ok(())
    );
    assert_eq!(load(&store, new).draft.draft.text, "moving");
    assert_eq!(load(&store, old).draft, StoredDraft::default());
    assert_eq!(index(&store).keys().cloned().collect::<Vec<_>>(), [new]);
}

#[test]
fn qt_drafts_are_read_and_recovered_without_writing_qt_s_store() {
    let scratch = Scratch::new("qt");
    let qt = scratch.0.join("hgs-tray.conf.drafts");
    {
        // Qt's store, written in the same format.
        let writer = DraftStore::open(qt.clone());
        save(&writer, KEY, draft("from Qt", &[("a.txt", b"hello")])).unwrap();
    }
    let snapshot = |root: &Path| -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut files = Vec::new();
        for dir in fs::read_dir(root).unwrap().flatten() {
            for file in fs::read_dir(dir.path()).unwrap().flatten() {
                let metadata = file.metadata().unwrap();
                files.push((
                    file.path(),
                    fs::read(file.path()).unwrap(),
                    metadata.modified().unwrap(),
                ));
            }
        }
        files.sort();
        files
    };
    let before = snapshot(&qt);
    assert_eq!(read_draft(&qt, KEY).draft.draft.text, "from Qt");
    assert_eq!(
        read_index(
            &qt,
            &IndexPage {
                limit: 10,
                before: None
            }
        )
        .items
        .len(),
        1
    );
    let store = DraftStore::open(scratch.0.join("client"));
    let recovered = wait(|done| store.recover(&qt, KEY, done)).unwrap();
    assert_eq!(recovered.draft.draft.attachments.len(), 1);
    assert_eq!(load(&store, KEY).draft.draft.text, "from Qt");
    // A draft of the client's own is never replaced.
    save(&store, KEY, draft("mine", &[])).unwrap();
    assert_eq!(
        wait(|done| store.recover(&qt, KEY, done)),
        Err(ALREADY_EXISTS.to_owned())
    );
    assert_eq!(load(&store, KEY).draft.draft.text, "mine");
    assert_eq!(snapshot(&qt), before, "Qt's store is untouched");
}

#[test]
fn dropping_the_store_writes_everything_queued() {
    let scratch = Scratch::new("drop");
    let root = scratch.0.join("drafts");
    {
        let store = DraftStore::open(root.clone());
        for index in 0..50 {
            store.save(KEY, draft(&format!("edit {index}"), &[]), |_| {});
        }
    }
    let store = DraftStore::open(root);
    assert_eq!(load(&store, KEY).draft.draft.text, "edit 49");
}

#[test]
fn saved_drafts_pages_stay_stable_while_drafts_are_saved() {
    let scratch = Scratch::new("pages");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let keys: Vec<String> = (0..1000)
        .map(|i| format!("studio\nclaude/p/s{i:04}"))
        .collect();
    for key in &keys {
        store.save(key, draft(key, &[]), |result| assert!(result.is_ok()));
    }
    // Empty drafts are skipped and never fill a page.
    for i in 0..50 {
        store.save(
            &format!("studio\nclaude/p/empty{i}"),
            StoredDraft::default(),
            |_| {},
        );
    }
    store.flush();
    let first = page(&store, 200, None);
    assert_eq!(first.items.len(), 200);
    // Newest first, with a stable order for equal times.
    let times: Vec<f64> = first
        .items
        .iter()
        .map(|(_, summary)| summary.modified)
        .collect();
    assert!(times.windows(2).all(|pair| pair[0] >= pair[1]));
    // A draft from a later page is saved while the list is open.
    let seen: std::collections::HashSet<&String> = first.items.iter().map(|(key, _)| key).collect();
    let edited = keys.iter().find(|key| !seen.contains(key)).unwrap().clone();
    std::thread::sleep(Duration::from_millis(20));
    save(&store, &edited, draft("edited while browsing", &[])).unwrap();
    let mut listed: Vec<String> = first.items.iter().map(|(key, _)| key.clone()).collect();
    let mut next = first.next;
    let mut pages = 1;
    while let Some(cursor) = next {
        let more = page(&store, 200, Some(cursor));
        listed.extend(more.items.iter().map(|(key, _)| key.clone()));
        next = more.next;
        pages += 1;
        assert!(pages < 10, "pages end");
    }
    let mut seen_unchanged: Vec<&String> = listed.iter().filter(|key| **key != edited).collect();
    seen_unchanged.sort();
    let mut expected: Vec<&String> = keys.iter().filter(|key| **key != edited).collect();
    expected.sort();
    assert_eq!(
        seen_unchanged, expected,
        "every unchanged draft exactly once"
    );
    // The edited draft moved to the front: it shows on the next first page.
    assert_eq!(page(&store, 1, None).items[0].0, edited);
    // Limits are bounded.
    assert_eq!(page(&store, 5000, None).items.len(), 200);
    assert_eq!(page(&store, 0, None).items.len(), 1);
}

/// Every file under `root` with its bytes and modification time.
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                files.push((path.clone(), fs::read(&path).unwrap(), modified));
            }
        }
    }
    files.sort();
    files
}

fn source(name: &str, root: &Path) -> ImportSource {
    ImportSource {
        name: name.into(),
        root: root.to_path_buf(),
    }
}

/// Saves drafts into a store of the client's (and Qt's) format, then closes it.
fn seed(root: &Path, drafts: &[(&str, StoredDraft)]) {
    let store = DraftStore::open(root.to_path_buf());
    for (key, value) in drafts {
        save(&store, key, value.clone()).unwrap();
        // Distinct modification times order the copies.
        std::thread::sleep(Duration::from_millis(20));
    }
    store.flush();
}

#[test]
fn the_import_copies_every_draft_once_and_never_writes_the_sources() {
    let scratch = Scratch::new("import");
    let (qt, dev, here) = (
        scratch.0.join("qt"),
        scratch.0.join("dev"),
        scratch.0.join("here"),
    );
    let other = "studio\ncodex/sample-project/plan";
    seed(
        &qt,
        &[
            (KEY, draft("from Qt", &[("notes.txt", b"qt bytes")])),
            (other, draft("plan", &[])),
        ],
    );
    seed(&dev, &[(other, draft("plan", &[]))]);
    let before = (snapshot(&qt), snapshot(&dev));
    // A source the client cannot write to still imports.
    #[cfg(unix)]
    for root in [&qt, &dev] {
        use std::os::unix::fs::PermissionsExt;
        for entry in fs::read_dir(root).unwrap().flatten() {
            fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o500)).unwrap();
        }
        fs::set_permissions(root, fs::Permissions::from_mode(0o500)).unwrap();
    }
    let report = import_once(&here, &[source("dev", &dev), source("qt", &qt)]);
    assert_eq!(
        report,
        ImportReport {
            already_done: false,
            imported: 2,
            identical: 1,
            conflicts: vec![],
            invalid: 0,
            sources: vec!["dev".into(), "qt".into()],
            unfinished: vec![],
        }
    );
    #[cfg(unix)]
    for root in [&qt, &dev] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        for entry in fs::read_dir(root).unwrap().flatten() {
            fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    assert_eq!(
        (snapshot(&qt), snapshot(&dev)),
        before,
        "the sources stay untouched"
    );
    let store = DraftStore::open(here.clone());
    let imported = load(&store, KEY);
    assert_eq!(imported.draft.draft.text, "from Qt");
    assert_eq!(&*imported.draft.draft.attachments[0].bytes, b"qt bytes");
    assert_eq!(imported.storage_error, None);
    drop(store);
    // One time: a second run reads and writes nothing.
    assert!(here.join(IMPORT_MARKER).exists());
    let again = import_once(&here, &[source("qt", &qt)]);
    assert!(again.already_done && again.imported == 0);
}

#[test]
fn the_newest_copy_wins_and_a_different_draft_here_is_kept() {
    let scratch = Scratch::new("import-conflict");
    let (qt, dev, here) = (
        scratch.0.join("qt"),
        scratch.0.join("dev"),
        scratch.0.join("here"),
    );
    let kept = "studio\nclaude/sample-project/kept";
    seed(&qt, &[(KEY, draft("older in Qt", &[]))]);
    seed(&dev, &[(KEY, draft("newer in the dev app", &[]))]);
    seed(&here, &[(kept, draft("typed here", &[]))]);
    seed(&qt, &[(kept, draft("a Qt copy", &[]))]);
    let report = import_once(&here, &[source("qt", &qt), source("dev", &dev)]);
    assert_eq!(report.imported, 1);
    let mut conflicts = report.conflicts.clone();
    conflicts.sort();
    let mut expected = vec![
        ImportConflict {
            key: KEY.into(),
            source: "qt".into(),
        },
        ImportConflict {
            key: kept.into(),
            source: "qt".into(),
        },
    ];
    expected.sort();
    assert_eq!(conflicts, expected);
    // The marker keeps the counts and the conflicts' keys and sources, never
    // a draft's text.
    let marker = fs::read_to_string(here.join(IMPORT_MARKER)).unwrap();
    let recorded: Value = serde_json::from_str(&marker).unwrap();
    assert_eq!(recorded["report"]["conflicts"][0]["source"], "qt");
    for text in [
        "older in Qt",
        "newer in the dev app",
        "a Qt copy",
        "typed here",
    ] {
        assert!(!marker.contains(text), "{text} leaked into the marker");
    }
    let store = DraftStore::open(here.clone());
    assert_eq!(load(&store, KEY).draft.draft.text, "newer in the dev app");
    assert_eq!(load(&store, kept).draft.draft.text, "typed here");
    // The losing copies stay recoverable in their source.
    assert_eq!(read_draft(&qt, KEY).draft.draft.text, "older in Qt");
}

#[test]
fn damaged_foreign_and_empty_copies_are_not_imported() {
    let scratch = Scratch::new("import-damaged");
    let (qt, here) = (scratch.0.join("qt"), scratch.0.join("here"));
    let empty = "studio\nclaude/sample-project/empty";
    seed(
        &qt,
        &[
            (KEY, draft("damaged", &[("a.txt", b"bytes")])),
            (empty, draft("", &[])),
        ],
    );
    // A blob that no longer matches its hash, read twice.
    let blob = fs::read_dir(folder(&qt, KEY))
        .unwrap()
        .flatten()
        .find(|entry| entry.path().extension().is_some_and(|ext| ext == "bin"))
        .unwrap()
        .path();
    fs::write(&blob, b"other").unwrap();
    // A folder whose name is not its key's hash.
    let foreign = qt.join("0".repeat(64));
    fs::create_dir_all(&foreign).unwrap();
    fs::write(
        foreign.join("draft.json"),
        json!({"version": 1, "key": "studio\nx", "text": "stray"}).to_string(),
    )
    .unwrap();
    // A folder without a manifest, one whose manifest is not JSON, and a
    // plain file, which is not a draft.
    fs::create_dir_all(qt.join("1".repeat(64))).unwrap();
    let unparsable = qt.join("2".repeat(64));
    fs::create_dir_all(&unparsable).unwrap();
    fs::write(unparsable.join("draft.json"), b"{not json").unwrap();
    fs::write(qt.join("notes.txt"), b"not a draft").unwrap();
    let report = import_once(&here, &[source("qt", &qt)]);
    assert_eq!((report.imported, report.invalid), (0, 4));
    assert!(report.conflicts.is_empty());
    // Damaged copies stay in their source; they never stop the marker.
    assert!(report.unfinished.is_empty());
    assert!(here.join(IMPORT_MARKER).exists());
    let store = DraftStore::open(here.clone());
    assert_eq!(load(&store, KEY).draft, StoredDraft::default());
    assert_eq!(load(&store, "studio\nx").draft, StoredDraft::default());
}

#[test]
fn an_interrupted_send_imports_as_uncertain() {
    let scratch = Scratch::new("import-sending");
    let (qt, here) = (scratch.0.join("qt"), scratch.0.join("here"));
    let mut sending = draft("was being sent", &[]);
    sending.sending = true;
    seed(&qt, &[(KEY, sending)]);
    assert_eq!(import_once(&here, &[source("qt", &qt)]).imported, 1);
    let store = DraftStore::open(here.clone());
    let imported = load(&store, KEY);
    assert!(imported.draft.uncertain);
    assert_eq!(imported.notice.as_deref(), Some(INTERRUPTED));
}

#[test]
fn the_import_runs_on_the_stores_writer() {
    let scratch = Scratch::new("import-writer");
    let (qt, here) = (scratch.0.join("qt"), scratch.0.join("here"));
    seed(&qt, &[(KEY, draft("from Qt", &[]))]);
    let store = DraftStore::open(here.clone());
    let report = wait(|done| store.import(vec![source("qt", &qt)], done));
    assert_eq!(report.imported, 1);
    assert_eq!(load(&store, KEY).draft.draft.text, "from Qt");
}

#[test]
fn the_import_never_reads_the_stores_own_folder() {
    let scratch = Scratch::new("import-own");
    let data = scratch.0.join("data");
    let own = data.join("dev.zerus.next/drafts");
    let (qt, phase2) = (
        scratch.0.join("qt"),
        data.join("dev.zerus.next.phase2/drafts"),
    );
    for root in [&own, &qt, &phase2] {
        fs::create_dir_all(root).unwrap();
    }
    let roots = |sources: Vec<ImportSource>| -> Vec<PathBuf> {
        sources.into_iter().map(|source| source.root).collect()
    };
    // The store's folder, a folder inside it and one holding it are never
    // sources.
    let dev = vec![own.clone(), own.join("old"), data.clone(), phase2.clone()];
    assert_eq!(
        roots(import_sources(&own, Some(qt.clone()), dev)),
        vec![qt.clone(), phase2.clone()]
    );
    // Nor are they under another spelling of the path, even before the
    // store's folder exists.
    #[cfg(unix)]
    {
        let link = scratch.0.join("link");
        std::os::unix::fs::symlink(&data, &link).unwrap();
        let through_link = Some(link.join("dev.zerus.next/drafts"));
        assert!(roots(import_sources(&own, through_link, vec![])).is_empty());
        let later = link.join("later/drafts");
        let inside = vec![data.join("later/drafts/old")];
        assert!(roots(import_sources(&later, None, inside)).is_empty());
    }
}

#[test]
fn startup_import_keeps_next_interim_drafts() {
    const CHILD: &str = "ZERUS_TEST_IMPORT_AT_START";
    const NAME: &str = "startup_import_keeps_next_interim_drafts";
    let other = "studio\nclaude/sample-project/other";
    if let Some(scratch) = std::env::var_os(CHILD).map(PathBuf::from) {
        // In the child, the dev app's store is under the scratch folder,
        // wherever this platform keeps it.
        let data = if cfg!(windows) {
            scratch.join("appdata")
        } else if cfg!(target_os = "macos") {
            scratch.join("home/Library/Application Support")
        } else {
            scratch.join("data")
        };
        let dev = data.join("dev.zerus.next/drafts");
        seed(&dev, &[(other, draft("from the dev app", &[]))]);
        let store = DraftStore::open(scratch.join("here"));
        import_at_start(&store);
        assert_eq!(load(&store, other).draft.draft.text, "from the dev app");
        return;
    }
    let scratch = Scratch::new("import-at-start");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([NAME, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, &scratch.0)
        .env("HOME", scratch.0.join("home"))
        .env("XDG_CONFIG_HOME", scratch.0.join("config"))
        .env("XDG_DATA_HOME", scratch.0.join("data"))
        .env("APPDATA", scratch.0.join("appdata"))
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("1 passed"), "{stdout}");
    assert!(
        stderr.contains("drafts import: 1 imported, 0 identical, 0 conflicts, 0 invalid"),
        "{stderr}"
    );
}

#[test]
fn large_attachments_import_one_draft_at_a_time() {
    // The import holds one copy in memory at a time; this checks that the
    // result stays right with several drafts near the attachment limits.
    let scratch = Scratch::new("import-large");
    let (qt, dev, here) = (
        scratch.0.join("qt"),
        scratch.0.join("dev"),
        scratch.0.join("here"),
    );
    let mib = 1024 * 1024;
    let bytes = |fill: u8| vec![fill; 5 * mib];
    let keys: Vec<String> = (0..4)
        .map(|index| format!("studio\nclaude/sample-project/large-{index}"))
        .collect();
    let qt_drafts: Vec<(&str, StoredDraft)> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let first = bytes(index as u8);
            let second = bytes(index as u8 + 100);
            let value = draft(
                &format!("qt {index}"),
                &[("a.bin", first.as_slice()), ("b.bin", second.as_slice())],
            );
            (key.as_str(), value)
        })
        .collect();
    seed(&qt, &qt_drafts);
    // The dev app holds a newer copy of one draft and the same copy of
    // another.
    let newer = bytes(200);
    seed(
        &dev,
        &[
            (
                keys[0].as_str(),
                draft("dev 0", &[("c.bin", newer.as_slice())]),
            ),
            (keys[1].as_str(), qt_drafts[1].1.clone()),
        ],
    );
    let report = import_once(&here, &[source("qt", &qt), source("dev", &dev)]);
    assert_eq!(
        (report.imported, report.identical, report.invalid),
        (4, 1, 0)
    );
    let mut conflicts = report.conflicts.clone();
    conflicts.sort();
    assert_eq!(
        conflicts,
        vec![ImportConflict {
            key: keys[0].clone(),
            source: "qt".into(),
        }]
    );
    let store = DraftStore::open(here.clone());
    let first = load(&store, &keys[0]);
    assert_eq!(first.draft.draft.text, "dev 0");
    assert_eq!(&*first.draft.draft.attachments[0].bytes, newer.as_slice());
    for (index, key) in keys.iter().enumerate().skip(1) {
        let imported = load(&store, key).draft;
        assert_eq!(imported.draft.text, format!("qt {index}"));
        assert_eq!(
            &*imported.draft.attachments[1].bytes,
            bytes(index as u8 + 100)
        );
    }
}

#[cfg(unix)]
#[test]
fn a_source_that_cannot_be_listed_runs_the_import_again() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("import-unlisted");
    let (qt, dev, here) = (
        scratch.0.join("qt"),
        scratch.0.join("dev"),
        scratch.0.join("here"),
    );
    let other = "studio\nclaude/sample-project/other";
    seed(&qt, &[(KEY, draft("from Qt", &[]))]);
    seed(&dev, &[(other, draft("from the dev app", &[]))]);
    fs::set_permissions(&dev, fs::Permissions::from_mode(0o000)).unwrap();
    let report = import_once(&here, &[source("qt", &qt), source("dev", &dev)]);
    fs::set_permissions(&dev, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(report.imported, 1);
    assert_eq!(report.unfinished.len(), 1);
    assert!(report.unfinished[0].starts_with("dev not listed"));
    assert!(!here.join(IMPORT_MARKER).exists(), "no marker yet");
    // The next start imports the rest; the first draft reads as identical.
    let again = import_once(&here, &[source("qt", &qt), source("dev", &dev)]);
    assert_eq!((again.imported, again.identical), (1, 1));
    assert!(again.unfinished.is_empty());
    assert!(here.join(IMPORT_MARKER).exists());
    let store = DraftStore::open(here.clone());
    assert_eq!(load(&store, other).draft.draft.text, "from the dev app");
}

#[test]
fn a_draft_that_cannot_be_saved_runs_the_import_again() {
    let scratch = Scratch::new("import-unsaved");
    let (qt, here) = (scratch.0.join("qt"), scratch.0.join("here"));
    seed(&qt, &[(KEY, draft("from Qt", &[("a.txt", b"bytes")]))]);
    // A plain file where the draft's folder belongs: the save fails, and
    // the store can still write its marker.
    fs::create_dir_all(&here).unwrap();
    fs::write(folder(&here, KEY), b"in the way").unwrap();
    let report = import_once(&here, &[source("qt", &qt)]);
    assert_eq!((report.imported, report.invalid), (0, 1));
    assert_eq!(report.unfinished, vec!["1 not saved".to_owned()]);
    assert!(!here.join(IMPORT_MARKER).exists(), "no marker yet");
    fs::remove_file(folder(&here, KEY)).unwrap();
    let again = import_once(&here, &[source("qt", &qt)]);
    assert_eq!((again.imported, again.invalid), (1, 0));
    assert!(here.join(IMPORT_MARKER).exists());
    let store = DraftStore::open(here.clone());
    assert_eq!(load(&store, KEY).draft.draft.text, "from Qt");
}

#[test]
fn form_checkpoint_compares_reserved_identity_and_preserves_unknown_fields() {
    let root = Scratch::new("form-checkpoint");
    let store = DraftStore::open(root.0.clone());
    let mut original = draft("Same title", &[]);
    original.form_state.insert(
        "request".into(),
        json!({"id":"original","private":{"kept":true}}),
    );
    wait(|done| store.save_durable(KEY, original.clone(), done)).unwrap();
    let mut newer = original.clone();
    newer.form_state["request"]["id"] = json!("newer");
    wait(|done| store.save_durable(KEY, newer.clone(), done)).unwrap();
    assert!(
        !wait(|done| store.replace_if_unchanged(KEY, original, StoredDraft::default(), done))
            .unwrap()
    );
    let loaded = wait(|done| store.load(KEY, done));
    assert_eq!(loaded.draft, newer);
    let mut settled = newer.clone();
    settled.sending = true;
    wait(|done| store.save_durable(KEY, settled, done)).unwrap();
    let interrupted = wait(|done| store.load(KEY, done)).draft;
    assert!(interrupted.uncertain);
    assert!(
        wait(|done| store.replace_if_unchanged(KEY, interrupted, newer.clone(), done)).unwrap()
    );
    assert_eq!(wait(|done| store.load(KEY, done)).draft, newer);
}

// zerus-5wmh.66.4.4: the deferred-input checkpoint saved with a draft.

use zerus_client_core::drafts::NOT_SAVED_CHECKPOINT;
use zerus_client_core::operation_recovery::deferred_input::{
    message_sha256, Checkpoint, Intent, Kind,
};

fn checkpoint(revision: u64) -> Checkpoint {
    Checkpoint::reserve(Intent {
        kind: Kind::Send,
        draft_revision: revision,
        message_sha256: message_sha256("was being sent", []),
        run_id: "run-1".into(),
        conversation_id: "conv-1".into(),
        question: None,
        start: None,
        retry_of: None,
        consent: false,
    })
}

fn manifest(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(folder(root, KEY).join("draft.json")).unwrap()).unwrap()
}

#[test]
fn a_checkpoint_round_trips_and_only_its_drafts_carry_the_field() {
    let scratch = Scratch::new("checkpoint-round-trip");
    let store = DraftStore::open(scratch.0.clone());
    save(&store, KEY, draft("plain", &[])).unwrap();
    assert!(
        manifest(&scratch.0).get("deferred_input").is_none(),
        "a draft without one keeps its earlier bytes"
    );
    let mut sending = draft("was being sent", &[]);
    sending.sending = true;
    sending.checkpoint = Some(Box::new(checkpoint(2)));
    save(&store, KEY, sending.clone()).unwrap();
    assert_eq!(
        manifest(&scratch.0)["deferred_input"]["request_id"],
        json!(sending.checkpoint.as_ref().unwrap().request_id)
    );
    let loaded = load(&store, KEY);
    assert_eq!(
        loaded.draft.checkpoint, sending.checkpoint,
        "loaded as data"
    );
    assert!(
        loaded.draft.uncertain,
        "a send saved as sending reloads uncertain"
    );
    assert!(loaded.storage_error.is_none());
}

#[test]
fn a_present_but_invalid_checkpoint_is_never_read_as_absent() {
    let scratch = Scratch::new("checkpoint-invalid");
    let store = DraftStore::open(scratch.0.clone());
    save(&store, KEY, draft("kept", &[])).unwrap();
    store.flush();
    let path = folder(&scratch.0, KEY).join("draft.json");
    let good = serde_json::to_value(checkpoint(1)).unwrap();
    let mut wrong_family = good.clone();
    wrong_family["receipts"] = json!("question_receipts");
    for (case, value) in [
        ("malformed", json!({"request_id": 7})),
        ("unknown field", {
            let mut v = good.clone();
            v["permission"] = json!(true);
            v
        }),
        ("inconsistent", wrong_family),
        ("null", Value::Null),
    ] {
        let mut manifest = manifest(&scratch.0);
        manifest["deferred_input"] = value;
        fs::write(&path, manifest.to_string()).unwrap();
        let loaded = read_draft(&scratch.0, KEY);
        assert_eq!(
            loaded.storage_error.as_deref(),
            Some(NOT_RESTORED),
            "{case}"
        );
        assert!(loaded.draft.checkpoint.is_none(), "{case}");
        assert!(path.exists(), "{case}: the files stay");
    }
}

#[test]
fn an_invalid_checkpoint_is_never_saved() {
    let scratch = Scratch::new("checkpoint-refused");
    let store = DraftStore::open(scratch.0.clone());
    save(&store, KEY, draft("before", &[])).unwrap();
    let mut bad = draft("after", &[]);
    let mut invalid = checkpoint(1);
    invalid.request_id = "not-a-uuid".into();
    bad.checkpoint = Some(Box::new(invalid));
    assert_eq!(save(&store, KEY, bad), Err(NOT_SAVED_CHECKPOINT.to_owned()));
    assert_eq!(load(&store, KEY).draft.draft.text, "before");
}

#[test]
fn imported_and_recovered_drafts_never_carry_a_checkpoint() {
    let scratch = Scratch::new("checkpoint-import");
    let (source_root, here, recovered) = (
        scratch.0.join("dev"),
        scratch.0.join("here"),
        scratch.0.join("recovered"),
    );
    let mut sending = draft("was being sent", &[]);
    sending.sending = true;
    sending.checkpoint = Some(Box::new(checkpoint(3)));
    seed(&source_root, &[(KEY, sending)]);
    assert!(read_draft(&source_root, KEY).draft.checkpoint.is_some());
    assert_eq!(
        import_once(&here, &[source("dev", &source_root)]).imported,
        1
    );
    let imported = load(&DraftStore::open(here.clone()), KEY);
    assert_eq!(imported.draft.draft.text, "was being sent");
    assert!(imported.draft.uncertain, "still interrupted, never re-sent");
    assert!(imported.draft.checkpoint.is_none());
    let store = DraftStore::open(recovered);
    let copy = wait(|done| store.recover(&source_root, KEY, done)).unwrap();
    assert!(copy.draft.checkpoint.is_none());
    assert!(load(&store, KEY).draft.checkpoint.is_none());
}

/// review-n's #1316 probe (zerus-5wmh.66.4.4).
#[test]
fn a_checkpoint_only_recovery_destination_is_not_empty() {
    let root = Scratch::new("probe-checkpoint-destination");
    let original = DraftStore::open(root.0.join("original"));
    save(
        &original,
        KEY,
        draft("source text", &[("a.txt", b"kept attachment")]),
    )
    .unwrap();
    let destination = DraftStore::open(root.0.join("destination"));
    let held = StoredDraft {
        checkpoint: Some(Box::new(checkpoint(3))),
        ..Default::default()
    };
    let before = held.checkpoint.clone();
    save(&destination, KEY, held).unwrap();
    let recovered = wait(|done| destination.recover(original.root(), KEY, done));
    let after = load(&destination, KEY);
    eprintln!(
        "checkpoint-only recover: refused={} original_still_present={}",
        recovered.is_err(),
        after.draft.checkpoint == before
    );
    assert!(
        recovered.is_err(),
        "recovery cannot replace a held identity"
    );
}

/// review-n's #1316 probe (zerus-5wmh.66.4.4).
#[test]
fn an_invalid_checkpoint_keeps_text_and_attachments_on_read_and_refused_save() {
    let root = Scratch::new("probe-invalid-attachments");
    let store = DraftStore::open(root.0.clone());
    let valid = draft("original text", &[("a.txt", b"original attachment")]);
    save(&store, KEY, valid.clone()).unwrap();
    let path = folder(&root.0, KEY).join("draft.json");
    let mut value = manifest(&root.0);
    value["deferred_input"] = Value::Null;
    fs::write(&path, value.to_string()).unwrap();
    let bytes = fs::read(&path).unwrap();
    let loaded = load(&store, KEY);
    assert_eq!(loaded.storage_error.as_deref(), Some(NOT_RESTORED));
    assert_eq!(loaded.draft.draft, valid.draft);
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes,
        "reading never normalizes a malformed checkpoint"
    );
    let mut bad_save = draft("changed", &[("b.txt", b"new attachment")]);
    let mut bad = checkpoint(9);
    bad.receipts = "wrong".into();
    bad_save.checkpoint = Some(Box::new(bad));
    assert!(save(&store, KEY, bad_save).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(load(&store, KEY).draft.draft, valid.draft);
}

#[test]
fn a_held_identity_reloads_uncertain_and_no_rename_replaces_it() {
    let scratch = Scratch::new("checkpoint-held");
    let store = DraftStore::open(scratch.0.join("drafts"));
    // A continuation's waiting save: neither sending nor uncertain.
    let mut waiting = draft("waits", &[]);
    waiting.checkpoint = Some(Box::new(checkpoint(1)));
    save(&store, KEY, waiting.clone()).unwrap();
    let loaded = load(&store, KEY);
    assert!(
        loaded.draft.uncertain,
        "held: never an unrestricted new attempt"
    );
    assert_eq!(loaded.notice.as_deref(), Some(UNCONFIRMED));
    assert_eq!(loaded.draft.checkpoint, waiting.checkpoint);
    // One saved while sending keeps its interrupted notice.
    let mut sending = waiting.clone();
    sending.sending = true;
    save(&store, KEY, sending).unwrap();
    assert_eq!(load(&store, KEY).notice.as_deref(), Some(INTERRUPTED));
    // A held identity without any content is not an empty destination.
    let (old, new) = ("studio\nclaude/p/old", "studio\nclaude/p/new");
    let held = StoredDraft {
        checkpoint: Some(Box::new(checkpoint(2))),
        ..Default::default()
    };
    save(&store, new, held.clone()).unwrap();
    save(&store, old, draft("moving", &[])).unwrap();
    assert_eq!(
        wait(|done| store.rename(old, new, draft("moving", &[]), done)),
        Err(ALREADY_EXISTS.to_owned())
    );
    assert_eq!(load(&store, new).draft.checkpoint, held.checkpoint);
    assert_eq!(load(&store, old).draft.draft.text, "moving");
}

#[test]
fn an_import_never_replaces_a_held_identity() {
    let scratch = Scratch::new("checkpoint-import-held");
    let (source_root, here) = (scratch.0.join("dev"), scratch.0.join("here"));
    seed(&source_root, &[(KEY, draft("from elsewhere", &[]))]);
    let store = DraftStore::open(here.clone());
    let held = StoredDraft {
        checkpoint: Some(Box::new(checkpoint(4))),
        ..Default::default()
    };
    save(&store, KEY, held.clone()).unwrap();
    drop(store);
    let report = import_once(&here, &[source("dev", &source_root)]);
    assert_eq!(report.imported, 0);
    assert_eq!(report.conflicts.len(), 1, "reported, never overwritten");
    let kept = load(&DraftStore::open(here), KEY);
    assert_eq!(kept.draft.checkpoint, held.checkpoint);
}

const SESSION_ID: &str = "3f2a9c1d-6b7e-4c2a-9d1f-0a1b2c3d4e5f";

/// zerus-ucck P2c-2: a session's draft keyed by its name moves once to its
/// session ID key, as stored, and never over a draft already there.
#[test]
fn a_name_key_draft_moves_once_to_its_session_id_key() {
    let scratch = Scratch::new("adopt");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let id_key = format!("studio\nid:{SESSION_ID}");
    let label = "claude/sample-project/review";
    assert!(
        !wait(|done| store.adopt(KEY, &id_key, label, done)),
        "nothing to move"
    );
    let mut held = draft("typed before the upgrade", &[("a.txt", b"kept attachment")]);
    held.checkpoint = Some(Box::new(checkpoint(3)));
    save(&store, KEY, held.clone()).unwrap();
    assert!(
        !wait(|done| store.adopt(KEY, KEY, label, done)),
        "a key is not its own legacy"
    );
    save(&store, &id_key, draft("already here", &[])).unwrap();
    assert!(!wait(|done| store.adopt(KEY, &id_key, label, done)));
    assert_eq!(
        load(&store, KEY).draft.draft.text,
        "typed before the upgrade"
    );
    assert_eq!(load(&store, &id_key).draft.draft.text, "already here");

    save(&store, &id_key, StoredDraft::default()).unwrap();
    assert!(wait(|done| store.adopt(KEY, &id_key, label, done)));
    let moved = load(&store, &id_key).draft;
    assert_eq!(moved.draft, held.draft);
    assert_eq!(
        moved.checkpoint, held.checkpoint,
        "its reserved input moves with it"
    );
    assert_eq!((moved.position, moved.anchor), (3, 1));
    assert_eq!(moved.session_label, label);
    assert_eq!(load(&store, KEY).draft, StoredDraft::default());
    let listed = index(&store);
    assert_eq!(
        listed.keys().cloned().collect::<Vec<_>>(),
        std::slice::from_ref(&id_key)
    );
    assert_eq!(listed[&id_key].session_label, label);
    assert!(
        !wait(|done| store.adopt(KEY, &id_key, "later", done)),
        "only once"
    );
}

/// #1418 review (review-3): the old key is emptied only after the new one is
/// durable, so a failed move leaves the name key's draft, reserved input
/// included. The drafts root is read-only, so the ID key's folder cannot be
/// created while `private()` would still let the name key be emptied.
#[cfg(unix)]
#[test]
fn a_failed_move_leaves_the_name_key_draft() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("adopt-failed");
    let root = scratch.0.join("drafts");
    let store = DraftStore::open(root.clone());
    let id_key = format!("studio\nid:{SESSION_ID}");
    let mut held = draft("kept", &[("a.txt", b"kept attachment")]);
    held.checkpoint = Some(Box::new(checkpoint(3)));
    save(&store, KEY, held.clone()).unwrap();
    // The ID key's folder cannot be created; the name key's still can be written.
    fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
    let moved = wait(|done| store.adopt(KEY, &id_key, "claude/p/a", done));
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!moved);
    let kept = load(&store, KEY).draft;
    assert_eq!((kept.draft, kept.checkpoint), (held.draft, held.checkpoint));
    assert!(!folder(&root, &id_key).join("draft.json").exists());
}

/// #1418 review: a crash between the move's two writes leaves the same draft
/// at both keys; the next move empties the name key. Different drafts at the
/// two keys are never merged or chosen between: both stay.
#[test]
fn an_interrupted_move_finishes_only_for_the_same_draft() {
    let scratch = Scratch::new("adopt-interrupted");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let id_key = format!("studio\nid:{SESSION_ID}");
    let mut held = draft("typed before the crash", &[("a.txt", b"kept attachment")]);
    held.checkpoint = Some(Box::new(checkpoint(3)));
    save(&store, KEY, held.clone()).unwrap();
    // The crash: the ID key was written, with the label the move adds; the
    // name key was not emptied yet.
    let mut written = held.clone();
    written.session_label = "claude/sample-project/review".into();
    save(&store, &id_key, written.clone()).unwrap();
    assert!(wait(|done| store.adopt(
        KEY,
        &id_key,
        "claude/sample-project/review",
        done
    )));
    assert_eq!(load(&store, KEY).draft, StoredDraft::default());
    assert_eq!(load(&store, &id_key).draft.draft, held.draft);
    // A different draft at each key: nothing changes.
    save(&store, KEY, draft("typed on an older build", &[])).unwrap();
    assert!(!wait(|done| store.adopt(
        KEY,
        &id_key,
        "claude/sample-project/review",
        done
    )));
    assert_eq!(
        load(&store, KEY).draft.draft.text,
        "typed on an older build"
    );
    assert_eq!(load(&store, &id_key).draft.draft, held.draft);
    // The same text with another reserved input is a different draft too.
    let mut other = held.clone();
    other.checkpoint = Some(Box::new(checkpoint(4)));
    save(&store, KEY, other.clone()).unwrap();
    assert!(!wait(|done| store.adopt(
        KEY,
        &id_key,
        "claude/sample-project/review",
        done
    )));
    assert_eq!(load(&store, KEY).draft.checkpoint, other.checkpoint);
}

#[test]
fn a_damaged_name_key_draft_stays_where_it_is() {
    let scratch = Scratch::new("adopt-damaged");
    let root = scratch.0.join("drafts");
    let store = DraftStore::open(root.clone());
    let id_key = format!("studio\nid:{SESSION_ID}");
    save(&store, KEY, draft("kept", &[("a.txt", b"kept attachment")])).unwrap();
    let dir = folder(&root, KEY);
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".bin") {
            fs::write(entry.path(), b"changed").unwrap();
        }
    }
    let manifest = fs::read(dir.join("draft.json")).unwrap();
    assert!(!wait(|done| store.adopt(KEY, &id_key, "claude/p/a", done)));
    assert_eq!(fs::read(dir.join("draft.json")).unwrap(), manifest);
    assert!(!folder(&root, &id_key).join("draft.json").exists());
}

#[test]
fn only_a_draft_keyed_by_session_id_records_its_session_label() {
    let scratch = Scratch::new("session-label");
    let root = scratch.0.join("drafts");
    let store = DraftStore::open(root.clone());
    let id_key = format!("studio\nid:{SESSION_ID}");
    save(&store, KEY, draft("by name", &[])).unwrap();
    let mut by_id = draft("by id", &[]);
    by_id.session_label = "claude/p/a".into();
    save(&store, &id_key, by_id).unwrap();
    let manifest = |key: &str| -> Value {
        serde_json::from_slice(&fs::read(folder(&root, key).join("draft.json")).unwrap()).unwrap()
    };
    assert!(
        manifest(KEY).get("session_label").is_none(),
        "a name key keeps its bytes"
    );
    assert_eq!(manifest(&id_key)["session_label"], json!("claude/p/a"));
    assert_eq!(load(&store, &id_key).draft.session_label, "claude/p/a");
}

#[test]
fn form_state_alone_moves_and_is_never_overwritten() {
    let scratch = Scratch::new("adopt-form");
    let store = DraftStore::open(scratch.0.join("drafts"));
    let id_key = format!("studio\nid:{SESSION_ID}");
    let mut chosen = StoredDraft::default();
    chosen.form_state.insert(
        "answers".into(),
        json!([{"id": "q_0", "options": ["opt_0_0"]}]),
    );
    save(&store, KEY, chosen.clone()).unwrap();
    // Another answer at the ID key: form state alone is never overwritten.
    let mut other = StoredDraft::default();
    other.form_state.insert(
        "answers".into(),
        json!([{"id": "q_0", "options": ["opt_0_1"]}]),
    );
    save(&store, &id_key, other.clone()).unwrap();
    assert!(!wait(|done| store.adopt(KEY, &id_key, "claude/p/a", done)));
    assert_eq!(load(&store, &id_key).draft.form_state, other.form_state);
    save(&store, &id_key, StoredDraft::default()).unwrap();
    assert!(wait(|done| store.adopt(KEY, &id_key, "claude/p/a", done)));
    assert_eq!(load(&store, &id_key).draft.form_state, chosen.form_state);
    assert!(load(&store, KEY).draft.form_state.is_empty());
}
