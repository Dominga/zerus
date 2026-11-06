//! Close-only custody for the fixed mutating helper and descendants which close
//! stdio. This is accounting, never a process selector, signal or Remove permit.
use std::{io, os::fd::AsRawFd, sync::Arc};
use zerus_client_core::run::custody::InheritedLock;

pub(super) struct Probe(Arc<std::sync::atomic::AtomicBool>);
impl Probe {
    pub(super) fn new(hold: Arc<dyn Send + Sync>) -> io::Result<(Self, Arc<InheritedLock>)> {
        let temporary = tempfile::Builder::new()
            .prefix("zerus-program-phase-")
            .tempfile()?;
        // A DIFFERENT open-file description observes the original flock. A
        // try_clone would share it and could falsely convert our own lock.
        let observer = temporary.reopen()?;
        let (original, path) = temporary.into_parts();
        let inherited = InheritedLock::try_lock(original)?;
        let drained = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let joined = drained.clone();
        std::thread::Builder::new()
            .name("program-phase-custody".into())
            .spawn(move || {
                let held = (hold, observer, path);
                loop {
                    if unsafe { libc::flock(held.1.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }
                        == 0
                    {
                        break;
                    }
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::EAGAIN) {
                        // An unreadable lock is not drain. Match the owned runner's
                        // unverified-child rule: keep the bounded admission charged.
                        std::mem::forget(held);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                // Dropping the GUI receiver cannot drop these original Source,
                // image, environment, Actions and read-only holder resources early.
                drop(held);
                joined.store(true, std::sync::atomic::Ordering::Release);
            })?;
        Ok((Self(drained), inherited))
    }
    /// No receipt/ACK/timeout substitutes for physical last-descriptor release.
    pub(super) fn drained(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::BufRead,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn a_detached_descendant_without_stdio_still_holds_the_original_phase() {
        let _process = crate::PROCESS_TREE.read().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let release = temporary.path().join("release");
        struct Release(std::path::PathBuf);
        impl Drop for Release {
            fn drop(&mut self) {
                let _ = std::fs::write(&self.0, b"release");
            }
        }
        let _release = Release(release.clone());
        let hold: Arc<dyn Send + Sync> = Arc::new(());
        let weak = Arc::downgrade(&hold);
        let (probe, inherited) = Probe::new(hold).unwrap();
        let mut command = Command::new("python3");
        command
            .args([
                "-c",
                r#"import os,pathlib,sys,time
if os.fork()==0:
 os.setsid()
 for fd in (0,1,2):
  null=os.open('/dev/null',os.O_RDWR);os.dup2(null,fd)
  if null>2:os.close(null)
 until=time.monotonic()+10
 while not pathlib.Path(sys.argv[1]).exists() and time.monotonic()<until:time.sleep(.005)
 os._exit(0)
print('original exited',flush=True)
"#,
            ])
            .arg(&release)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        inherited.inherit(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line, "original exited\n");
        assert!(child.wait().unwrap().success());
        drop(child);
        drop(command); // releases the pre-exec closure's original lock copy
        drop(inherited); // all parent resources are now actually gone
        assert!(
            !probe.drained(),
            "exit and stdio EOF do not prove descendant drain"
        );
        assert!(
            weak.upgrade().is_some(),
            "original resources stay charged without child or pipes"
        );
        let observed = probe.0.clone();
        drop(probe); // a discarded GUI continuation cannot drop its real charge
        assert!(weak.upgrade().is_some());
        std::fs::write(&release, b"release").unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !observed.load(std::sync::atomic::Ordering::Acquire) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(observed.load(std::sync::atomic::Ordering::Acquire));
        assert!(weak.upgrade().is_none());
    }
}
