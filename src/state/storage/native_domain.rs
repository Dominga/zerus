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

mod fresh;
mod kind;
mod reservation;
pub(in crate::state) use fresh::Fresh;
pub(in crate::state) use kind::{InputKind, KindProof};

/// A negative allocation decision only. This does not select or mutate an
/// existing first holder, nor prove that any record move completed.
pub(in crate::state) fn legacy_key_available(name: &str) -> bool {
    reservation::legacy_key_available(name)
}

/// Retains exclusion only. This is not admission, consent, a delivery receipt
/// or proof of a completed move. Drop unlocks but deliberately keeps aliases,
/// including after an interrupted preparation; recovery must not guess.
pub(in crate::state) struct Domain {
    state: Directory,
    inputs: Directory,
    binding: Option<Directory>,
    binding_name: String,
    record: Value,
    file: File,
    original: (u64, u64, u32, u32),
    old: String,
    new: String,
    until: Instant,
    reservation: reservation::Held,
    kind: reservation::SidHeld,
}

/// Original exclusion acquired BEFORE the record/job transaction writes a
/// pending marker. It cannot send input or prove a completed metadata move.
pub(in crate::state) struct Original {
    state: Directory,
    inputs: Directory,
    binding: Directory,
    binding_name: String,
    record: Value,
    file: File,
    original: (u64, u64, u32, u32),
    version: Value,
    old: String,
    new: String,
    until: Instant,
}

fn file_version(file: &File) -> Result<Value> {
    let m = file.metadata().map_err(|e| e.to_string())?;
    Ok(json!([
        m.dev(),
        m.ino(),
        m.mode(),
        m.uid(),
        m.gid(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec()
    ]))
}
impl Original {
    pub(in crate::state) fn acquire(record: &Value, until: Instant) -> Result<Self> {
        Self::open(record, until, true, None)
    }
    /// The metadata transaction checks the actual writer BEFORE any input
    /// namespace/lock creation, then retains that writer in its own scope.
    pub(in crate::state) fn acquire_under(
        writer: &Guard,
        record: &Value,
        until: Instant,
    ) -> Result<Self> {
        Self::open(record, until, true, Some(writer))
    }
    /// No creation or reconstruction in recovery, even before comparison of
    /// the immutable journal's original file/directory version.
    pub(in crate::state) fn recover(
        record: &Value,
        version: &Value,
        until: Instant,
    ) -> Result<Self> {
        let held = Self::open(record, until, false, None)?;
        if held.version()? != *version {
            return Err("The original pre-transition input object changed".into());
        }
        Ok(held)
    }
    fn open(record: &Value, until: Instant, create: bool, writer: Option<&Guard>) -> Result<Self> {
        let id = string(record, "session_id");
        if in_id_layout(record)
            || string(record, "name").is_empty()
            || !session_id::canonical(id)
            || uuid::Uuid::parse_str(id).is_ok_and(|id| id.is_nil())
        {
            return Err("The original legacy session identity is unavailable".into());
        }
        deadline(until)?;
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        if !state.private()? {
            return Err("The original state directory is not private".into());
        }
        if let Some(writer) = writer {
            let current = state.read_private(".lock")?;
            if crate::platform::FileIdentity::of(&current).map_err(|e| e.to_string())?
                != writer.identity().map_err(|e| e.to_string())?
            {
                return Err("The original metadata writer changed before input preparation".into());
            }
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
        let inputs = if create {
            reservation::inputs_for_prepare(&state, record)?
        } else {
            reservation::inputs_for_existing(&state, record)?
        };
        if !inputs.private()? {
            return Err("The native input directory is not private".into());
        }
        if create {
            reservation::refuse_allocated_sid(&state, &inputs, id)?;
        }
        let old = format!("{}.lock", owner_key(record));
        let new = format!("{id}.lock");
        let file = if create {
            inputs.open_private_lock(&old)?
        } else {
            open_entry(&inputs, &old, true)?
        };
        file.try_lock_exclusive()
            .map_err(|e| format!("The original native input is busy or unavailable: {e}"))?;
        let m = file.metadata().map_err(|e| e.to_string())?;
        if m.len() != 0
            || !crate::platform::verify_private_file(&file).map_err(|e| e.to_string())?
        {
            return Err("The original native input lock is not empty and private".into());
        }
        let held = Self {
            state,
            inputs,
            binding,
            binding_name,
            record: record.clone(),
            original: identity(&m),
            version: file_version(&file)?,
            file,
            old,
            new,
            until,
        };
        held.verify(record)?;
        Ok(held)
    }
    pub(in crate::state) fn version(&self) -> Result<Value> {
        Ok(json!({"state":self.state.object()?,"inputs":self.inputs.object()?,"file":self.version}))
    }
    fn verify(&self, current: &Value) -> Result<()> {
        deadline(self.until)?;
        self.state.verify()?;
        self.inputs.verify()?;
        self.binding.verify()?;
        // Only negative transition/protection fields may have been added by
        // the same writer. Native/run/label/history values never change here.
        let mut unchanged = current.clone();
        for key in ["layout_transition", "worktree_protection"] {
            match self.record.get(key) {
                Some(value) if current.get(key) != Some(value) => {
                    return Err("Original negative retention evidence changed".into())
                }
                Some(value) => unchanged[key] = value.clone(),
                None => {
                    unchanged
                        .as_object_mut()
                        .ok_or("Invalid original binding")?
                        .remove(key);
                }
            }
        }
        if current
            .get("worktree_protection")
            .is_some_and(|value| value != &current["worktree_owner"]["id"])
            || current.get("layout_transition").is_some_and(|marker| {
                marker["version"] != 1
                    || string(marker, "before_sha256").len() != 64
                    || !string(marker, "before_sha256")
                        .bytes()
                        .all(|c| c.is_ascii_hexdigit())
            })
        {
            return Err("Invalid negative transition evidence".into());
        }
        if unchanged != self.record
            || read_original(&self.binding, &self.binding_name)? != *current
            || !self.state.private()?
            || !self.inputs.private()?
            || crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path
            || file_version(&self.file)? != self.version
            || !crate::platform::verify_private_file(&self.file).map_err(|e| e.to_string())?
        {
            return Err("The original native input preparation changed".into());
        }
        let current_file = open_entry(&self.inputs, &self.old, false)?;
        if identity(&current_file.metadata().map_err(|e| e.to_string())?) != self.original {
            return Err("The original native input name changed".into());
        }
        deadline(self.until)
    }
    pub(in crate::state) fn prepare(self, current: &Value) -> Result<Domain> {
        self.verify(current)?;
        let reservation =
            reservation::Held::create(&self.state, &self.inputs, current, self.original)?;
        let kind =
            reservation::SidHeld::create_moved(&self.state, &self.inputs, &reservation, current)?;
        let domain = Domain {
            state: self.state,
            inputs: self.inputs,
            binding: Some(self.binding),
            binding_name: self.binding_name,
            record: current.clone(),
            file: self.file,
            original: self.original,
            old: self.old,
            new: self.new,
            until: self.until,
            reservation,
            kind,
        };
        domain.verify_before_link()?;
        domain.link()?;
        domain.verify()?;
        Ok(domain)
    }
}

impl Domain {
    /// One try, under caller-held state writer exclusion. Only an existing
    /// verified legacy record can prepare its fixed session-ID key.
    pub(in crate::state) fn prepare(record: &Value, until: Instant) -> Result<Self> {
        Original::acquire(record, until)?.prepare(record)
    }

    /// Recover exclusion from the immutable original before-image and its
    /// retained proof, even after rename/archive removed the old record. This
    /// opens the SAME lock object without creating/replacing either lock. It
    /// proves only exclusion, never completed metadata or an input outcome.
    pub(in crate::state) fn recover(record: &Value, until: Instant) -> Result<Self> {
        deadline(until)?;
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let inputs = state.child("native-input")?;
        if !state.private()? || !inputs.private()? {
            return Err("The original native input directories are not private".into());
        }
        let reservation = reservation::Held::load(&state, &inputs, record)?;
        let kind = reservation::SidHeld::load(&state, &inputs, string(record, "session_id"))?;
        kind.verify_moved(&state, &inputs, &reservation)?;
        let old = format!("{}.lock", owner_key(record));
        let new = format!("{}.lock", string(record, "session_id"));
        let file = open_entry(&inputs, &old, true)?;
        file.try_lock_exclusive()
            .map_err(|e| format!("The retained native input is busy or unavailable: {e}"))?;
        let domain = Self {
            state,
            inputs,
            binding: None,
            binding_name: String::new(),
            record: record.clone(),
            file,
            original: reservation.original(),
            old,
            new,
            until,
            reservation,
            kind,
        };
        domain.verify_scope()?;
        // Never reconstruct even a missing SID name: a pre-link crash and a
        // later deletion cannot be distinguished from this proof. Both remain
        // negative allocations and require review, not a newly minted domain.
        domain.verify()?;
        Ok(domain)
    }

    fn link(&self) -> Result<()> {
        let domain = self;
        let old = component(&domain.old)?;
        let new = component(&domain.new)?;
        let parent = domain.inputs.descriptor();
        cut_write()?;
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
        Ok(())
    }

    fn verify_before_link(&self) -> Result<()> {
        self.verify_scope()?;
        self.verify_file(1)?;
        self.verify_entry(&self.old)?;
        Ok(())
    }

    /// Fresh verification of the original objects and retained reservation.
    /// Preparation also requires its unchanged legacy record. Recovery relies
    /// on the exact original before-image in the proof; it does not recreate it.
    pub(in crate::state) fn verify(&self) -> Result<()> {
        self.verify_scope()?;
        self.verify_file(2)?;
        self.verify_entry(&self.old)?;
        self.verify_entry(&self.new)?;
        self.verify_file(2)?;
        deadline(self.until)
    }

    fn verify_scope(&self) -> Result<()> {
        self.verify_scope_objects()?;
        if let Some(binding) = &self.binding {
            binding.verify()?;
            if read_original(binding, &self.binding_name)? != self.record {
                return Err("The original legacy binding changed".into());
            }
        }
        deadline(self.until)
    }

    fn verify_scope_objects(&self) -> Result<()> {
        deadline(self.until)?;
        self.state.verify()?;
        self.inputs.verify()?;
        self.reservation
            .verify(&self.state, &self.inputs, &self.record)?;
        self.kind
            .verify_moved(&self.state, &self.inputs, &self.reservation)?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path
            || !self.state.private()?
            || !self.inputs.private()?
        {
            return Err("The original native input scope changed".into());
        }
        deadline(self.until)
    }

    /// Detach the legacy binding check ONLY after the exact canonical ID
    /// record exists and that same complete original is retained by the
    /// transaction. This grants no input or historical-effect acceptance.
    pub(in crate::state) fn record_retained(
        &mut self,
        retained: &Directory,
        name: &str,
    ) -> Result<()> {
        self.verify_scope_objects()?;
        self.verify_file(2)?;
        self.verify_entry(&self.old)?;
        self.verify_entry(&self.new)?;
        retained.verify()?;
        if !retained.private()? || read_original(retained, name)? != self.record {
            return Err("The retained original record is unconfirmed".into());
        }
        if let Some(binding) = &self.binding {
            binding.verify()?;
            if binding.read_private_optional(&self.binding_name)?.is_some() {
                return Err("The original legacy record is still published".into());
            }
        }
        let mut expected = self.record.clone();
        expected["layout"] = json!("id");
        let sessions = self.state.child("sessions")?;
        if read_original(
            &sessions,
            &format!("{}.json", string(&self.record, "session_id")),
        )? != expected
        {
            return Err("The published ID record does not retain the exact original".into());
        }
        self.binding = None;
        self.verify()
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
        let file = open_entry(&self.inputs, name, false)?;
        let m = file.metadata().map_err(|e| e.to_string())?;
        if !m.is_file() || identity(&m) != self.original {
            return Err("A native input name no longer refers to the original object".into());
        }
        Ok(())
    }
}

fn open_entry(directory: &Directory, name: &str, writable: bool) -> Result<File> {
    let name = component(name)?;
    // SAFETY: one fixed component under the original parent. No creation,
    // link-follow, device control or blocking FIFO open is requested. The two
    // intentional links are verified by this narrow domain, never by relaxing
    // the general private-file contract.
    let fd = unsafe {
        libc::openat(
            directory.descriptor(),
            name.as_ptr(),
            (if writable {
                libc::O_RDWR
            } else {
                libc::O_RDONLY
            }) | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_NOCTTY
                | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: this call exclusively owns the new descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
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
mod kind_tests;

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
    fn a_later_label_holder_has_its_own_domain_and_an_old_keep_warm_cannot_send() {
        control("reservation_reuse");
    }

    #[test]
    fn reservation_and_original_inodes_survive_archive_restore_and_recovery() {
        control("reservation_archive");
    }

    #[test]
    fn every_preparation_crash_cut_keeps_the_legacy_key_unavailable() {
        control("reservation_cuts");
    }

    #[test]
    fn a_record_already_moved_without_a_reservation_recovers_fail_closed() {
        control("reservation_move_cut");
    }

    #[test]
    fn missing_malformed_and_irregular_reservation_evidence_never_permits_reuse() {
        control("reservation_evidence");
    }

    #[test]
    fn a_rebuilt_empty_namespace_cannot_supply_negative_entry_absence() {
        control("reservation_namespace");
    }

    #[test]
    fn recovery_never_recreates_either_original_lock_name() {
        control("reservation_missing_lock");
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
            "reservation_reuse" | "reservation_archive" => {
                assert!(
                    !legacy_key_available(string(&record, "name")),
                    "unanchored absence is unknown"
                );
                // The worker opened its native-input descriptor BEFORE the
                // metadata move and retains that same open file throughout.
                let old_worker_file = lock_file(&old).unwrap();
                let domain = Domain::prepare(&record, until()).unwrap();
                let original = domain.original;
                let proof_path = root()
                    .join("native-input-reservations")
                    .join(format!("{}.json", owner_key(&record)));
                let proof_before = fs::read(&proof_path).unwrap();
                let mut moved = record.clone();
                moved["layout"] = json!("id");
                // Private metadata cut only: the full record/job transaction
                // and its production caller are deliberately still absent.
                let sessions = Directory::open(&absolute_root().unwrap())
                    .unwrap()
                    .ensure_child("sessions")
                    .unwrap();
                labels::add(string(&moved, "name"), string(&moved, "session_id")).unwrap();
                sessions
                    .write_new(
                        &format!("{}.json", string(&moved, "session_id")),
                        moved.to_string().as_bytes(),
                    )
                    .unwrap();
                archive::remove_synced(&binding).unwrap();
                drop(domain);
                assert!(!binding.exists());
                assert!(!route_path(string(&record, "name")).exists());
                assert!(!legacy_key_available(string(&record, "name")));
                assert!(legacy_key_available("claude/example/unrelated"));
                {
                    let held = Domain::recover(&record, until()).unwrap();
                    held.verify().unwrap();
                    assert_eq!(held.original, original);
                }
                let old_name = string(&record, "name");
                let renamed = "claude/example/moved";
                labels::add(renamed, string(&moved, "session_id")).unwrap();
                moved["name"] = json!(renamed);
                write(&mut moved).unwrap();
                labels::remove(old_name, string(&moved, "session_id")).unwrap();
                let mut later = record.clone();
                later["session_id"] = json!(uuid::Uuid::new_v4().to_string());
                later["run_id"] = json!(uuid::Uuid::new_v4().to_string());
                later["conversation_id"] = json!("later-conversation");
                later["layout"] = json!("id");
                publish_new_id(&_writer, &mut later).unwrap();
                let old_worker = Guard::exclusive(old_worker_file).unwrap();
                let later_driver = startup_choice::driving(&later).unwrap();
                assert_ne!(
                    old_worker.identity().unwrap(),
                    later_driver.identity().unwrap()
                );
                assert!(lock_until(&new, Instant::now()).unwrap().is_none());
                assert_eq!(identity(&fs::metadata(&old).unwrap()), original);
                assert_eq!(identity(&fs::metadata(&new).unwrap()), original);
                drop(later_driver);
                if case == "reservation_archive" {
                    let archived = archive::save(&moved).unwrap();
                    assert!(!id_record_path(string(&moved, "session_id"))
                        .unwrap()
                        .exists());
                    let mut restored = archive::read_archive(renamed, &archived).unwrap();
                    assert!(
                        restored["layout"].is_null(),
                        "existing archive schema stays unchanged"
                    );
                    assert!(!legacy_key_available(old_name));
                    restored["layout"] = json!("id");
                    restored.as_object_mut().unwrap().remove("archive_id");
                    restored.as_object_mut().unwrap().remove("archived_at");
                    restored["state"] = json!("stopped");
                    sessions
                        .write_new(
                            &format!("{}.json", string(&restored, "session_id")),
                            restored.to_string().as_bytes(),
                        )
                        .unwrap();
                    assert_eq!(fs::read(&proof_path).unwrap(), proof_before);
                }
                assert_eq!(
                    fs::read_dir(root().join("native-input-reservations"))
                        .unwrap()
                        .count(),
                    3 // one old-key proof plus the moved and later SID kinds
                );
                assert_eq!(
                    fs::metadata(inputs.join(format!("{}.lock", owner_key(&later))))
                        .unwrap()
                        .nlink(),
                    1
                );
                assert!(!binding.exists());
                drop(old_worker);
                Domain::recover(&record, until()).unwrap().verify().unwrap();
                assert_eq!(fs::read(&proof_path).unwrap(), proof_before);
                if case == "reservation_reuse" {
                    // An actual old worker request, with its captured original
                    // name/run/conversation, reaches the maintained input
                    // admission. Identity refusal precedes native inspection,
                    // settings, receipt reservation and every terminal call.
                    drop(_writer);
                    let request_id = uuid::Uuid::new_v4().to_string();
                    let request = json!({"request_id":request_id,"text":keep_warm::TEXT,
                        "expected_run_id":record["run_id"],"expected_conversation_id":record["conversation_id"]});
                    match input::submit_keep_warm(
                        old_name,
                        request.to_string().as_bytes(),
                        &request_id,
                    ) {
                        input::Submission::NotOffered(witness) => {
                            assert!(witness.reason.contains("identity changed"))
                        }
                        _ => panic!("the old worker must be refused before input"),
                    }
                    assert!(!root()
                        .join("input_receipts")
                        .join(format!("{request_id}.json"))
                        .exists());
                }
            }
            "reservation_cuts" => {
                // Each cut has a distinct legacy session/namespace. No cut
                // gets a newly reconstructed lock or a legacy reuse answer.
                for budget in 0..=3 {
                    let mut original = record.clone();
                    original["name"] = json!(format!("claude/example/cut-{budget}"));
                    original["session_id"] = json!(uuid::Uuid::new_v4().to_string());
                    // Bootstrap is separate for each first-write cut.
                    let selected = root();
                    let sub = selected.join(format!("cut-{budget}"));
                    private_dir(&sub).unwrap();
                    unsafe { std::env::set_var("HGS_STATE_DIR", &sub) };
                    let path = legacy_record_path(string(&original, "name"));
                    atomic(&path, &original.to_string()).unwrap();
                    cut::arm(budget);
                    let result = Domain::prepare(&original, until());
                    cut::disarm();
                    drop(result);
                    assert_eq!(
                        serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
                        original
                    );
                    assert!(!legacy_key_available(string(&original, "name")));
                    let old = root()
                        .join("native-input")
                        .join(format!("{}.lock", owner_key(&original)));
                    let sid = root()
                        .join("native-input")
                        .join(format!("{}.lock", string(&original, "session_id")));
                    let old_identity = identity(&fs::metadata(&old).unwrap());
                    if sid.exists() {
                        assert_eq!(identity(&fs::metadata(&sid).unwrap()), old_identity);
                        Domain::recover(&original, until())
                            .unwrap()
                            .verify()
                            .unwrap();
                    } else {
                        assert!(Domain::recover(&original, until()).is_err());
                        assert!(!sid.exists(), "recovery must not manufacture a lost alias");
                    }
                    unsafe { std::env::set_var("HGS_STATE_DIR", &selected) };
                }
            }
            "reservation_evidence" => {
                let domain = Domain::prepare(&record, until()).unwrap();
                drop(domain);
                let mut different_before = record.clone();
                different_before["conversation_id"] = json!("different");
                assert!(Domain::recover(&different_before, until()).is_err());
                let path = root()
                    .join("native-input-reservations")
                    .join(format!("{}.json", owner_key(&record)));
                let original = fs::read(&path).unwrap();
                for contents in [b"null".as_slice(), b"{}", b"[]", b"garbage"] {
                    fs::write(&path, contents).unwrap();
                    assert!(!legacy_key_available(string(&record, "name")));
                    assert!(Domain::recover(&record, until()).is_err());
                }
                fs::write(&path, original).unwrap();
                fs::rename(&path, path.with_extension("retained")).unwrap();
                assert!(
                    !legacy_key_available(string(&record, "name")),
                    "the original lock pair is independently negative"
                );
                assert!(Domain::recover(&record, until()).is_err());
                use std::os::unix::fs::symlink;
                symlink(path.with_extension("retained"), &path).unwrap();
                assert!(!legacy_key_available(string(&record, "name")));
                assert!(Domain::recover(&record, until()).is_err());
                fs::remove_file(&path).unwrap();
                let path_c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) }, 0);
                let started = Instant::now();
                assert!(!legacy_key_available(string(&record, "name")));
                assert!(Domain::recover(&record, until()).is_err());
                assert!(started.elapsed() < Duration::from_secs(1));
            }
            "reservation_namespace" => {
                drop(Domain::prepare(&record, until()).unwrap());
                let directory = root().join("native-input-reservations");
                fs::rename(&directory, root().join("original-reservations")).unwrap();
                private_dir(&directory).unwrap();
                assert!(!legacy_key_available("claude/example/unused"));
                assert!(Domain::recover(&record, until()).is_err());
                fs::remove_dir(&directory).unwrap();
                fs::rename(root().join("original-reservations"), &directory).unwrap();
                fs::rename(&inputs, root().join("original-inputs")).unwrap();
                private_dir(&inputs).unwrap();
                assert!(!legacy_key_available("claude/example/unused"));
                assert!(Domain::recover(&record, until()).is_err());
                assert!(Domain::prepare(&record, until()).is_err());
                assert_eq!(
                    fs::read_dir(&inputs).unwrap().count(),
                    0,
                    "recovery must not create a new old lock"
                );
            }
            "reservation_missing_lock" => {
                drop(Domain::prepare(&record, until()).unwrap());
                let original = identity(&fs::metadata(&old).unwrap());
                fs::rename(&new, inputs.join("retained-sid.lock")).unwrap();
                assert!(Domain::recover(&record, until()).is_err());
                assert!(!new.exists());
                assert_eq!(identity(&fs::metadata(&old).unwrap()), original);
                fs::rename(inputs.join("retained-sid.lock"), &new).unwrap();
                fs::rename(&old, inputs.join("retained-legacy.lock")).unwrap();
                assert!(Domain::recover(&record, until()).is_err());
                assert!(Domain::prepare(&record, until()).is_err());
                assert!(!old.exists());
                assert_eq!(identity(&fs::metadata(&new).unwrap()), original);
            }
            "reservation_move_cut" => {
                // A private post-move/pre-reservation cut in the older dormant
                // preparation shape: both original links exist, no negative
                // entry was published, and the legacy record is already gone.
                // New preparation orders its reservation first; recovery must
                // still refuse this incomplete evidence without reconstructing.
                let old_file = lock_file(&old).unwrap();
                fs::hard_link(&old, &new).unwrap();
                let original = identity(&old_file.metadata().unwrap());
                let mut moved = record.clone();
                moved["layout"] = json!("id");
                let sessions = Directory::open(&absolute_root().unwrap())
                    .unwrap()
                    .ensure_child("sessions")
                    .unwrap();
                labels::add(string(&moved, "name"), string(&moved, "session_id")).unwrap();
                sessions
                    .write_new(
                        &format!("{}.json", string(&moved, "session_id")),
                        moved.to_string().as_bytes(),
                    )
                    .unwrap();
                archive::remove_synced(&binding).unwrap();
                assert!(!legacy_key_available(string(&record, "name")));
                assert!(Domain::recover(&record, until()).is_err());
                assert_eq!(
                    read_id(string(&record, "session_id")).unwrap()["run_id"],
                    record["run_id"]
                );
                assert_eq!(identity(&fs::metadata(&old).unwrap()), original);
                assert_eq!(identity(&fs::metadata(&new).unwrap()), original);
                assert!(!root().join("native-input-reservations").exists());
                return;
            }
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
        if matches!(case.as_str(), "reservation_reuse" | "reservation_archive") {
            return;
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
