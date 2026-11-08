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
        // An exited leader with live tasks is a live process (zerus-lo8o1),
        // read through one stable observation of its tasks.
        let links = |task: &Path| {
            [
                fs::read_link(task.join("cwd")),
                fs::read_link(task.join("root")),
            ]
        };
        // Each planned task's reads, the reference's and those of further
        // tasks with an fs context or descriptor table of their own. Only an
        // actual denial is kept as such; any other failed read, a task gone
        // meanwhile included, leaves the observation unsettled (None).
        let reads = inspect(&path, pid, before.1, deadline, |tasks| {
            let mut contexts = Vec::new();
            for task in std::iter::once(&tasks.reference).chain(&tasks.fs) {
                for link in links(task) {
                    if link.as_ref().is_err_and(unsettled) {
                        return Ok(None);
                    }
                    contexts.push(link);
                }
            }
            let image = fs::read_link(tasks.reference.join("exe"));
            if image.as_ref().is_err_and(unsettled) {
                return Ok(None);
            }
            let Some(files) = files(&tasks.reference, deadline)? else {
                return Ok(None);
            };
            let mut tables = Vec::new();
            for task in &tasks.files {
                let Some(table) = self::files(task, deadline)? else {
                    return Ok(None);
                };
                tables.push(table);
            }
            Ok(Some((contexts, image, files, tables)))
        });
        let (contexts, image, files, tables) = match reads {
            Ok(Some(reads)) => reads,
            Ok(None) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !path.exists() => continue,
            Err(_) => return Err(error()),
        };
        let after = match identity(&path) {
            Ok(after) => after,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(error()),
        };
        if after.1 != before.1 {
            // The leader exited during the reads: only a group with no live
            // task left has exited.
            if !before.1 && matches!(group_exited(&path, pid, deadline), Ok(true)) {
                continue;
            }
            return Err(error());
        }
        if before != after {
            return Err(error());
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
        let mut files = files;
        for table in tables {
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

/// One task's descriptor paths. None: the table could not be read for a
/// reason other than a denial, so its observation is unsettled.
fn files(process: &Path, deadline: std::time::Instant) -> io::Result<Option<Files>> {
    let mut out = Files::default();
    let entries = match fs::read_dir(process.join("fd")) {
        Ok(entries) => entries,
        Err(error) if denied(&error) => {
            out.denied = true;
            return Ok(Some(out));
        }
        Err(error) if unsettled(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    #[cfg(test)]
    let entries = roster::listed(entries);
    for (count, entry) in entries.enumerate() {
        bounded(count, deadline)?;
        let Some(entry) = settled(entry)? else {
            return Ok(None);
        };
        match fs::read_link(entry.path()) {
            Ok(path) if path.is_absolute() => out.push(path)?,
            Ok(_) => {} // Pipes, sockets and anonymous descriptors have no filesystem path.
            Err(error) if closed(&error, process) => {}
            Err(error) if denied(&error) => out.denied = true,
            Err(error) if unsettled(&error) => return Ok(None),
            Err(error) => return Err(error),
        }
    }
    Ok(Some(out))
}

/// The read budget's own error, from bounded(): it ends the inspection.
fn budget(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::TimedOut && error.raw_os_error().is_none()
}

/// Whether a failed read of a planned task leaves its observation unsettled
/// (zerus-lo8o1.1, design-tima2's ruling), classified by the read's actual
/// error. An EACCES/EPERM denial keeps the existing per-read denial policy,
/// and the read budget ends the inspection. Any other failure (the task gone
/// with ENOENT or ESRCH, caught mid-exit, an I/O error, short or malformed
/// content) proves nothing about the task, and no conclusion of absence may
/// rest on it.
fn unsettled(error: &io::Error) -> bool {
    !denied(error) && !budget(error)
}

/// One read of a planned task, classified: None when it leaves the
/// observation unsettled, the error itself for a denial or the budget.
fn settled<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if unsettled(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// A descriptor closed between the table's listing and its own read: ENOENT
/// for it while the task's entry is still there. It is skipped, never
/// unsettled and never a denial. This per-descriptor rule predates
/// zerus-lo8o1 and is kept with its documented residual (see `same_tasks`,
/// zerus-1qnef); a task gone meanwhile is unsettled instead.
fn closed(error: &io::Error, task: &Path) -> bool {
    error.kind() == io::ErrorKind::NotFound && task.exists()
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

/// Whether kcmp(2) works here. Docker's default seccomp profile, for
/// example, refuses it without CAP_SYS_PTRACE (zerus-igl4q).
#[cfg(test)]
pub(crate) fn kcmp_available() -> bool {
    shares(std::process::id(), std::process::id(), KCMP_FILES)
}

/// The tasks one process's membership reads (zerus-lo8o1). The tasks of a
/// thread group need not share one fs context or descriptor table, and its
/// leader can exit while other tasks live on, so the leader's own entry
/// alone proves nothing then.
#[derive(PartialEq)]
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
}

/// One observed task: its ID and whether it has exited.
struct Task {
    id: u32,
    exited: bool,
}

/// One observation of a process's tasks, sorted by ID. Each task's own state
/// is read only while the leader has `exited`, to find the live ones. A live
/// leader's tasks are all taken as live: an exited one has no fs context or
/// descriptor table for kcmp to match, so it is read itself, and its missing
/// cwd unsettles the observation. None: a listed task's state could not be
/// read, for a reason other than a denial, the task gone meanwhile included.
/// It may have handed its context to a task the listing missed, so the
/// observation proves nothing.
fn task_set(process: &Path, exited: bool, deadline: Instant) -> io::Result<Option<Vec<Task>>> {
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
    #[cfg(test)]
    roster::after_list();
    let mut tasks = Vec::with_capacity(ids.len());
    for (count, id) in ids.into_iter().enumerate() {
        bounded(count, deadline)?;
        if !exited {
            tasks.push(Task { id, exited: false });
            continue;
        }
        let task = process.join("task").join(id.to_string());
        match settled(identity(&task))? {
            Some((_, exited, _)) => tasks.push(Task { id, exited }),
            None => return Ok(None),
        }
    }
    Ok(Some(tasks))
}

/// Whether two observations list the same task IDs.
///
/// An ID alone identifies a task here; per-task start times are not read
/// (zerus-lo8o1.1, design-tima2's ruling). No conclusion rests on a task
/// whose read failed: such a read leaves its observation unsettled (see
/// `unsettled`), however the ID looks by the second listing.
///
/// The accepted residual: a task read successfully as outside the checkout
/// exits, and a sibling in the checkout takes its ID before the second
/// listing. The kernel allocates task IDs with idr_alloc_cyclic, whose cursor
/// persists across calls, so near the wrap at pid_max a freed low ID can come
/// back after only a few allocations. The case needs a thread exit and a
/// near-wrap reuse of that exact ID inside one read window, in the same
/// thread group. Only a stable thread identity (a start time or a thread
/// pidfd) would close it, and the census deliberately reads none. Same UID is
/// the trust boundary. The leader keeps its pid and start-time identity,
/// which the callers check around the reads.
///
/// The accepted descriptor-handoff residual (zerus-1qnef, design-tima2's
/// ruling): a descriptor whose own read returns ENOENT while its task's
/// entry remains is a closed descriptor and is skipped (`closed`), as before
/// zerus-lo8o1. A process that duplicates a checkout file's descriptor to a
/// number the table's listing missed, and closes the old number, inside the
/// 1 to 4 ms table read is not seen through that file in that observation.
/// Two stricter rules were measured on arch: any descriptor ENOENT is
/// unsettled, or a second fd-number listing that changed is unsettled.
/// - For open/close, dup/close and close-then-reopen churn, both stay at 0 to
///   2% census limit, against 0% for this rule.
/// - Once a process opens new descriptors and later closes older ones, at
///   1k/s with 1000 held or 10k/s with 100 held, both reach 87 to 99%.
///   Because any limit fails the whole census, one such process anywhere
///   would fail Terminate/Remove closed host-wide.
/// - A second listing alone is also defeated by number reuse.
///
/// Skipping a descriptor that vanished mid-read is of a kind with the
/// explicit Remove policy, which already ignores descriptors that cannot be
/// inspected (EACCES/EPERM).
///
/// Where kcmp(2) is refused (for example under Docker's default seccomp
/// profile, without CAP_SYS_PTRACE), no task can be proven to share the
/// reference's fs context or descriptor table, so every task is read itself
/// (zerus-igl4q, design-tima2's ruling). Membership stays complete, at more
/// reads per process. A multi-threaded holder then refuses, because its own
/// root pin, seen through a thread's table, is not the leader's excluded pin.
/// The production holder is a single-threaded hgs child, and that refusal
/// fails closed.
///
/// The macOS census's thread residual (a live thread whose TSD base changes
/// mid-read) is documented at `thread_cwds` in macos.rs (zerus-q0cd5).
fn same_tasks(before: &[Task], after: &[Task]) -> bool {
    before
        .iter()
        .map(|task| task.id)
        .eq(after.iter().map(|task| task.id))
}

/// The tasks to read for `process` from one observation, its leader
/// `exited` or not. None: the leader exited and no task of its group lives.
fn task_plan(
    process: &Path,
    pid: u32,
    exited: bool,
    set: &[Task],
    deadline: Instant,
) -> io::Result<Option<Tasks>> {
    // An exited task other than the leader has released its fs context and
    // descriptor table: there is nothing of it to read.
    let mut live = set.iter().filter(|task| task.id != pid && !task.exited);
    let (reference_id, reference) = if exited {
        let Some(first) = live.next() else {
            return Ok(None);
        };
        (first.id, process.join("task").join(first.id.to_string()))
    } else {
        (pid, process.to_path_buf())
    };
    let (mut fs_tasks, mut files_tasks) = (Vec::new(), Vec::new());
    for (count, task) in live.enumerate() {
        bounded(count, deadline)?;
        let path = process.join("task").join(task.id.to_string());
        if !shares(reference_id, task.id, KCMP_FS) {
            fs_tasks.push(path.clone());
        }
        if !shares(reference_id, task.id, KCMP_FILES) {
            files_tasks.push(path);
        }
    }
    Ok(Some(Tasks {
        reference,
        leader: !exited,
        fs: fs_tasks,
        files: files_tasks,
    }))
}

/// Attempts at one stable observation of a process's tasks.
const ATTEMPTS: usize = 4;

/// Reads one process through its tasks (zerus-lo8o1, zerus-lo8o1.1). The
/// task set, by ID, and the plan's kcmp sharing decisions are observed
/// before and after the reads, and only an unchanged, settled observation
/// lets the reads stand. A task gone during them may have handed its context
/// to a task created since; a task left out as sharing the reference may
/// have unshared and kept a context the reference then left. An exited group
/// needs the same second observation: a live task the first listing missed
/// may still hold a context. A changed or unsettled observation is read
/// again, within ATTEMPTS and the deadline, and is never absence. None: the
/// leader exited and no task of its group lives.
fn inspect<T>(
    process: &Path,
    pid: u32,
    exited: bool,
    deadline: Instant,
    mut read: impl FnMut(&Tasks) -> io::Result<Option<T>>,
) -> io::Result<Option<T>> {
    for _ in 0..ATTEMPTS {
        let Some(set) = task_set(process, exited, deadline)? else {
            continue;
        };
        let tasks = task_plan(process, pid, exited, &set, deadline)?;
        #[cfg(test)]
        roster::after_plan();
        let result = match tasks.as_ref().map(&mut read) {
            Some(Ok(None)) => continue,
            Some(Ok(Some(value))) => Some(Ok(value)),
            Some(Err(e)) => Some(Err(e)),
            None => None,
        };
        #[cfg(test)]
        roster::after_read();
        let Some(after) = task_set(process, exited, deadline)? else {
            continue;
        };
        if same_tasks(&set, &after) && task_plan(process, pid, exited, &after, deadline)? == tasks {
            return result.transpose();
        }
    }
    Err(limit())
}

/// Whether a group whose leader exited has no live task left, from one
/// unchanged, settled observation like any other.
fn group_exited(process: &Path, pid: u32, deadline: Instant) -> io::Result<bool> {
    Ok(inspect(process, pid, true, deadline, |_| Ok(Some(())))?.is_none())
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
/// One process's membership, its identity unchanged across the reads.
fn member_with_holder(
    process: &Path,
    inventory: &objects::Inventory,
    root: Option<HolderRoot>,
    deadline: Instant,
) -> io::Result<bool> {
    let pid = process_id(process)?;
    let before = identity(process)?;
    // A group with no live task has no cwd to read: as before, NotFound.
    let found = inspect(process, pid, before.1, deadline, |tasks| {
        member_tasks(tasks, inventory, root, deadline)
    })?
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the process has exited"))?;
    let after = identity(process)?;
    if before != after {
        return Err(limit());
    }
    Ok(found)
}

/// Membership through every task `tasks` names. The holder exclusion
/// applies to the leader's own descriptor table only; a holder whose leader
/// exited is never proven.
/// One process's membership from its planned tasks. None: a planned task's
/// read failed for a reason other than a denial, the task gone meanwhile
/// included, so the observation is unsettled and the process is read again.
fn member_tasks(
    tasks: &Tasks,
    inventory: &objects::Inventory,
    root: Option<HolderRoot>,
    deadline: Instant,
) -> io::Result<Option<bool>> {
    bounded(0, deadline)?;
    let strict = root.is_some();
    if strict && !tasks.leader {
        return Err(limit());
    }
    let Some(mut found) = fs_member(&tasks.reference, inventory, strict)? else {
        return Ok(None);
    };
    let Some(files) = files_member(&tasks.reference, inventory, root, strict, deadline)? else {
        return Ok(None);
    };
    found |= files;
    for task in &tasks.fs {
        let Some(member) = fs_member(task, inventory, strict)? else {
            return Ok(None);
        };
        found |= member;
    }
    for task in &tasks.files {
        let Some(member) = files_member(task, inventory, None, strict, deadline)? else {
            return Ok(None);
        };
        found |= member;
    }
    Ok(Some(found))
}

/// One task's fs context: whether its cwd or its root is in the checkout. A
/// chroot or bwrap root inside the checkout uses it as much as a cwd does.
/// None: a read failed for a reason other than a denial.
fn fs_member(
    task: &Path,
    inventory: &objects::Inventory,
    strict: bool,
) -> io::Result<Option<bool>> {
    let mut found = false;
    for link in ["cwd", "root"] {
        match settled(native_object(&task.join(link))) {
            Ok(Some(id)) => found |= inventory.contains(id),
            Ok(None) => return Ok(None),
            Err(e) if denied(&e) && !strict => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Some(found))
}

/// One task's descriptor table. `root` is the holder's exact root pin, which
/// is excluded once and must be found; `strict` makes a denial an error.
/// None: a read failed for a reason other than a denial or a closed
/// descriptor.
fn files_member(
    task: &Path,
    inventory: &objects::Inventory,
    root: Option<HolderRoot>,
    strict: bool,
    deadline: Instant,
) -> io::Result<Option<bool>> {
    let mut found = false;
    let mut skipped = false;
    let fds = match settled(fs::read_dir(task.join("fd"))) {
        Ok(Some(fds)) => fds,
        Ok(None) => return Ok(None),
        Err(e) if denied(&e) && !strict => return Ok(Some(false)),
        Err(e) => return Err(e),
    };
    #[cfg(test)]
    let fds = roster::listed(fds);
    for (count, fd) in fds.enumerate() {
        bounded(count, deadline)?;
        let Some(fd) = settled(fd)? else {
            return Ok(None);
        };
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
            Err(e) if (denied(&e) && !strict) || closed(&e, task) => {}
            Err(e) if unsettled(&e) => return Ok(None),
            Err(e) => return Err(e),
        }
    }
    if root.is_some() && !skipped {
        return Err(limit());
    }
    Ok(Some(found))
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
        collections::BTreeSet,
        path::{Path, PathBuf},
    };

    thread_local! {
        static OWNED: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
        static REQUESTED: RefCell<BTreeSet<u32>> = const { RefCell::new(BTreeSet::new()) };
        static COMPLETED: RefCell<BTreeSet<u32>> = const { RefCell::new(BTreeSet::new()) };
        static AFTER_PLAN: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
        static AFTER_LIST: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
        static AFTER_READ: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
        static AFTER_FDS: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
    }

    /// A descriptor table's listing, collected before any of its reads, after
    /// which this thread's descriptor hook runs once (zerus-1qnef). Only test
    /// builds collect; production reads the listing as it goes.
    pub(crate) fn listed(
        fds: std::fs::ReadDir,
    ) -> std::vec::IntoIter<std::io::Result<std::fs::DirEntry>> {
        let listed: Vec<_> = fds.collect();
        let hook = AFTER_FDS.with(|hook| hook.borrow_mut().take());
        if let Some(mut hook) = hook {
            hook();
        }
        listed.into_iter()
    }
    pub(crate) fn on_fds(hook: impl FnMut() + 'static) {
        AFTER_FDS.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    /// Runs this thread's hook, once, between a process's reads and its
    /// second listing (zerus-lo8o1.1).
    pub(crate) fn after_read() {
        let hook = AFTER_READ.with(|hook| hook.borrow_mut().take());
        if let Some(mut hook) = hook {
            hook();
        }
    }
    pub(crate) fn on_read(hook: impl FnMut() + 'static) {
        AFTER_READ.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    /// Runs this thread's hook, once, between a task listing and the state
    /// reads of its tasks (zerus-lo8o1.1).
    pub(crate) fn after_list() {
        let hook = AFTER_LIST.with(|hook| hook.borrow_mut().take());
        if let Some(mut hook) = hook {
            hook();
        }
    }
    pub(crate) fn on_list(hook: impl FnMut() + 'static) {
        AFTER_LIST.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    /// Runs this thread's hook, once, between a process's task plan and its
    /// reads: a control's change to its own synthetic entry while the census
    /// is inside one process (zerus-lo8o1.1).
    pub(crate) fn after_plan() {
        let hook = AFTER_PLAN.with(|hook| hook.borrow_mut().take());
        if let Some(mut hook) = hook {
            hook();
        }
    }
    pub(crate) fn on_plan(hook: impl FnMut() + 'static) {
        AFTER_PLAN.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
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
        requested: BTreeSet<u32>,
    }
    impl Owned {
        /// The roster directory, for a control's synthetic entry.
        pub(crate) fn path(&self) -> &Path {
            self.dir.path()
        }
        pub(crate) fn assert_completed(&self) {
            assert!(
                !self.requested.is_empty(),
                "an owned roster cannot be empty"
            );
            COMPLETED.with(|completed| {
                let completed = completed.borrow();
                let missing: Vec<_> = self.requested.difference(&completed).collect();
                assert!(
                    missing.is_empty(),
                    "owned processes did not complete native membership and identity checks: {missing:?}"
                );
            });
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            REQUESTED.with(|requested| requested.borrow_mut().clear());
            COMPLETED.with(|completed| completed.borrow_mut().clear());
            OWNED.with(|owned| *owned.borrow_mut() = None);
            AFTER_PLAN.with(|hook| *hook.borrow_mut() = None);
            AFTER_LIST.with(|hook| *hook.borrow_mut() = None);
            AFTER_READ.with(|hook| *hook.borrow_mut() = None);
            AFTER_FDS.with(|hook| *hook.borrow_mut() = None);
        }
    }

    pub(crate) fn completed(pid: u32) {
        REQUESTED.with(|requested| {
            if requested.borrow().contains(&pid) {
                COMPLETED.with(|completed| {
                    completed.borrow_mut().insert(pid);
                });
            }
        });
    }

    pub(crate) fn owned(pids: &[u32]) -> Owned {
        let requested: BTreeSet<_> = pids.iter().copied().collect();
        let dir = tempfile::tempdir().unwrap();
        for pid in pids {
            std::os::unix::fs::symlink(format!("/proc/{pid}"), dir.path().join(pid.to_string()))
                .unwrap();
        }
        OWNED.with(|owned| {
            let mut owned = owned.borrow_mut();
            assert!(owned.is_none(), "one owned roster per test thread");
            REQUESTED.with(|current| {
                *current.borrow_mut() = requested.clone();
            });
            COMPLETED.with(|completed| completed.borrow_mut().clear());
            *owned = Some(dir.path().to_path_buf());
        });
        Owned { dir, requested }
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
            let root = holder.and_then(|h| h.root_for(pid));
            let membership = inspect(&process, pid, before.1, deadline, |tasks| {
                member_tasks(tasks, inventory, root, deadline)
            });
            let after = identity(&process)?;
            if after.1 != before.1 {
                // The leader exited during the reads: only a group with no
                // live task left has exited.
                if !before.1 && group_exited(&process, pid, deadline)? {
                    return Ok(None);
                }
                return Err(limit());
            }
            if before != after {
                return Err(limit());
            }
            #[cfg(test)]
            let membership = membership.inspect(|membership| {
                if membership.is_some() {
                    roster::completed(pid);
                }
            });
            if membership? != Some(true) {
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
        .unwrap()
        .expect("a settled descriptor table");
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
        // A single-threaded holder outside the checkout opens each file. This
        // test process is no subject: libtest starts a thread for every test,
        // so its task set never holds still for the one stable observation
        // membership needs (zerus-lo8o1.1).
        let (holder, mut hold) = holder(parent.path());
        let process = PathBuf::from(format!("/proc/{}", holder.pid()));
        let id = native_object(&process.join("fd").join(hold(&inside))).unwrap();
        assert!(scope.inventory(until()).unwrap().contains(id));
        let inventory = scope.inventory(until()).unwrap();
        drop(scope); // The observer's intentional root pin is not test evidence.
        assert!(member(&process, &inventory, until()).unwrap());
        fs::rename(&inside, parent.path().join("moved-away")).unwrap();
        fs::write(&inside, b"replacement").unwrap();
        let scope = objects::Scope::open(&root, until()).unwrap();
        let inventory = scope.inventory(until()).unwrap();
        drop(scope);
        assert!(!inventory.contains(id));
        assert!(!member(&process, &inventory, until()).unwrap());
        std::os::unix::fs::symlink(&outside, root.join("outside-alias")).unwrap();
        hold(&root.join("outside-alias"));
        let scope = objects::Scope::open(&root, until()).unwrap();
        let inventory = scope.inventory(until()).unwrap();
        drop(scope);
        assert!(!member(&process, &inventory, until()).unwrap());
    }

    /// A single-threaded python3 child in `cwd` that opens each path it is
    /// given and answers with the descriptor number.
    fn holder(cwd: &Path) -> (Python, impl FnMut(&Path) -> String) {
        use std::io::{BufRead, Write};
        let mut child = std::process::Command::new("python3")
            .args([
                "-I",
                "-c",
                "import os,sys\nfor path in sys.stdin:\n    print(os.open(path[:-1],os.O_RDONLY),flush=True)\n",
            ])
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        let hold = move |path: &Path| {
            writeln!(input, "{}", path.to_str().unwrap()).unwrap();
            let mut line = String::new();
            output.read_line(&mut line).unwrap();
            let descriptor = line.trim().to_owned();
            assert!(
                descriptor.parse::<u32>().is_ok(),
                "the holder did not open {path:?}"
            );
            descriptor
        };
        (Python(child), hold)
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
    /// exits, fails the census exactly as /proc does; it is never an empty
    /// roster. Since zerus-lo8o1.1 that read leaves each observation
    /// unsettled, so the bounded re-reads end in limit. A real owned occupant
    /// is still listed.
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
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{error}");
        drop(owned);
        occupant.kill().unwrap();
        occupant.wait().unwrap();
    }

    /// A real python3 child for the zerus-lo8o1 controls, killed on drop.
    struct Python(std::process::Child);
    impl Python {
        fn start(cwd: &Path, code: &str) -> Self {
            let (child, line) = Self::spawn(cwd, code);
            assert_eq!(line, "ready\n", "the control child did not start");
            child
        }
        /// Starts a child and returns its first line.
        fn spawn(cwd: &Path, code: &str) -> (Self, String) {
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
            (Python(child), line)
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
    /// with a file inside, is an occupant. Where unshare is refused (for
    /// example under Docker's default seccomp profile), the thread's context
    /// stays the process's own: only the precondition that the leader is
    /// outside is skipped, and the occupancy is still asserted (zerus-igl4q).
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
                "import ctypes,os,sys,threading,time\nsys.argv=['',{root:?}]\nready=threading.Event()\nown=[False]\ndef work():\n    own[0]=ctypes.CDLL(None,use_errno=True).unshare({unshare})==0\n    {work}\n    ready.set()\n    time.sleep(30)\nthreading.Thread(target=work,daemon=True).start()\nready.wait(5)\nprint('ready own' if own[0] else 'ready shared',flush=True)\ntime.sleep(30)\n",
                root = root.to_str().unwrap()
            );
            let (child, line) = Python::spawn(elsewhere.path(), &code);
            let process = PathBuf::from(format!("/proc/{}", child.pid()));
            match line.as_str() {
                "ready own\n" => assert!(
                    !inventory.contains(native_object(&process.join("cwd")).unwrap()),
                    "the leader itself is outside the checkout"
                ),
                "ready shared\n" => {}
                other => panic!("the control child did not start: {other:?}"),
            }
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
        // uncertainty, whether it is the first live task or a further one:
        // each observation is unsettled, and the re-reads end in limit.
        let later = entry.join("task").join(further.to_string());
        synthetic(&later, 'S');
        link(Path::new("/"), later.join("root"));
        assert_eq!(
            programs(&inventory, soon()).unwrap_err().kind(),
            io::ErrorKind::InvalidData,
            "a further live task without a cwd"
        );
        assert!(
            sample(soon()).is_err(),
            "sample: a further task without a cwd"
        );
        // The same for its descriptor table.
        link(Path::new("/"), later.join("cwd"));
        fs::remove_dir(later.join("fd")).unwrap();
        assert_eq!(
            programs(&inventory, soon()).unwrap_err().kind(),
            io::ErrorKind::InvalidData,
            "a further live task without a descriptor table"
        );
        assert!(
            sample(soon()).is_err(),
            "sample: a further task without a descriptor table"
        );
        fs::remove_dir_all(&later).unwrap();
        fs::remove_file(task.join("cwd")).unwrap();
        assert_eq!(
            programs(&inventory, soon()).unwrap_err().kind(),
            io::ErrorKind::InvalidData,
            "the first live task without a cwd"
        );
        assert!(
            sample(soon()).is_err(),
            "sample: the first task without a cwd"
        );
        drop(owned);
    }

    /// zerus-lo8o1.1, review-win-e's replacement control on #1869: a further
    /// task that holds the checkout, through its fs context or its own
    /// descriptor table, hands it to a task created after the plan, then
    /// exits, so the process occupies the checkout throughout. The changed
    /// task set is read again: the census, member() and sample() see the
    /// replacement, never absence. A task that exits without a replacement
    /// leaves nothing behind.
    #[test]
    fn a_context_handed_on_during_the_reads_is_never_absence() {
        let (_dir, root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        let pid = i32::MAX as u32;
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'S');
        fs::create_dir(entry.join("task")).unwrap();
        for link in ["cwd", "root"] {
            std::os::unix::fs::symlink("/", entry.join(link)).unwrap();
        }
        let worker = entry.join("task").join((pid - 2).to_string());
        let next = entry.join("task").join((pid - 1).to_string());
        // An occupant through its cwd, or through its own descriptor table:
        // a descriptor for a file in the checkout, its cwd outside.
        let occupant = |task: &Path, root: &Path, table: bool| {
            synthetic(task, 'S');
            let cwd = if table { Path::new("/") } else { root };
            std::os::unix::fs::symlink(cwd, task.join("cwd")).unwrap();
            std::os::unix::fs::symlink("/", task.join("root")).unwrap();
            if table {
                std::os::unix::fs::symlink(root.join("held"), task.join("fd").join("3")).unwrap();
            }
        };
        for table in [false, true] {
            // The worker is planned; between the plan and the reads it hands
            // its context to `next` (or to nothing) and exits.
            let arm = |hand_on: bool| {
                let _ = fs::remove_dir_all(&next);
                occupant(&worker, &root, table);
                let (worker, next, root) = (worker.clone(), next.clone(), root.clone());
                roster::on_plan(move || {
                    if hand_on {
                        occupant(&next, &root, table);
                    }
                    fs::remove_dir_all(&worker).unwrap();
                });
            };
            arm(true);
            assert!(
                programs(&inventory, soon())
                    .unwrap()
                    .iter()
                    .any(|p| p.pid == pid),
                "a live leader's continuous checkout occupant was reported absent ({table})"
            );
            assert!(next.exists());
            arm(true);
            assert!(
                member_with_holder(&entry, &inventory, None, soon()).unwrap(),
                "{table}"
            );
            arm(true);
            assert!(sampled(pid, &root), "sample ({table})");
            arm(false);
            assert!(
                programs(&inventory, soon()).unwrap().is_empty(),
                "gone alone ({table})"
            );
            arm(false);
            assert!(
                !member_with_holder(&entry, &inventory, None, soon()).unwrap(),
                "{table}"
            );
            arm(false);
            assert!(!sampled(pid, &root), "sample gone alone ({table})");
        }
        drop(owned);
    }

    /// zerus-lo8o1.1, review-win-e's kcmp-lifetime variant on #1869: a real
    /// thread that shares the leader's fs context in the checkout is left out
    /// of the plan. Between the plan and the reads it unshares, keeping that
    /// cwd, and the leader moves out. The tasks are the same, but the plan's
    /// sharing no longer holds, so the process is read again: an occupant
    /// for the census, member() and sample() alike. Where kcmp is refused,
    /// every task is read itself and the plan cannot leave the thread out;
    /// where unshare is refused, the leader stays with the thread in the
    /// checkout. Each skips only its precondition; the occupancy is still
    /// asserted (zerus-igl4q).
    #[test]
    fn a_thread_that_unshares_after_the_plan_is_read_again() {
        use std::io::{BufRead, Write};
        let away = tempfile::tempdir().unwrap();
        for reader in ["census", "member", "sample"] {
            let (_dir, root, _scope, inventory) = checkout();
            let mut child = std::process::Command::new("python3")
                .args([
                    "-I",
                    "-c",
                    "import ctypes,os,sys,threading,time\ngo=threading.Event()\nmoved=threading.Event()\nown=[False]\ndef work():\n    go.wait(30)\n    own[0]=ctypes.CDLL(None,use_errno=True).unshare(0x200)==0\n    moved.set()\n    time.sleep(30)\nthreading.Thread(target=work,daemon=True).start()\nprint('ready',flush=True)\nsys.stdin.readline()\ngo.set()\nmoved.wait(5)\nif own[0]:\n    os.chdir(sys.argv[1])\nprint('moved' if own[0] else 'kept',flush=True)\ntime.sleep(30)\n",
                    away.path().to_str().unwrap(),
                ])
                .current_dir(&root)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let mut input = child.stdin.take().unwrap();
            let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
            let child = Python(child);
            let mut line = String::new();
            output.read_line(&mut line).unwrap();
            assert_eq!(line, "ready\n", "the control child did not start");
            let process = PathBuf::from(format!("/proc/{}", child.pid()));
            let threads: Vec<u32> = fs::read_dir(process.join("task"))
                .unwrap()
                .map(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_str()
                        .unwrap()
                        .parse()
                        .unwrap()
                })
                .filter(|tid| *tid != child.pid())
                .collect();
            assert_eq!(threads.len(), 1);
            if kcmp_available() {
                assert!(
                    shares(child.pid(), threads[0], KCMP_FS),
                    "the plan leaves the thread out"
                );
            }
            let _owned = roster::owned(&[child.pid()]);
            let moved = std::rc::Rc::new(std::cell::Cell::new(None));
            let reply = moved.clone();
            roster::on_plan(move || {
                writeln!(input, "go").unwrap();
                let mut line = String::new();
                output.read_line(&mut line).unwrap();
                reply.set(Some(match line.as_str() {
                    "moved\n" => true,
                    "kept\n" => false,
                    other => panic!("the control child did not answer: {other:?}"),
                }));
            });
            let found = match reader {
                "census" => programs(&inventory, soon())
                    .unwrap()
                    .iter()
                    .any(|row| row.pid == child.pid()),
                "member" => member(&process, &inventory, soon()).unwrap(),
                _ => sampled(child.pid(), &root),
            };
            if moved.get().expect("the hook ran") {
                assert!(
                    !inventory.contains(native_object(&process.join("cwd")).unwrap()),
                    "the leader moved out"
                );
            }
            assert!(found, "{reader}");
        }
    }

    /// zerus-lo8o1.1: a task set that changes on every observation within
    /// the bounded attempts never establishes absence: uncertainty.
    #[test]
    fn a_task_set_that_never_settles_is_uncertainty() {
        fn churn(entry: PathBuf, next: u32) {
            roster::on_plan(move || {
                let task = entry.join("task").join(next.to_string());
                synthetic(&task, 'S');
                for link in ["cwd", "root"] {
                    std::os::unix::fs::symlink("/", task.join(link)).unwrap();
                }
                churn(entry.clone(), next - 1);
            });
        }
        let (_dir, _root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        let pid = i32::MAX as u32;
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'S');
        fs::create_dir(entry.join("task")).unwrap();
        for link in ["cwd", "root"] {
            std::os::unix::fs::symlink("/", entry.join(link)).unwrap();
        }
        assert!(programs(&inventory, soon()).unwrap().is_empty(), "settled");
        churn(entry.clone(), pid - 10);
        assert!(programs(&inventory, soon()).is_err(), "census");
        churn(entry.clone(), pid - 100);
        assert!(member_with_holder(&entry, &inventory, None, soon()).is_err());
        churn(entry.clone(), pid - 1000);
        assert!(sample(soon()).is_err(), "sample");
        roster::on_plan(|| {});
        drop(owned);
    }

    /// zerus-lo8o1.1: a new or vanished task ID is a changed observation even
    /// when the plan stays the same. Exited tasks that appear on every
    /// observation of an exited leader's group never settle it.
    #[test]
    fn task_ids_that_never_settle_are_uncertainty_without_a_plan_change() {
        fn churn(entry: PathBuf, next: u32) {
            roster::on_plan(move || {
                synthetic(&entry.join("task").join(next.to_string()), 'Z');
                churn(entry.clone(), next - 1);
            });
        }
        let (_dir, _root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        let pid = i32::MAX as u32;
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'Z');
        let live = entry.join("task").join((pid - 1).to_string());
        synthetic(&live, 'S');
        for link in ["cwd", "root"] {
            std::os::unix::fs::symlink("/", live.join(link)).unwrap();
        }
        assert!(programs(&inventory, soon()).unwrap().is_empty(), "settled");
        churn(entry.clone(), pid - 10);
        assert!(programs(&inventory, soon()).is_err(), "census");
        churn(entry.clone(), pid - 100);
        assert!(member_with_holder(&entry, &inventory, None, soon()).is_err());
        churn(entry.clone(), pid - 1000);
        assert!(sample(soon()).is_err(), "sample");
        roster::on_plan(|| {});
        drop(owned);
    }

    /// A synthetic live task whose cwd is in the checkout.
    fn occupant(task: &Path, root: &Path) {
        synthetic(task, 'S');
        std::os::unix::fs::symlink(root, task.join("cwd")).unwrap();
        std::os::unix::fs::symlink("/", task.join("root")).unwrap();
    }

    /// Arms the next listings in turn: after each, the task it listed hands
    /// its checkout context to the next task of `chain` and is gone before
    /// its state is read.
    fn hand_on(entry: PathBuf, root: PathBuf, chain: Vec<u32>) {
        roster::on_list(move || {
            occupant(&entry.join("task").join(chain[1].to_string()), &root);
            fs::remove_dir_all(entry.join("task").join(chain[0].to_string())).unwrap();
            if chain.len() > 2 {
                hand_on(entry.clone(), root.clone(), chain[1..].to_vec());
            }
        });
    }

    /// zerus-lo8o1.1, review-win-e on #1869 e2bf7043: an exited leader's task
    /// that hands its checkout context to a new task and is gone before its
    /// state is read leaves an observation that proves nothing, in the first
    /// listing or the second. The group is read again and its live occupant
    /// found by the census, member() and sample(), never reported exited.
    #[test]
    fn a_task_gone_before_its_state_is_read_never_proves_an_exited_group() {
        let (_dir, root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        let pid = i32::MAX as u32;
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'Z');
        let chain = vec![pid - 3, pid - 2, pid - 1];
        let arm = || {
            for tid in &chain {
                let _ = fs::remove_dir_all(entry.join("task").join(tid.to_string()));
            }
            occupant(&entry.join("task").join(chain[0].to_string()), &root);
            hand_on(entry.clone(), root.clone(), chain.clone());
        };
        arm();
        assert!(
            programs(&inventory, soon())
                .unwrap()
                .iter()
                .any(|p| p.pid == pid),
            "census"
        );
        arm();
        assert!(member_with_holder(&entry, &inventory, None, soon()).unwrap());
        arm();
        assert!(sampled(pid, &root), "sample");
        drop(owned);
    }

    /// zerus-lo8o1.1: an exited group needs an unchanged second listing too.
    /// A live task listed after the first observation found none is read,
    /// never reported exited.
    #[test]
    fn a_live_task_listed_after_an_exited_observation_is_read() {
        let (_dir, root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        let pid = i32::MAX as u32;
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'Z');
        synthetic(&entry.join("task").join((pid - 2).to_string()), 'Z');
        let late = entry.join("task").join((pid - 1).to_string());
        let arm = || {
            let _ = fs::remove_dir_all(&late);
            let (late, root) = (late.clone(), root.clone());
            roster::on_plan(move || occupant(&late, &root));
        };
        assert!(programs(&inventory, soon()).unwrap().is_empty(), "exited");
        arm();
        assert!(
            programs(&inventory, soon())
                .unwrap()
                .iter()
                .any(|p| p.pid == pid),
            "census"
        );
        arm();
        assert!(member_with_holder(&entry, &inventory, None, soon()).unwrap());
        arm();
        assert!(sampled(pid, &root), "sample");
        drop(owned);
    }

    /// zerus-lo8o1.1: a leader that exits during the reads leaves an exited
    /// group only by a settled observation. A task that hands its context on
    /// and is gone before its state is read, in the listing that decides,
    /// keeps the census and sample() uncertain.
    #[test]
    fn a_leader_exiting_during_the_reads_is_exited_only_by_a_settled_observation() {
        let (_dir, root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        let pid = i32::MAX as u32;
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'S');
        fs::create_dir(entry.join("task")).unwrap();
        for link in ["cwd", "root"] {
            std::os::unix::fs::symlink("/", entry.join(link)).unwrap();
        }
        let (worker, next) = (pid - 2, pid - 1);
        let arm = || {
            synthetic(&entry, 'S');
            let _ = fs::remove_dir_all(entry.join("task").join(next.to_string()));
            let task = entry.join("task").join(worker.to_string());
            synthetic(&task, 'S');
            for link in ["cwd", "root"] {
                let _ = fs::remove_file(task.join(link));
                std::os::unix::fs::symlink("/", task.join(link)).unwrap();
            }
            // The leader exits during the reads; the reads' own second
            // listing passes, and the listing that decides whether the group
            // has exited loses the worker to its replacement.
            let (entry, root) = (entry.clone(), root.clone());
            roster::on_plan(move || {
                synthetic(&entry, 'Z');
                let (entry, root) = (entry.clone(), root.clone());
                roster::on_list(move || hand_on(entry.clone(), root.clone(), vec![worker, next]));
            });
        };
        arm();
        assert!(programs(&inventory, soon()).is_err(), "census");
        arm();
        assert!(sample(soon()).is_err(), "sample");
        drop(owned);
    }

    /// zerus-lo8o1.1, design-tima2's ruling: a failed read is classified by
    /// its own errno. A denial (EACCES, EPERM) keeps the existing per-read
    /// policy; a task gone (ENOENT, ESRCH), an I/O error or unreadable content
    /// is unsettled and never a denial; the read budget ends the inspection.
    #[test]
    fn failed_reads_are_classified_by_their_own_errno() {
        let os = io::Error::from_raw_os_error;
        for errno in [libc::EACCES, libc::EPERM] {
            assert!(!unsettled(&os(errno)), "{errno}");
            let error = settled::<()>(Err(os(errno))).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(errno));
        }
        for errno in [libc::ENOENT, libc::ESRCH, libc::EIO, libc::EINVAL] {
            assert!(!denied(&os(errno)), "{errno}");
            assert!(unsettled(&os(errno)), "{errno}");
            assert!(settled::<()>(Err(os(errno))).unwrap().is_none(), "{errno}");
        }
        assert!(unsettled(&io::Error::other("malformed content")));
        let budget = bounded(0, Instant::now()).unwrap_err();
        assert!(!unsettled(&budget));
        assert!(settled::<()>(Err(budget)).is_err());
        // A descriptor's ENOENT is a closed descriptor only while its task's
        // entry is still there; with the task gone it is unsettled.
        let task = tempfile::tempdir().unwrap();
        assert!(closed(&os(libc::ENOENT), task.path()));
        assert!(!closed(&os(libc::EIO), task.path()));
        assert!(!closed(&os(libc::EACCES), task.path()));
        assert!(!closed(&os(libc::ENOENT), &task.path().join("gone")));
    }

    /// zerus-lo8o1.1, design-tima2's ruling, the B to C to B' variant: a live
    /// leader's further task B, in the checkout, hands its context to C and is
    /// gone at its read; C hands it on to B', which takes B's ID near the
    /// allocator's wrap, and exits before the second listing. That listing
    /// names the same IDs and the same plan, but B's failed read proves
    /// nothing: the process is read again and B' found by the census,
    /// member() and sample(). The vanished read never takes the denial
    /// branch: sample() reports no unreadable process for it.
    #[test]
    fn a_task_gone_at_its_read_is_never_absence_even_when_its_id_returns() {
        let (_dir, root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        let pid = i32::MAX as u32;
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'S');
        fs::create_dir(entry.join("task")).unwrap();
        for link in ["cwd", "root"] {
            std::os::unix::fs::symlink("/", entry.join(link)).unwrap();
        }
        let b = entry.join("task").join((pid - 2).to_string());
        let c = entry.join("task").join((pid - 1).to_string());
        let arm = || {
            for task in [&b, &c] {
                let _ = fs::remove_dir_all(task);
            }
            occupant(&b, &root);
            let (from, to, root_) = (b.clone(), c.clone(), root.clone());
            roster::on_plan(move || {
                occupant(&to, &root_);
                fs::remove_dir_all(&from).unwrap();
            });
            let (from, to, root_) = (c.clone(), b.clone(), root.clone());
            roster::on_read(move || {
                occupant(&to, &root_);
                fs::remove_dir_all(&from).unwrap();
            });
        };
        arm();
        assert!(
            programs(&inventory, soon())
                .unwrap()
                .iter()
                .any(|p| p.pid == pid),
            "census"
        );
        arm();
        assert!(member_with_holder(&entry, &inventory, None, soon()).unwrap());
        arm();
        let rows = sample(soon()).unwrap();
        assert!(
            rows.paths
                .iter()
                .any(|(p, path, _)| *p == pid && path.starts_with(&root)),
            "sample"
        );
        assert!(
            !rows.unreadable.iter().any(|row| row.pid == pid),
            "a vanished read is not a denial"
        );
        drop(owned);
    }

    /// zerus-1qnef, design-tima2's ruling: the accepted descriptor-handoff
    /// residual, pinned. Between the table's listing and its reads, a
    /// checkout file's descriptor 3 is duplicated to 7, a number the listing
    /// missed, and 3 is closed. Descriptor 3 reads ENOENT while the task's
    /// entry remains: a closed descriptor, skipped, never unsettled and never
    /// a denial. The census, member() and sample() report the process absent
    /// for that observation; a fresh one sees descriptor 7.
    #[test]
    fn a_descriptor_handed_on_during_the_table_read_is_the_accepted_residual() {
        let (_dir, root, _scope, inventory) = checkout();
        let owned = roster::owned(&[]);
        let pid = i32::MAX as u32;
        let entry = owned.path().join(pid.to_string());
        synthetic(&entry, 'S');
        fs::create_dir(entry.join("task")).unwrap();
        for link in ["cwd", "root"] {
            std::os::unix::fs::symlink("/", entry.join(link)).unwrap();
        }
        let (old, new) = (entry.join("fd").join("3"), entry.join("fd").join("7"));
        let arm = || {
            for fd in [&old, &new] {
                let _ = fs::remove_file(fd);
            }
            std::os::unix::fs::symlink(root.join("held"), &old).unwrap();
            let (old, new, held) = (old.clone(), new.clone(), root.join("held"));
            roster::on_fds(move || {
                std::os::unix::fs::symlink(&held, &new).unwrap();
                fs::remove_file(&old).unwrap();
            });
        };
        arm();
        assert!(
            programs(&inventory, soon()).unwrap().is_empty(),
            "census: the residual"
        );
        assert!(
            programs(&inventory, soon())
                .unwrap()
                .iter()
                .any(|p| p.pid == pid),
            "a fresh census sees descriptor 7"
        );
        arm();
        assert!(!member_with_holder(&entry, &inventory, None, soon()).unwrap());
        arm();
        let rows = sample(soon()).unwrap();
        assert!(
            !rows
                .paths
                .iter()
                .any(|(p, path, _)| *p == pid && path.starts_with(&root)),
            "sample: the residual"
        );
        assert!(
            !rows.unreadable.iter().any(|row| row.pid == pid),
            "a closed descriptor is not a denial"
        );
        assert!(sampled(pid, &root), "a fresh sample sees descriptor 7");
        drop(owned);
    }
}
