use super::*;
use std::os::unix::fs::PermissionsExt;

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
    let pin = Pin::open(Path::new("/proc/self/exe")).unwrap();
    let actual = File::open("/proc/self/exe").unwrap().metadata().unwrap();
    let retained = pin.source.metadata().unwrap();
    assert_eq!(
        (retained.dev(), retained.ino()),
        (actual.dev(), actual.ino())
    );
    assert!(sealed(&pin.image));
    assert!(pin.current());
}
