use super::*;
use std::{
    cell::Cell,
    fs,
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn the_real_bounded_runner_keeps_its_exited_leader_while_pipes_are_held() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("leader");
    let release = root.path().join("release");
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            "printf '%s' $$ > \"$1\"; (while ! test -e \"$2\"; do sleep .005; done) & exit 0",
            "sh",
        ])
        .arg(&marker)
        .arg(&release)
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    let reserved = Cell::new(false);
    let lost = Cell::new(false);
    let observe = || {
        if let Ok(text) = fs::read_to_string(&marker) {
            if let Ok(pid) = text.parse::<libc::id_t>() {
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        pid,
                        &mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                if result == 0 && unsafe { info.si_pid() } as libc::id_t == pid {
                    reserved.set(true);
                    fs::write(&release, b"release").unwrap();
                } else if result != 0
                    && io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)
                {
                    // The negative control reaps this SAME original early.
                    // Releasing its owned holder avoids any unsafe cleanup.
                    lost.set(true);
                    fs::write(&release, b"release").unwrap();
                }
            }
        }
        false
    };
    let result = super::super::run_bounded(
        &mut command,
        [32; 2],
        Instant::now() + Duration::from_secs(5),
        &observe,
    )
    .unwrap();
    assert!(result.0.success());
    assert!(reserved.get() && !lost.get());
}

fn spawn(exit: u8) -> Child {
    admit().unwrap();
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", &format!("exit {exit}")])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    super::super::spawn_in_turn(&mut command).unwrap()
}

#[test]
fn group_cleanup_keeps_the_original_leader_reserved_until_reap() {
    for exit in [0, 7] {
        let mut child = spawn(exit);
        let mut group = Group::new(&mut child);
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(success) = group.exited().unwrap() {
                assert_eq!(success, exit == 0);
                break;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(test_unreaped(group.child));
        let status = group.finish().unwrap();
        assert_eq!(status.code(), Some(exit.into()));
        assert!(group.reaped && !group.lost);
        let count = usize::from(exit != 0);
        assert_eq!(group.signals, count);
        group.stop().unwrap();
        assert!(group.exited().is_err());
        assert_eq!(group.signals, count);
    }
}

#[test]
fn a_leader_that_exits_after_the_live_check_is_reaped_not_pinned() {
    // zerus-c7yqf: stop() saw the leader live and it exited before killpg.
    // XNU reports EPERM for that zombie-only group; the unreaped exit proves
    // it, so the original is reaped instead of the cleanup failing.
    let mut child = spawn(0);
    let mut group = Group::new(&mut child);
    let until = Instant::now() + Duration::from_secs(5);
    while group.exited().unwrap().is_none() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    group
        .signal(false)
        .expect("a verified exited sole leader is not refused");
    assert!(group.signalled && !group.lost);
    assert!(group.reap().unwrap().success());
    assert_eq!(group.signals, 1);
}

#[test]
fn a_lost_original_wait_suppresses_every_later_group_or_pid_stop() {
    let mut child = spawn(0);
    let mut group = Group::new(&mut child);
    // A private competing wait of this SAME known original, with no PID reuse.
    // The test-only native veto prevents any bad mutation from signalling it.
    group.child.wait().unwrap();
    assert!(!test_unreaped(group.child));
    assert!(group.exited().is_err());
    assert!(group.lost);
    for _ in 0..3 {
        assert!(group.stop().is_err());
    }
    assert_eq!(group.signals, 0);
}

#[test]
fn a_live_original_helper_is_stopped_before_the_wait_releases_its_identity() {
    admit().unwrap();
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "read value"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = super::super::spawn_in_turn(&mut command).unwrap();
    let mut group = Group::new(&mut child);
    assert_eq!(group.exited().unwrap(), None);
    group.stop().unwrap();
    assert!(group.reaped && !group.lost);
    assert_eq!(group.signals, 1);
    group.stop().unwrap();
    assert_eq!(group.signals, 1);
    assert!(!test_unreaped(group.child));
}

fn policy_probe(case: &str, root: &Path) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "platform::unix::helper_group::tests::private_policy_probe",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("HGS_CONFIG_DIR", root.join("config"))
        .env("HGS_STATE_DIR", root.join("state"))
        .env("HGS_TEST_GROUP_POLICY", case);
    let result = super::super::run_bounded(
        &mut command,
        [32 << 10; 2],
        Instant::now() + Duration::from_secs(10),
        &|| false,
    )
    .unwrap();
    assert!(
        result.0.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&result.1),
        String::from_utf8_lossy(&result.2)
    );
}

#[test]
fn inherited_auto_reaping_refuses_before_a_bounded_helper_is_started() {
    for case in ["ignore", "no-wait"] {
        let root = tempfile::tempdir().unwrap();
        policy_probe(case, root.path());
    }
}

#[test]
#[ignore = "exact private reexec: changes SIGCHLD in this process only"]
fn private_policy_probe() {
    let case = std::env::var("HGS_TEST_GROUP_POLICY").unwrap();
    assert!(matches!(case.as_str(), "ignore" | "no-wait"));
    let marker = Path::new(&std::env::var("HOME").unwrap()).join("started");
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = if case == "ignore" {
        libc::SIG_IGN
    } else {
        libc::SIG_DFL
    };
    action.sa_flags = if case == "no-wait" {
        libc::SA_NOCLDWAIT
    } else {
        0
    };
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) },
        0
    );
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "printf started > \"$1\"", "sh"])
        .arg(&marker);
    let result = super::super::run_bounded(
        &mut command,
        [16; 2],
        Instant::now() + Duration::from_secs(1),
        &|| false,
    );
    assert!(matches!(result, Err(super::super::RunError::Spawn(_))));
    assert!(!marker.exists());
    assert!(fs::read_dir(marker.parent().unwrap()).unwrap().count() == 0);
}

mod spawn_turn;
