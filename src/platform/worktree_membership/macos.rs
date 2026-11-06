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
    flags: u32,
    status: u32,
    _offset: i64,
    _kind: i32,
    _guard: u32,
}
#[repr(C)]
struct VnodeFile {
    info: FileInfo,
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
fn vnode_files(
    pid: u32,
    deadline: Instant,
    mut visit: impl FnMut(i32, &VnodeFile) -> io::Result<()>,
) -> io::Result<bool> {
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
    let mut access_denied = false;
    if used < 0 || used == 0 && error.raw_os_error() != Some(0) {
        if denied(&error) {
            return Ok(true);
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
            visit(fd.proc_fd, &unsafe { value.assume_init() })?;
        } else {
            let error = failed(errno());
            match error.raw_os_error() {
                Some(libc::EBADF | libc::ENOENT) => {} // This enumerated descriptor closed.
                _ if denied(&error) => access_denied = true,
                _ => return Err(error),
            }
        }
    }
    Ok(access_denied)
}

fn files(pid: u32, deadline: Instant) -> io::Result<Files> {
    let mut files = Files::default();
    files.denied = vnode_files(pid, deadline, |_, file| {
        files.push(vnode_path(&file.vnode)?)
    })?;
    Ok(files)
}

fn vnode_object(value: &libc::vnode_info_path) -> objects::Object {
    objects::Object {
        device: u64::from(value.vip_vi.vi_stat.vst_dev),
        inode: value.vip_vi.vi_stat.vst_ino,
    }
}

pub(super) fn member(
    pid: u32,
    inventory: &objects::Inventory,
    deadline: Instant,
) -> io::Result<bool> {
    member_with_holder(pid, inventory, None, deadline)
}
fn member_with_holder(
    pid: u32,
    inventory: &objects::Inventory,
    root: Option<HolderRoot>,
    deadline: Instant,
) -> io::Result<bool> {
    let mut found = false;
    let mut skipped = false;
    match info::<libc::proc_vnodepathinfo>(pid, libc::PROC_PIDVNODEPATHINFO) {
        Ok(cwd) => found |= inventory.contains(vnode_object(&cwd.pvi_cdir)),
        Err(e) if denied(&e) && root.is_none() => {}
        Err(e) => return Err(e),
    }
    // Known native membership wins even when the cwd or another FD was denied.
    let denied = vnode_files(pid, deadline, |number, file| {
        let id = vnode_object(&file.vnode);
        if root.is_some_and(|root| root.object == id && root.descriptor == number) && !skipped {
            // XNU proc_info.h: fi_openflags FREAD=1/FWRITE=2,
            // fi_status PROC_FP_CLEXEC=2. No pathname or PID exemption.
            if file.info.flags & 3 != 1 || file.info.status & 2 == 0 {
                return Err(limit());
            }
            skipped = true;
        } else {
            found |= inventory.contains(id);
        }
        Ok(())
    })?;
    if root.is_some() && (denied || !skipped) {
        return Err(limit());
    }
    Ok(found)
}

fn ancestors(deadline: Instant) -> io::Result<std::collections::BTreeMap<u32, Identity>> {
    let mut rows = std::collections::BTreeMap::new();
    let mut pid = std::process::id();
    while pid > 1 {
        if rows.len() >= 128 || rows.contains_key(&pid) || Instant::now() >= deadline {
            return Err(limit());
        }
        let current = snapshot(pid)?;
        if current.zombie {
            return Err(limit());
        }
        let parent = current.identity.parent;
        rows.insert(pid, current.identity);
        pid = parent;
    }
    Ok(rows)
}

/// Separate accounting for THIS helper's actual borrowed Scope object. This
/// does not extend the original holder exception to any borrower or child.
pub(super) fn own_clear(
    scope: &objects::Scope,
    inventory: &objects::Inventory,
    deadline: Instant,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let own = scope.own_root_descriptor().as_raw_fd();
    let root = objects::Object {
        device: scope.identity().device,
        inode: scope.identity().inode,
    };
    let cwd = info::<libc::proc_vnodepathinfo>(std::process::id(), libc::PROC_PIDVNODEPATHINFO)?;
    if inventory.contains(vnode_object(&cwd.pvi_cdir)) {
        return Err(limit());
    }
    let mut seen = false;
    let denied = vnode_files(std::process::id(), deadline, |number, file| {
        let object = vnode_object(&file.vnode);
        if number == own {
            if seen || object != root || file.info.flags & 3 != 1 || file.info.status & 2 == 0 {
                return Err(limit());
            }
            seen = true;
        } else if inventory.contains(object) {
            return Err(io::Error::other(
                "this cleanup helper also uses the checkout",
            ));
        }
        Ok(())
    })?;
    if denied || !seen {
        return Err(limit());
    }
    scope.current(deadline)
}

pub(super) fn programs(
    inventory: &objects::Inventory,
    deadline: Instant,
) -> io::Result<Vec<zerus_client_core::worktree_programs::Program>> {
    programs_with_holder(inventory, None, deadline)
}
pub(super) fn programs_with_holder(
    inventory: &objects::Inventory,
    holder: Option<&Custody<'_>>,
    deadline: Instant,
) -> io::Result<Vec<zerus_client_core::worktree_programs::Program>> {
    let uid = unsafe { libc::getuid() };
    let invocation = ancestors(deadline)?;
    let mut rows = Vec::new();
    let mut pids = BTreeSet::new();
    for kind in [4, 5] {
        let mut buffer = vec![0i32; 32_768];
        let size = (buffer.len() * size_of::<i32>()) as i32;
        let used = unsafe { libc::proc_listpids(kind, uid, buffer.as_mut_ptr().cast(), size) };
        if used <= 0 || used >= size || !(used as usize).is_multiple_of(size_of::<i32>()) {
            return Err(limit());
        }
        pids.extend(
            buffer
                .into_iter()
                .take(used as usize / size_of::<i32>())
                .filter(|pid| *pid > 0),
        );
        if pids.len() >= 32_768 {
            return Err(limit());
        }
    }
    if !pids.contains(&(std::process::id() as i32)) {
        return Err(limit());
    }
    for pid in pids.into_iter().map(|pid| pid as u32) {
        // The observer deliberately holds the root FD, and is never a signal target.
        if pid <= 1 || pid == std::process::id() {
            continue;
        }
        if Instant::now() >= deadline {
            return Err(limit());
        }
        let before = match snapshot(pid) {
            Ok(value) => value,
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => continue,
            Err(e) => return Err(e),
        };
        if before.zombie || before.identity.uid != uid && before.identity.ruid != uid {
            continue;
        }
        let root = holder.and_then(|h| h.root_for(pid));
        let membership = member_with_holder(pid, inventory, root, deadline);
        let after = match snapshot(pid) {
            Ok(value) => value,
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => continue,
            Err(e) => return Err(e),
        };
        if after.zombie {
            continue;
        }
        if before.identity != after.identity {
            return Err(limit());
        }
        if !membership? {
            continue;
        }
        if root.is_some() || invocation.get(&pid) == Some(&before.identity) {
            return Err(io::Error::other(
                "a program in the checkout is needed to settle this action",
            ));
        }
        // The original PinnedProcess signal primitive uses this native birth
        // spelling. Short/unique identity alone never substitutes for it.
        let original = crate::platform::process_identity(pid).ok_or_else(limit)?;
        if snapshot(pid)?.identity != before.identity {
            return Err(limit());
        }
        let label = before.command;
        if label.is_empty() || label.len() > 512 || label.chars().any(char::is_control) {
            return Err(limit());
        }
        rows.push(zerus_client_core::worktree_programs::Program {
            pid,
            birth: original.birth,
            label,
        });
        if rows.len() > zerus_client_core::worktree_programs::MAX_PROGRAMS {
            return Err(limit());
        }
    }
    if ancestors(deadline)? != invocation {
        return Err(limit());
    }
    Ok(rows)
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
    fn only_one_actual_close_on_exec_read_only_root_pin_is_excluded() {
        use std::os::fd::AsRawFd;
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().canonicalize().unwrap();
        let until = || Instant::now() + std::time::Duration::from_secs(3);
        let scope = objects::Scope::open(&root, until()).unwrap();
        let inventory = scope.inventory(until()).unwrap();
        let id = objects::Object {
            device: scope.identity().device,
            inode: scope.identity().inode,
        };
        let file = std::fs::File::open(&root).unwrap();
        drop(scope);
        let id = HolderRoot {
            object: id,
            descriptor: file.as_raw_fd(),
        };
        assert!(!member_with_holder(std::process::id(), &inventory, Some(id), until()).unwrap());
        let wrong = HolderRoot {
            descriptor: file.as_raw_fd() + 1024,
            ..id
        };
        assert!(
            member_with_holder(std::process::id(), &inventory, Some(wrong), until()).is_err(),
            "a different descriptor cannot identify the original read-only holder pin"
        );
        // Duplicating the deliberate pin does not exempt the process or both
        // FDs. Actual BSD fi_openflags/PROC_FP_CLEXEC are read from the kernel.
        let copy = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        assert!(copy >= 0);
        use std::os::fd::FromRawFd;
        let copy = unsafe { std::fs::File::from_raw_fd(copy) };
        assert!(member_with_holder(std::process::id(), &inventory, Some(id), until()).unwrap());
        drop(copy);
        assert_eq!(
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) },
            0
        );
        assert!(member_with_holder(std::process::id(), &inventory, Some(id), until()).is_err());
    }

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
