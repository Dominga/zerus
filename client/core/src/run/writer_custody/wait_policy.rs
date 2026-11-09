//! Read-only admission of the host's existing child-wait policy. No signal
//! disposition is changed, and this cannot establish exclusive wait ownership:
//! the trusted host must still provide that invariant for its original Child.
pub(super) fn waitable() -> bool {
    let mut policy: libc::sigaction = unsafe { std::mem::zeroed() };
    // SAFETY: query only, with an output structure of the native ABI size.
    let queried = unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut policy) == 0 };
    queried && policy.sa_sigaction != libc::SIG_IGN && policy.sa_flags & libc::SA_NOCLDWAIT == 0
}
