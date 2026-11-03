//! Private image writes belong to an original child, never the forking test host.
use super::*;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::{fs::MetadataExt, io::FromRawFd, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const CHILD: &str = "desktop_service::audit::tests::helper_image::image_child";
const FD_BOUND: i32 = 4096;

pub(super) struct OriginalChild {
    child: Option<Child>,
    ready: PathBuf,
    log: PathBuf,
    retained: Arc<AtomicBool>,
}
impl OriginalChild {
    fn retain_unknown(&self, mut child: Child) -> &'static str {
        drop(child.stdin.take()); // Release the original pipe, never an inferred PID.
        self.retained.store(true, Ordering::Release);
        std::mem::forget(child);
        "original fixture child wait unconfirmed; private root retained"
    }
    fn settle(&mut self) -> Result<ExitStatus, &'static str> {
        let Some(mut child) = self.child.take() else {
            return Err("fixture child already joined");
        };
        drop(child.stdin.take()); // EOF releases only this original child.
        for forced in [false, true] {
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => return Ok(status),
                    Ok(None) if Instant::now() < until => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Ok(None) => break,
                    Err(_) => return Err(self.retain_unknown(child)),
                }
            }
            if !forced {
                let _ = child.kill(); // Its original Child handle, never a PID search.
            }
        }
        // A kill request does not prove exit. Retain its scope on uncertainty.
        Err(self.retain_unknown(child))
    }
    fn ready(&mut self) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if std::fs::read_to_string(&self.ready).is_ok_and(|value| !value.is_empty()) {
                return;
            }
            match self.child.as_mut().unwrap().try_wait() {
                Ok(None) => (),
                Ok(Some(status)) => panic!(
                    "fixture child exited before readiness: {status}: {}",
                    self.log.display()
                ),
                Err(error) => {
                    let child = self.child.take().unwrap();
                    let reason = self.retain_unknown(child);
                    panic!("{reason}: {error}: {}", self.log.display());
                }
            }
            assert!(
                Instant::now() < until,
                "fixture child readiness: {}",
                self.log.display()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub(super) fn finish(mut self) {
        let status = self.settle().unwrap();
        assert!(std::fs::metadata(&self.log).unwrap().len() <= 64 << 10);
        assert!(
            status.success(),
            "{status}: {}",
            std::fs::read_to_string(&self.log).unwrap()
        );
    }
}
impl Drop for OriginalChild {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.settle();
        }
    }
}

#[derive(Clone, Copy)]
struct Identity {
    device: u64,
    inode: u64,
}
impl Identity {
    fn at(path: &Path) -> Self {
        let metadata = std::fs::symlink_metadata(path).unwrap();
        assert!(
            metadata.file_type().is_file(),
            "private image must not be a symlink"
        );
        assert_ne!(metadata.ino(), 0, "native file identity required");
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
    // Only fcntl/fstat and stack POD run here, including in pre_exec.
    #[allow(clippy::unnecessary_cast)] // libc::stat field widths vary across Unix.
    fn writable(self, fd: i32) -> bool {
        // SAFETY: these syscalls inspect one descriptor and a writable POD buffer.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || flags & libc::O_ACCMODE == libc::O_RDONLY {
                return false;
            }
            let mut stat: libc::stat = std::mem::zeroed();
            libc::fstat(fd, &mut stat) == 0
                && stat.st_dev as u64 == self.device
                && stat.st_ino as u64 == self.inode
        }
    }
}

fn spawn(
    fixture: &Fixture,
    operation: &str,
    target: &str,
    inherit: Option<Identity>,
) -> OriginalChild {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let ready = fixture.0.join(format!("image-{id}.ready"));
    let log_path = fixture.0.join(format!("image-{id}.log"));
    let log = File::create(&log_path).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("ZERUS_AUDIT_IMAGE_ROOT", &fixture.0)
        .env("ZERUS_AUDIT_IMAGE_OPERATION", operation)
        .env("ZERUS_AUDIT_IMAGE_TARGET", target)
        .env("ZERUS_AUDIT_IMAGE_READY", &ready)
        .env_remove("HGS_SESSION")
        .env_remove("HGS_RUN_ID")
        .stdin(Stdio::piped())
        .stdout(log.try_clone().unwrap())
        .stderr(log);
    if let Some(identity) = inherit {
        // Model a concurrent fork at the image writer's open interval. A correct
        // isolated writer gives this parent no matching writable descriptor.
        // SAFETY: the hook uses only bounded fcntl/fstat calls and errno access.
        unsafe {
            command.pre_exec(move || {
                for fd in 3..FD_BOUND {
                    if identity.writable(fd) && libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    let mut original = OriginalChild {
        child: Some(command.spawn().unwrap()),
        ready,
        log: log_path,
        retained: fixture.1.clone(),
    };
    original.ready();
    original
}

pub(super) fn replace(fixture: &Fixture) {
    spawn(fixture, "copy", "hgs-new", None).finish();
    std::fs::rename(fixture.0.join("hgs-new"), fixture.0.join("hgs")).unwrap();
}

pub(super) fn append(fixture: &Fixture) {
    spawn(fixture, "append", "hgs", None).finish();
}

pub(super) fn replace_during_parent_fork(fixture: &Fixture) -> OriginalChild {
    let writer = spawn(fixture, "copy", "hgs-new", None);
    let identity = Identity::at(&fixture.0.join("hgs-new"));
    let inherited = spawn(fixture, "retain", "hgs-new", Some(identity));
    writer.finish(); // Actual original exit closes its writer before publication.
    std::fs::rename(fixture.0.join("hgs-new"), fixture.0.join("hgs")).unwrap();
    inherited
}

#[test]
#[ignore = "owned exact child for the audit image fixture"]
fn image_child() {
    use std::os::unix::fs::OpenOptionsExt;
    let root = PathBuf::from(std::env::var_os("ZERUS_AUDIT_IMAGE_ROOT").unwrap());
    assert!(
        root.is_absolute()
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("hgs-svc-")
    );
    assert!(std::fs::symlink_metadata(&root)
        .unwrap()
        .file_type()
        .is_dir());
    let target = std::env::var("ZERUS_AUDIT_IMAGE_TARGET").unwrap();
    assert!(matches!(target.as_str(), "hgs" | "hgs-new"));
    let target = root.join(target);
    let mut files = Vec::new();
    match std::env::var("ZERUS_AUDIT_IMAGE_OPERATION")
        .unwrap()
        .as_str()
    {
        "copy" => {
            let mut writer = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o700)
                .open(&target)
                .unwrap();
            std::io::copy(&mut File::open(root.join("hgs")).unwrap(), &mut writer).unwrap();
            files.push(writer);
        }
        "append" => {
            Identity::at(&target);
            let mut writer = OpenOptions::new().append(true).open(&target).unwrap();
            writer.write_all(b"\n# changed helper image\n").unwrap();
            files.push(writer);
        }
        "retain" => {
            let identity = Identity::at(&target);
            for fd in 3..FD_BOUND {
                if identity.writable(fd) {
                    // SAFETY: only this exact child owns these inherited handles.
                    files.push(unsafe { File::from_raw_fd(fd) });
                }
            }
        }
        _ => panic!("unknown private image operation"),
    }
    let ready = PathBuf::from(std::env::var_os("ZERUS_AUDIT_IMAGE_READY").unwrap());
    assert_eq!(ready.parent(), Some(root.as_path()));
    std::fs::write(ready, "ready").unwrap();
    // Keep the original writer/copy alive until its own controller releases EOF.
    assert_eq!(std::io::stdin().read(&mut [0]).unwrap(), 0);
    drop(files);
}
