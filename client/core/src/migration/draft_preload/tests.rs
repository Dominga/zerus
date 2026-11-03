use super::*;
use serde_json::json;
use std::fs;

/// A private data folder with a Next draft store, an optional Qt store and
/// an optional dev store, all synthetic.
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zerus-draft-preload-{}", uuid::Uuid::new_v4()));
        drop(disk::Parents::open(&root, true).unwrap());
        let scratch = Self(root.canonicalize().unwrap());
        drop(disk::Parents::open(&scratch.data(), true).unwrap());
        scratch
    }
    fn data(&self) -> PathBuf {
        self.0.join("data")
    }
    fn next(&self) -> PathBuf {
        self.0.join("next-drafts")
    }
    fn qt(&self) -> PathBuf {
        self.0.join("qt-drafts")
    }
    fn dev(&self) -> PathBuf {
        self.0.join("dev-drafts")
    }
    fn capture(&self) -> Capture {
        drop(disk::Parents::open(&self.data(), true).unwrap());
        Capture::new(self.data(), self.next(), Some(self.qt()), vec![self.dev()])
    }
    fn slot(&self) -> PathBuf {
        self.data().join(SLOT)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A valid draft of `key` under `root`, with one attachment of `blob` bytes
/// whose snapshot is written when `snapshot` is set.
fn draft(root: &Path, key: &str, text: &str, blob: &[u8], snapshot: bool) -> PathBuf {
    let folder = root.join(sha256(key.as_bytes()));
    fs::create_dir_all(&folder).unwrap();
    let digest = sha256(blob);
    let manifest = json!({"version": 1, "key": key, "text": text, "attachments": [
        {"name": "notes.txt", "mime": "text/plain", "reference": "[notes.txt]", "sha256": digest, "bytes": blob.len()}
    ]});
    fs::write(folder.join("draft.json"), manifest.to_string()).unwrap();
    if snapshot {
        fs::write(folder.join(format!("{digest}.bin")), blob).unwrap();
    }
    folder
}

#[test]
fn every_store_is_captured_once_and_reopens_after_restart_unchanged() {
    let s = Scratch::new();
    let kept = draft(&s.next(), "codex/sample/a", "next text", b"abc", true);
    draft(&s.next(), "codex/sample/b", "missing blob", b"defg", false);
    // A manifest the decoder refuses, and a folder without a manifest.
    let odd = s.next().join(sha256(b"codex/sample/odd"));
    fs::create_dir_all(&odd).unwrap();
    fs::write(odd.join("draft.json"), b"not json").unwrap();
    fs::create_dir_all(s.next().join(sha256(b"codex/sample/empty"))).unwrap();
    // A plain file and a name that is not a draft folder are not drafts.
    fs::write(s.next().join("notes.txt"), b"x").unwrap();
    fs::create_dir_all(s.next().join("cache")).unwrap();
    draft(&s.qt(), "codex/sample/a", "qt text", b"abc", true);
    let gate = s.capture().prepare();
    assert_eq!(
        gate,
        Gate {
            writes: true,
            import: true
        }
    );
    let status = s.capture().status();
    assert_eq!(
        status.code, "draft_capture_waiting",
        "a new capture has not run"
    );
    let capture = s.capture();
    capture.prepare();
    let first = capture.status();
    assert_eq!(first.code, "draft_capture_captured");
    assert_eq!(first.manifests, 5);
    let seed: Seed = read_json(&s.slot().join("identity.json")).unwrap().unwrap();
    let snapshot = Snapshot::open(&s.slot(), &seed).unwrap();
    let classes: Vec<(Store, Class)> = snapshot
        .index
        .entries
        .iter()
        .map(|entry| (entry.store, entry.class))
        .collect();
    assert_eq!(
        classes
            .iter()
            .filter(|(store, _)| *store == Store::Next)
            .count(),
        4
    );
    assert!(
        classes.contains(&(Store::Next, Class::Unsupported)),
        "{classes:?}"
    );
    assert!(classes.contains(&(Store::Next, Class::NoManifest)));
    assert!(classes.contains(&(Store::Qt, Class::Supported)));
    let blobs: Vec<Blob> = snapshot
        .index
        .entries
        .iter()
        .filter(|entry| entry.store == Store::Next)
        .flat_map(|entry| entry.blobs.iter().map(|blob| blob.class))
        .collect();
    assert!(blobs.contains(&Blob::Present) && blobs.contains(&Blob::Missing));
    assert_eq!(
        snapshot.index.roots,
        vec![
            Root {
                store: Store::Next,
                root: 0,
                present: true
            },
            Root {
                store: Store::Dev,
                root: 0,
                present: false
            },
            Root {
                store: Store::Qt,
                root: 0,
                present: true
            },
        ]
    );
    // The original bytes, exactly, through the index.
    let entry = snapshot
        .index
        .entries
        .iter()
        .find(|entry| entry.folder == sha256(b"codex/sample/a") && entry.store == Store::Next)
        .unwrap();
    let original = fs::read(kept.join("draft.json")).unwrap();
    assert_eq!(snapshot.bytes(entry), original.as_slice());
    // Typing changes the live draft later; the capture stays the original.
    fs::write(kept.join("draft.json"), b"{\"version\":1}").unwrap();
    let later = s.capture();
    assert_eq!(
        later.prepare(),
        Gate {
            writes: true,
            import: true
        }
    );
    assert_eq!(later.status().capture_id, first.capture_id);
    let reopened = Snapshot::open(&s.slot(), &seed).unwrap();
    assert_eq!(reopened.bytes(entry), original.as_slice());
}

#[test]
fn a_failed_attempt_is_never_backfilled_and_pauses_only_the_import() {
    let s = Scratch::new();
    let big = s.next().join(sha256(b"codex/sample/big"));
    fs::create_dir_all(&big).unwrap();
    fs::write(big.join("draft.json"), vec![b' '; MANIFEST + 1]).unwrap();
    let capture = s.capture();
    assert_eq!(
        capture.prepare(),
        Gate {
            writes: true,
            import: false
        }
    );
    assert_eq!(capture.status().code, "draft_capture_too_large");
    assert!(s.slot().join("failed.json").exists());
    assert!(!s.slot().join("receipt.json").exists());
    // The source becomes capturable, but its original is already gone.
    fs::remove_dir_all(&big).unwrap();
    draft(&s.next(), "codex/sample/a", "typed after", b"abc", true);
    let again = s.capture();
    assert_eq!(
        again.prepare(),
        Gate {
            writes: true,
            import: false
        }
    );
    assert_eq!(again.status().code, "draft_capture_too_large");
    assert!(!s.slot().join("receipt.json").exists());
}

#[test]
fn every_cap_refuses_the_whole_capture_never_a_prefix() {
    let s = Scratch::new();
    fs::create_dir_all(s.next()).unwrap();
    for n in 0..=MAX_SCAN {
        fs::write(s.next().join(format!("file-{n}")), b"").unwrap();
    }
    let capture = s.capture();
    assert!(!capture.prepare().import);
    assert_eq!(capture.status().code, "draft_capture_too_large");
    assert!(!s.slot().join("index.json").exists(), "no partial index");
}

#[test]
fn an_attempt_that_died_inside_the_capture_job_captures_again() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "text", b"abc", true);
    drop(disk::Parents::open(&s.slot(), true).unwrap());
    let seed = Seed {
        version: 1,
        boundary: BOUNDARY.into(),
        capture_id: uuid::Uuid::new_v4().to_string(),
    };
    create_json(&s.slot().join("identity.json"), &seed).unwrap();
    let capture = s.capture();
    assert_eq!(
        capture.prepare(),
        Gate {
            writes: true,
            import: true
        }
    );
    assert_eq!(capture.status().code, "draft_capture_captured");
    assert_eq!(
        capture.status().capture_id.as_deref(),
        Some(seed.capture_id.as_str())
    );
}

#[test]
fn a_crashed_attempt_that_saw_other_originals_fails_instead_of_replacing_them() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "first", b"abc", true);
    // The crashed attempt's seed and index, then a different source.
    drop(disk::Parents::open(&s.slot(), true).unwrap());
    let seed = Seed {
        version: 1,
        boundary: BOUNDARY.into(),
        capture_id: uuid::Uuid::new_v4().to_string(),
    };
    create_json(&s.slot().join("identity.json"), &seed).unwrap();
    let roots = [(Store::Next, 0, s.next())];
    let roots: Vec<(Store, u32, &Path)> = roots
        .iter()
        .map(|(a, b, c)| (*a, *b, c.as_path()))
        .collect();
    let (index, _) = scan(&roots, &seed.capture_id).unwrap();
    disk::create(
        &s.slot().join("index.json"),
        &serde_json::to_vec(&index).unwrap(),
    )
    .unwrap();
    draft(&s.qt(), "codex/sample/q", "qt", b"q", true);
    let capture = s.capture();
    assert_eq!(
        capture.prepare(),
        Gate {
            writes: true,
            import: false
        }
    );
    assert_eq!(capture.status().code, "draft_capture_changed");
}

#[test]
fn no_draft_anywhere_is_not_applicable_and_still_one_recorded_outcome() {
    let s = Scratch::new();
    let capture = s.capture();
    assert_eq!(
        capture.prepare(),
        Gate {
            writes: true,
            import: true
        }
    );
    assert_eq!(capture.status().code, "draft_capture_not_applicable");
    assert!(s.slot().join("receipt.json").exists());
    assert!(!s.slot().join("original.bin").exists());
}

#[test]
fn lineage_that_cannot_be_persisted_refuses_draft_writes() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "text", b"abc", true);
    // The data folder is a plain file: no private slot can exist.
    fs::remove_dir_all(s.data()).unwrap();
    fs::write(s.data(), b"").unwrap();
    let capture = Capture::new(s.data(), s.next(), None, Vec::new());
    assert_eq!(
        capture.prepare(),
        Gate {
            writes: false,
            import: false
        }
    );
    assert_eq!(capture.status().code, "draft_capture_private_slot");
}

// The draft store's side of the fence: the capture is its first job, a
// failure without lineage refuses every write path, and a recorded failure
// pauses only the one-time import until Retry.

fn text(text: &str) -> drafts::StoredDraft {
    drafts::StoredDraft {
        draft: attachments::Draft {
            text: text.into(),
            ..attachments::Draft::default()
        },
        ..drafts::StoredDraft::default()
    }
}

/// The result `done` receives from one store operation.
fn wait<T: Send + 'static>(queue: impl FnOnce(Box<dyn FnOnce(T) + Send>)) -> T {
    let (sender, receiver) = std::sync::mpsc::channel();
    queue(Box::new(move |value| {
        let _ = sender.send(value);
    }));
    receiver
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the store answers")
}

/// A captured store wired as the desktop does: Retry queues the store's job.
fn open(s: &Scratch, capture: Capture) -> (Arc<drafts::DraftStore>, Arc<Capture>) {
    let capture = Arc::new(capture);
    let store = Arc::new(drafts::DraftStore::open_captured(s.next(), capture.clone()));
    let weak = Arc::downgrade(&store);
    capture.hook_retry(Arc::new(move || {
        if let Some(store) = weak.upgrade() {
            store.retry_import();
        }
    }));
    (store, capture)
}

fn sources(s: &Scratch) -> Vec<drafts::ImportSource> {
    drafts::import_sources(&s.next(), Some(s.qt()), Vec::new())
}

fn loaded(store: &drafts::DraftStore, key: &str) -> String {
    wait(|done| store.load(key, done)).draft.draft.text
}

#[test]
fn the_capture_records_next_before_saves_queued_with_the_store() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "before", b"abc", true);
    let (store, capture) = open(&s, s.capture());
    // Queued at once, still after the capture job.
    let saved = wait(|done| store.save_durable("codex/sample/a", text("typed"), done));
    assert_eq!(saved, Ok(()));
    assert_eq!(loaded(&store, "codex/sample/a"), "typed");
    let status = capture.status();
    assert_eq!(status.code, "draft_capture_captured");
    assert!(status.writes && !status.paused);
    let seed: Seed = read_json(&s.slot().join("identity.json")).unwrap().unwrap();
    let snapshot = Snapshot::open(&s.slot(), &seed).unwrap();
    let entry = &snapshot.index.entries[0];
    let original: Value = serde_json::from_slice(snapshot.bytes(entry)).unwrap();
    assert_eq!(original["text"], "before");
}

#[test]
fn without_durable_lineage_every_write_path_and_the_import_wait() {
    let s = Scratch::new();
    let manifest = draft(&s.next(), "codex/sample/a", "before", b"abc", true).join("draft.json");
    let before = fs::read(&manifest).unwrap();
    draft(&s.qt(), "codex/sample/q", "from qt", b"q", true);
    fs::remove_dir_all(s.data()).unwrap();
    fs::write(s.data(), b"").unwrap();
    let capture = Capture::for_store(s.data(), s.next(), &sources(&s));
    let (store, capture) = open(&s, capture);
    let refused = Err(drafts::NOT_RECORDED.to_owned());
    let key = "codex/sample/a";
    // Reads still work; the comparisons match, so each path reaches its write.
    let current = wait(|done| store.load(key, done)).draft;
    assert_eq!(current.draft.text, "before");
    assert_eq!(wait(|done| store.save(key, text("x"), done)), refused);
    assert_eq!(
        wait(|done| store.save_durable(key, text("x"), done)),
        refused
    );
    assert_eq!(
        wait(|done| store.settle_if_unchanged(key, current.draft.clone(), true, done)),
        Err(drafts::NOT_RECORDED.to_owned())
    );
    assert_eq!(
        wait(|done| store.replace_if_unchanged(key, current.clone(), text("x"), done)),
        Err(drafts::NOT_RECORDED.to_owned())
    );
    assert_eq!(
        wait(|done| store.rename(key, "codex/sample/b", text("x"), done)),
        refused
    );
    assert_eq!(
        wait(|done| store.recover(&s.qt(), "codex/sample/q", done)).map(|_| ()),
        refused
    );
    let report = wait(|done| store.import(sources(&s), done));
    assert_eq!(report.unfinished, vec![drafts::IMPORT_PAUSED.to_owned()]);
    assert_eq!(report.imported, 0);
    store.flush();
    assert_eq!(
        fs::read(&manifest).unwrap(),
        before,
        "Next's draft is untouched"
    );
    assert!(!s.next().join(sha256(b"codex/sample/q")).exists());
    assert!(!s.next().join(drafts::IMPORT_MARKER).exists());
    let status = capture.status();
    assert_eq!(status.code, "draft_capture_private_slot");
    assert!(!status.writes && status.paused);
    // The data folder is fixed: this run wrote nothing, so Retry captures
    // the version before the store's first job and runs the import.
    fs::remove_file(s.data()).unwrap();
    drop(disk::Parents::open(&s.data(), true).unwrap());
    capture.request_retry();
    store.flush();
    let status = capture.status();
    assert_eq!(status.code, "draft_capture_captured");
    assert!(status.writes && !status.paused);
    assert_eq!(loaded(&store, "codex/sample/q"), "from qt");
    assert_eq!(wait(|done| store.save(key, text("typed"), done)), Ok(()));
}

#[test]
fn a_recorded_failure_keeps_typing_and_retry_imports_without_backfilling() {
    let s = Scratch::new();
    let big = s.next().join(sha256(b"codex/sample/big"));
    fs::create_dir_all(&big).unwrap();
    fs::write(big.join("draft.json"), vec![b' '; MANIFEST + 1]).unwrap();
    draft(&s.qt(), "codex/sample/q", "from qt", b"q", true);
    let (store, capture) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    let key = "codex/sample/a";
    assert_eq!(wait(|done| store.save(key, text("typed"), done)), Ok(()));
    let report = wait(|done| store.import(sources(&s), done));
    assert_eq!(report.unfinished, vec![drafts::IMPORT_PAUSED.to_owned()]);
    assert_eq!(loaded(&store, "codex/sample/q"), "");
    let status = capture.status();
    assert_eq!(status.code, "draft_capture_too_large");
    assert!(status.writes && status.paused);
    capture.request_retry();
    store.flush();
    assert_eq!(loaded(&store, "codex/sample/q"), "from qt");
    assert!(s.next().join(drafts::IMPORT_MARKER).exists());
    let status = capture.status();
    assert_eq!(
        status.code, "draft_capture_too_large",
        "Retry never captures"
    );
    assert!(status.writes && !status.paused);
    assert!(!s.slot().join("receipt.json").exists());
    // A second Retry has nothing waiting.
    capture.request_retry();
    store.flush();
    assert!(!capture.status().paused);
    drop(store);
    // The next start: the failure stays, and the import is done.
    let (store, capture) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    let report = wait(|done| store.import(sources(&s), done));
    assert!(report.already_done);
    let status = capture.status();
    assert_eq!(status.code, "draft_capture_too_large");
    assert!(status.writes && !status.paused);
}

#[test]
fn the_capture_reads_the_same_sources_as_the_import() {
    let s = Scratch::new();
    fs::create_dir_all(s.qt()).unwrap();
    fs::create_dir_all(s.dev()).unwrap();
    let inside = s.next().join("nested");
    fs::create_dir_all(&inside).unwrap();
    let sources = drafts::import_sources(&s.next(), Some(s.qt()), vec![s.dev(), inside]);
    let capture = Capture::for_store(s.data(), s.next(), &sources);
    assert_eq!(capture.qt.as_deref(), Some(s.qt().as_path()));
    assert_eq!(
        capture.dev,
        vec![s.dev()],
        "a root inside Next is not a source"
    );
}
