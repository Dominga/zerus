use super::*;
use serde_json::json;
use std::{fs, time::Instant};
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("zerus-native-local-read-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self {
            root: root.canonicalize().unwrap(),
        }
    }
    fn path(&self) -> PathBuf {
        self.root.join("attention-marks.json")
    }
    fn capture(&self) -> Arc<Capture> {
        Capture::local_reads(self.root.clone())
    }
    fn write(&self, bytes: &[u8]) {
        zerus_client_core::fsx::write_private_durable(&self.path(), bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn key() -> String {
    json!(["", "codex", "conversation"]).to_string()
}
fn raw() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({"marks":[[key(),{"review":false}],[key(),{"review":true}]],"replies":[],"private":"original"})).unwrap()
}
fn until(mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while !done() {
        assert!(Instant::now() < end, "native source did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn the_original_writer_archives_before_load_and_uses_that_snapshot_after_live_writes() {
    let f = Fixture::new();
    f.write(&raw());
    let capture = f.capture();
    let owner = Attention::new_with_capture(f.path(), Some(capture.clone()));
    until(|| !capture.status().paused && owner.shared.lock().marks.is_some());
    assert_eq!(
        fs::read(f.root.join("local-read-preload-v1/original.bin")).unwrap(),
        raw()
    );
    owner
        .try_update(Default::default(), [(key(), Some("later".into()))].into())
        .unwrap();
    assert!(owner.flush_within(Duration::from_secs(3)));
    assert_ne!(fs::read(f.path()).unwrap(), raw());
    let zerus_client_core::migration::preload::Prepared::Snapshot(original) =
        capture.retained().unwrap()
    else {
        panic!("missing")
    };
    assert_eq!(original.bytes().unwrap(), raw());
}
#[test]
fn a_failed_capture_answers_presentation_refuses_updates_and_retries_in_the_same_owner() {
    let f = Fixture::new();
    f.write(&raw());
    let slot = f.root.join("local-read-preload-v1");
    f.write(&raw());
    fs::write(&slot, b"blocked").unwrap();
    let capture = f.capture();
    let owner = Attention::new_with_capture(f.path(), Some(capture.clone()));
    until(|| capture.status().code == "local_read_source_private_slot");
    assert!(owner.snapshot().marks.is_empty());
    assert!(owner.shared.snapshot_checked().is_err());
    assert!(owner
        .try_update(Default::default(), [(key(), Some("unsaved".into()))].into())
        .is_err());
    assert!(!owner.flush_within(Duration::ZERO));
    assert_eq!(fs::read(f.path()).unwrap(), raw());
    fs::remove_file(&slot).unwrap();
    capture.request_retry();
    until(|| !owner.shared.lock().source_paused);
    assert_eq!(owner.snapshot().marks.len(), 1);
    owner
        .try_update(Default::default(), [(key(), Some("saved".into()))].into())
        .unwrap();
    assert!(owner.flush_within(Duration::from_secs(3)));
    assert_eq!(fs::read(slot.join("original.bin")).unwrap(), raw());
}
#[test]
fn a_thread_spawn_failure_never_falls_back_to_loading_or_writing_the_source() {
    let f = Fixture::new();
    f.write(&raw());
    let capture = f.capture();
    let owner = Attention::new_with_spawn(f.path(), Some(capture.clone()), |_| {
        Err(std::io::ErrorKind::Other.into())
    });
    assert_eq!(capture.status().code, "local_read_source_owner_unavailable");
    assert!(owner.snapshot().marks.is_empty());
    assert!(owner
        .shared
        .lock()
        .marks
        .as_ref()
        .unwrap()
        .snapshot()
        .0
        .is_empty());
    assert!(!owner.flush_within(Duration::ZERO));
    assert!(owner
        .try_update(
            Default::default(),
            [(key(), Some("not saved".into()))].into()
        )
        .is_err());
    drop(owner);
    assert_eq!(fs::read(f.path()).unwrap(), raw());
    assert!(!f.root.join("local-read-preload-v1").exists());
}

#[test]
fn an_unsaved_dirty_write_is_visible_and_retry_keeps_the_original_owner_and_leaf() {
    let f = Fixture::new();
    f.write(&raw());
    let capture = f.capture();
    let shared = Arc::new(AttentionShared {
        listener: MarksListener::default(),
        capture: Some(capture.clone()),
        state: Mutex::new(AttentionState {
            source_paused: true,
            ..AttentionState::default()
        }),
        signal: Condvar::new(),
    });
    let weak = Arc::downgrade(&shared);
    capture.retry_hook(Arc::new(move || {
        if let Some(shared) = weak.upgrade() {
            let _state = shared.lock();
            shared.signal.notify_all();
        }
    }));
    let owner = Attention {
        shared: shared.clone(),
        listener: shared.listener.clone(),
    };
    let (entered, rx) = std::sync::mpsc::sync_channel(1);
    let (release, go) = std::sync::mpsc::sync_channel(1);
    let file = f.path();
    let worker = shared.clone();
    let cap = capture.clone();
    let join = std::thread::spawn(move || {
        let mut first = true;
        worker.write_captured_with(file, &cap, |live, contents| {
            if std::mem::take(&mut first) {
                entered.send(()).unwrap();
                let _ = go.recv_timeout(Duration::from_secs(3));
                Err(SourceError::SourceUnreadable)
            } else {
                live.write(contents)
            }
        });
    });
    until(|| !shared.lock().source_paused);
    owner
        .try_update(Default::default(), [(key(), Some("first".into()))].into())
        .unwrap();
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    owner
        .try_update(Default::default(), [(key(), Some("latest".into()))].into())
        .unwrap();
    release.send(()).unwrap();
    until(|| capture.status().code == "local_read_source_writer_unavailable");
    assert!(!owner.flush_within(Duration::ZERO));
    assert!(shared.lock().dirty);
    assert_eq!(fs::read(f.path()).unwrap(), raw());
    // A known model remains editable while FILE writes are paused, so an
    // oversized/unsaved change can be corrected before the explicit Retry.
    owner
        .try_update(
            Default::default(),
            [(key(), Some("corrected".into()))].into(),
        )
        .unwrap();
    assert_eq!(fs::read(f.path()).unwrap(), raw());
    capture.request_retry();
    assert!(owner.flush_within(Duration::from_secs(3)));
    assert_eq!(owner.snapshot().replies[&key()], "corrected");
    assert_eq!(
        fs::read(f.root.join("local-read-preload-v1/original.bin")).unwrap(),
        raw()
    );
    drop(owner);
    join.join().unwrap();
}
#[test]
fn quit_during_capture_never_performs_a_late_load_or_write() {
    let f = Fixture::new();
    f.write(&raw());
    let capture = f.capture();
    let (entered, rx) = std::sync::mpsc::sync_channel(1);
    let (release, go) = std::sync::mpsc::sync_channel(1);
    let go = Mutex::new(go);
    capture.hook(Arc::new(move || {
        entered.send(()).unwrap();
        let _ = go.lock().unwrap().recv_timeout(Duration::from_secs(3));
    }));
    let owner = Attention::new_with_capture(f.path(), Some(capture));
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let shared = owner.shared.clone();
    assert!(shared.lock().marks.is_none());
    drop(owner);
    release.send(()).unwrap();
    until(|| !shared.lock().writing);
    assert!(shared.lock().marks.is_none());
    assert_eq!(fs::read(f.path()).unwrap(), raw());
}

#[test]
fn the_ready_signal_is_sent_only_after_the_actual_model_can_be_read() {
    let f = Fixture::new();
    f.write(&raw());
    let capture = f.capture();
    let held = Mutex::new(None);
    let owner = Attention::new_with_spawn(f.path(), Some(capture.clone()), |job| {
        *held.lock().unwrap() = Some(job);
        Ok(())
    });
    let weak = Arc::downgrade(&owner.shared);
    let source = Arc::downgrade(&capture);
    let (ready, rx) = std::sync::mpsc::sync_channel(1);
    capture.hook(Arc::new(move || {
        if source.upgrade().unwrap().status().view_ready == Some(true) {
            let view = weak
                .upgrade()
                .unwrap()
                .snapshot_checked()
                .expect("ready must expose the actual model");
            ready.send(view.marks.len()).unwrap();
        }
    }));
    let job = held.lock().unwrap().take().unwrap();
    let join = std::thread::spawn(job);
    assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
    drop(owner);
    join.join().unwrap();
}
