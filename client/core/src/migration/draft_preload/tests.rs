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
                present: true,
                earlier_run: false,
            },
            Root {
                store: Store::Dev,
                root: 0,
                present: false,
                earlier_run: false,
            },
            Root {
                store: Store::Qt,
                root: 0,
                present: true,
                earlier_run: false,
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
    let (index, _) = scan(&roots, &seed.capture_id, None).unwrap();
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

// review-q R1 on #1230: a capture never reports Next's store as captured
// when an earlier run wrote it without capturing first, and every run of the
// switch build captures first, in service mode or not.

fn seed_without_outcome(s: &Scratch) -> Seed {
    drop(disk::Parents::open(&s.slot(), true).unwrap());
    let seed = Seed {
        version: 1,
        boundary: BOUNDARY.into(),
        capture_id: uuid::Uuid::new_v4().to_string(),
    };
    create_json(&s.slot().join("identity.json"), &seed).unwrap();
    seed
}

fn next_texts(s: &Scratch) -> Vec<String> {
    let seed: Seed = read_json(&s.slot().join("identity.json")).unwrap().unwrap();
    let snapshot = Snapshot::open(&s.slot(), &seed).unwrap();
    snapshot
        .index
        .entries
        .iter()
        .filter(|entry| entry.store == Store::Next)
        .map(|entry| {
            let manifest: Value = serde_json::from_slice(snapshot.bytes(entry)).unwrap();
            manifest["text"].as_str().unwrap_or_default().to_owned()
        })
        .collect()
}

#[test]
fn probe_a_an_import_by_an_uncaptured_run_is_never_reported_captured() {
    let s = Scratch::new();
    draft(&s.qt(), "codex/sample/k", "qt text", b"k", true);
    // A run that did not capture first imported Qt's draft into Next.
    let report = drafts::import_once(&s.next(), &sources(&s));
    assert_eq!(report.imported, 1);
    let (store, capture) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    store.flush();
    assert_eq!(capture.status().code, "draft_capture_next_earlier_run");
    assert!(
        next_texts(&s).is_empty(),
        "the imported copy is no Next original"
    );
    let seed: Seed = read_json(&s.slot().join("identity.json")).unwrap().unwrap();
    let snapshot = Snapshot::open(&s.slot(), &seed).unwrap();
    let next = snapshot
        .index
        .roots
        .iter()
        .find(|r| r.store == Store::Next)
        .unwrap();
    assert!(next.earlier_run);
    assert!(
        snapshot.index.entries.iter().any(|e| e.store == Store::Qt),
        "Qt's original is still recorded"
    );
    drop(store);
    // Restarted: the outcome stays incomplete.
    let (_store, again) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    _store.flush();
    assert_eq!(again.status().code, "draft_capture_next_earlier_run");
}

#[test]
fn probe_a_fenced_every_switch_run_captures_before_its_import() {
    let s = Scratch::new();
    draft(&s.qt(), "codex/sample/k", "qt text", b"k", true);
    // A switch-build run outside service mode still opens with the capture.
    let capture = Capture::for_run(true, None, &s.data()).expect("switch runs capture");
    assert!(Capture::for_run(false, None, &s.data()).is_none());
    drop(capture);
    let (store, capture) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    wait(|done| store.import(sources(&s), done));
    store.flush();
    assert_eq!(loaded(&store, "codex/sample/k"), "qt text");
    // Qt's draft is recorded; Next held nothing before its first job.
    assert_eq!(capture.status().code, "draft_capture_captured");
    assert!(next_texts(&s).is_empty());
    drop(store);
    // A later service-mode run reopens that receipt, not today's store.
    let (_store, later) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    _store.flush();
    assert_eq!(later.status().code, "draft_capture_captured");
    assert!(
        next_texts(&s).is_empty(),
        "the imported copy is no Next original"
    );
}

#[test]
fn probe_b_typing_by_an_uncaptured_run_after_a_crashed_capture_is_never_captured() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "before", b"abc", true);
    seed_without_outcome(&s);
    std::thread::sleep(std::time::Duration::from_millis(20));
    // A run that did not capture first saves over Next's original.
    let plain = drafts::DraftStore::open(s.next());
    assert_eq!(
        wait(|done| plain.save_durable("codex/sample/a", text("typed later"), done)),
        Ok(())
    );
    drop(plain);
    let (store, capture) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    store.flush();
    assert_eq!(capture.status().code, "draft_capture_next_earlier_run");
    assert!(next_texts(&s).is_empty(), "typed later is no original");
}

#[test]
fn probe_b_fenced_a_crashed_capture_is_redone_before_any_typing() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "before", b"abc", true);
    seed_without_outcome(&s);
    std::thread::sleep(std::time::Duration::from_millis(20));
    let (store, capture) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    assert_eq!(
        wait(|done| store.save_durable("codex/sample/a", text("typed later"), done)),
        Ok(())
    );
    assert_eq!(capture.status().code, "draft_capture_captured");
    assert_eq!(next_texts(&s), vec!["before".to_owned()]);
    drop(store);
    let (_store, later) = open(&s, Capture::for_store(s.data(), s.next(), &sources(&s)));
    _store.flush();
    assert_eq!(later.status().code, "draft_capture_captured");
    assert_eq!(next_texts(&s), vec!["before".to_owned()]);
}

// Windows CI on #1230: the private reader refuses read limits above 16 MiB,
// so every capture there failed at its own leaf reads. The container is kept
// in parts of at most 16 MiB, each read with its exact size.

#[test]
fn a_container_over_one_part_is_kept_in_exact_parts_and_reopens() {
    let s = Scratch::new();
    // Seventeen near-1 MiB manifests: past one part, within every cap.
    let body = vec![b'x'; MANIFEST - 64];
    for n in 0..17 {
        let folder = s
            .next()
            .join(sha256(format!("codex/sample/{n}").as_bytes()));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("draft.json"), &body).unwrap();
    }
    let capture = s.capture();
    assert_eq!(
        capture.prepare(),
        Gate {
            writes: true,
            import: true
        }
    );
    assert_eq!(capture.status().code, "draft_capture_captured");
    let first = fs::metadata(s.slot().join("original.bin")).unwrap().len();
    let second = fs::metadata(s.slot().join("original-1.bin")).unwrap().len();
    assert_eq!(first as usize, PART);
    assert_eq!(first + second, 17 * body.len() as u64);
    assert!(!s.slot().join("original-2.bin").exists());
    // Restarted: the receipt reopens and every entry is exact.
    let again = s.capture();
    assert_eq!(
        again.prepare(),
        Gate {
            writes: true,
            import: true
        }
    );
    let seed: Seed = read_json(&s.slot().join("identity.json")).unwrap().unwrap();
    let snapshot = Snapshot::open(&s.slot(), &seed).unwrap();
    assert_eq!(snapshot.index.entries.len(), 17);
    for entry in &snapshot.index.entries {
        assert_eq!(snapshot.bytes(entry), body.as_slice());
    }
}

#[test]
fn a_single_part_receipt_keeps_its_earlier_format() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "text", b"abc", true);
    assert!(s.capture().prepare().import);
    let receipt: Value =
        serde_json::from_slice(&fs::read(s.slot().join("receipt.json")).unwrap()).unwrap();
    assert!(receipt.get("more").is_none(), "{receipt}");
    assert!(!s.slot().join("original-1.bin").exists());
}

fn failed_v1(s: &Scratch, reason: &str) {
    let seed = seed_without_outcome(s);
    // The defective attempt's own leftovers.
    fs::write(s.slot().join("index.json"), b"{}").unwrap();
    let failed = Failed {
        version: 1,
        capture_id: seed.capture_id,
        reason: reason.into(),
    };
    create_json(&s.slot().join("failed.json"), &failed).unwrap();
}

fn on_windows(value: bool) {
    WINDOWS.with(|windows| windows.set(value));
}

#[test]
fn only_a_windows_v1_unreadable_failure_is_superseded() {
    let failed = |version, reason: &str| Failed {
        version,
        capture_id: uuid::Uuid::new_v4().to_string(),
        reason: reason.into(),
    };
    assert!(superseded(&failed(1, "unreadable"), true));
    assert!(!superseded(&failed(1, "unreadable"), false));
    assert!(!superseded(&failed(1, "too_large"), true));
    assert!(!superseded(&failed(1, "changed"), true));
    assert!(!superseded(&failed(2, "unreadable"), true));
}

#[test]
fn the_windows_limit_failure_is_captured_again_and_kept_for_review() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "before", b"abc", true);
    std::thread::sleep(std::time::Duration::from_millis(20));
    failed_v1(&s, "unreadable");
    on_windows(true);
    let capture = s.capture();
    let gate = capture.prepare();
    on_windows(false);
    assert_eq!(
        gate,
        Gate {
            writes: true,
            import: true
        }
    );
    assert_eq!(capture.status().code, "draft_capture_captured");
    assert_eq!(next_texts(&s), vec!["before".to_owned()]);
    let kept = s.slot().join("superseded-v1");
    assert!(kept.join("failed.json").exists() && kept.join("index.json").exists());
    assert!(!s.slot().join("failed.json").exists());
}

#[test]
fn drafts_typed_after_the_windows_limit_failure_are_never_called_captured() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "before", b"abc", true);
    std::thread::sleep(std::time::Duration::from_millis(20));
    failed_v1(&s, "unreadable");
    std::thread::sleep(std::time::Duration::from_millis(20));
    // Typing continued after that recorded failure.
    let plain = drafts::DraftStore::open(s.next());
    assert_eq!(
        wait(|done| plain.save_durable("codex/sample/a", text("typed later"), done)),
        Ok(())
    );
    drop(plain);
    on_windows(true);
    let capture = s.capture();
    let gate = capture.prepare();
    on_windows(false);
    assert!(gate.writes && gate.import);
    assert_eq!(capture.status().code, "draft_capture_next_earlier_run");
    assert!(next_texts(&s).is_empty());
}

#[test]
fn every_other_failure_stays_permanent() {
    for (windows, version, reason) in [
        (false, 1, "unreadable"),
        (true, 1, "too_large"),
        (true, 2, "unreadable"),
    ] {
        let s = Scratch::new();
        draft(&s.next(), "codex/sample/a", "before", b"abc", true);
        let seed = seed_without_outcome(&s);
        let failed = Failed {
            version,
            capture_id: seed.capture_id,
            reason: reason.into(),
        };
        create_json(&s.slot().join("failed.json"), &failed).unwrap();
        on_windows(windows);
        let capture = s.capture();
        let gate = capture.prepare();
        on_windows(false);
        assert_eq!(
            gate,
            Gate {
                writes: true,
                import: false
            },
            "{windows} {version} {reason}"
        );
        assert!(s.slot().join("failed.json").exists());
        assert!(!s.slot().join("receipt.json").exists());
        assert!(!s.slot().join("superseded-v1").exists());
    }
}

#[test]
fn a_failure_written_now_is_never_superseded_on_windows() {
    let s = Scratch::new();
    // Next's root is a plain file: its listing fails, so the capture does.
    fs::write(s.next(), b"").unwrap();
    let capture = s.capture();
    assert!(!capture.prepare().import);
    assert_eq!(capture.status().code, "draft_capture_unreadable");
    let failed: Failed = read_json(&s.slot().join("failed.json")).unwrap().unwrap();
    assert_eq!(failed.version, 2);
    on_windows(true);
    let again = s.capture();
    let gate = again.prepare();
    on_windows(false);
    assert_eq!(
        gate,
        Gate {
            writes: true,
            import: false
        }
    );
    assert!(
        !s.slot().join("superseded-v1").exists(),
        "a version 2 failure stays"
    );
}

/// review-q on #1242: failed.json moves last, so a recovery interrupted
/// after some leaves moved is resumed on the next start.
#[test]
fn an_interrupted_supersede_is_resumed_on_the_next_start() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "before", b"abc", true);
    std::thread::sleep(std::time::Duration::from_millis(20));
    failed_v1(&s, "unreadable");
    // Interrupted after index.json moved, before failed.json did.
    let kept = s.slot().join("superseded-v1");
    drop(disk::Parents::open(&kept, true).unwrap());
    fs::rename(s.slot().join("index.json"), kept.join("index.json")).unwrap();
    on_windows(true);
    let capture = s.capture();
    let gate = capture.prepare();
    on_windows(false);
    assert_eq!(
        gate,
        Gate {
            writes: true,
            import: true
        }
    );
    assert_eq!(capture.status().code, "draft_capture_captured");
    assert!(kept.join("failed.json").exists() && kept.join("index.json").exists());
}

#[test]
fn a_supersede_that_cannot_finish_keeps_the_failure_and_its_record() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "before", b"abc", true);
    failed_v1(&s, "unreadable");
    // A review copy already holds index.json: the move stops there.
    let kept = s.slot().join("superseded-v1");
    drop(disk::Parents::open(&kept, true).unwrap());
    fs::write(kept.join("index.json"), b"earlier").unwrap();
    on_windows(true);
    let capture = s.capture();
    let gate = capture.prepare();
    on_windows(false);
    assert_eq!(
        gate,
        Gate {
            writes: true,
            import: false
        },
        "typing continues"
    );
    assert_eq!(capture.status().code, "draft_capture_unreadable");
    assert!(
        s.slot().join("failed.json").exists(),
        "failed.json moves last"
    );
    assert!(!kept.join("failed.json").exists());
}

/// review-q M2 on #1242: a receipt's parts must be exactly the container's
/// parts. An extra empty part, even with its true stamp, is refused.
#[test]
fn a_receipt_with_an_extra_empty_part_is_refused() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "text", b"abc", true);
    assert!(s.capture().prepare().import);
    let empty = disk::create(&s.slot().join("original-1.bin"), b"").unwrap();
    let mut receipt: Receipt = read_json(&s.slot().join("receipt.json")).unwrap().unwrap();
    receipt.more = vec![empty];
    fs::write(
        s.slot().join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    let seed: Seed = read_json(&s.slot().join("identity.json")).unwrap().unwrap();
    assert!(Snapshot::open(&s.slot(), &seed).is_err());
    let again = s.capture();
    assert_eq!(
        again.prepare(),
        Gate {
            writes: true,
            import: false
        }
    );
    assert_eq!(again.status().code, "draft_capture_changed");
}
