//! Read-only process membership evidence. This never signals a process.
use super::{UnreadableWorktreeProcess, WorktreeProcesses};
use std::{io, path::PathBuf, time::Instant};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(super) use linux::sample;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(super) use macos::sample;

const MAX_FILES: usize = 16_384;
const MAX_PATHS: usize = 65_536;
const MAX_PATH_BYTES: usize = 16 * 1024 * 1024;
#[derive(Default)]
struct Files {
    paths: Vec<PathBuf>,
    denied: bool,
    bytes: usize,
}
impl Files {
    fn push(&mut self, path: PathBuf) -> io::Result<()> {
        self.bytes = self
            .bytes
            .checked_add(path.as_os_str().len())
            .ok_or_else(limit)?;
        if self.bytes > 4 * 1024 * 1024 {
            return Err(limit());
        }
        self.paths.push(path);
        Ok(())
    }
}
fn limit() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "process path inventory exceeded its bound",
    )
}
#[derive(Default)]
struct PathBudget {
    bytes: usize,
}
impl PathBudget {
    fn push(
        &mut self,
        rows: &mut WorktreeProcesses,
        pid: u32,
        path: PathBuf,
        name: &str,
    ) -> io::Result<()> {
        self.bytes = self
            .bytes
            .checked_add(path.as_os_str().len())
            .ok_or_else(limit)?;
        if rows.paths.len() >= MAX_PATHS || self.bytes > MAX_PATH_BYTES {
            return Err(limit());
        }
        rows.paths.push((pid, path, name.into()));
        Ok(())
    }
}
fn denied(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM))
}
fn bounded(count: usize, deadline: Instant) -> io::Result<()> {
    if count >= MAX_FILES || Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "process file inspection exceeded its bound",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_actual_access_denials_are_excluded_and_bounds_are_not_denials() {
        for code in [libc::EACCES, libc::EPERM] {
            assert!(denied(&io::Error::from_raw_os_error(code)));
        }
        for code in [
            libc::EIO,
            libc::ENOENT,
            libc::ESRCH,
            libc::EINVAL,
            libc::ETIMEDOUT,
        ] {
            assert!(!denied(&io::Error::from_raw_os_error(code)));
        }
        let limit = bounded(
            MAX_FILES,
            Instant::now() + std::time::Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(!denied(&limit));
        assert!(bounded(0, Instant::now()).is_err());
        let mut rows = WorktreeProcesses::default();
        let mut budget = PathBudget {
            bytes: MAX_PATH_BYTES,
        };
        let overflow = budget
            .push(&mut rows, 7, PathBuf::from("/original"), "fixture")
            .unwrap_err();
        assert!(!denied(&overflow));
        assert!(
            rows.paths.is_empty(),
            "exhaustion must not publish a partial inventory as complete"
        );
    }
}
