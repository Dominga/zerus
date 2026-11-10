//! Owned short-lived helper groups (probes, watches and isolated test cases).
//! Never agent execution, caller-supplied groups or detached sessions.
use std::{
    io,
    process::{Child, Command, ExitStatus},
};

pub(crate) fn prepare(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = command;
}

pub(crate) fn try_wait(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    #[cfg(unix)]
    {
        if exited_unreaped(child)? {
            // The leader's still-reserved PID prevents PGID reuse, including
            // when it exits before its pipe-holding ssh/ProxyCommand child.
            signal_and_reap(child, true).map(Some)
        } else {
            Ok(None)
        }
    }
    #[cfg(not(unix))]
    child.try_wait()
}

pub(crate) fn terminate(child: &mut Child) -> io::Result<ExitStatus> {
    #[cfg(unix)]
    {
        // Also establishes exclusive wait ownership when the child is live.
        // ECHILD/unsupported inspection never signals an unverified PID/group.
        let exited = exited_unreaped(child)?;
        signal_and_reap(child, exited)
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        child.wait()
    }
}

#[cfg(unix)]
fn exited_unreaped(child: &Child) -> io::Result<bool> {
    // Only this supervisor observes/reaps this helper; do not call Child's
    // try_wait first. WNOWAIT retains a terminated leader until group cleanup.
    // https://man7.org/linux/man-pages/man2/waitid.2.html
    let mut policy: libc::sigaction = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut policy) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if policy.sa_sigaction == libc::SIG_IGN || policy.sa_flags & libc::SA_NOCLDWAIT != 0 {
        return Err(io::Error::other("helper wait ownership is unavailable"));
    }
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    for _ in 0..3 {
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            let pid = unsafe { info.si_pid() };
            if pid != 0 && pid as u32 != child.id() {
                return Err(io::Error::other("helper wait identity changed"));
            }
            return Ok(pid != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    Err(io::ErrorKind::Interrupted.into())
}

#[cfg(unix)]
fn signal_and_reap(child: &mut Child, exited: bool) -> io::Result<ExitStatus> {
    #[cfg(not(target_os = "macos"))]
    let _ = exited;
    let pid =
        libc::pid_t::try_from(child.id()).map_err(|_| io::Error::other("invalid helper PID"))?;
    // prepare() made precisely this child the group leader. An unreaped child
    // pins that identity until wait(). No process scan, name/path match or
    // shared group is an authority to signal. setsid-detached children escape
    // this group intentionally and their open pipes remain accounted.
    if unsafe { libc::killpg(pid, libc::SIGKILL) } != 0 {
        let error = io::Error::last_os_error();
        // XNU excludes zombies from signal recipients and reports EPERM when
        // the group contains only our unreaped leader. Do not ignore arbitrary
        // EPERM: a fixed two-PID kernel query must prove that exact sole member.
        // https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c
        // The leader may also have exited after the caller saw it live, or be
        // exiting: XNU refuses the signal before waitid reports it. Its
        // unreaped exit, seen within a short bound, is the same proof.
        #[cfg(target_os = "macos")]
        let zombie_only = error.raw_os_error() == Some(libc::EPERM)
            && (exited || exit_visible(child))
            && sole_member(pid);
        #[cfg(not(target_os = "macos"))]
        let zombie_only = false;
        if error.raw_os_error() != Some(libc::ESRCH) && !zombie_only {
            return Err(error);
        }
    }
    child.wait()
}

/// Waits up to two seconds for an exiting leader's exit to be reported.
/// Unsupported inspection or a leader still live after that stays refused.
#[cfg(target_os = "macos")]
fn exit_visible(child: &Child) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match exited_unreaped(child) {
            Ok(true) => return true,
            Ok(false) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(2))
            }
            _ => return false,
        }
    }
}

#[cfg(target_os = "macos")]
fn sole_member(pid: libc::pid_t) -> bool {
    let mut members = [0 as libc::pid_t; 2];
    let found = unsafe {
        libc::proc_listpgrppids(
            pid,
            members.as_mut_ptr().cast(),
            std::mem::size_of_val(&members) as libc::c_int,
        )
    };
    found == 1 && members[0] == pid
}

#[cfg(all(test, unix))]
mod late_exit_tests {
    use super::*;
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };

    /// zerus-c7yqf: terminate() saw the leader live, then it exited before
    /// killpg. XNU reports EPERM for that zombie-only group; the exit is still
    /// verified, so the leader is reaped instead of its admission pinned.
    #[test]
    fn a_leader_that_exits_after_the_live_check_is_reaped_not_pinned() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "exit 0"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        prepare(&mut command);
        let mut child = command.spawn().unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while !exited_unreaped(&child).unwrap() {
            assert!(Instant::now() < until, "fixture leader did not exit");
            std::thread::sleep(Duration::from_millis(2));
        }
        let status = signal_and_reap(&mut child, false)
            .expect("a verified exited sole leader is reaped, not pinned");
        assert!(status.success());
    }
}
