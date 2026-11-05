// The cases of QLockFile's format and stale rule (qtbase 6.12) a shared
// folder relies on.
use super::*;

/// A folder of the test's own, removed however the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("zerus-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A lock left by a process that is gone.
fn dead_lock(path: &Path) {
    fs::write(
        path,
        contents(i64::from(i32::MAX - 7), "hgs-tray", &this_machine()),
    )
    .unwrap();
}

fn here() -> Machine {
    Machine {
        hostname: "arch".into(),
        hostid: b"0123456789abcdef0123456789abcdef".to_vec(),
        bootid: b"aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa".to_vec(),
    }
}

#[test]
fn a_five_line_qt_lock_reads_as_qt_reads_it() {
    let qt = b"4242\nhgs-tray\narch\n0123456789abcdef0123456789abcdef\naaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa\n";
    let info = parse(qt).unwrap();
    assert_eq!(
        info,
        LockInfo {
            pid: 4242,
            appname: "hgs-tray".into(),
            hostname: "arch".into(),
            hostid: b"0123456789abcdef0123456789abcdef".to_vec(),
            bootid: b"aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa".to_vec(),
        }
    );
    // Ours is byte for byte what Qt writes.
    assert_eq!(contents(4242, "hgs-tray", &here()), qt.to_vec());
    // A pre-5.10 three-line lock, and no PID.
    let old = parse(b"7\napp\nhost\n").unwrap();
    assert!(old.hostid.is_empty() && old.bootid.is_empty());
    assert!(parse(b"0\napp\nhost\n").is_none());
    assert!(parse(b"x\n").is_none());
    assert!(parse(b"").is_none());
}

#[test]
fn our_own_lock_is_live_by_qts_rule() {
    let machine = this_machine();
    let pid = i64::from(std::process::id());
    let info = parse(&contents(pid, &own_name(), &machine)).unwrap();
    assert!(!apparently_stale(Some(&info), &machine, process_running));
}

#[test]
fn a_reused_pid_with_another_name_is_stale() {
    let pid = i64::from(std::process::id());
    let name = own_name();
    assert!(process_running(pid, &name));
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        assert!(!process_running(pid, "some-other-program"));
        let info = LockInfo {
            pid,
            appname: "some-other-program".into(),
            ..parse(&contents(pid, &name, &this_machine())).unwrap()
        };
        assert!(apparently_stale(
            Some(&info),
            &this_machine(),
            process_running
        ));
    }
}

#[test]
fn a_lock_from_an_earlier_boot_is_stale() {
    let mut info = parse(&contents(1, "hgs-tray", &here())).unwrap();
    let alive = |_: i64, _: &str| true;
    assert!(!apparently_stale(Some(&info), &here(), alive));
    info.bootid = b"bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb".to_vec();
    assert!(apparently_stale(Some(&info), &here(), alive));
}

#[test]
fn another_machine_or_an_unreadable_lock_is_never_stale() {
    let gone = |_: i64, _: &str| false;
    let mut info = parse(&contents(1, "hgs-tray", &here())).unwrap();
    assert!(apparently_stale(Some(&info), &here(), gone));
    // The host ID decides over the name when both are known.
    info.hostid = b"ffffffffffffffffffffffffffffffff".to_vec();
    assert!(!apparently_stale(Some(&info), &here(), gone));
    let unknown_id = Machine {
        hostid: Vec::new(),
        ..here()
    };
    assert!(apparently_stale(Some(&info), &unknown_id, gone));
    info.hostname = "mac".into();
    assert!(!apparently_stale(Some(&info), &unknown_id, gone));
    assert!(!apparently_stale(None, &here(), gone));
}

#[test]
fn a_held_lock_blocks_until_released_and_a_dead_one_is_taken_over() {
    let scratch = Scratch::new("qt-lock");
    let dir = &scratch.0;
    let path = dir.join(".zerus-files.lock");
    let held = lock(&path, Duration::ZERO).unwrap().unwrap();
    let text = fs::read(&path).unwrap();
    assert_eq!(parse(&text).unwrap().pid, i64::from(std::process::id()));
    // Held (our PID, our name): another attempt waits and gives up.
    assert!(lock(&path, Duration::from_millis(150)).unwrap().is_none());
    drop(held);
    assert!(!path.exists());
    // A lock left by a process that is gone is removed and taken.
    dead_lock(&path);
    let taken = lock(&path, Duration::from_secs(2)).unwrap();
    assert!(taken.is_some());
    assert!(!dir.join(".zerus-files.lock.rmlock").exists());
    drop(taken);
    assert!(!path.exists());
}

#[test]
fn a_guard_left_behind_is_stale_by_qts_default_rule() {
    let scratch = Scratch::new("qt-rmlock");
    let path = scratch.0.join(".zerus-files.lock");
    let guard = scratch.0.join(".zerus-files.lock.rmlock");
    // A dead lock and a dead guard: both go, and the lock is taken.
    dead_lock(&path);
    dead_lock(&guard);
    let taken = lock(&path, Duration::from_millis(1500)).unwrap();
    assert!(taken.is_some());
    assert!(!guard.exists());
    drop(taken);
    // A guard a live process left over 30 s ago is stale by its age.
    dead_lock(&path);
    let pid = i64::from(std::process::id());
    fs::write(&guard, contents(pid, &own_name(), &this_machine())).unwrap();
    assert!(lock(&path, Duration::ZERO).unwrap().is_none());
    let old = SystemTime::now() - Duration::from_secs(31);
    File::options()
        .write(true)
        .open(&guard)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let taken = lock(&path, Duration::ZERO).unwrap();
    assert!(taken.is_some());
    assert!(!guard.exists());
    // The lock itself never ages: a live holder keeps it however old.
    drop(taken);
    fs::write(&path, contents(pid, &own_name(), &this_machine())).unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert!(lock(&path, Duration::ZERO).unwrap().is_none());
}
