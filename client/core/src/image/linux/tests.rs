use super::*;
use std::os::unix::fs::PermissionsExt;

thread_local! {
    static AFTER_COPY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn after_copy() {
    if let Some(change) = AFTER_COPY.with_borrow_mut(Option::take) {
        change();
    }
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("zerus-sealed-image-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn script(&self) -> PathBuf {
        let path = self.0.join("helper");
        std::fs::write(&path, b"#!/bin/sh\nprintf 'original\\n'\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn command(pin: &Pin) -> Command {
    let mut command = Command::new(pin.executable());
    command.stdin(std::process::Stdio::null());
    pin.inherit(&mut command).unwrap();
    command
}

#[test]
fn original_native_child_observation_binds_the_snapshot_and_keeps_input_private() {
    let f = Fixture::new();
    let path = f.0.join("cat");
    std::fs::copy("/bin/cat", &path).unwrap();
    let pin = Pin::open(&path).unwrap();
    let mut cmd = command(&pin);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let identity = pin.observe_original_child(&mut child).unwrap();
    assert_eq!(identity.pid(), child.id());
    assert_eq!(identity.parent(), std::process::id());
    assert!(!identity.birth().is_empty());
    assert!(child.stdin.is_some());
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"original input\n")
        .unwrap();
    assert_eq!(
        child.wait_with_output().unwrap().stdout,
        b"original input\n"
    );

    // Same bytes in another sealed object do not identify the original image.
    let different = Pin::open(&path).unwrap();
    let mut wrong_cmd = command(&different);
    wrong_cmd.stdin(std::process::Stdio::piped());
    let mut wrong = wrong_cmd.spawn().unwrap();
    assert!(pin.observe_original_child(&mut wrong).is_err());
    assert!(wrong.stdin.is_none());
    assert!(wrong.wait().unwrap().success());
}

#[test]
fn script_extracted_input_or_a_reaped_child_cannot_supply_native_custody_identity() {
    let f = Fixture::new();
    let pin = Pin::open(&f.script()).unwrap();
    let mut cmd = command(&pin);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null());
    let mut child = cmd.spawn().unwrap();
    assert!(pin.observe_original_child(&mut child).is_err());
    assert!(child.stdin.is_none());
    child.wait().unwrap();
    let path = f.0.join("cat");
    std::fs::copy("/bin/cat", &path).unwrap();
    let pin = Pin::open(&path).unwrap();
    let mut cmd = command(&pin);
    cmd.stdin(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    drop(child.stdin.take());
    assert!(pin.observe_original_child(&mut child).is_err());
    child.wait().unwrap();
    assert!(pin.observe_original_child(&mut child).is_err());
}

#[test]
fn snapshot_has_every_seal_and_cannot_be_written_resized_or_unsealed() {
    let f = Fixture::new();
    let pin = Pin::open(&f.script()).unwrap();
    assert!(sealed(&pin.image));
    let fd = pin.image.as_raw_fd();
    let required = libc::F_SEAL_WRITE | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL;
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_GET_SEALS) } & required,
        required
    );
    assert_eq!(
        unsafe { libc::pwrite(fd, c"replacement".as_ptr().cast(), 11, 0) },
        -1
    );
    assert_eq!(unsafe { libc::ftruncate(fd, 0) }, -1);
    assert_eq!(unsafe { libc::ftruncate(fd, 1 << 20) }, -1);
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, 0) }, -1);
    assert!(sealed(&pin.image));
    assert!(
        !sealed(&pin.source),
        "ordinary files never qualify as sealed"
    );
}

#[test]
fn same_inode_script_rewrite_after_the_last_check_cannot_change_probed_code() {
    let f = Fixture::new();
    let path = f.script();
    let pin = Pin::open(&path).unwrap();
    let before = command(&pin).arg("--help").output().unwrap();
    assert!(before.status.success());
    let id = std::fs::metadata(&path).unwrap().ino();
    assert!(pin.current());
    let mut native = command(&pin);
    // Deliberately AFTER the last current/inherit check, same inode, real write.
    std::fs::write(&path, b"#!/bin/sh\nprintf 'replacement\\n'\nexit 23\n").unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), id);
    let after = native.output().unwrap();
    assert!(after.status.success(), "{:?}", after);
    assert_eq!(after.stdout, before.stdout);
    assert_eq!(after.stdout, b"original\n");
    assert!(!pin.current(), "a new request still refuses stale routing");
}

#[test]
fn same_inode_native_rewrite_after_the_last_check_executes_the_immutable_original() {
    let f = Fixture::new();
    let path = f.0.join("native");
    std::fs::copy("/bin/true", &path).unwrap();
    let pin = Pin::open(&path).unwrap();
    let id = std::fs::metadata(&path).unwrap().ino();
    assert!(pin.current());
    let mut native = command(&pin);
    std::fs::copy("/bin/false", &path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), id);
    assert!(native.status().unwrap().success());
    assert!(!pin.current());
}

#[test]
fn snapshots_refuse_empty_oversize_or_non_executable_originals() {
    let f = Fixture::new();
    let path = f.script();
    std::fs::write(&path, []).unwrap();
    assert!(Pin::open(&path).is_err());
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_IMAGE_BYTES + 1)
        .unwrap();
    assert!(Pin::open(&path).is_err());
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Pin::open(&path).is_err());
}

#[test]
fn fixed_kernel_self_selection_reads_the_actual_running_object() {
    let actual = File::open("/proc/self/exe").unwrap().metadata().unwrap();
    if actual.len() > MAX_IMAGE_BYTES {
        // Hosted debug binaries can exceed 500 MiB. They are deliberately
        // refused, not permission to increase the release helper's copy cap.
        assert!(matches!(Pin::open(Path::new("/proc/self/exe")), Err(e)
            if e.kind() == io::ErrorKind::InvalidData));
        return;
    }
    let pin = Pin::open(Path::new("/proc/self/exe")).unwrap();
    let retained = pin.source.metadata().unwrap();
    assert_eq!(
        (retained.dev(), retained.ino()),
        (actual.dev(), actual.ino())
    );
    assert!(sealed(&pin.image));
    assert!(pin.current());
}

#[test]
fn every_missing_seal_and_future_write_only_is_refused_by_the_verifier() {
    for missing in [
        libc::F_SEAL_WRITE,
        libc::F_SEAL_SHRINK,
        libc::F_SEAL_GROW,
        libc::F_SEAL_SEAL,
    ] {
        let fd = unsafe {
            libc::memfd_create(
                c"zerus-incomplete-seals".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        };
        assert!(fd >= 0);
        // SAFETY: the successful call returned one new owned fd.
        let file = unsafe { File::from_raw_fd(fd) };
        let required =
            libc::F_SEAL_WRITE | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL;
        let incomplete = (required & !missing) | libc::F_SEAL_FUTURE_WRITE;
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, incomplete) }, 0);
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GET_SEALS) } & missing, 0);
        assert!(
            !sealed(&file),
            "the original verifier must require seal {missing}"
        );
    }
}

#[test]
fn same_size_source_rewrite_after_copy_refuses_publication_of_the_snapshot() {
    use std::os::unix::fs::FileExt;
    let f = Fixture::new();
    let path = f.script();
    let mut source = open(&path).unwrap();
    let expected = stamp(&source).unwrap();
    let writer = OpenOptions::new().write(true).open(&path).unwrap();
    let size = expected.bytes;
    let inode = expected.inode;
    AFTER_COPY.with_borrow_mut(|slot| {
        *slot = Some(Box::new(move || {
            // The real original file changes only after the full copy has read it.
            // No injected stamp/result or changed-length guard supplies refusal.
            writer.write_all_at(&vec![b'x'; size as usize], 0).unwrap();
            // Make the real version change deterministic on coarse-timestamp
            // filesystems too, without changing length or substituting a stamp.
            writer
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1))
                .unwrap();
            assert_eq!(writer.metadata().unwrap().len(), size);
            assert_eq!(writer.metadata().unwrap().ino(), inode);
        }))
    });
    let result = snapshot(&mut source, &expected);
    assert!(
        stamp(&source).unwrap() != expected,
        "control must actually change the source version"
    );
    assert!(matches!(result, Err(e) if e.kind() == io::ErrorKind::PermissionDenied));
}
