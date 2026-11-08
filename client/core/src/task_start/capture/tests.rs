use super::*;
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::fs::{symlink, DirBuilderExt, PermissionsExt},
    sync::mpsc,
    time::Duration,
};

// A parallel fixture's fork may retain a CLOEXEC lock description until exec.
// Busy refuses before any write. Retry only that refusal, never an offer or an
// uncertain publication; keep every other result and one fixed wait bound.
// Shared by every journal fixture (zerus-p1l5z).
#[track_caller]
pub(super) fn retry_busy<T>(
    mut operation: impl FnMut() -> Result<T, journal::Error>,
) -> Result<T, journal::Error> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        match operation() {
            Err(journal::Error::Busy) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            result => return result,
        }
    }
}

pub(super) struct Fixture {
    pub(super) base: PathBuf,
    pub(super) store: PathBuf,
    pub(super) file: PathBuf,
    pub(super) marker: PathBuf,
    pub(super) intent: wire::Intent,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let base = std::env::temp_dir().join(format!("hgs-task-capture-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        let store = base.join("data");
        for folder in [
            store.clone(),
            store.join("task-start"),
            store.join("task-start/task-delivery"),
        ] {
            fs::DirBuilder::new().mode(0o700).create(folder).unwrap();
        }
        let intent = wire::test_intent();
        let file = outbox::path(
            &store.join("task-start"),
            &intent.binding.key.project,
            &intent.binding.key.task,
        );
        let marker = file.with_extension("marker");
        let fixture = Self {
            base,
            store,
            file,
            marker,
            intent,
        };
        fixture.write();
        fixture
    }
    pub(super) fn write(&self) {
        crate::fsx::write_private_atomic(&self.file, self.intent.original.as_bytes(), true)
            .unwrap();
        crate::fsx::write_private_atomic(
            &self.marker,
            self.intent.binding.key.request_id.as_bytes(),
            true,
        )
        .unwrap();
    }
    pub(super) fn change(&mut self, change: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_str(&self.intent.original).unwrap();
        change(&mut value);
        // Keep noncanonical whitespace as part of the actual native source.
        self.intent.original = format!(" \n{}\n ", serde_json::to_string_pretty(&value).unwrap());
        self.intent.binding.ui_snapshot_sha256 = outbox::snapshot(&value);
        self.intent.binding.source_record_sha256 = sha256(self.intent.original.as_bytes());
        self.write();
    }
    pub(super) fn capture(&self) -> Result<Captured, &'static str> {
        Captured::read(
            &self.store,
            &self.intent.binding.key.project,
            &self.intent.binding.key.task,
            &self.intent.binding.key.request_id,
            &self.intent.binding.ui_snapshot_sha256,
            self.intent.scope.clone(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn exact_native_bytes_marker_and_three_hashes_are_retained_without_a_write() {
    let mut f = Fixture::new();
    f.change(|v| v["opaque_future"] = json!({"kept":[1,"private fixture",null]}));
    let before = fs::read(&f.file).unwrap();
    let marker = fs::read(&f.marker).unwrap();
    let source = f.capture().unwrap();
    assert!(source.current());
    let intent = source.intent(&f.intent.reconcile_id).unwrap();
    assert_eq!(intent.original.as_bytes(), before);
    assert_eq!(intent.binding, f.intent.binding);
    assert_ne!(
        intent.binding.source_record_sha256,
        intent.binding.ui_snapshot_sha256
    );
    assert_ne!(
        intent.binding.source_record_sha256,
        intent.binding.task_start_reconcile_input_v1_sha256
    );
    assert_eq!(
        intent.verify().unwrap().original.binding(),
        &f.intent.binding
    );
    assert!(source.intent(&f.intent.binding.key.request_id).is_err());
    assert_eq!(fs::read(&f.file).unwrap(), before);
    assert_eq!(fs::read(&f.marker).unwrap(), marker);
    let saved = serde_json::to_string(source.version()).unwrap();
    assert!(!saved.contains("private fixture"));
    assert_eq!(
        serde_json::from_str::<Version>(&saved).unwrap(),
        *source.version()
    );
}

#[test]
fn replaced_record_or_marker_with_identical_bytes_is_not_the_original_object() {
    for marker in [false, true] {
        let f = Fixture::new();
        let source = f.capture().unwrap();
        let path = if marker { &f.marker } else { &f.file };
        let bytes = fs::read(path).unwrap();
        crate::fsx::write_private_atomic(path, &bytes, true).unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert!(
            !source.current(),
            "identical bytes cannot replace native custody"
        );
        assert!(source.intent(&f.intent.reconcile_id).is_err());
        assert_ne!(source.version(), f.capture().unwrap().version());
    }
}

#[test]
fn current_refuses_in_place_bytes_privacy_and_single_link_changes() {
    for mode in ["bytes", "private", "link"] {
        let f = Fixture::new();
        let source = f.capture().unwrap();
        match mode {
            "bytes" => OpenOptions::new()
                .write(true)
                .open(&f.file)
                .unwrap()
                .write_all(b"!")
                .unwrap(),
            "private" => fs::set_permissions(&f.file, fs::Permissions::from_mode(0o644)).unwrap(),
            "link" => fs::hard_link(&f.file, f.base.join("alias")).unwrap(),
            _ => unreachable!(),
        }
        assert!(!source.current(), "{mode}");
        assert!(f.capture().is_err(), "{mode}");
    }
}

#[test]
fn both_private_files_refuse_symlinks_and_hardlinks_before_reading() {
    for marker in [false, true] {
        for link in [false, true] {
            let f = Fixture::new();
            let path = if marker { &f.marker } else { &f.file };
            let target = f.base.join("target");
            fs::rename(path, &target).unwrap();
            if link {
                fs::hard_link(&target, path).unwrap();
            } else {
                symlink(&target, path).unwrap();
            }
            let original = fs::read(&target).unwrap();
            assert!(f.capture().is_err(), "marker={marker}, hardlink={link}");
            assert_eq!(fs::read(&target).unwrap(), original);
        }
    }
}

#[test]
fn no_follow_nonblocking_and_cloexec_flags_compose_on_the_actual_open() {
    let f = Fixture::new();
    let source = f.capture().unwrap();
    for file in [&source.record, &source.marker] {
        // SAFETY: fcntl only queries each retained live descriptor.
        let status = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        let descriptor = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
        assert!(status >= 0 && status & libc::O_NONBLOCK != 0);
        assert!(descriptor >= 0 && descriptor & libc::FD_CLOEXEC != 0);
    }
}

#[test]
fn fifo_record_and_marker_never_need_a_writer_to_finish_capture() {
    for marker in [false, true] {
        let f = Fixture::new();
        let path = if marker { &f.marker } else { &f.file };
        fs::remove_file(path).unwrap();
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: a new path in this exclusive fixture, no existing object.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let rescue_path = path.clone();
        let (done, cancelled) = mpsc::channel();
        // A broken blocking open must still let the suite join its own rescue.
        // This is a fail-safe, never a source of passing readiness evidence.
        let rescue = std::thread::spawn(move || {
            if cancelled.recv_timeout(Duration::from_secs(2)).is_ok() {
                return false;
            }
            if let Ok(mut writer) = OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(rescue_path)
            {
                let _ = writer.write_all(b"refused fixture");
            }
            true
        });
        let result = f.capture();
        let _ = done.send(());
        let rescued = rescue.join().unwrap();
        assert!(result.is_err());
        assert!(
            !rescued,
            "capture blocked on a FIFO until its rescue writer"
        );
    }
}

#[test]
fn parent_replacement_symlink_and_permissions_refuse_without_adopting_the_copy() {
    for which in ["store", "root", "folder", "symlink", "permissions"] {
        let f = Fixture::new();
        let source = f.capture().unwrap();
        let path = match which {
            "store" => f.store.clone(),
            "root" | "symlink" | "permissions" => f.store.join("task-start"),
            "folder" => f.store.join("task-start/task-delivery"),
            _ => unreachable!(),
        };
        if which == "permissions" {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        } else {
            let old = f.base.join("old");
            fs::rename(&path, &old).unwrap();
            if which == "symlink" {
                symlink(&old, &path).unwrap();
            } else {
                fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            }
        }
        assert!(!source.current(), "{which}");
        assert!(source.intent(&f.intent.reconcile_id).is_err(), "{which}");
    }
}

#[test]
fn selected_managed_alias_may_resolve_once_but_cannot_retarget_the_source() {
    let f = Fixture::new();
    let alias = f.base.join("selected");
    symlink(&f.store, &alias).unwrap();
    let source = Captured::read(
        &alias,
        &f.intent.binding.key.project,
        &f.intent.binding.key.task,
        &f.intent.binding.key.request_id,
        &f.intent.binding.ui_snapshot_sha256,
        f.intent.scope.clone(),
    )
    .unwrap();
    assert!(source.current());
    fs::remove_file(&alias).unwrap();
    symlink(&f.base, &alias).unwrap();
    assert!(!source.current());
    assert!(source.intent(&f.intent.reconcile_id).is_err());
    assert!(
        f.capture().unwrap().current(),
        "the original managed source stays intact"
    );
}

#[test]
fn missing_or_mismatched_original_is_never_created_repaired_or_backfilled() {
    for mode in ["record", "marker", "id", "snapshot", "legacy", "terminal"] {
        let mut f = Fixture::new();
        match mode {
            "record" => fs::remove_file(&f.file).unwrap(),
            "marker" => fs::remove_file(&f.marker).unwrap(),
            "id" => crate::fsx::write_private_atomic(&f.marker, b"other", true).unwrap(),
            "snapshot" => f.intent.binding.ui_snapshot_sha256 = "f".repeat(64),
            "legacy" => f.change(|v| {
                v["intent"]
                    .as_object_mut()
                    .unwrap()
                    .remove("retained_input");
            }),
            "terminal" => f.change(|v| v["phase"] = json!("started")),
            _ => unreachable!(),
        }
        let before = (fs::read(&f.file).ok(), fs::read(&f.marker).ok());
        assert!(f.capture().is_err(), "{mode}");
        assert_eq!((fs::read(&f.file).ok(), fs::read(&f.marker).ok()), before);
    }
    let f = Fixture::new();
    let missing = f.store.join("missing");
    assert!(Captured::read(
        &missing,
        "example",
        &f.intent.binding.key.task,
        &f.intent.binding.key.request_id,
        &f.intent.binding.ui_snapshot_sha256,
        f.intent.scope.clone()
    )
    .is_err());
    assert!(!missing.exists());
}

#[test]
fn complete_original_limit_and_duplicate_field_refusals_preserve_the_source() {
    let mut f = Fixture::new();
    f.change(|v| v["padding"] = json!(""));
    let length = f.intent.original.len();
    f.change(|v| v["padding"] = json!("x".repeat(wire::SOURCE_BYTES - length)));
    assert_eq!(f.intent.original.len(), wire::SOURCE_BYTES);
    assert!(f.capture().is_ok());
    f.intent.original.push(' ');
    f.write();
    assert!(f.capture().is_err());
    assert_eq!(fs::read(&f.file).unwrap().len(), wire::SOURCE_BYTES + 1);
    for input in [false, true] {
        let mut f = Fixture::new();
        if input {
            f.change(|v| {
                let body = v["intent"]["retained_input"].as_str().unwrap();
                v["intent"]["retained_input"] = json!(body.replacen('{', "{\"schema\":1,", 1));
            });
        } else {
            f.intent.original = f.intent.original.replacen('{', "{\"schema\":1,", 1);
            let value: Value = serde_json::from_str(&f.intent.original).unwrap();
            f.intent.binding.ui_snapshot_sha256 = outbox::snapshot(&value);
            f.write();
        }
        let before = fs::read(&f.file).unwrap();
        assert!(f.capture().is_err(), "duplicate in input={input}");
        assert_eq!(fs::read(&f.file).unwrap(), before);
    }
}
