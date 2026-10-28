//! The draft store on a real disk, in a private scratch folder per test.
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use zerus_client_core::attachments::{Attachment, Draft};
use zerus_client_core::drafts::{
    read_draft, read_index, Cursor, DraftStore, IndexPage, Loaded, Page, StoredDraft, Summary,
    ALREADY_EXISTS, INTERRUPTED, NOT_RESTORED,
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
