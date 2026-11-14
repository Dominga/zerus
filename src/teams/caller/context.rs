//! Platform availability for both callers, and the one namespace admission
//! they share.
//!
//! A caller may run in a private user namespace whose owner can overmount the
//! process table and the process's own `ns` directory, so neither the marked
//! descent proof nor the unmarked walk can be trusted there. Both callers run
//! the same admission before either proof is attempted.
//!
//! The admission opens `/proc` itself and requires a procfs root, then
//! resolves `self/ns/user` from that descriptor with `openat2` under
//! `RESOLVE_NO_XDEV`: no mount is crossed, so a bind mount placed over the
//! process's `ns` directory cannot answer for the real namespace. The last
//! link is not followed (`O_PATH | O_NOFOLLOW`), so the descriptor stays on
//! the procfs link and `readlinkat` compares its text with the initial
//! namespace. Opening `/proc/self/ns/user` by path is never used: a handle to
//! the initial namespace taken before entering a private user and mount
//! namespace can be bound, with its directory, over the process's own `ns`
//! directory and make that path answer falsely.
//!
//! Any failure refuses, including `ENOSYS` from a kernel without `openat2` or
//! `EPERM` from a sandbox; there is no fallback to the path, `/proc/1` or
//! `uid_map`.
//!
//! macOS was qualified by the real-process caller suites on a Mac. It has no
//! user, mount or PID namespaces, so it has no admission: the walk reads the
//! kernel's one process table through `proc_pidinfo` and `sysctl`, which no
//! mount can replace, and the Linux forgery has no counterpart there. The walk
//! and the record check read a process's birth through the same call,
//! `platform::process_identity` (`PROC_PIDTBSDINFO`, start seconds and
//! microseconds).
//!
//! macOS withholds the environment of a process whose code-signing status
//! carries the `restrict` flag from a reader of the same user. On the Mac
//! that qualified this proof the flag held for every one of 464 withheld
//! processes and for none of 49 readable ones. The system shells `/bin/zsh`,
//! `/bin/bash` and `/bin/sh`, `ssh` and `sshd-session` carry it; a shell
//! installed by Homebrew does not. `KERN_PROCARGS2` still answers, but nothing
//! follows the arguments. A process whose environment is
//! really empty still leaves the kernel's own `key=value` strings there, so a
//! tail with no byte other than NUL is a withheld read and never an empty
//! environment. The parser also refuses an answer that fills its buffer,
//! which may be cut, and an argument that begins with a marker name, which
//! means the split between arguments and environment has shifted. A marker
//! named twice in one environment is unreadable on every platform. The
//! caller's own markers come from the process itself, never from this read,
//! so a release build of `hgs` that macOS hides is still read correctly.
//!
//! A withheld environment is unreadable, and the rule for an unreadable
//! ancestor applies: tolerated at the verified top, refused below it. What
//! decides is whether a program that hides its environment stands between the
//! command and the top, not the terminal or the connection as such. The Zerus
//! window runs `hgs` as its own child with no shell between, and passes when
//! launchd started it (Dock, Finder, `open`). A lead's or a member's marked
//! command reads no ancestor's environment and passes below any shell. A
//! person's command typed into a system shell is refused unless that shell is
//! itself the top, as when a terminal starts it through the root-owned
//! `login`; a shell installed outside the system, for example by Homebrew, is
//! readable and does not refuse. The refusal names the cause and the Zerus
//! window as the way out, and promises no terminal. The limit is at the top:
//! an agent whose own environment is withheld and which is itself the top of
//! the user's tree (started or adopted by launchd) is not detected, like an
//! agent started without markers. `open`, `osascript` and `launchctl` start a
//! process under launchd or `login`, a new top without markers, like `cron`,
//! `at` and ssh to the same computer.
//!
//! A macOS sandbox (Seatbelt, the App Sandbox) cannot give a process its own
//! process table, owner, parent or birth; it can only deny these reads, which
//! the same rule treats as unreadable. A command adopted by launchd after its
//! parent exited starts a tree of its own: a marked one is refused, since it
//! no longer descends from its agent, and an unmarked one is outside this
//! rule like any reparented command. Other platforms stay unavailable.
use zerus_team_plan::problem::code;
use zerus_team_plan::Problem;

pub(super) fn available() -> Result<(), Problem> {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return Err(Problem::new(code::NOT_ALLOWED,
            "The Teams preview is not available on this platform yet: its real-process ancestry proof has not been qualified."));
    }
    Ok(())
}

/// The one admission both callers use. The refused text names the unmarked
/// proof; `caller` substitutes the sandbox wording for a marked command.
pub(super) fn admission() -> Result<(), Problem> {
    admitted().map_err(|_| Problem::new(code::NOT_ALLOWED,
        "The unmarked Teams caller proof requires the initial Linux user namespace; this process context cannot establish the person."))
}

#[cfg(not(target_os = "linux"))]
fn admitted() -> std::io::Result<()> {
    Ok(())
}

/// Linux UAPI linux/nsfs.h: USER_NS_INIT_INO.
#[cfg(target_os = "linux")]
pub(super) const USER_NS_INIT_INO: u64 = 0xEFFF_FFFD;
#[cfg(target_os = "linux")]
pub(super) const PROC_SUPER_MAGIC: libc::c_long = 0x9fa0;
/// `RESOLVE_NO_XDEV`: the resolution must not traverse a mount point.
#[cfg(target_os = "linux")]
const RESOLVE_NO_XDEV: u64 = 0x01;

/// The check behind [`admission`], kept separate so a test can observe which
/// mount rule refused. Production never exposes a reason beyond refusal.
#[cfg(target_os = "linux")]
pub(super) fn admitted() -> std::io::Result<()> {
    #[cfg(test)]
    if super::test_sources::selected("namespace") {
        let (_root, namespace) = super::test_sources::namespace_input()?;
        return require_initial_namespace(&namespace);
    }
    let proc = open_proc_root()?;
    admit_from(&proc)
}

/// The resolution and link checks behind a validated root, kept separate so a
/// test can point them at another filesystem and prove the second `f_type`
/// check rather than only the procfs-root check.
#[cfg(target_os = "linux")]
pub(super) fn admit_from(root: &std::os::fd::OwnedFd) -> std::io::Result<()> {
    let namespace = open_user_namespace(root)?;
    require_initial_namespace(&namespace)
}

/// The procfs root itself: a procfs superblock whose root inode is 1.
#[cfg(target_os = "linux")]
fn open_proc_root() -> std::io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    // SAFETY: `path` is a valid NUL-terminated string and `open` retains no
    // pointer.
    let raw = unsafe {
        libc::open(
            c"/proc".as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw` is a fresh descriptor owned here and never used again.
    let proc = unsafe { OwnedFd::from_raw_fd(raw) };
    if !is_procfs_root(
        fstatfs_type(proc.as_raw_fd())?,
        fstat_ino(proc.as_raw_fd())?,
    ) {
        return Err(std::io::Error::other("not the procfs root"));
    }
    Ok(proc)
}

/// Resolve the process's user-namespace link from the procfs root without
/// crossing a mount and without following the link itself.
#[cfg(target_os = "linux")]
fn open_user_namespace(proc: &std::os::fd::OwnedFd) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    // SAFETY: `open_how` is plain data whose all-zero representation is valid.
    let mut how: libc::open_how = unsafe { std::mem::zeroed() };
    how.flags = (libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC) as u64;
    how.mode = 0;
    how.resolve = RESOLVE_NO_XDEV;
    // SAFETY: the dirfd is open, the path is valid, `how` is initialised for
    // its full stated size, and the syscall retains no pointer.
    let raw = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            proc.as_raw_fd(),
            c"self/ns/user".as_ptr(),
            &how as *const libc::open_how,
            std::mem::size_of::<libc::open_how>(),
        )
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a nonnegative syscall result is this call's new descriptor.
    let namespace = unsafe { OwnedFd::from_raw_fd(raw as i32) };
    if !is_procfs(fstatfs_type(namespace.as_raw_fd())?) {
        return Err(std::io::Error::other("the namespace link is not on procfs"));
    }
    Ok(namespace)
}

/// The link's own text must name the initial user namespace exactly.
#[cfg(target_os = "linux")]
fn require_initial_namespace(namespace: &std::os::fd::OwnedFd) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let expected = format!("user:[{USER_NS_INIT_INO}]");
    if link_text(namespace.as_raw_fd())? != expected {
        return Err(std::io::Error::other("not the initial user namespace"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn is_procfs(kind: libc::c_long) -> bool {
    kind == PROC_SUPER_MAGIC
}

#[cfg(target_os = "linux")]
pub(super) fn is_procfs_root(kind: libc::c_long, ino: u64) -> bool {
    is_procfs(kind) && ino == 1
}

#[cfg(target_os = "linux")]
fn fstatfs_type(fd: std::os::fd::RawFd) -> std::io::Result<libc::c_long> {
    // SAFETY: `statfs` is zeroed and valid for `fstatfs` to fill.
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is open and `stats` is a valid writable struct.
    if unsafe { libc::fstatfs(fd, &mut stats) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(stats.f_type)
}

#[cfg(target_os = "linux")]
fn fstat_ino(fd: std::os::fd::RawFd) -> std::io::Result<u64> {
    // SAFETY: `stat` is zeroed and valid for `fstat` to fill.
    let mut stats: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is open and `stats` is a valid writable struct.
    if unsafe { libc::fstat(fd, &mut stats) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(stats.st_ino)
}

#[cfg(target_os = "linux")]
fn link_text(fd: std::os::fd::RawFd) -> std::io::Result<String> {
    let mut buffer = [0u8; 64];
    // SAFETY: the descriptor is an `O_PATH` link descriptor, the empty path
    // reads that descriptor itself, and `buffer` is writable for its length.
    let size =
        unsafe { libc::readlinkat(fd, c"".as_ptr(), buffer.as_mut_ptr().cast(), buffer.len()) };
    if size < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let text = std::str::from_utf8(&buffer[..size as usize])
        .map_err(|_| std::io::Error::other("the namespace link is not text"))?;
    Ok(text.to_owned())
}
