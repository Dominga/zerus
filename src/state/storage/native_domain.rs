//! One original native-input file across a future legacy-to-ID move.
//!
//! Dormant: no record move or writer calls this yet. The caller must retain
//! the state writer throughout preparation and the eventual metadata commit.
//! A busy driver refuses immediately. Both keys refer to the same kernel lock;
//! neither a new lock nor a dropped pathname lock can bridge the move.
use super::*;
use crate::state::repositories::directory::Directory;
use std::fs::{File, Metadata};
use std::os::fd::FromRawFd;
use std::os::unix::fs::MetadataExt;
use std::time::Instant;

/// Retains exclusion only. This is not admission, consent, a delivery receipt
/// or proof of a completed move. Drop unlocks but deliberately keeps aliases,
/// including after an interrupted preparation; recovery must not guess.
pub(in crate::state) struct Domain {
    state: Directory,
    inputs: Directory,
    binding: Directory,
    binding_name: String,
    record: Value,
    file: File,
    original: (u64, u64, u32, u32),
    old: String,
    new: String,
    until: Instant,
}

impl Domain {
    /// One try, under caller-held state writer exclusion. Only an existing
    /// verified legacy record can prepare its fixed session-ID key.
    pub(in crate::state) fn prepare(record: &Value, until: Instant) -> Result<Self> {
        let id = string(record, "session_id");
        if in_id_layout(record)
            || string(record, "name").is_empty()
            || !session_id::canonical(id)
            || uuid::Uuid::parse_str(id).is_ok_and(|id| id.is_nil())
        {
            return Err("The original legacy session identity is unavailable".into());
        }
        deadline(until)?;
        // Resolve only the configured root. All descendant opens are no-follow.
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        if !state.private()? {
            return Err("The original state directory is not private".into());
        }
        let path = label_record_path(string(record, "name"));
        let relative = path.strip_prefix(root()).map_err(|e| e.to_string())?;
        let binding = if relative.parent() == Some(Path::new("bindings")) {
            state.child("bindings")?
        } else if relative.parent() == Some(Path::new("")) {
            Directory::open(&state.path)?
        } else {
            return Err("The legacy binding path changed".into());
        };
        let binding_name = relative
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("The legacy binding name is unavailable")?
            .to_owned();
        if read_original(&binding, &binding_name)? != *record {
            return Err("The original legacy session changed".into());
        }
        let inputs = state.ensure_child("native-input")?;
        if !inputs.private()? {
            return Err("The native input directory is not private".into());
        }
        let old = format!("{}.lock", owner_key(record));
        let new = format!("{id}.lock");
        let file = inputs.open_private_lock(&old)?;
        file.try_lock_exclusive()
            .map_err(|e| format!("The original native input is busy or unavailable: {e}"))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if metadata.len() != 0 {
            return Err("The original native input lock is not empty".into());
        }
        let domain = Self {
            state,
            inputs,
            binding,
            binding_name,
            record: record.clone(),
            original: identity(&metadata),
            file,
            old,
            new,
            until,
        };
        domain.verify_before_link()?;
        let old = component(&domain.old)?;
        let new = component(&domain.new)?;
        let parent = domain.inputs.descriptor();
        // SAFETY: both fixed single-component names are under the same held
        // parent. linkat never replaces a destination or follows the source
        // symlink. Both resulting names are checked against the held file.
        if unsafe { libc::linkat(parent, old.as_ptr(), parent, new.as_ptr(), 0) } != 0 {
            return Err(format!(
                "Cannot retain the native input domain: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: parent is this Domain's original live directory descriptor.
        if unsafe { libc::fsync(parent) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        domain.verify()?;
        Ok(domain)
    }

    fn verify_before_link(&self) -> Result<()> {
        self.verify_scope()?;
        self.verify_file(1)?;
        self.verify_entry(&self.old)?;
        Ok(())
    }

    /// Fresh verification of both original names and the original record.
    /// A later move will need a separate committed-record/recovery protocol;
    /// this preparatory guard deliberately cannot claim that it completed one.
    pub(in crate::state) fn verify(&self) -> Result<()> {
        self.verify_scope()?;
        self.verify_file(2)?;
        self.verify_entry(&self.old)?;
        self.verify_entry(&self.new)?;
        self.verify_file(2)?;
        deadline(self.until)
    }

    fn verify_scope(&self) -> Result<()> {
        deadline(self.until)?;
        self.state.verify()?;
        self.inputs.verify()?;
        self.binding.verify()?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path
            || !self.state.private()?
            || !self.inputs.private()?
            || read_original(&self.binding, &self.binding_name)? != self.record
        {
            return Err("The original native input scope changed".into());
        }
        deadline(self.until)
    }

    fn verify_file(&self, links: u64) -> Result<()> {
        let m = self.file.metadata().map_err(|e| e.to_string())?;
        if !m.is_file()
            || identity(&m) != self.original
            || m.nlink() != links
            || m.len() != 0
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
        {
            return Err("The original native input object changed".into());
        }
        Ok(())
    }

    fn verify_entry(&self, name: &str) -> Result<()> {
        let name = component(name)?;
        // SAFETY: a fixed component under the original directory; never follows
        // links or waits on a special file. This narrow check allows the two
        // intentional links without weakening any general private-file check.
        let fd = unsafe {
            libc::openat(
                self.inputs.descriptor(),
                name.as_ptr(),
                libc::O_RDONLY
                    | libc::O_NOFOLLOW
                    | libc::O_NONBLOCK
                    | libc::O_NOCTTY
                    | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: this call owns the descriptor returned above.
        let file = unsafe { File::from_raw_fd(fd) };
        let m = file.metadata().map_err(|e| e.to_string())?;
        if !m.is_file() || identity(&m) != self.original {
            return Err("A native input name no longer refers to the original object".into());
        }
        Ok(())
    }
}

impl Drop for Domain {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn read_original(directory: &Directory, name: &str) -> Result<Value> {
    serde_json::from_slice(&directory.read_file(name, ID_RECORD_BYTES as usize)?)
        .map_err(|e| format!("Cannot verify the original legacy session: {e}"))
}
fn identity(m: &Metadata) -> (u64, u64, u32, u32) {
    (m.dev(), m.ino(), m.uid(), m.mode())
}
fn component(name: &str) -> Result<std::ffi::CString> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
        return Err("Invalid native input name".into());
    }
    std::ffi::CString::new(name).map_err(|e| e.to_string())
}
fn deadline(until: Instant) -> Result<()> {
    if Instant::now() >= until {
        Err("The original native input preparation expired".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    fn control(case: &str) {
        let temp = tempfile::tempdir().unwrap();
        let output = tempfile::tempfile().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::storage::native_domain::tests::fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", temp.path().join("state"))
            .env("ZERUS_NATIVE_DOMAIN_CONTROL", case)
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output.try_clone().unwrap())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= until {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("{case}: original fixture blocked");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        use std::io::{Read, Seek};
        let mut output = output;
        output.rewind().unwrap();
        let mut text = String::new();
        output.read_to_string(&mut text).unwrap();
        assert!(
            status.success() && text.contains("test result: ok. 1 passed"),
            "{case}: {text}"
        );
    }

    #[test]
    fn an_actual_picker_driver_refuses_preparation_without_a_second_domain() {
        control("busy");
    }
    #[test]
    fn both_names_keep_one_kernel_lock_across_guard_drop() {
        control("domain");
    }
    #[test]
    fn a_waiter_that_opened_the_original_file_keeps_the_same_domain() {
        control("waiter");
    }

    #[test]
    fn foreign_id_entries_are_never_replaced() {
        control("destination");
    }
    #[test]
    fn a_fifo_source_refuses_without_waiting() {
        control("fifo");
    }
    #[test]
    fn an_aliased_input_directory_never_creates_a_foreign_lock() {
        control("directory_alias");
    }
    #[test]
    fn replaced_objects_and_extra_links_invalidate_the_original_guard() {
        control("objects");
    }
    #[test]
    fn changed_record_and_original_deadline_refuse_without_metadata_mutation() {
        control("scope");
    }

    #[test]
    #[ignore = "run only by the bounded private controls"]
    fn fixture() {
        let Ok(case) = std::env::var("ZERUS_NATIVE_DOMAIN_CONTROL") else {
            return;
        };
        private_dir(&root()).unwrap();
        let _writer = lock(None).unwrap();
        let record = json!({"version":1,"name":"claude/example/shared","session_id":uuid::Uuid::new_v4().to_string(),
            "run_id":uuid::Uuid::new_v4().to_string(),"conversation_id":"original","agent":"claude"});
        let binding = legacy_record_path(string(&record, "name"));
        atomic(&binding, &record.to_string()).unwrap();
        let before = fs::read(&binding).unwrap();
        let inputs = root().join("native-input");
        private_dir(&inputs).unwrap();
        let old = inputs.join(format!("{}.lock", owner_key(&record)));
        let new = inputs.join(format!("{}.lock", string(&record, "session_id")));
        let until = || Instant::now() + Duration::from_secs(2);
        match case.as_str() {
            "busy" => {
                let _actual_driver = startup_choice::driving(&record).unwrap();
                let start = Instant::now();
                assert!(Domain::prepare(&record, until()).is_err());
                assert!(start.elapsed() < Duration::from_secs(1));
                assert!(!new.exists());
                assert_eq!(fs::metadata(&old).unwrap().nlink(), 1);
            }
            "domain" => {
                let guard = Domain::prepare(&record, until()).unwrap();
                guard.verify().unwrap();
                assert_eq!(
                    identity(&fs::metadata(&old).unwrap()),
                    identity(&fs::metadata(&new).unwrap())
                );
                assert!(lock_until(&old, Instant::now()).unwrap().is_none());
                assert!(lock_until(&new, Instant::now()).unwrap().is_none());
                // SAFETY: the guard still owns this exact live descriptor.
                assert_ne!(
                    unsafe { libc::fcntl(guard.file.as_raw_fd(), libc::F_GETFD) }
                        & libc::FD_CLOEXEC,
                    0
                );
                drop(guard);
                let actual_legacy = startup_choice::driving(&record).unwrap();
                assert!(lock_until(&new, Instant::now()).unwrap().is_none());
                drop(actual_legacy);
                // Changes only the key supplied to the actual lock API. It
                // creates no ID record and authorizes no native action.
                let mut moved = record.clone();
                moved["layout"] = json!("id");
                let actual_id = startup_choice::driving(&moved).unwrap();
                assert!(lock_until(&old, Instant::now()).unwrap().is_none());
                drop(actual_id);
                assert!(lock_until(&old, Instant::now()).unwrap().is_some());
                assert!(lock_until(&new, Instant::now()).unwrap().is_some());
                // Interrupted/repeated preparation never guesses that the
                // retained aliases mean a metadata move already completed.
                assert!(Domain::prepare(&record, until()).is_err());
                assert!(old.exists() && new.exists());
            }
            "waiter" => {
                // The original driver opened this descriptor before preparation.
                let waiting_file = lock_file(&old).unwrap();
                let domain = Domain::prepare(&record, until()).unwrap();
                let (acquired_tx, acquired_rx) = std::sync::mpsc::sync_channel(1);
                let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
                let waiter = std::thread::spawn(move || {
                    let original_driver = Guard::exclusive(waiting_file).unwrap();
                    acquired_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    drop(original_driver);
                });
                assert!(matches!(
                    acquired_rx.recv_timeout(Duration::from_millis(30)),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                ));
                drop(domain);
                acquired_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                let excluded = lock_until(&new, Instant::now()).unwrap().is_none();
                release_tx.send(()).unwrap();
                waiter.join().unwrap();
                assert!(
                    excluded,
                    "the old queued descriptor must exclude the new key"
                );
                assert!(lock_until(&new, Instant::now()).unwrap().is_some());
            }
            "destination" => {
                fs::write(&new, b"foreign").unwrap();
                assert!(Domain::prepare(&record, until()).is_err());
                assert_eq!(fs::read(&new).unwrap(), b"foreign");
                fs::remove_file(&new).unwrap();
                let foreign = root().join("foreign");
                fs::write(&foreign, b"retained").unwrap();
                std::os::unix::fs::symlink(&foreign, &new).unwrap();
                assert!(Domain::prepare(&record, until()).is_err());
                assert_eq!(fs::read(&foreign).unwrap(), b"retained");
                assert!(fs::symlink_metadata(&new).unwrap().file_type().is_symlink());
            }
            "fifo" => {
                let name = std::ffi::CString::new(old.as_os_str().as_encoded_bytes()).unwrap();
                // SAFETY: this fixture owns this absent NUL-free entry.
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                let start = Instant::now();
                assert!(Domain::prepare(&record, until()).is_err());
                assert!(start.elapsed() < Duration::from_secs(1));
                assert!(!new.exists());
            }
            "directory_alias" => {
                fs::remove_dir(&inputs).unwrap();
                let foreign = root().join("foreign");
                private_dir(&foreign).unwrap();
                std::os::unix::fs::symlink(&foreign, &inputs).unwrap();
                assert!(Domain::prepare(&record, until()).is_err());
                assert_eq!(fs::read_dir(&foreign).unwrap().count(), 0);
            }
            "objects" => {
                let guard = Domain::prepare(&record, until()).unwrap();
                fs::hard_link(&old, inputs.join("extra")).unwrap();
                assert!(guard.verify().is_err());
                fs::remove_file(inputs.join("extra")).unwrap();
                guard.verify().unwrap();
                fs::rename(&new, inputs.join("original-id")).unwrap();
                fs::write(&new, b"foreign").unwrap();
                assert!(guard.verify().is_err());
                assert_eq!(fs::read(&new).unwrap(), b"foreign");
                fs::remove_file(&new).unwrap();
                fs::rename(inputs.join("original-id"), &new).unwrap();
                guard.verify().unwrap();
                fs::rename(&inputs, root().join("retained-inputs")).unwrap();
                private_dir(&inputs).unwrap();
                fs::write(inputs.join("foreign"), b"retained").unwrap();
                assert!(guard.verify().is_err());
                assert_eq!(fs::read_dir(&inputs).unwrap().count(), 1);
            }
            "scope" => {
                assert!(Domain::prepare(&record, Instant::now()).is_err());
                assert_eq!(fs::read_dir(&inputs).unwrap().count(), 0);
                let mut guard = Domain::prepare(&record, until()).unwrap();
                let mut changed = record.clone();
                changed["conversation_id"] = json!("different");
                atomic(&binding, &changed.to_string()).unwrap();
                assert!(guard.verify().is_err());
                atomic(&binding, &record.to_string()).unwrap();
                guard.verify().unwrap();
                guard.until = Instant::now();
                assert!(guard.verify().is_err());
            }
            _ => panic!("unknown private control"),
        }
        assert_eq!(
            fs::read(&binding).unwrap(),
            before,
            "preparation must not move or rewrite a record"
        );
        assert!(!root().join("sessions").exists());
        assert!(!root().join("labels").exists());
    }
}
