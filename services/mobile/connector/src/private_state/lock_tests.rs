//! Actual Store exclusion with an independently forked holder of its description.
//! Passing a cloned description through stdin avoids unsafe code in this crate.
use super::*;
use std::os::{fd::OwnedFd, unix::net::UnixStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct ForkHolder {
    child: Child,
    gate: UnixStream,
}

impl ForkHolder {
    fn start(file: &File) -> Self {
        let (gate, child_gate) = UnixStream::pair().unwrap();
        gate.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        gate.set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let child = Command::new("python3")
            .args([
                "-I",
                "-c",
                r#"import os, select
pid = os.fork()
if pid == 0:
    os.write(2, b'r')
    ready, _, _ = select.select([2], [], [], 10)
    os._exit(0 if ready and os.read(2, 1) else 1)
os.close(0)
_, status = os.waitpid(pid, 0)
os._exit(os.waitstatus_to_exitcode(status))
"#,
            ])
            .stdin(Stdio::from(file.try_clone().unwrap()))
            .stdout(Stdio::null())
            .stderr(Stdio::from(OwnedFd::from(child_gate)))
            .spawn()
            .unwrap();
        let mut holder = Self { child, gate };
        // The Python fork child now retains the exact Store lock description,
        // and cannot close it until this test opens its private gate.
        gate_read(&mut holder.gate, b'r');
        holder
    }
}

fn gate_read(gate: &mut UnixStream, expected: u8) {
    let mut byte = [0];
    gate.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [expected]);
}

impl Drop for ForkHolder {
    fn drop(&mut self) {
        let _ = self.gate.write_all(b"x");
        let until = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        // Only our still-unreaped fixture child; no session or native agent.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

#[test]
fn owner_drop_releases_store_while_a_fork_retains_its_description() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private");
    let store = Store::lock(&path, true).unwrap();
    let _holder = ForkHolder::start(store._lock.file());
    assert!(Store::try_lock(&path, false).unwrap().is_none());
    drop(store);
    assert!(
        Store::try_lock(&path, false).unwrap().is_some(),
        "the owner scope must release before the private fork exits"
    );
}

#[test]
fn a_failed_private_turn_releases_before_the_retained_description_closes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("private");
    let mut holder = None;
    let result = (|| -> Result<()> {
        let store = Store::lock(&path, true)?;
        holder = Some(ForkHolder::start(store._lock.file()));
        store.replace("../outside", "", b"refused")?;
        Ok(())
    })();
    assert!(result.is_err());
    assert!(
        Store::try_lock(&path, false).unwrap().is_some(),
        "a failed turn must not leave exclusion with the private fork"
    );
    drop(holder);
}
