use super::*;
use std::{
    collections::BTreeSet,
    mem::{size_of, size_of_val, MaybeUninit},
    os::unix::ffi::OsStrExt,
};

// XNU proc_info_private.h, also used by desktop/ui_authority/macos_ancestry.
#[repr(C)]
struct Unique {
    image: [u8; 16],
    birth: u64,
    parent_birth: u64,
    version: i32,
    _parent_version: i32,
    _reserved: [u64; 2],
}
const _: [(); 56] = [(); size_of::<Unique>()];

#[derive(PartialEq, Eq)]
struct Identity {
    pid: u32,
    parent: u32,
    uid: u32,
    ruid: u32,
    svuid: u32,
    birth: u64,
    parent_birth: u64,
    version: i32,
    image: [u8; 16],
}
struct Snapshot {
    identity: Identity,
    zombie: bool,
    command: String,
}

// proc_info.h: vnode_fdinfowithpath = proc_fileinfo + vnode_info_path.
// https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/proc_info.h
#[repr(C)]
struct FileInfo {
    _flags: u32,
    _status: u32,
    _offset: i64,
    _kind: i32,
    _guard: u32,
}
#[repr(C)]
struct VnodeFile {
    _info: FileInfo,
    vnode: libc::vnode_info_path,
}
const _: [(); 1200] = [(); size_of::<VnodeFile>()];

fn errno() -> io::Error {
    io::Error::last_os_error()
}
fn clear_errno() {
    unsafe {
        *libc::__error() = 0;
    }
}
fn failed(error: io::Error) -> io::Error {
    if error.raw_os_error() == Some(0) {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete native process metadata",
        )
    } else {
        error
    }
}
fn info<T>(pid: u32, flavor: i32) -> io::Result<T> {
    let mut value = MaybeUninit::<T>::uninit();
    clear_errno();
    let used = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            flavor,
            0,
            value.as_mut_ptr().cast(),
            size_of::<T>() as i32,
        )
    };
    if used != size_of::<T>() as i32 {
        return Err(failed(errno()));
    }
    Ok(unsafe { value.assume_init() })
}
fn snapshot(pid: u32) -> io::Result<Snapshot> {
    let short: libc::proc_bsdshortinfo = info(pid, libc::PROC_PIDT_SHORTBSDINFO)?;
    let unique: Unique = info(pid, 17)?;
    if short.pbsi_pid != pid || unique.birth == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native process identity changed",
        ));
    }
    let command = unsafe {
        std::slice::from_raw_parts(short.pbsi_comm.as_ptr().cast::<u8>(), short.pbsi_comm.len())
    };
    Ok(Snapshot {
        identity: Identity {
            pid,
            parent: short.pbsi_ppid,
            uid: short.pbsi_uid,
            ruid: short.pbsi_ruid,
            svuid: short.pbsi_svuid,
            birth: unique.birth,
            parent_birth: unique.parent_birth,
            version: unique.version,
            image: unique.image,
        },
        zombie: short.pbsi_status == libc::SZOMB,
        command: String::from_utf8_lossy(
            &command[..command
                .iter()
                .position(|b| *b == 0)
                .unwrap_or(command.len())],
        )
        .into_owned(),
    })
}
fn path(bytes: &[u8]) -> io::Result<PathBuf> {
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .filter(|n| *n > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing native process path"))?;
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..end]));
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "relative native process path",
        ));
    }
    Ok(path)
}
fn vnode_path(value: &libc::vnode_info_path) -> io::Result<PathBuf> {
    path(unsafe {
        std::slice::from_raw_parts(
            value.vip_path.as_ptr().cast::<u8>(),
            size_of_val(&value.vip_path),
        )
    })
}
fn image(pid: u32) -> io::Result<PathBuf> {
    let mut bytes = [0u8; 4096];
    clear_errno();
    let used =
        unsafe { libc::proc_pidpath(pid as i32, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if used <= 0 || used as usize >= bytes.len() {
        return Err(failed(errno()));
    }
    path(&bytes)
}
fn files(pid: u32, deadline: Instant) -> io::Result<Files> {
    // A full buffer is ambiguous/truncated, never a complete roster.
    let mut fds: Vec<libc::proc_fdinfo> = (0..=MAX_FILES)
        .map(|_| unsafe { std::mem::zeroed() })
        .collect();
    let bytes = (fds.len() * size_of::<libc::proc_fdinfo>()) as i32;
    clear_errno();
    let used = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            bytes,
        )
    };
    let error = errno();
    let mut result = Files::default();
    if used < 0 || used == 0 && error.raw_os_error() != Some(0) {
        if denied(&error) {
            result.denied = true;
            return Ok(result);
        }
        return Err(failed(error));
    }
    if used >= bytes || !(used as usize).is_multiple_of(size_of::<libc::proc_fdinfo>()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete process file roster",
        ));
    }
    for (count, fd) in fds
        .into_iter()
        .take(used as usize / size_of::<libc::proc_fdinfo>())
        .enumerate()
    {
        bounded(count, deadline)?;
        if fd.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
            continue;
        }
        let mut value = MaybeUninit::<VnodeFile>::uninit();
        clear_errno();
        let used = unsafe {
            libc::proc_pidfdinfo(
                pid as i32,
                fd.proc_fd,
                2,
                value.as_mut_ptr().cast(),
                size_of::<VnodeFile>() as i32,
            )
        };
        if used == size_of::<VnodeFile>() as i32 {
            result.push(vnode_path(&unsafe { value.assume_init() }.vnode)?)?;
        } else {
            let error = failed(errno());
            match error.raw_os_error() {
                Some(libc::EBADF | libc::ENOENT) => {} // This enumerated descriptor closed.
                _ if denied(&error) => result.denied = true,
                _ => return Err(error),
            }
        }
    }
    Ok(result)
}

pub(in crate::platform) fn sample(deadline: Instant) -> Result<WorktreeProcesses, String> {
    let deadline = deadline.min(Instant::now() + std::time::Duration::from_secs(2));
    let uid = unsafe { libc::getuid() };
    let error = |detail: &str| {
        format!("Running process coverage is incomplete: {detail}; review the worktree manually")
    };
    let mut pids = BTreeSet::new();
    for kind in [4, 5] {
        // Kernel effective-UID and real-UID rosters.
        let mut buffer = vec![0i32; 32768];
        let bytes = (buffer.len() * size_of::<i32>()) as i32;
        let used = unsafe { libc::proc_listpids(kind, uid, buffer.as_mut_ptr().cast(), bytes) };
        if used <= 0 || used >= bytes || !(used as usize).is_multiple_of(size_of::<i32>()) {
            return Err(error("the owned process roster could not be verified"));
        }
        pids.extend(
            buffer
                .into_iter()
                .take(used as usize / size_of::<i32>())
                .filter(|pid| *pid > 0),
        );
        if pids.len() >= 32768 {
            return Err(error("the process roster exceeded its bound"));
        }
    }
    if !pids.contains(&(std::process::id() as i32)) {
        return Err(error("the current process is missing"));
    }
    let mut rows = WorktreeProcesses::default();
    let mut budget = PathBudget::default();
    for pid in pids.into_iter().map(|pid| pid as u32) {
        if Instant::now() >= deadline {
            return Err(error("the process check reached its deadline"));
        }
        let before = match snapshot(pid) {
            Ok(value) => value,
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => continue,
            Err(_) => return Err(error(&format!("process {pid} could not be identified"))),
        };
        if before.zombie || before.identity.uid != uid && before.identity.ruid != uid {
            continue;
        }
        // Short/unique identity does not replace membership: always perform
        // the actual cwd, image and descriptor reads, even for /usr/bin/login.
        let cwd = info::<libc::proc_vnodepathinfo>(pid, libc::PROC_PIDVNODEPATHINFO)
            .and_then(|value| vnode_path(&value.pvi_cdir));
        let image = image(pid);
        let files = files(pid, deadline);
        let after = match snapshot(pid) {
            Ok(value) => value,
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => continue,
            Err(_) => return Err(error(&format!("process {pid} could not be identified"))),
        };
        if after.zombie {
            continue;
        }
        if before.identity != after.identity {
            return Err(error("a process changed while being checked"));
        }
        let mut cwd_denied = false;
        let mut image_denied = false;
        for (value, access) in [(cwd, &mut cwd_denied), (image, &mut image_denied)] {
            match value {
                Ok(path) => budget
                    .push(&mut rows, pid, path, &before.command)
                    .map_err(|_| error("process path inventory exceeded its bound"))?,
                Err(e) if denied(&e) => *access = true,
                Err(_) => return Err(error(&format!("process {pid} membership is incomplete"))),
            }
        }
        let files =
            files.map_err(|_| error(&format!("process {pid} file inspection is incomplete")))?;
        if cwd_denied || image_denied || files.denied {
            rows.unreadable.push(UnreadableWorktreeProcess {
                pid,
                identity: format!(
                    "macos:{}:{}",
                    before.identity.birth, before.identity.version
                ),
                command: before.command.clone(),
                cwd: cwd_denied,
                executable: image_denied,
                files: files.denied,
                invocation_ancestor: false,
            });
        }
        // A readable membership always wins over a denied half.
        for path in files.paths {
            budget
                .push(&mut rows, pid, path, &before.command)
                .map_err(|_| error("process path inventory exceeded its bound"))?;
        }
    }
    if Instant::now() >= deadline {
        return Err(error("the process check reached its deadline"));
    }
    rows.membership_complete = true;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_short_unique_identity_and_fd_path_find_the_same_held_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("held-original");
        let _file = std::fs::File::create(&path).unwrap();
        let before = snapshot(std::process::id()).unwrap();
        let files = files(
            std::process::id(),
            Instant::now() + std::time::Duration::from_secs(2),
        )
        .unwrap();
        let after = snapshot(std::process::id()).unwrap();
        assert!(before.identity == after.identity);
        assert!(!files.denied);
        assert!(files.paths.contains(&path.canonicalize().unwrap()));
    }
}
