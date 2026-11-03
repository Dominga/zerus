use super::*;
use serde_json::json;
use std::{fs, path::PathBuf};
struct Fixture {
    root: PathBuf,
    jobs: Arc<Mutex<Vec<Job>>>,
    session: NewSession,
    capture: Arc<Capture>,
}
fn raw() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({"a".repeat(64): {"state":"ok","sampled_at":1,"stale":true,"worktrees":[],"extension":"unchanged original"}})).unwrap()
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("zerus-newsession-preload-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        zerus_client_core::fsx::write_private_durable(
            &root.join("worktree-snapshots.json"),
            &raw(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(root.join("hgs"),"#!/usr/bin/env python3\nimport json\nprint(json.dumps({'state':'ok','path':'/srv/example','common_dir':'/srv/example/.git','worktrees':[]}))\n").unwrap();
            fs::set_permissions(root.join("hgs"), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let capture = Capture::worktree_cache(root.clone());
        let jobs = Arc::new(Mutex::new(Vec::<Job>::new()));
        let held = jobs.clone();
        let session = NewSession::new_captured(
            root.join("hgs"),
            true,
            Some(root.join("worktree-snapshots.json")),
            Arc::new(|| 1000.),
            capture.clone(),
            Arc::new(move |job| {
                let mut jobs = lock(&held);
                jobs.push(job);
                assert_eq!(jobs.len(), 1, "only one original cache job");
            }),
        );
        Self {
            root,
            jobs,
            session,
            capture,
        }
    }
    fn run(&self) {
        let job = lock(&self.jobs).pop().expect("queued job");
        job();
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.root.join("worktree-snapshots.json")).unwrap()
    }
    fn archive(&self) -> Vec<u8> {
        fs::read(self.root.join("worktree-cache-preload-v1/original.bin")).unwrap()
    }
    #[cfg(unix)]
    fn restart(&mut self) {
        self.session.close_capture();
        self.capture = Capture::worktree_cache(self.root.clone());
        let jobs = self.jobs.clone();
        self.session = NewSession::new_captured(
            self.root.join("hgs"),
            true,
            Some(self.root.join("worktree-snapshots.json")),
            Arc::new(|| 1000.),
            self.capture.clone(),
            Arc::new(move |job| {
                let mut jobs = lock(&jobs);
                jobs.push(job);
                assert_eq!(jobs.len(), 1);
            }),
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.session.close_capture();
        lock(&self.jobs).clear();
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn setup_never_reads_or_normalizes_before_the_original_background_capture() {
    let f = Fixture::new();
    assert!(!f.root.join("worktree-cache-preload-v1").exists());
    assert_eq!(f.bytes(), raw());
    assert_eq!(f.capture.status().code, "worktree_cache_source_waiting");
    assert_eq!(
        *lock(&f.session.0.catalogs),
        Catalogs::new(&f.session.0.hgs.to_string_lossy(), None)
    );
    f.run();
    assert_eq!(f.archive(), raw());
    assert!(!f.capture.status().paused);
    assert!(lock(&f.jobs).is_empty());
    assert_eq!(
        lock(&f.session.0.catalogs).saved(),
        serde_json::from_slice::<Value>(&raw()).unwrap()
    );
    let id = f.capture.status().capture_id.clone();
    f.capture.request_retry();
    f.run();
    assert_eq!(f.capture.status().capture_id, id);
    assert_eq!(f.archive(), raw());
    assert_eq!(f.bytes(), raw());
}
#[cfg(unix)]
#[test]
fn a_fresh_cli_reply_during_capture_is_not_overwritten_and_only_then_can_persist() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new();
    let reply = f
        .session
        .catalog(&ComputerId::local(), "/srv/example", true)
        .unwrap();
    assert_eq!(reply["state"], "ok");
    let fresh = lock(&f.session.0.catalogs).clone();
    assert_eq!(f.bytes(), raw());
    assert_eq!(lock(&f.jobs).len(), 1);
    f.run();
    assert_eq!(*lock(&f.session.0.catalogs), fresh);
    assert_eq!(f.archive(), raw());
    assert_ne!(f.bytes(), raw());
    assert_eq!(
        serde_json::from_slice::<Value>(&f.bytes()).unwrap(),
        fresh.saved()
    );
    use std::os::unix::fs::MetadataExt;
    assert_eq!(
        fs::metadata(f.root.join("worktree-snapshots.json"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
}
#[cfg(unix)]
#[test]
fn a_saved_receipt_never_permits_normalizing_bad_current_cache_and_fresh_queries_still_answer() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let mut f = Fixture::new();
    f.run();
    let id = f.capture.status().capture_id.clone();
    let over = serde_json::to_vec(
        &(0..65)
            .map(|i| {
                (
                    format!("{i:064x}"),
                    json!({"state":"ok","sampled_at":1,"worktrees":[]}),
                )
            })
            .collect::<serde_json::Map<_, _>>(),
    )
    .unwrap();
    for bytes in [b"{bad".to_vec(), b"[]".to_vec(), over] {
        zerus_client_core::fsx::write_private_durable(
            &f.root.join("worktree-snapshots.json"),
            &bytes,
        )
        .unwrap();
        f.restart();
        f.run();
        assert!(f.capture.status().paused);
        assert_eq!(f.capture.status().capture_id, id);
        assert_eq!(f.bytes(), bytes);
        assert_eq!(f.archive(), raw());
        assert_eq!(
            f.session
                .catalog(&ComputerId::local(), "/srv/example", true)
                .unwrap()["state"],
            "ok"
        );
        assert!(lock(&f.jobs).is_empty());
        assert_eq!(f.bytes(), bytes);
        f.capture.request_retry();
        f.run();
        assert!(f.capture.status().paused);
        assert_eq!(f.bytes(), bytes);
        assert_eq!(f.archive(), raw());
    }
}
#[test]
fn replacement_refuses_file_writes_and_retry_keeps_the_same_owner_and_original() {
    let f = Fixture::new();
    f.run();
    let id = f.capture.status().capture_id.clone();
    zerus_client_core::fsx::write_private_durable(&f.root.join("worktree-snapshots.json"), &raw())
        .unwrap();
    f.session.save();
    f.run();
    assert!(f.capture.status().paused);
    assert_eq!(f.capture.status().code, "worktree_cache_source_changed");
    assert!(
        lock(&f.session.0.capture.as_ref().unwrap().state).dirty,
        "failed write must remain unsaved"
    );
    assert!(lock(&f.jobs).is_empty());
    for _ in 0..8 {
        f.capture.request_retry();
    }
    assert_eq!(lock(&f.jobs).len(), 1);
    f.run();
    assert!(lock(&f.jobs).is_empty());
    assert_eq!(f.capture.status().capture_id, id);
    assert_eq!(f.archive(), raw());
    assert_eq!(f.bytes(), raw());
}
#[test]
fn quit_during_capture_never_installs_a_late_model_or_writes_a_pending_cache() {
    let f = Fixture::new();
    let weak = Arc::downgrade(&f.session.0);
    f.capture.hook(Arc::new(move || {
        if let Some(shared) = weak.upgrade() {
            NewSession(shared).close_capture();
        }
    }));
    f.session.save();
    f.run();
    assert_eq!(f.archive(), raw());
    assert_eq!(f.bytes(), raw());
    assert_eq!(
        *lock(&f.session.0.catalogs),
        Catalogs::new(&f.session.0.hgs.to_string_lossy(), None)
    );
    assert!(lock(&f.jobs).is_empty());
    f.capture.request_retry();
    f.session.save();
    assert!(lock(&f.jobs).is_empty());
}
#[test]
fn closing_before_the_queued_job_does_no_source_io_and_spawn_failure_has_no_fallback() {
    let f = Fixture::new();
    f.session.close_capture();
    f.run();
    assert!(!f.root.join("worktree-cache-preload-v1").exists());
    assert_eq!(f.bytes(), raw());
    let capture = Capture::worktree_cache(f.root.clone());
    let failed = NewSession::new_captured(
        f.root.join("hgs"),
        true,
        Some(f.root.join("worktree-snapshots.json")),
        Arc::new(|| 1000.),
        capture.clone(),
        Arc::new(|_| panic!("synthetic executor refusal")),
    );
    assert_eq!(
        capture.status().code,
        "worktree_cache_source_owner_unavailable"
    );
    assert!(capture.status().paused);
    capture.request_retry();
    failed.save();
    assert_eq!(f.bytes(), raw());
    assert!(!f.root.join("worktree-cache-preload-v1").exists());
}

#[cfg(unix)]
#[test]
fn an_initial_capture_failure_does_not_invent_a_dirty_empty_cache_or_rewrite_on_retry() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let slot = f.root.join("worktree-cache-preload-v1");
    fs::create_dir(&slot).unwrap();
    fs::set_permissions(&slot, fs::Permissions::from_mode(0o755)).unwrap();
    f.run();
    assert!(f.capture.status().paused);
    assert!(!lock(&f.session.0.capture.as_ref().unwrap().state).dirty);
    assert_eq!(f.bytes(), raw());
    // Repair the PRIVATE archive folder only, never chmod the source.
    fs::set_permissions(&slot, fs::Permissions::from_mode(0o700)).unwrap();
    f.capture.request_retry();
    f.run();
    assert!(!f.capture.status().paused);
    assert_eq!(f.archive(), raw());
    assert_eq!(
        f.bytes(),
        raw(),
        "Retry must not save a fabricated dirty model"
    );
    assert!(!lock(&f.session.0.capture.as_ref().unwrap().state).dirty);
    assert!(lock(&f.jobs).is_empty());
}
