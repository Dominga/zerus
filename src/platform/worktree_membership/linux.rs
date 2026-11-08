use super::*;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use std::io::Read;

fn read(path: &Path) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(std::io::Error::other("process metadata exceeds its bound"));
    }
    String::from_utf8(bytes).map_err(std::io::Error::other)
}
fn identity(path: &Path) -> std::io::Result<(String, bool, u32)> {
    let raw = read(&path.join("stat"))?;
    let fields: Vec<_> = raw
        .rsplit_once(") ")
        .ok_or_else(|| std::io::Error::other("invalid process identity"))?
        .1
        .split_whitespace()
        .collect();
    let start = fields
        .get(19)
        .ok_or_else(|| std::io::Error::other("invalid process start"))?;
    let parent = fields
        .get(1)
        .ok_or_else(|| std::io::Error::other("invalid process parent"))?
        .parse()
        .map_err(std::io::Error::other)?;
    Ok((
        (*start).to_owned(),
        matches!(fields.first(), Some(&"Z" | &"X")),
        parent,
    ))
}
fn ancestors(
    deadline: std::time::Instant,
) -> std::io::Result<std::collections::BTreeMap<u32, String>> {
    let mut rows = std::collections::BTreeMap::new();
    let mut pid = std::process::id();
    while pid != 0 {
        if rows.len() >= 128 || std::time::Instant::now() >= deadline || rows.contains_key(&pid) {
            return Err(std::io::Error::other("incomplete invocation ancestry"));
        }
        let (start, exited, parent) = identity(&PathBuf::from(format!("/proc/{pid}")))?;
        if exited {
            return Err(std::io::Error::other("exited invocation ancestor"));
        }
        rows.insert(pid, start);
        pid = parent;
    }
    Ok(rows)
}

pub(in crate::platform) fn sample(
    deadline: std::time::Instant,
) -> std::result::Result<super::WorktreeProcesses, String> {
    let deadline = deadline.min(std::time::Instant::now() + std::time::Duration::from_secs(2));
    let uid = unsafe { libc::getuid() };
    let error =
        || "Running process coverage is incomplete; review the worktree manually".to_owned();
    let invocation = ancestors(deadline).map_err(|_| error())?;
    #[cfg(not(test))]
    let entries = fs::read_dir("/proc").map_err(|_| error())?;
    #[cfg(test)]
    let entries = fs::read_dir(roster::current()).map_err(|_| error())?;
    let boot = read(Path::new("/proc/sys/kernel/random/boot_id")).map_err(|_| error())?;
    let mut rows = super::WorktreeProcesses::default();
    let mut budget = PathBudget::default();
    for (count, entry) in entries.enumerate() {
        if count >= 32768 || std::time::Instant::now() >= deadline {
            return Err(error());
        }
        let entry = entry.map_err(|_| error())?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let path = entry.path();
        let status = match read(&path.join("status")) {
            Ok(status) => status,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(error()),
        };
        let uids = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .ok_or_else(error)?
            .split_whitespace()
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| error())?;
        if uids.len() != 4 {
            return Err(error());
        }
        if !uids.contains(&uid) {
            continue;
        }
        let before = match identity(&path) {
            Ok(before) => before,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(error()),
        };
        // An exited leader with live tasks is a live process (zerus-lo8o1).
        let tasks = match task_plan(&path, pid, before.1, deadline) {
            Ok(Some(tasks)) => tasks,
            Ok(None) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !path.exists() => continue,
            Err(_) => return Err(error()),
        };
        let links = |task: &Path| {
            [
                fs::read_link(task.join("cwd")),
                fs::read_link(task.join("root")),
            ]
        };
        let mut contexts = Vec::from(links(&tasks.reference));
        let image = fs::read_link(tasks.reference.join("exe"));
        let files = files(&tasks.reference, deadline);
        // Further tasks with an fs context or descriptor table of their own;
        // a task gone meanwhile has none.
        for task in &tasks.fs {
            for link in links(task) {
                match link {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound && !task.exists() => {}
                    other => contexts.push(other),
                }
            }
        }
        let mut tables = Vec::new();
        for task in &tasks.files {
            match self::files(task, deadline) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !task.exists() => {}
                other => tables.push(other),
            }
        }
        let after = match identity(&path) {
            Ok(after) => after,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(error()),
        };
        if after.1 != before.1 {
            // The leader exited during the reads: only a group with no live
            // task left has exited.
            if !before.1 && matches!(task_plan(&path, pid, true, deadline), Ok(None)) {
                continue;
            }
            return Err(error());
        }
        if before != after {
            return Err(error());
        }
        if let Some(live) = &tasks.live {
            if live_tasks(&path, pid, deadline).map_err(|_| error())? != *live {
                return Err(error());
            }
        }
        let name = read(&path.join("comm"))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        // Only access denials on stable, live, fully identified processes are
        // reviewable. Timeouts, roster/identity errors and other IO errors stay
        // hard failures. A readable half still protects its actual worktree.
        let is_denied = |value: &std::io::Result<PathBuf>| value.as_ref().is_err_and(denied);
        let (cwd_denied, image_denied) = (contexts.iter().any(is_denied), is_denied(&image));
        if contexts
            .iter()
            .any(|link| link.is_err() && !is_denied(link))
            || (image.is_err() && !image_denied)
        {
            return Err(error());
        }
        let mut files = files.map_err(|_| error())?;
        for table in tables {
            let table = table.map_err(|_| error())?;
            files.denied |= table.denied;
            for path in table.paths {
                files.push(path).map_err(|_| error())?;
            }
        }
        if cwd_denied || image_denied || files.denied {
            rows.unreadable.push(super::UnreadableWorktreeProcess {
                pid,
                identity: format!("{}:{}", boot.trim(), before.0),
                command: name.clone(),
                cwd: cwd_denied,
                executable: image_denied,
                files: files.denied,
                invocation_ancestor: invocation.get(&pid) == Some(&before.0),
            });
        }
        // Every cwd, and every root but the host's own: a chroot or bwrap
        // root inside a worktree uses it as much as a cwd does.
        for context in contexts.into_iter().flatten() {
            if context == Path::new("/") {
                continue;
            }
            budget
                .push(&mut rows, pid, context, &name)
                .map_err(|_| error())?;
        }
        if let Ok(image) = image {
            budget
                .push(&mut rows, pid, image, &name)
                .map_err(|_| error())?;
        }
        for path in files.paths {
            budget
                .push(&mut rows, pid, path, &name)
                .map_err(|_| error())?;
        }
    }
    if std::time::Instant::now() >= deadline
        || ancestors(deadline).map_err(|_| error())? != invocation
    {
        return Err(error());
    }
    rows.unreadable.sort_by_key(|row| row.pid);
    rows.membership_complete = true;
    Ok(rows)
}

fn files(process: &Path, deadline: std::time::Instant) -> io::Result<Files> {
    let mut out = Files::default();
    let entries = match fs::read_dir(process.join("fd")) {
        Ok(entries) => entries,
        Err(error) if denied(&error) => {
            out.denied = true;
            return Ok(out);
        }
        Err(error) => return Err(error),
    };
    for (count, entry) in entries.enumerate() {
        bounded(count, deadline)?;
        match fs::read_link(entry?.path()) {
            Ok(path) if path.is_absolute() => out.push(path)?,
            Ok(_) => {} // Pipes, sockets and anonymous descriptors have no filesystem path.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {} // This FD closed.
            Err(error) if denied(&error) => out.denied = true,
            Err(error) => return Err(error),
        }
    }
    Ok(out)
}

/// Unlike the generic review's display paths, native object observations can
/// establish membership for the explicit program manifest. Opening /proc
/// magic links as O_PATH never opens a FIFO/device for IO or trusts its name.
fn native_object(path: &Path) -> io::Result<objects::Object> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { fs::File::from_raw_fd(fd) };
    let mut value = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(file.as_raw_fd(), &mut value) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(objects::object(&value))
}

/// kcmp(2) types from uapi linux/kcmp.h: the descriptor table and the
/// filesystem context (cwd and root) a task uses.
const KCMP_FILES: libc::c_int = 2;
const KCMP_FS: libc::c_int = 3;

/// Whether kcmp(2) proves that task `other` uses the `kind` object of task
/// `reference`. A different object, an unavailable kcmp or any error proves
/// nothing, and the task is then read itself.
fn shares(reference: u32, other: u32, kind: libc::c_int) -> bool {
    let (Ok(reference), Ok(other)) = (
        libc::pid_t::try_from(reference),
        libc::pid_t::try_from(other),
    ) else {
        return false;
    };
    let zero: libc::c_ulong = 0;
    unsafe { libc::syscall(libc::SYS_kcmp, reference, other, kind, zero, zero) == 0 }
}

/// The tasks one process's membership reads (zerus-lo8o1). The tasks of a
/// thread group need not share one fs context or descriptor table, and its
/// leader can exit while other tasks live on, so the leader's own entry
/// alone proves nothing then.
struct Tasks {
    /// Read for its cwd, root and descriptors: the leader's own entry while
    /// the leader lives, otherwise the first live task's.
    reference: PathBuf,
    /// Whether `reference` is the leader's own entry.
    leader: bool,
    /// Further live tasks with an fs context of their own, or that kcmp
    /// could not compare: read for their cwd and root.
    fs: Vec<PathBuf>,
    /// Further live tasks with a descriptor table of their own, or that kcmp
    /// could not compare: read for their descriptors.
    files: Vec<PathBuf>,
    /// For an exited leader: the live tasks, which must be the same after
    /// the reads.
    live: Option<Vec<u32>>,
}

fn task_ids(process: &Path, deadline: Instant) -> io::Result<Vec<u32>> {
    let mut ids = Vec::new();
    for (count, entry) in fs::read_dir(process.join("task"))?.enumerate() {
        bounded(count, deadline)?;
        if let Some(tid) = entry?
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        {
            ids.push(tid);
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

/// The live tasks of a group whose leader exited, each by its own state. A
/// task gone meanwhile is left out; any other read error is uncertainty.
fn live_tasks(process: &Path, pid: u32, deadline: Instant) -> io::Result<Vec<u32>> {
    let mut live = Vec::new();
    for tid in task_ids(process, deadline)? {
        if tid == pid {
            continue;
        }
        let task = process.join("task").join(tid.to_string());
        match identity(&task) {
            Ok((_, false, _)) => live.push(tid),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound && !task.exists() => {}
            Err(e) => return Err(e),
        }
    }
    Ok(live)
}

/// The tasks to read for `process`, whose leader is `exited` or not. None:
/// the leader exited and no task of its group lives.
fn task_plan(
    process: &Path,
    pid: u32,
    exited: bool,
    deadline: Instant,
) -> io::Result<Option<Tasks>> {
    let (reference_id, reference, others, live) = if exited {
        let live = live_tasks(process, pid, deadline)?;
        let Some(&first) = live.first() else {
            return Ok(None);
        };
        let reference = process.join("task").join(first.to_string());
        (first, reference, live[1..].to_vec(), Some(live))
    } else {
        let mut others = task_ids(process, deadline)?;
        others.retain(|&tid| tid != pid);
        (pid, process.to_path_buf(), others, None)
    };
    let (mut fs_tasks, mut files_tasks) = (Vec::new(), Vec::new());
    for (count, &tid) in others.iter().enumerate() {
        bounded(count, deadline)?;
        let task = process.join("task").join(tid.to_string());
        if !shares(reference_id, tid, KCMP_FS) {
            fs_tasks.push(task.clone());
        }
        if !shares(reference_id, tid, KCMP_FILES) {
            files_tasks.push(task);
        }
    }
    Ok(Some(Tasks {
        reference,
        leader: !exited,
        fs: fs_tasks,
        files: files_tasks,
        live,
    }))
}

/// The process ID an entry names: its own number, or /proc/self's target.
fn process_id(process: &Path) -> io::Result<u32> {
    let name = match process.file_name().and_then(|name| name.to_str()) {
        Some("self") => fs::read_link(process)?.to_string_lossy().into_owned(),
        Some(name) => name.to_owned(),
        None => return Err(limit()),
    };
    name.parse().map_err(|_| limit())
}

pub(super) fn member(
    process: &Path,
    inventory: &objects::Inventory,
    deadline: Instant,
) -> io::Result<bool> {
    member_with_holder(process, inventory, None, deadline)
}
/// One process's membership, its identity and, for an exited leader, its
/// live tasks unchanged across the reads.
fn member_with_holder(
    process: &Path,
    inventory: &objects::Inventory,
    root: Option<HolderRoot>,
    deadline: Instant,
) -> io::Result<bool> {
    let pid = process_id(process)?;
    let before = identity(process)?;
    // A group with no live task has no cwd to read: as before, NotFound.
    let tasks = task_plan(process, pid, before.1, deadline)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the process has exited"))?;
    let found = member_tasks(&tasks, inventory, root, deadline)?;
    let after = identity(process)?;
    if before != after {
        return Err(limit());
    }
    if let Some(live) = &tasks.live {
        if live_tasks(process, pid, deadline)? != *live {
            return Err(limit());
        }
    }
    Ok(found)
}

/// Membership through every task `tasks` names. The holder exclusion
/// applies to the leader's own descriptor table only; a holder whose leader
/// exited is never proven.
fn member_tasks(
    tasks: &Tasks,
    inventory: &objects::Inventory,
    root: Option<HolderRoot>,
    deadline: Instant,
) -> io::Result<bool> {
    bounded(0, deadline)?;
    let strict = root.is_some();
    if strict && !tasks.leader {
        return Err(limit());
    }
    let mut found = fs_member(&tasks.reference, inventory, strict)?;
    found |= files_member(&tasks.reference, inventory, root, strict, deadline)?;
    for task in &tasks.fs {
        found |= vanished_or(task, fs_member(task, inventory, strict))?;
    }
    for task in &tasks.files {
        found |= vanished_or(task, files_member(task, inventory, None, strict, deadline))?;
    }
    Ok(found)
}

/// A further task's result: a task gone meanwhile has no membership. A
/// missing read while its entry still exists stays uncertainty.
fn vanished_or(task: &Path, result: io::Result<bool>) -> io::Result<bool> {
    match result {
        Err(e) if e.kind() == io::ErrorKind::NotFound && !task.exists() => Ok(false),
        other => other,
    }
}

/// One task's fs context: whether its cwd or its root is in the checkout. A
/// chroot or bwrap root inside the checkout uses it as much as a cwd does.
fn fs_member(task: &Path, inventory: &objects::Inventory, strict: bool) -> io::Result<bool> {
    let mut found = false;
    for link in ["cwd", "root"] {
        match native_object(&task.join(link)) {
            Ok(id) => found |= inventory.contains(id),
            Err(e) if denied(&e) && !strict => {}
            Err(e) => return Err(e),
        }
    }
    Ok(found)
}

/// One task's descriptor table. `root` is the holder's exact root pin, which
/// is excluded once and must be found; `strict` makes a denial an error.
fn files_member(
    task: &Path,
    inventory: &objects::Inventory,
    root: Option<HolderRoot>,
    strict: bool,
    deadline: Instant,
) -> io::Result<bool> {
    let mut found = false;
    let mut skipped = false;
    let fds = match fs::read_dir(task.join("fd")) {
        Ok(fds) => fds,
        Err(e) if denied(&e) && !strict => return Ok(false),
        Err(e) => return Err(e),
    };
    for (count, fd) in fds.enumerate() {
        bounded(count, deadline)?;
        let fd = fd?;
        match native_object(&fd.path()) {
            Ok(id)
                if root.is_some_and(|root| {
                    root.object == id
                        && fd
                            .file_name()
                            .to_str()
                            .and_then(|name| name.parse::<i32>().ok())
                            == Some(root.descriptor)
                }) && !skipped =>
            {
                let info = read(&task.join("fdinfo").join(fd.file_name()))?;
                let flags = u32::from_str_radix(
                    info.lines()
                        .find_map(|line| line.strip_prefix("flags:"))
                        .ok_or_else(limit)?
                        .trim(),
                    8,
                )
                .map_err(io::Error::other)?;
                if flags & libc::O_ACCMODE as u32 != libc::O_RDONLY as u32
                    || flags & libc::O_CLOEXEC as u32 == 0
                    || native_object(&fd.path())? != id
                {
                    return Err(limit());
                }
                skipped = true;
            }
            Ok(id) => found |= inventory.contains(id),
            Err(e) if (denied(&e) && !strict) || e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    if root.is_some() && !skipped {
        return Err(limit());
    }
    Ok(found)
}

/// Account only for THIS Scope's actual native borrowed FD. Its descriptor
/// number comes from the owned object, never the wire or an ignore-PID flag.
pub(super) fn own_clear(
    scope: &objects::Scope,
    inventory: &objects::Inventory,
    deadline: Instant,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let fd = scope.own_root_descriptor();
    let own = fd.as_raw_fd();
    let root = objects::Object {
        device: scope.identity().device,
        inode: scope.identity().inode,
    };
    let process = Path::new("/proc/self");
    if inventory.contains(native_object(&process.join("cwd"))?) {
        return Err(limit());
    }
    let mut seen = false;
    for (count, entry) in fs::read_dir(process.join("fd"))?.enumerate() {
        bounded(count, deadline)?;
        let entry = entry?;
        let number = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
            .ok_or_else(limit)?;
        match native_object(&entry.path()) {
            Ok(object) if number == own => {
                let descriptor = unsafe { libc::fcntl(own, libc::F_GETFD) };
                let access = unsafe { libc::fcntl(own, libc::F_GETFL) };
                if object != root
                    || seen
                    || descriptor < 0
                    || access < 0
                    || descriptor & libc::FD_CLOEXEC == 0
                    || access & libc::O_ACCMODE != libc::O_RDONLY
                {
                    return Err(limit());
                }
                seen = true;
            }
            Ok(object) if inventory.contains(object) => {
                return Err(io::Error::other(
                    "this cleanup helper also uses the checkout",
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    if !seen {
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
    #[cfg(not(test))]
    return collect_programs(inventory, holder, deadline, || fs::read_dir("/proc"));
    #[cfg(test)]
    return collect_programs(inventory, holder, deadline, || {
        fs::read_dir(roster::current())
    });
}

/// Test builds only (zerus-89pkd): the process roster this thread's census
/// lists. It is /proc unless a test owns a roster of its own subjects, so a
/// unit test's assertions about its own processes do not depend on every
/// other process of this user on a shared host staying inspectable while it
/// exits. The classifier, identity checks and uncertainty rules are the
/// production ones; production builds always list /proc.
#[cfg(test)]
pub(crate) mod roster {
    use std::{
        cell::RefCell,
        path::{Path, PathBuf},
    };

    thread_local! {
        static OWNED: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    /// The directory this thread's census lists.
    pub(crate) fn current() -> PathBuf {
        OWNED
            .with(|owned| owned.borrow().clone())
            .unwrap_or_else(|| PathBuf::from("/proc"))
    }

    /// While alive, this thread's census lists only these processes, each
    /// through its own real /proc entry.
    pub(crate) struct Owned {
        dir: tempfile::TempDir,
    }
    impl Owned {
        /// The roster directory, for a control's synthetic entry.
        pub(crate) fn path(&self) -> &Path {
            self.dir.path()
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            OWNED.with(|owned| *owned.borrow_mut() = None);
        }
    }

    pub(crate) fn owned(pids: &[u32]) -> Owned {
        let dir = tempfile::tempdir().unwrap();
        for pid in pids {
            std::os::unix::fs::symlink(format!("/proc/{pid}"), dir.path().join(pid.to_string()))
                .unwrap();
        }
        OWNED.with(|owned| {
            let mut owned = owned.borrow_mut();
            assert!(owned.is_none(), "one owned roster per test thread");
            *owned = Some(dir.path().to_path_buf());
        });
        Owned { dir }
    }
}

// The production entry always supplies the complete fixed kernel roster.
// Tests can exercise the same classifier with only their owned live subjects,
// without assuming every unrelated process on the host stays inspectable.
fn collect_programs(
    inventory: &objects::Inventory,
    holder: Option<&Custody<'_>>,
    deadline: Instant,
    entries: impl FnOnce() -> io::Result<fs::ReadDir>,
) -> io::Result<Vec<zerus_client_core::worktree_programs::Program>> {
    let uid = unsafe { libc::getuid() };
    let invocation = ancestors(deadline)?;
    let mut rows = Vec::new();
    for (count, entry) in entries()?.enumerate() {
        if count >= 32_768 || Instant::now() >= deadline {
            return Err(limit());
        }
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        // The observer deliberately holds the root FD; it is never a signal target.
        // The action owner separately checks its own cwd, image and data resources.
        if pid <= 1 || pid == std::process::id() {
            continue;
        }
        let process = entry.path();
        let inspect = || -> io::Result<Option<zerus_client_core::worktree_programs::Program>> {
            let status = read(&process.join("status"))?;
            let uids: Vec<u32> = status
                .lines()
                .find_map(|line| line.strip_prefix("Uid:"))
                .ok_or_else(limit)?
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .map_err(io::Error::other)?;
            if uids.len() != 4 {
                return Err(limit());
            }
            if !uids.contains(&uid) {
                return Ok(None);
            }
            let before = identity(&process)?;
            // An exited leader with live tasks is a live process (zerus-lo8o1).
            let Some(tasks) = task_plan(&process, pid, before.1, deadline)? else {
                return Ok(None);
            };
            let root = holder.and_then(|h| h.root_for(pid));
            let membership = member_tasks(&tasks, inventory, root, deadline);
            let after = identity(&process)?;
            if after.1 != before.1 {
                // The leader exited during the reads: only a group with no
                // live task left has exited.
                if !before.1 && task_plan(&process, pid, true, deadline)?.is_none() {
                    return Ok(None);
                }
                return Err(limit());
            }
            if before != after {
                return Err(limit());
            }
            if let Some(live) = &tasks.live {
                if live_tasks(&process, pid, deadline)? != *live {
                    return Err(limit());
                }
            }
            if !membership? {
                return Ok(None);
            }
            if root.is_some() || invocation.get(&pid) == Some(&before.0) {
                return Err(io::Error::other(
                    "a program in the checkout is needed to settle this action",
                ));
            }
            let name = read(&process.join("comm"))?;
            let label = name.trim().to_owned();
            if label.is_empty() || label.len() > 512 || label.chars().any(char::is_control) {
                return Err(limit());
            }
            Ok(Some(zerus_client_core::worktree_programs::Program {
                pid,
                birth: before.0,
                label,
            }))
        };
        match inspect() {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound && !process.exists() => {}
            Err(e) => return Err(e),
        }
        if rows.len() > zerus_client_core::worktree_programs::MAX_PROGRAMS {
            return Err(limit());
        }
    }
    if ancestors(deadline)? != invocation {
        return Err(limit());
    }
    rows.sort_by_key(|row| row.pid);
    Ok(rows)
}

#[cfg(test)]
pub(super) fn fixture_programs(
    inventory: &objects::Inventory,
    deadline: Instant,
    roster: &Path,
) -> io::Result<Vec<zerus_client_core::worktree_programs::Program>> {
    collect_programs(inventory, None, deadline, || fs::read_dir(roster))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_holder_exclusion_is_one_native_read_only_root_fd_not_a_process_exemption() {
        use std::{
            io::{BufRead, Write},
            process::{Command, Stdio},
            time::Duration,
        };
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        fs::write(root.join("held"), b"original").unwrap();
        struct HeldChild(std::process::Child);
        impl Drop for HeldChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = HeldChild(
            Command::new("python3")
                .args([
                    "-I",
                    "-c",
                    r#"import os,sys
root=sys.argv[1]
fd=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
original=fd
print('ready',fd,flush=True)
for op in sys.stdin:
 op=op.strip()
 if op=='duplicate': other=os.dup(fd)
 elif op=='file': other=os.open(root+'/held',os.O_RDONLY|os.O_CLOEXEC)
 elif op=='close': os.close(other)
 elif op=='inherit': os.set_inheritable(fd,True)
 elif op=='cloexec': os.set_inheritable(fd,False)
 elif op=='cwd': os.chdir(root)
 elif op=='relocate':
  other=os.dup(fd);os.close(fd);fd=other
 elif op=='restore':
  os.dup2(fd,original,inheritable=False);os.close(fd);fd=original
 elif op=='replace':
  other=os.open(os.path.dirname(root),os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
  os.dup2(other,fd,inheritable=False);os.close(other)
 print('ready',fd,flush=True)
"#,
                ])
                .arg(&root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut output = std::io::BufReader::new(child.0.stdout.take().unwrap());
        let mut ready = || {
            use std::os::fd::AsRawFd;
            let mut poll = libc::pollfd {
                fd: output.get_ref().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            assert_eq!(unsafe { libc::poll(&mut poll, 1, 5000) }, 1);
            let mut line = String::new();
            output.read_line(&mut line).unwrap();
            line.strip_prefix("ready ")
                .unwrap()
                .trim()
                .parse::<i32>()
                .unwrap()
        };
        let original_descriptor = ready();
        let until = || Instant::now() + Duration::from_secs(3);
        let scope = objects::Scope::open(&root, until()).unwrap();
        let inventory = scope.inventory(until()).unwrap();
        let root = HolderRoot {
            object: objects::Object {
                device: scope.identity().device,
                inode: scope.identity().inode,
            },
            descriptor: original_descriptor,
        };
        let path = PathBuf::from(format!("/proc/{}", child.0.id()));
        assert!(!member_with_holder(&path, &inventory, Some(root), until()).unwrap());
        for op in ["duplicate", "file"] {
            writeln!(child.0.stdin.as_mut().unwrap(), "{op}").unwrap();
            ready();
            assert!(
                member_with_holder(&path, &inventory, Some(root), until()).unwrap(),
                "{op}"
            );
            writeln!(child.0.stdin.as_mut().unwrap(), "close").unwrap();
            ready();
        }
        writeln!(child.0.stdin.as_mut().unwrap(), "inherit").unwrap();
        ready();
        assert!(member_with_holder(&path, &inventory, Some(root), until()).is_err());
        writeln!(child.0.stdin.as_mut().unwrap(), "cloexec").unwrap();
        ready();
        assert!(!member_with_holder(&path, &inventory, Some(root), until()).unwrap());
        writeln!(child.0.stdin.as_mut().unwrap(), "relocate").unwrap();
        assert_ne!(ready(), original_descriptor);
        assert!(
            member_with_holder(&path, &inventory, Some(root), until()).is_err(),
            "the same root at another FD cannot replace the original held descriptor"
        );
        writeln!(child.0.stdin.as_mut().unwrap(), "restore").unwrap();
        assert_eq!(ready(), original_descriptor);
        assert!(!member_with_holder(&path, &inventory, Some(root), until()).unwrap());
        writeln!(child.0.stdin.as_mut().unwrap(), "cwd").unwrap();
        ready();
        assert!(member_with_holder(&path, &inventory, Some(root), until()).unwrap());
        writeln!(child.0.stdin.as_mut().unwrap(), "replace").unwrap();
        assert_eq!(ready(), original_descriptor);
        assert!(
            member_with_holder(&path, &inventory, Some(root), until()).is_err(),
            "reusing the original FD for a different directory cannot clear custody"
        );
    }

    #[test]
    fn native_fd_membership_finds_an_open_file_outside_the_current_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("held-original");
        let _file = fs::File::create(&path).unwrap();
        assert!(!std::env::current_dir().unwrap().starts_with(dir.path()));
        let sample = files(
            Path::new("/proc/self"),
            std::time::Instant::now() + std::time::Duration::from_secs(2),
        )
        .unwrap();
        assert!(!sample.denied);
        assert!(
            sample.paths.contains(&path),
            "open-file membership must not be inferred from cwd only"
        );
    }

    #[test]
    fn native_manifest_uses_the_held_file_object_not_a_prefix_or_replaced_name() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("work");
        fs::create_dir(&root).unwrap();
        let inside = root.join("original");
        let outside = parent.path().join("work-with-similar-name");
        fs::write(&inside, b"original").unwrap();
        fs::write(&outside, b"other").unwrap();
        let until = || Instant::now() + std::time::Duration::from_secs(3);
        let scope = objects::Scope::open(&root, until()).unwrap();
        let original = fs::File::open(&inside).unwrap();
        let id = native_object(&PathBuf::from(format!(
            "/proc/self/fd/{}",
            std::os::fd::AsRawFd::as_raw_fd(&original)
        )))
        .unwrap();
        assert!(scope.inventory(until()).unwrap().contains(id));
        let inventory = scope.inventory(until()).unwrap();
        drop(scope); // The observer's intentional root pin is not test evidence.
        assert!(member(Path::new("/proc/self"), &inventory, until()).unwrap());
        fs::rename(&inside, parent.path().join("moved-away")).unwrap();
        fs::write(&inside, b"replacement").unwrap();
        let scope = objects::Scope::open(&root, until()).unwrap();
        let inventory = scope.inventory(until()).unwrap();
        drop(scope);
        assert!(!inventory.contains(id));
        assert!(!member(Path::new("/proc/self"), &inventory, until()).unwrap());
        std::os::unix::fs::symlink(&outside, root.join("outside-alias")).unwrap();
        let _other = fs::File::open(root.join("outside-alias")).unwrap();
        let scope = objects::Scope::open(&root, until()).unwrap();
        let inventory = scope.inventory(until()).unwrap();
        drop(scope);
        assert!(!member(Path::new("/proc/self"), &inventory, until()).unwrap());
    }

    /// zerus-89pkd: the census of a test build lists /proc unless a test owns
    /// a roster, and an owned roster ends with its guard.
    #[test]
    fn the_default_census_roster_is_proc() {
        assert_eq!(roster::current(), PathBuf::from("/proc"));
        let owned = roster::owned(&[std::process::id()]);
        assert_eq!(roster::current(), owned.path());
        assert_eq!(
            fs::read_link(owned.path().join(std::process::id().to_string())).unwrap(),
            PathBuf::from(format!("/proc/{}", std::process::id()))
        );
        drop(owned);
        assert_eq!(roster::current(), PathBuf::from("/proc"));
    }

    /// zerus-89pkd: an owned roster never hides uncertainty. An entry that
    /// still exists but whose cwd cannot be read, as a process caught while it
    /// exits, fails the census NotFound exactly as /proc does; it is never an
    /// empty roster. A real owned occupant is still listed.
    #[test]
    fn an_owned_roster_keeps_a_missing_read_in_an_existing_entry_uncertain() {
        use std::process::{Command, Stdio};
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let mut occupant = Command::new("/bin/sleep")
            .arg("20")
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let until = || Instant::now() + std::time::Duration::from_secs(5);
        let scope = objects::Scope::open(&root, until()).unwrap();
        let inventory = scope.inventory(until()).unwrap();
        let owned = roster::owned(&[occupant.id()]);
        let rows = programs(&inventory, until()).unwrap();
        assert!(rows.iter().any(|row| row.pid == occupant.id()));
        // The occupant's own status and identity, without a cwd or fds.
        let real = PathBuf::from(format!("/proc/{}", occupant.id()));
        let entry = owned.path().join(u32::MAX.to_string());
        fs::create_dir(&entry).unwrap();
        fs::create_dir(entry.join("task")).unwrap();
        for name in ["status", "stat"] {
            fs::write(entry.join(name), fs::read(real.join(name)).unwrap()).unwrap();
        }
        let error = programs(&inventory, until()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        drop(owned);
        occupant.kill().unwrap();
        occupant.wait().unwrap();
    }

    /// A real python3 child for the zerus-lo8o1 controls, killed on drop.
    struct Python(std::process::Child);
    impl Python {
        fn start(cwd: &Path, code: &str) -> Self {
            use std::io::BufRead;
            let mut child = std::process::Command::new("python3")
                .args(["-I", "-c", code])
                .current_dir(cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let mut line = String::new();
            std::io::BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(line, "ready\n", "the control child did not start");
            Python(child)
        }
        fn pid(&self) -> u32 {
            self.0.id()
        }
        /// Waits, bounded, until the leader's own state reads `state`.
        fn leader(&self, state: char) {
            let until = Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let raw = fs::read_to_string(format!("/proc/{}/stat", self.pid())).unwrap();
                if raw
                    .rsplit_once(") ")
                    .and_then(|(_, rest)| rest.chars().next())
                    == Some(state)
                {
                    return;
                }
                assert!(Instant::now() < until, "the leader never reached {state}");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
    impl Drop for Python {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn checkout() -> (
        tempfile::TempDir,
        PathBuf,
        objects::Scope,
        objects::Inventory,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::write(root.join("held"), b"original").unwrap();
        let until = Instant::now() + std::time::Duration::from_secs(5);
        let scope = objects::Scope::open(&root, until).unwrap();
        let inventory = scope.inventory(until).unwrap();
        (dir, root, scope, inventory)
    }
    fn soon() -> Instant {
        Instant::now() + std::time::Duration::from_secs(5)
    }
    fn sampled(pid: u32, root: &Path) -> bool {
        let rows = sample(soon()).unwrap();
        rows.paths
            .iter()
            .any(|(p, path, _)| *p == pid && path.starts_with(root))
    }

    /// zerus-lo8o1: a process whose main thread exited while another thread
    /// lives on with its cwd and an open file in the checkout is a live
    /// occupant for the census, member() and sample() alike, under the
    /// leader's own pid and birth.
    #[test]
    fn an_exited_leader_with_a_live_thread_in_the_checkout_is_an_occupant() {
        let (_dir, root, _scope, inventory) = checkout();
        let child = Python::start(
            &root,
            "import ctypes,threading,time\nheld=open('held','rb')\nthreading.Thread(target=lambda: time.sleep(30)).start()\nprint('ready',flush=True)\ntime.sleep(0.1)\nctypes.CDLL(None).pthread_exit(None)\n",
        );
        child.leader('Z');
        let process = PathBuf::from(format!("/proc/{}", child.pid()));
        let birth = identity(&process).unwrap().0;
        let _owned = roster::owned(&[child.pid()]);
        let rows = programs(&inventory, soon()).unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row.pid, row.birth.clone()))
                .collect::<Vec<_>>(),
            [(child.pid(), birth)]
        );
        assert!(member(&process, &inventory, soon()).unwrap());
        assert!(sampled(child.pid(), &root));
    }

    /// zerus-lo8o1: a live process whose leader is outside the checkout but
    /// one of whose threads has its own fs context (CLONE_FS unshared) with
    /// its cwd inside, or its own descriptor table (CLONE_FILES unshared)
    /// with a file inside, is an occupant.
    #[test]
    fn a_thread_with_its_own_fs_context_or_descriptor_table_in_the_checkout_is_an_occupant() {
        let elsewhere = tempfile::tempdir().unwrap();
        for (unshare, work) in [
            ("0x200", "os.chdir(sys.argv[1])"),
            (
                "0x400",
                "fd=os.open(os.path.join(sys.argv[1],'held'),os.O_RDONLY)",
            ),
        ] {
            let (_dir, root, _scope, inventory) = checkout();
            let code = format!(
                "import ctypes,os,sys,threading,time\nsys.argv=['',{root:?}]\nready=threading.Event()\ndef work():\n    assert ctypes.CDLL(None,use_errno=True).unshare({unshare})==0\n    {work}\n    ready.set()\n    time.sleep(30)\nthreading.Thread(target=work,daemon=True).start()\nready.wait(5)\nprint('ready',flush=True)\ntime.sleep(30)\n",
                root = root.to_str().unwrap()
            );
            let child = Python::start(elsewhere.path(), &code);
            let process = PathBuf::from(format!("/proc/{}", child.pid()));
            assert!(
                !inventory.contains(native_object(&process.join("cwd")).unwrap()),
                "the leader itself is outside the checkout"
            );
            let _owned = roster::owned(&[child.pid()]);
            let rows = programs(&inventory, soon()).unwrap();
            assert!(rows.iter().any(|row| row.pid == child.pid()), "{unshare}");
            assert!(member(&process, &inventory, soon()).unwrap(), "{unshare}");
            assert!(sampled(child.pid(), &root), "{unshare}");
        }
    }

    /// zerus-lo8o1: a zombie whose group has no live task left is still an
    /// exited process: not listed, and member() cannot read it.
    #[test]
    fn a_zombie_with_no_live_task_is_still_exited() {
        let (_dir, root, _scope, inventory) = checkout();
        let child = Python::start(&root, "import os\nprint('ready',flush=True)\nos._exit(0)\n");
        child.leader('Z');
        let process = PathBuf::from(format!("/proc/{}", child.pid()));
        let _owned = roster::owned(&[child.pid()]);
        assert!(programs(&inventory, soon()).unwrap().is_empty());
        assert_eq!(
            member(&process, &inventory, soon()).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!sampled(child.pid(), &root));
    }

    /// A synthetic /proc entry for the zerus-lo8o1 controls: this process's
    /// own status and stat, the stat's state replaced by `state`.
    fn synthetic(entry: &Path, state: char) {
        fs::create_dir_all(entry.join("fd")).unwrap();
        fs::write(entry.join("status"), fs::read("/proc/self/status").unwrap()).unwrap();
        fs::write(entry.join("comm"), "synthetic\n").unwrap();
        let _ = fs::remove_file(entry.join("exe"));
        std::os::unix::fs::symlink("/bin/true", entry.join("exe")).unwrap();
        let stat = fs::read_to_string("/proc/self/stat").unwrap();
        let (head, rest) = stat.rsplit_once(") ").unwrap();
        fs::write(entry.join("stat"), format!("{head}) {state}{}", &rest[1..])).unwrap();
    }

    /// zerus-lo8o1: a root inside the checkout is membership, as a cwd is;
    /// a task that kcmp cannot compare with the leader is read itself; and
    /// an exited leader whose live task cannot be read is uncertainty,
    /// never an empty roster.
    #[test]
    fn roots_uncomparable_tasks_and_unreadable_live_tasks_are_never_absent() {
        let (_dir, root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        // IDs a pid_t can name but no task holds, so kcmp(2) runs and fails.
        let pid = i32::MAX as u32;
        let (first, further) = (pid - 2, pid - 1);
        let link = |target: &Path, at: PathBuf| {
            let _ = fs::remove_file(&at);
            std::os::unix::fs::symlink(target, at).unwrap();
        };
        // A leader whose root, not its cwd, is the checkout.
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'S');
        fs::create_dir(entry.join("task")).unwrap();
        link(Path::new("/"), entry.join("cwd"));
        link(&root, entry.join("root"));
        assert_eq!(
            programs(&inventory, soon()).unwrap().len(),
            1,
            "root membership"
        );
        assert!(member_with_holder(&entry, &inventory, None, soon()).unwrap());
        assert!(sampled(pid, &root), "a root in the checkout is sampled");
        // Its root outside, but a task kcmp cannot compare is read itself
        // and has its cwd in the checkout.
        link(Path::new("/"), entry.join("root"));
        assert!(programs(&inventory, soon()).unwrap().is_empty());
        let task = entry.join("task").join(first.to_string());
        synthetic(&task, 'S');
        link(&root, task.join("cwd"));
        link(Path::new("/"), task.join("root"));
        assert_eq!(
            programs(&inventory, soon()).unwrap().len(),
            1,
            "uncompared task"
        );
        // An exited leader: its live tasks decide, and a group whose tasks
        // all exited is exited, whatever their entries still show.
        synthetic(&entry, 'Z');
        synthetic(&task, 'Z');
        assert!(
            programs(&inventory, soon()).unwrap().is_empty(),
            "every task exited"
        );
        synthetic(&task, 'S');
        link(Path::new("/"), task.join("cwd"));
        assert!(
            programs(&inventory, soon()).unwrap().is_empty(),
            "both outside"
        );
        // A live task whose entry still exists without a readable cwd is
        // uncertainty, whether it is the first live task or a further one.
        let later = entry.join("task").join(further.to_string());
        synthetic(&later, 'S');
        link(Path::new("/"), later.join("root"));
        assert_eq!(
            programs(&inventory, soon()).unwrap_err().kind(),
            io::ErrorKind::NotFound,
            "a further live task without a cwd"
        );
        fs::remove_dir_all(&later).unwrap();
        fs::remove_file(task.join("cwd")).unwrap();
        assert_eq!(
            programs(&inventory, soon()).unwrap_err().kind(),
            io::ErrorKind::NotFound,
            "the first live task without a cwd"
        );
        drop(owned);
    }
}
