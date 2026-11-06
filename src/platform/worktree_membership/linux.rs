use super::*;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(in crate::platform) fn sample(
    deadline: std::time::Instant,
) -> std::result::Result<super::WorktreeProcesses, String> {
    use std::io::Read;
    let deadline = deadline.min(std::time::Instant::now() + std::time::Duration::from_secs(2));
    let uid = unsafe { libc::getuid() };
    let error =
        || "Running process coverage is incomplete; review the worktree manually".to_owned();
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
            if rows.len() >= 128 || std::time::Instant::now() >= deadline || rows.contains_key(&pid)
            {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
