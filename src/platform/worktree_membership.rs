//! Read-only process membership evidence. This never signals a process.
use super::{UnreadableWorktreeProcess, WorktreeProcesses};
use std::{io, path::PathBuf, time::Instant};

// Native object selection is used only by the explicit program manifest; the
// existing generic/manual cleanup sampler keeps its original contract.
#[allow(dead_code)]
pub(crate) mod objects;

/// A completed, read-only native program review. This type owns no stop
/// permission. Access-denied halves are omitted; readable membership wins.
#[allow(dead_code)]
pub(crate) fn programs(
    scope: &objects::Scope,
    deadline: Instant,
) -> io::Result<Vec<zerus_client_core::worktree_programs::Program>> {
    let inventory = scope.inventory(deadline)?;
    #[cfg(target_os = "linux")]
    let rows = linux::programs(&inventory, deadline)?;
    #[cfg(target_os = "macos")]
    let rows = macos::programs(&inventory, deadline)?;
    inventory.current(scope, deadline)?;
    Ok(rows)
}

/// Only an already kernel-bound original holder can supply this exclusion.
/// The exemption is exactly ONE read-only close-on-exec root descriptor,
/// never its cwd, other descriptors, PID, or process membership as a whole.
#[derive(Clone, Copy)]
struct HolderRoot {
    object: objects::Object,
    descriptor: i32,
}
struct Custody<'a> {
    peer: &'a objects::loan::peer::Peer,
    socket: &'a std::os::unix::net::UnixStream,
    root: HolderRoot,
}
impl Custody<'_> {
    fn current(&self, until: Instant) -> io::Result<()> {
        self.peer.current(self.socket, until)
    }
    fn root_for(&self, pid: u32) -> Option<HolderRoot> {
        (pid == self.peer.pid()).then_some(self.root)
    }
}
fn programs_borrowed(
    scope: &objects::Scope,
    peer: &objects::loan::peer::Peer,
    socket: &std::os::unix::net::UnixStream,
    original_descriptor: i32,
    deadline: Instant,
) -> io::Result<Vec<zerus_client_core::worktree_programs::Program>> {
    let custody = Custody {
        peer,
        socket,
        root: HolderRoot {
            object: objects::Object {
                device: scope.identity().device,
                inode: scope.identity().inode,
            },
            descriptor: original_descriptor,
        },
    };
    custody.current(deadline)?;
    let inventory = scope.inventory(deadline)?;
    // This helper's actual borrowed pin is accounted separately. Never turn
    // the holder exclusion into permission to ignore its borrower or children.
    #[cfg(target_os = "linux")]
    linux::own_clear(scope, &inventory, deadline)?;
    #[cfg(target_os = "macos")]
    macos::own_clear(scope, &inventory, deadline)?;
    #[cfg(target_os = "linux")]
    let rows = linux::programs_with_holder(&inventory, Some(&custody), deadline)?;
    #[cfg(target_os = "macos")]
    let rows = macos::programs_with_holder(&inventory, Some(&custody), deadline)?;
    inventory.current(scope, deadline)?;
    custody.current(deadline)?;
    Ok(rows)
}

/// Fresh observation of ONE original process for the later per-signal gate.
/// This does not signal, adopt a reused PID or turn a denied read into known
/// use. The executing owner must also keep its original PinnedProcess.
#[allow(dead_code)]
pub(crate) fn original_member(
    scope: &objects::Scope,
    original: &zerus_client_core::worktree_programs::Program,
    deadline: Instant,
) -> io::Result<bool> {
    if original.pid <= 1 || original.pid == std::process::id() {
        return Err(limit());
    }
    let inventory = scope.inventory(deadline)?;
    let identity = || super::process_identity(original.pid).ok_or_else(limit);
    let before = identity()?;
    if before.birth != original.birth {
        return Err(limit());
    }
    #[cfg(target_os = "linux")]
    let found = linux::member(
        &PathBuf::from(format!("/proc/{}", original.pid)),
        &inventory,
        deadline,
    )?;
    #[cfg(target_os = "macos")]
    let found = macos::member(original.pid, &inventory, deadline)?;
    let after = identity()?;
    if before != after {
        return Err(limit());
    }
    inventory.current(scope, deadline)?;
    Ok(found)
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(all(test, target_os = "linux"))]
pub(crate) use linux::{kcmp_available, roster as test_roster};
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

    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn actual_denied_cwd_is_omitted_while_a_readable_checkout_occupant_is_listed() {
        use std::{
            io::{BufRead, Read},
            process::{Command, Stdio},
            time::Duration,
        };
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        std::fs::write(root.join("held"), b"original").unwrap();
        let mut denied=Child(Command::new("python3").args(["-I","-c",
            "import ctypes,os,sys,time\nf=open('held','rb')\nassert ctypes.CDLL(None).prctl(4,0,0,0,0)==0\nprint('denied-ready',flush=True)\ntime.sleep(20)"])
            .current_dir(&root).stdin(Stdio::null()).stdout(Stdio::piped()).spawn().unwrap());
        let output = denied.0.stdout.take().unwrap();
        // Bounded actual readiness, not a sleep or a guessed permission class.
        let mut poll = libc::pollfd {
            fd: std::os::fd::AsRawFd::as_raw_fd(&output),
            events: libc::POLLIN,
            revents: 0,
        };
        assert_eq!(unsafe { libc::poll(&mut poll, 1, 5000) }, 1);
        let mut line = String::new();
        std::io::BufReader::new(output)
            .take(128)
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line, "denied-ready\n");
        for leaf in ["cwd", "fd"] {
            let path = PathBuf::from(format!("/proc/{}/{leaf}", denied.0.id()));
            let error = if leaf == "cwd" {
                std::fs::metadata(path).unwrap_err()
            } else {
                std::fs::read_dir(path).unwrap_err()
            };
            assert!(
                super::denied(&error),
                "the control must establish actual native denial: {error}"
            );
        }
        let mut readable = Child(
            Command::new("/bin/sleep")
                .arg("20")
                .current_dir(&root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let until = Instant::now() + Duration::from_secs(5);
        let scope = objects::Scope::open(&root, until).unwrap();
        let inventory = scope.inventory(until).unwrap();
        assert!(
            !linux::member(
                &PathBuf::from(format!("/proc/{}", denied.0.id())),
                &inventory,
                until
            )
            .unwrap(),
            "this actual denied cwd must not count as membership"
        );
        // This control concerns the two genuine native subjects above, not
        // readiness of the entire shared host. Keep their actual /proc reads,
        // identity checks and aggregation; production still enumerates /proc.
        let roster = tempfile::tempdir().unwrap();
        for pid in [denied.0.id(), readable.0.id()] {
            std::os::unix::fs::symlink(
                format!("/proc/{pid}"),
                roster.path().join(pid.to_string()),
            )
            .unwrap();
        }
        let found = linux::fixture_programs(&inventory, until, roster.path()).unwrap();
        inventory.current(&scope, until).unwrap();
        assert!(
            !found.iter().any(|p| p.pid == denied.0.id()),
            "a denied read is not proven membership"
        );
        assert!(
            found.iter().any(|p| p.pid == readable.0.id()),
            "readable membership still protects the checkout"
        );
        // A missing read in an existing entry remains uncertainty, not an
        // access denial or an empty roster. Do not "fix" the flake by skipping
        // ENOENT while its entry still exists.
        let entry = roster.path().join(readable.0.id().to_string());
        std::fs::remove_file(&entry).unwrap();
        std::fs::create_dir(&entry).unwrap();
        let error = linux::fixture_programs(&inventory, until, roster.path()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(readable.0.try_wait().unwrap().is_none());
        assert!(denied.0.try_wait().unwrap().is_none());
    }
    #[test]
    fn a_borrowers_own_pin_is_not_a_bypass_for_its_other_open_files() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        std::fs::write(root.join("held"), b"original").unwrap();
        let until = || Instant::now() + std::time::Duration::from_secs(3);
        let scope = objects::Scope::open(&root, until()).unwrap();
        let inventory = scope.inventory(until()).unwrap();
        #[cfg(target_os = "linux")]
        use linux::own_clear;
        #[cfg(target_os = "macos")]
        use macos::own_clear;
        own_clear(&scope, &inventory, until()).unwrap();
        let file = std::fs::File::open(root.join("held")).unwrap();
        assert!(own_clear(&scope, &inventory, until()).is_err());
        drop(file);
        own_clear(&scope, &inventory, until()).unwrap();
        use std::os::fd::AsRawFd;
        let root_pin = scope.own_root_descriptor();
        let duplicate = unsafe { libc::fcntl(root_pin.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        assert!(duplicate >= 0);
        use std::os::fd::FromRawFd;
        let duplicate = unsafe { std::fs::File::from_raw_fd(duplicate) };
        assert!(own_clear(&scope, &inventory, until()).is_err());
        drop(duplicate);
        own_clear(&scope, &inventory, until()).unwrap();
    }

    #[test]
    fn original_kernel_cwd_or_open_file_is_required_and_a_replacement_name_is_not_enough() {
        use std::{
            fs,
            process::{Command, Stdio},
        };
        let parent = tempfile::tempdir().unwrap();
        let parent = parent.path().canonicalize().unwrap();
        let root = parent.join("work");
        fs::create_dir(&root).unwrap();
        let path = root.join("held");
        fs::write(&path, b"original").unwrap();
        let until = || Instant::now() + std::time::Duration::from_secs(5);
        let scope = objects::Scope::open(&root, until()).unwrap();
        // spawn returns only after exec: cwd and inherited FDs are established,
        // with no timer or guessed child-ready interval.
        let child = Child(
            Command::new("/bin/sleep")
                .arg("300")
                .current_dir(&parent)
                .stdin(Stdio::from(fs::File::open(&path).unwrap()))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let original = zerus_client_core::worktree_programs::Program {
            pid: child.0.id(),
            birth: super::super::process_identity(child.0.id()).unwrap().birth,
            label: "fixture".into(),
        };
        assert!(original_member(&scope, &original, until()).unwrap());
        fs::rename(&path, parent.join("old")).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert!(!original_member(&scope, &original, until()).unwrap());
        let recycled = zerus_client_core::worktree_programs::Program {
            birth: "different-original".into(),
            ..original.clone()
        };
        assert!(original_member(&scope, &recycled, until()).is_err());
        assert!(original_member(&scope, &original, Instant::now()).is_err());
        let cwd_child = Child(
            Command::new("/bin/sleep")
                .arg("300")
                .current_dir(&root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let cwd = zerus_client_core::worktree_programs::Program {
            pid: cwd_child.0.id(),
            birth: super::super::process_identity(cwd_child.0.id())
                .unwrap()
                .birth,
            label: "fixture".into(),
        };
        assert!(original_member(&scope, &cwd, until()).unwrap());
    }
}
