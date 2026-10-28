//! The draft store on a real disk, in a private scratch folder per test.
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use zerus_client_core::attachments::{Attachment, Draft};
use zerus_client_core::drafts::{
    import_at_start, import_once, import_sources, qt_root, read_draft, read_index, Cursor,
    DraftStore, ImportConflict, ImportReport, ImportSource, IndexPage, Loaded, Page, StoredDraft,
    Summary, ALREADY_EXISTS, IMPORT_MARKER, INTERRUPTED, NOT_RESTORED,
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
    let report = import_once(&here, &[source("qt", &qt)]);
    assert_eq!((report.imported, report.invalid), (0, 1));
    assert!(report.conflicts.is_empty());
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
fn a_read_right_after_import_at_start_sees_the_imported_draft() {
    const CHILD: &str = "ZERUS_TEST_IMPORT_AT_START";
    const NAME: &str = "a_read_right_after_import_at_start_sees_the_imported_draft";
    let other = "studio\nclaude/sample-project/other";
    if let Some(scratch) = std::env::var_os(CHILD).map(PathBuf::from) {
        // In the child, Qt's and the dev app's stores are under the scratch
        // folder, wherever this platform keeps them.
        seed(&qt_root().unwrap(), &[(KEY, draft("from Qt", &[]))]);
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
        assert_eq!(load(&store, KEY).draft.draft.text, "from Qt");
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
        stderr.contains("drafts import: 2 imported, 0 identical, 0 conflicts, 0 invalid"),
        "{stderr}"
    );
}
