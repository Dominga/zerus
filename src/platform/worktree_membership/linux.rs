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
    let entries = fs::read_dir("/proc").map_err(|_| error())?;
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
        if before.1 {
            continue;
        }
        let cwd = fs::read_link(path.join("cwd"));
        let image = fs::read_link(path.join("exe"));
        let files = files(&path, deadline);
        let after = match identity(&path) {
            Ok(after) => after,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(error()),
        };
        if after.1 {
            continue;
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
        let (cwd_denied, image_denied) = (is_denied(&cwd), is_denied(&image));
        if (cwd.is_err() && !cwd_denied) || (image.is_err() && !image_denied) {
            return Err(error());
        }
        let files = files.map_err(|_| error())?;
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
        if let Ok(cwd) = cwd {
            budget
                .push(&mut rows, pid, cwd, &name)
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

pub(super) fn member(
    process: &Path,
    inventory: &objects::Inventory,
    deadline: Instant,
) -> io::Result<bool> {
    member_with_holder(process, inventory, None, deadline)
}
fn member_with_holder(
    process: &Path,
    inventory: &objects::Inventory,
    root: Option<HolderRoot>,
    deadline: Instant,
) -> io::Result<bool> {
    let mut found = false;
    let mut skipped = false;
    bounded(0, deadline)?;
    match native_object(&process.join("cwd")) {
        Ok(id) => found |= inventory.contains(id),
        Err(e) if denied(&e) && root.is_none() => {}
        Err(e) => return Err(e),
    }
    let fds = match fs::read_dir(process.join("fd")) {
        Ok(fds) => fds,
        Err(e) if denied(&e) && root.is_none() => return Ok(found),
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
                let info = read(&process.join("fdinfo").join(fd.file_name()))?;
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
            Err(e) if (denied(&e) && root.is_none()) || e.kind() == io::ErrorKind::NotFound => {}
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
    collect_programs(inventory, holder, deadline, || fs::read_dir("/proc"))
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
            if before.1 {
                return Ok(None);
            }
            let root = holder.and_then(|h| h.root_for(pid));
            let membership = member_with_holder(&process, inventory, root, deadline);
            let after = identity(&process)?;
            if after.1 {
                return Ok(None);
            }
            if before != after {
                return Err(limit());
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
}
