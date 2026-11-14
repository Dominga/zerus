//! Bounded process-table walk. An unreadable environment is tolerated only at
//! the verified top of this user's tree, whose parent is foreign. Parent zero
//! refuses, including a same-user process at a PID namespace boundary. An
//! environment that macOS withholds is unreadable; below the top its refusal
//! tells the person where their commands run. The caller's own environment
//! comes from the process itself; the system's read starts at its parent.
use super::{claim, environment, is_withheld, own_environment, unreadable, withheld_below_top};
use zerus_team_plan::Problem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Process {
    pub(super) parent: u32,
    pub(super) birth: Option<String>,
}

#[derive(Debug)]
pub(super) struct Ancestor {
    pub(super) pid: u32,
    pub(super) identity: Process,
    pub(super) claim: Option<super::Claim>,
    pub(super) top: bool,
}

#[derive(Clone)]
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub(super) enum Inspection {
    Own(Process),
    Foreign,
}

pub(super) fn ancestors() -> Result<Vec<Ancestor>, Problem> {
    #[cfg(test)]
    if super::test_sources::selected("opaque") {
        return from_caller(
            4,
            super::test_sources::opaque_inspect,
            Vec::new,
            super::test_sources::opaque_environment,
        );
    }
    from_caller(std::process::id(), inspect, own_environment, environment)
}

/// The walk from the caller `me`. Its own markers come from `own`, the
/// process itself, and never from `read`, the system's read of it: a signed
/// release build of `hgs` may be a program whose environment macOS withholds,
/// and the caller is usually below the top, where a withheld read refuses.
fn from_caller(
    me: u32,
    inspect: impl FnMut(u32) -> Result<Inspection, Problem>,
    own: impl Fn() -> Vec<u8>,
    mut read: impl FnMut(u32) -> std::io::Result<Vec<u8>>,
) -> Result<Vec<Ancestor>, Problem> {
    walk(
        me,
        inspect,
        |pid| {
            if pid == me {
                Ok(own())
            } else {
                read(pid)
            }
        },
    )
}

/// This command and its ancestors up to the top of the user's tree, by the
/// process table alone: the walk of [`ancestors`], with its bound and births,
/// reading no environment, so no row carries a claim. A marked claim's host
/// check needs only process IDs and births (zerus-63rru.28), and a marked
/// command reads no ancestor's environment.
pub(super) fn processes() -> Result<Vec<Ancestor>, Problem> {
    #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
    WALKS.with(|n| n.set(n.get() + 1));
    walk_reading(std::process::id(), inspect, None)
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
thread_local! { static WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
/// How many times this thread walked [`processes`].
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(super) fn walks() -> usize {
    WALKS.with(|n| n.get())
}

/// A replacement, unknown parent, loop or cap is never a successful end.
pub(super) fn walk(
    pid: u32,
    inspect: impl FnMut(u32) -> Result<Inspection, Problem>,
    mut environment: impl FnMut(u32) -> std::io::Result<Vec<u8>>,
) -> Result<Vec<Ancestor>, Problem> {
    walk_reading(pid, inspect, Some(&mut environment))
}

/// The walk, with each process's environment read by `environment`, or none
/// read at all. Without a reader no row carries a claim, and a birth is
/// still required below the top.
fn walk_reading(
    mut pid: u32,
    mut inspect: impl FnMut(u32) -> Result<Inspection, Problem>,
    mut environment: Option<&mut dyn FnMut(u32) -> std::io::Result<Vec<u8>>>,
) -> Result<Vec<Ancestor>, Problem> {
    let mut chain: Vec<Ancestor> = Vec::new();
    let mut current = inspect(pid)?;
    for _ in 0..64 {
        let Inspection::Own(identity) = current else {
            return Err(unreadable());
        };
        let parent = identity.parent;
        if chain.iter().any(|row| row.pid == pid) || parent == pid || parent == 0 {
            return Err(unreadable());
        }
        let next = inspect(parent)?;
        let top = matches!(next, Inspection::Foreign);
        let claim = match environment.as_mut().map(|read| read(pid)) {
            None => None,
            Some(Ok(bytes)) => {
                if identity.birth.is_none() {
                    return Err(unreadable());
                }
                claim(&bytes)?
            }
            Some(Err(_)) if top => None,
            Some(Err(error)) if is_withheld(&error) => return Err(withheld_below_top()),
            Some(Err(_)) => return Err(unreadable()),
        };
        if !top && identity.birth.is_none() {
            return Err(unreadable());
        }
        if !matches!(inspect(pid)?, Inspection::Own(ref now) if *now == identity) {
            return Err(unreadable());
        }
        chain.push(Ancestor {
            pid,
            identity,
            claim,
            top,
        });
        if top {
            return Ok(chain);
        }
        pid = parent;
        current = next;
    }
    Err(unreadable())
}

pub(super) fn unchanged(chain: &[Ancestor]) -> Result<(), Problem> {
    for ancestor in chain {
        if !matches!(inspect(ancestor.pid)?, Inspection::Own(ref now) if *now == ancestor.identity)
        {
            return Err(unreadable());
        }
        // T3: no deterministic test pins a credential change in the boundary
        // parent after the first walk; that requires a native race/injection seam.
        if ancestor.top && !matches!(inspect(ancestor.identity.parent)?, Inspection::Foreign) {
            return Err(unreadable());
        }
    }
    Ok(())
}

/// Credentials come from Uid in the process table. A nondumpable Linux process
/// has root-owned procfs entries even while its effective UID remains ours.
#[cfg(target_os = "linux")]
pub(super) fn inspect(pid: u32) -> Result<Inspection, Problem> {
    let status = bounded(&format!("/proc/{pid}/status"))?;
    if !effective_owner(&status, unsafe { libc::geteuid() })? {
        return Ok(Inspection::Foreign);
    }
    let stat = bounded(&format!("/proc/{pid}/stat"))?;
    let (_, tail) = stat.rsplit_once(')').ok_or_else(unreadable)?;
    let fields: Vec<_> = tail.split_whitespace().collect();
    if fields.len() < 20 || matches!(fields[0], "Z" | "X") {
        return Err(unreadable());
    }
    let parent = fields[1].parse().map_err(|_| unreadable())?;
    let native = crate::platform::process_identity(pid);
    if native
        .as_ref()
        .is_some_and(|native| native.parent != parent)
    {
        return Err(unreadable());
    }
    // No synthetic birth: None is usable only at the verified foreign-parent
    // top with an unreadable environment, and never verifies an agent claim.
    Ok(Inspection::Own(Process {
        parent,
        birth: native.map(|native| native.birth),
    }))
}

#[cfg(target_os = "linux")]
fn effective_owner(status: &str, owner: u32) -> Result<bool, Problem> {
    let uids = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .ok_or_else(unreadable)?
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| unreadable())?;
    if uids.len() != 4 {
        return Err(unreadable());
    }
    Ok(uids[1] == owner)
}

#[cfg(target_os = "linux")]
fn bounded(path: &str) -> Result<String, Problem> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| unreadable())?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    if bytes.len() > 65536 {
        return Err(unreadable());
    }
    String::from_utf8(bytes).map_err(|_| unreadable())
}

#[cfg(target_os = "macos")]
pub(super) fn inspect(pid: u32) -> Result<Inspection, Problem> {
    let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    // SAFETY: a correctly sized writable native structure.
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdshortinfo).cast(),
            size,
        )
    };
    if count != size || info.pbsi_pid != pid {
        return Err(unreadable());
    }
    if info.pbsi_uid != unsafe { libc::geteuid() } {
        return Ok(Inspection::Foreign);
    }
    if info.pbsi_status == 5 {
        return Err(unreadable());
    }
    let native = crate::platform::process_identity(pid);
    if native
        .as_ref()
        .is_some_and(|native| native.parent != info.pbsi_ppid)
    {
        return Err(unreadable());
    }
    Ok(Inspection::Own(Process {
        parent: info.pbsi_ppid,
        birth: native.map(|native| native.birth),
    }))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn inspect(_pid: u32) -> Result<Inspection, Problem> {
    Err(unreadable())
}

#[cfg(test)]
mod tests;
