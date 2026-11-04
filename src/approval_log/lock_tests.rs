//! Probe from another process: a same-process fcntl cannot see its own locks.
use std::{os::fd::AsRawFd, path::Path, process::Command, time::Duration};

pub(crate) fn assert_lock(path: &Path, held: bool) {
    let output = crate::platform::output_until(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "approval_log::lock_tests::probe_child",
                "--ignored",
                "--nocapture",
            ])
            .env_remove("HGS_SESSION")
            .env_remove("HGS_RUN_ID")
            .env("ZERUS_TEST_SQLITE_LOCK_PATH", path)
            .env(
                "ZERUS_TEST_SQLITE_LOCK_OWNER",
                if held { std::process::id() } else { 0 }.to_string(),
            ),
        Duration::from_secs(5),
    )
    .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("SQLITE_LOCK_PROBED"),
        "original probe did not run"
    );
}

#[test]
#[ignore = "isolated child of an original SQLite lock control"]
fn probe_child() {
    let Some(path) = std::env::var_os("ZERUS_TEST_SQLITE_LOCK_PATH") else {
        return;
    };
    let owner: i32 = std::env::var("ZERUS_TEST_SQLITE_LOCK_OWNER")
        .unwrap()
        .parse()
        .unwrap();
    let file = std::fs::File::open(path).unwrap();
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as _;
    lock.l_whence = libc::SEEK_SET as _;
    lock.l_start = 0x40000000;
    lock.l_len = 512;
    assert_eq!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut lock) },
        0
    );
    if owner == 0 {
        assert_eq!(
            i64::from(lock.l_type),
            i64::from(libc::F_UNLCK),
            "original SQLite lock was not released"
        );
    } else {
        assert_ne!(
            i64::from(lock.l_type),
            i64::from(libc::F_UNLCK),
            "live SQLite connection lost its shared lock"
        );
        assert_eq!(lock.l_pid, owner, "lock belongs to a different process");
    }
    println!("SQLITE_LOCK_PROBED");
}
