//! Same original short-lived helper, with its leader unreaped until cleanup.
//! No agent, caller-supplied group or replacement process is adopted here.
use std::{
    io,
    process::{Child, ExitStatus},
};

pub(super) fn admit() -> io::Result<()> {
    let mut policy: libc::sigaction = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut policy) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if policy.sa_sigaction == libc::SIG_IGN || policy.sa_flags & libc::SA_NOCLDWAIT != 0 {
        return Err(io::Error::other("helper wait ownership is unavailable"));
    }
    Ok(())
}

pub(super) struct Group<'a> {
    child: &'a mut Child,
    lost: bool,
    reaped: bool,
    signalled: bool,
    #[cfg(test)]
    signals: usize,
}
impl<'a> Group<'a> {
    pub(super) fn new(child: &'a mut Child) -> Self {
        Self {
            child,
            lost: false,
            reaped: false,
            signalled: false,
            #[cfg(test)]
            signals: 0,
        }
    }
    fn refused(&mut self, error: io::Error) -> io::Error {
        self.lost = true;
        error
    }
    pub(super) fn exited(&mut self) -> io::Result<Option<bool>> {
        if self.lost || self.reaped {
            return Err(io::Error::other("original helper wait is unavailable"));
        }
        if let Err(error) = admit() {
            return Err(self.refused(error));
        }
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        for _ in 0..3 {
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                let pid = unsafe { info.si_pid() };
                if pid == 0 {
                    return Ok(None);
                }
                let status = unsafe { info.si_status() };
                if pid as u32 != self.child.id()
                    || !matches!(
                        info.si_code,
                        libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED
                    )
                    || (info.si_code == libc::CLD_EXITED && !(0..=255).contains(&status))
                    || (info.si_code != libc::CLD_EXITED && !(1..=127).contains(&status))
                {
                    return Err(self.refused(io::Error::other("helper wait identity changed")));
                }
                return Ok(Some(info.si_code == libc::CLD_EXITED && status == 0));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(self.refused(error));
            }
        }
        Err(self.refused(io::ErrorKind::Interrupted.into()))
    }
    fn signal(&mut self, exited: bool) -> io::Result<()> {
        // A private fault-control veto runs BEFORE any PID/group signal. It
        // records an unsafe attempted call instead of sending it after loss.
        #[cfg(test)]
        {
            self.signals += 1;
            if self.lost || self.reaped || !test_unreaped(self.child) {
                return Err(io::Error::other("unsafe test signal suppressed"));
            }
        }
        if self.lost || self.reaped || self.signalled {
            return Err(io::Error::other("original helper signal is unavailable"));
        }
        self.signalled = true;
        let pid = libc::pid_t::try_from(self.child.id())
            .ok()
            .filter(|&id| id > 0)
            .ok_or_else(|| io::Error::other("invalid helper PID"))?;
        // The unreaped original leader reserves precisely this owned PGID.
        if unsafe { libc::killpg(pid, libc::SIGKILL) } != 0 {
            let error = io::Error::last_os_error();
            #[cfg(target_os = "macos")]
            let zombie_only =
                exited && error.raw_os_error() == Some(libc::EPERM) && sole_member(pid);
            #[cfg(not(target_os = "macos"))]
            let zombie_only = false;
            if error.raw_os_error() != Some(libc::ESRCH) && !zombie_only {
                return Err(error);
            }
        }
        // Never signal the PID after wait(), including when the group leader
        // is already a zombie. A live original may have left its initial group.
        if !exited {
            if let Err(error) = self.child.kill() {
                if error.raw_os_error() != Some(libc::ESRCH) || self.exited()?.is_none() {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    fn reap(&mut self) -> io::Result<ExitStatus> {
        if self.lost || self.reaped {
            return Err(io::Error::other("original helper reap is unavailable"));
        }
        match self.child.wait() {
            Ok(status) => {
                self.reaped = true;
                Ok(status)
            }
            Err(error) => Err(self.refused(error)),
        }
    }
    pub(super) fn finish(&mut self) -> io::Result<ExitStatus> {
        let success = self
            .exited()?
            .ok_or_else(|| io::Error::other("helper is still live"))?;
        if !success {
            self.signal(true)?;
        }
        self.reap()
    }
    pub(super) fn stop(&mut self) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        let exited = self.exited()?.is_some();
        if !self.signalled {
            self.signal(exited)?;
        }
        self.reap().map(|_| ())
    }
}

#[cfg(test)]
fn test_unreaped(child: &Child) -> bool {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id() as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    let pid = unsafe { info.si_pid() } as u32;
    result == 0 && (pid == 0 || pid == child.id())
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

#[cfg(test)]
mod tests;
