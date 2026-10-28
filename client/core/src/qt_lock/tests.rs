// The cases of QLockFile's format and stale rule (qtbase 6.12) a shared
// folder relies on.
use super::*;

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
    let dir = std::env::temp_dir().join(format!("zerus-qt-lock-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".zerus-files.lock");
    let held = lock(&path, Duration::ZERO).unwrap().unwrap();
    let text = fs::read(&path).unwrap();
    assert_eq!(parse(&text).unwrap().pid, i64::from(std::process::id()));
    // Held (our PID, our name): another attempt waits and gives up.
    assert!(lock(&path, Duration::from_millis(150)).unwrap().is_none());
    drop(held);
    assert!(!path.exists());
    // A lock left by a process that is gone is removed and taken.
    fs::write(
        &path,
        contents(i64::from(i32::MAX - 7), "hgs-tray", &this_machine()),
    )
    .unwrap();
    let taken = lock(&path, Duration::from_secs(2)).unwrap();
    assert!(taken.is_some());
    assert!(!dir.join(".zerus-files.lock.rmlock").exists());
    drop(taken);
    let _ = fs::remove_dir_all(&dir);
}

/// Against Qt's own QLockFile: with ZERUS_QT_LOCK_CHECK naming
/// client/scripts/qt-lock-check built with Qt 6, Qt and the client honour
/// each other's held locks and each takes over the other's dead one.
#[cfg(unix)]
#[test]
#[ignore = "needs ZERUS_QT_LOCK_CHECK, a qt-lock-check built with Qt 6"]
fn qt_and_the_client_honour_each_others_locks() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let checker = std::env::var("ZERUS_QT_LOCK_CHECK").expect("ZERUS_QT_LOCK_CHECK");
    let dir = std::env::temp_dir().join(format!("zerus-qt-interop-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(".zerus-files.lock");
    let qt_try = || {
        let out = Command::new(&checker)
            .arg("try")
            .arg(&path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };

    // Ours, held: Qt judges it live and leaves it.
    let held = lock(&path, Duration::ZERO).unwrap().unwrap();
    assert_eq!(qt_try(), "busy");
    assert!(path.exists());
    drop(held);

    // Qt's, held: we wait and give up.
    let mut qt = Command::new(&checker)
        .arg("hold")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(qt.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "held");
    let info = parse(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(info.appname, "qt-lock-check");
    assert_eq!(info.hostname, this_machine().hostname);
    assert!(lock(&path, Duration::from_millis(300)).unwrap().is_none());

    // Qt killed: its lock is left behind, and we take it over.
    qt.kill().unwrap();
    qt.wait().unwrap();
    assert!(path.exists());
    let taken = lock(&path, Duration::from_secs(3)).unwrap();
    assert!(taken.is_some());
    drop(taken);

    // Ours left behind by a process that is gone: Qt takes it over.
    fs::write(
        &path,
        contents(i64::from(i32::MAX - 7), &own_name(), &this_machine()),
    )
    .unwrap();
    assert_eq!(qt_try(), "locked");
    let _ = fs::remove_dir_all(&dir);
}
