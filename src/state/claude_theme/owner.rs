//! The resident supervisor's original native binding and actual exclusions.
use super::*;
use crate::state::{
    guarded_recovery::Original, repositories::directory::Directory, storage::Guard,
};
use fs2::FileExt;
use std::{
    fs::File,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Instant,
};

const BINDING: &[&str] = &[
    "agent",
    "run_identity_version",
    "layout",
    "session_id",
    "run_id",
    "pane",
    "pid",
    "process_start",
    "supervisor",
    "expected_id",
    "launch_dir",
    "account_id",
    "agent_home",
];
pub(super) const CLAIMS: &str = "claude-theme-confirmations";

pub(super) fn claim_name(record: &Value) -> String {
    // One slot per original run is stricter than a fresh PID/UUID slot: a
    // first->base fallback cannot mint a second automatic Enter.
    format!(
        "{:x}.json",
        Sha256::digest(
            json!([
                "claude-first-run-theme-v1",
                record["session_id"],
                record["run_id"]
            ])
            .to_string()
        )
    )
}

fn path(record: &Value) -> Result<PathBuf> {
    if storage::in_id_layout(record) {
        storage::id_record_path(string(record, "session_id"))
            .ok_or("Invalid theme session ID".into())
    } else {
        Ok(storage::label_record_path(string(record, "name")))
    }
}

fn metadata(original: &Value) -> Result<Value> {
    let records =
        session_index::metadata_records_for_id_unrepaired(string(original, "session_id"))?;
    let mut candidates = records.into_iter().filter(|record| {
        record["session_id"] == original["session_id"] && record["run_id"] == original["run_id"]
    });
    let record = candidates
        .next()
        .ok_or("Original Claude startup run disappeared")?;
    if candidates.next().is_some() {
        return Err("Original Claude startup run is ambiguous".into());
    }
    Ok(record)
}

pub(super) struct Completion(mpsc::Sender<()>);
impl Drop for Completion {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

/// Immutable evidence captured by the actual spawn, not adopted from a later
/// record or a numeric PID lookup. It never waits or signals the agent.
#[derive(Clone)]
pub(super) struct Binding {
    pub(super) record: Value,
    pub(super) process: crate::platform::ProcessIdentity,
    pub(super) supervisor_birth: String,
}
impl Binding {
    pub(super) fn current(&self, record: &Value) -> Result<()> {
        let pid = self.record["pid"]
            .as_u64()
            .ok_or("Missing original Claude PID")? as u32;
        let process =
            crate::platform::process_identity(pid).ok_or("Original Claude process unavailable")?;
        let supervisor = crate::platform::process_identity(std::process::id())
            .ok_or("Original Claude supervisor unavailable")?;
        if process.birth != self.process.birth
            || process.parent != self.process.parent
            || process.parent != std::process::id()
            || supervisor.birth != self.supervisor_birth
            || !eligible(record)
            || BINDING
                .iter()
                .any(|key| record.get(*key) != self.record.get(*key))
        {
            return Err("Original Claude startup identity changed".into());
        }
        Ok(())
    }
}

pub(super) struct Observation {
    state: Directory,
    binding: Binding,
    until: Instant,
    actor: std::thread::ThreadId,
    _complete: Completion,
}
impl Observation {
    pub(super) fn new(binding: Binding, until: Instant) -> Result<(Arc<Self>, mpsc::Receiver<()>)> {
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let (send, done) = mpsc::channel();
        let source = Arc::new(Self {
            state,
            binding,
            until,
            actor: std::thread::current().id(),
            _complete: Completion(send),
        });
        helper::Source::check(source.as_ref())?;
        Ok((source, done))
    }
}
impl helper::Source for Observation {
    fn check(&self) -> Result<()> {
        helper::boundary(self.until)?;
        self.state.verify()?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path {
            return Err("Claude startup state route changed".into());
        }
        self.binding.current(&metadata(&self.binding.record)?)?;
        helper::boundary(self.until)
    }
    fn until(&self) -> Instant {
        self.until
    }
    fn offer(&self, call: &helper::Call) -> Result<()> {
        if self.actor != std::thread::current().id()
            || !matches!(call,helper::Call::Capture(pane) if pane==string(&self.binding.record,"pane"))
        {
            return Err("Only the original supervisor may observe its startup pane".into());
        }
        self.check()
    }
}

pub(super) enum Acquisition {
    Busy,
    Spent,
    Ready(Arc<Scope>, mpsc::Receiver<()>),
}

struct Claim {
    file: File,
    version: Value,
}
fn file_version(file: &File) -> Result<Value> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata().map_err(|e| e.to_string())?;
    Ok(json!([
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
        m.mode(),
        m.uid(),
        m.gid(),
        m.nlink()
    ]))
}

pub(super) struct Scope {
    // FIRST: the SAME actual locks and every original data proof drop before
    // the last-field completion can let the supervisor resume its agent wait.
    writer: Guard,
    input: Guard,
    state: Directory,
    inputs: Directory,
    writer_proof: Original,
    input_proof: Original,
    record_proof: Original,
    pub(super) record: Value,
    claims: Directory,
    binding: Binding,
    until: Instant,
    actor: std::thread::ThreadId,
    ended: AtomicBool,
    permit: AtomicBool,
    pub(super) may_have_sent: AtomicBool,
    claim: Mutex<Option<Claim>>,
    _complete: Completion,
}
impl Scope {
    pub(super) fn acquire(binding: Binding, until: Instant) -> Result<Acquisition> {
        helper::boundary(until)?;
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let writer_file = state.existing_private_lock(".lock")?;
        match writer_file.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if crate::platform::lock_contended(&error) => return Ok(Acquisition::Busy),
            Err(error) => return Err(error.to_string()),
        }
        let writer = Guard::held(writer_file);
        let record = metadata(&binding.record)?;
        binding.current(&record)?;
        let record_path = path(&record)?;
        let record_proof = Original::read_existing(&record_path, storage::ID_RECORD_BYTES)?
            .ok_or("Original Claude startup record disappeared")?;
        let retained = storage::verify_metadata_at(
            &record_path,
            string(&record, "name"),
            record_proof.bytes(),
        )?;
        if retained != record {
            return Err("Claude startup record changed during acquisition".into());
        }
        let inputs = state.ensure_child("native-input")?;
        let input_name = format!("{}.lock", storage::owner_key(&record));
        let input_file = inputs.open_private_lock(&input_name)?;
        match input_file.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if crate::platform::lock_contended(&error) => return Ok(Acquisition::Busy),
            Err(error) => return Err(error.to_string()),
        }
        let input = Guard::held(input_file);
        let writer_proof = Original::read_existing(&state.path.join(".lock"), 0)?
            .ok_or("Original Claude writer disappeared")?;
        let input_proof = Original::read_existing(&inputs.path.join(&input_name), 0)?
            .ok_or("Original Claude input exclusion disappeared")?;
        let claims = state.ensure_child(CLAIMS)?;
        if claims
            .read_private_optional(&claim_name(&record))?
            .is_some()
        {
            return Ok(Acquisition::Spent);
        }
        let (send, done) = mpsc::channel();
        let scope = Arc::new(Self {
            writer,
            input,
            state,
            inputs,
            writer_proof,
            input_proof,
            record_proof,
            record,
            claims,
            binding,
            until,
            actor: std::thread::current().id(),
            ended: AtomicBool::new(false),
            permit: AtomicBool::new(false),
            may_have_sent: AtomicBool::new(false),
            claim: Mutex::new(None),
            _complete: Completion(send),
        });
        helper::Source::check(scope.as_ref())?;
        Ok(Acquisition::Ready(scope, done))
    }
    pub(super) fn actor(&self) -> Result<()> {
        if self.ended.load(Ordering::Acquire) || self.actor != std::thread::current().id() {
            return Err("Claude appearance owner ended or belongs to another thread".into());
        }
        Ok(())
    }
    pub(super) fn end(&self) {
        self.permit.store(false, Ordering::Release);
        self.ended.store(true, Ordering::Release);
    }
    pub(super) fn reserve(&self, id: &str, panel: &parser::Panel) -> Result<()> {
        self.actor()?;
        helper::Source::check(self)?;
        let mut claim = self
            .claim
            .lock()
            .map_err(|_| "Claude appearance claim unavailable")?;
        if claim.is_some() {
            return Err("Claude appearance already reserved".into());
        }
        let value = &self.record;
        let value = json!({"kind":"claude-first-run-theme-v1","status":"reserved","request_id":id,
            "session_id":value["session_id"],"run_id":value["run_id"],"pid":value["pid"],
            "process_start":value["process_start"],"pane":value["pane"],
            "native_version":panel.version,"frame_hash":panel.fingerprint,"label":panel.selected_default().ok_or("Theme default changed")?.label(),
            "reserved_at":now()});
        // No replacement: malformed, interrupted and any earlier claim block
        // the entire original run. This file and its parent are synced by the
        // native directory primitive before any possible effectful spawn.
        let file = self
            .claims
            .write_new_file(&claim_name(&self.record), value.to_string().as_bytes())?;
        let version = file_version(&file)?;
        *claim = Some(Claim { file, version });
        drop(claim);
        helper::Source::check(self)
    }
    pub(super) fn permit(&self) -> Result<()> {
        self.actor()?;
        helper::Source::check(self)?;
        if self
            .claim
            .lock()
            .map_err(|_| "Claude claim unavailable")?
            .is_none()
        {
            return Err("Claude appearance has no durable original claim".into());
        }
        self.permit.store(true, Ordering::Release);
        Ok(())
    }
    pub(super) fn outcome(&self, status: &str, detail: &str) -> Result<()> {
        self.actor()?;
        helper::Source::check(self)?;
        let name = claim_name(&self.record).replace(".json", ".result.json");
        let result = json!({"status":status,"detail":detail,"may_have_sent":self.may_have_sent.load(Ordering::Acquire),
            "session_id":self.record["session_id"],"run_id":self.record["run_id"],"pid":self.record["pid"],
            "process_start":self.record["process_start"],"pane":self.record["pane"]});
        self.claims.write_new(&name, result.to_string().as_bytes())
    }
}
impl helper::Source for Scope {
    fn check(&self) -> Result<()> {
        helper::boundary(self.until)?;
        self.state.verify()?;
        if !self.state.owner_controls_entries()?
            || !self.inputs.private()?
            || !self.claims.private()?
        {
            return Err("Claude appearance directories are no longer private".into());
        }
        self.inputs.verify()?;
        self.claims.verify()?;
        self.writer_proof.verify()?;
        self.input_proof.verify()?;
        self.record_proof.verify()?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path
            || path(&self.record)? != self.record_proof.path()
            || self.writer.identity().map_err(|e| e.to_string())?
                != crate::platform::FileIdentity::of(
                    self.writer_proof.file().ok_or("Missing original writer")?,
                )
                .map_err(|e| e.to_string())?
            || self.input.identity().map_err(|e| e.to_string())?
                != crate::platform::FileIdentity::of(
                    self.input_proof.file().ok_or("Missing original input")?,
                )
                .map_err(|e| e.to_string())?
        {
            return Err("Original Claude startup resource changed".into());
        }
        self.binding.current(&self.record)?;
        if let Some(claim) = self
            .claim
            .lock()
            .map_err(|_| "Claude claim unavailable")?
            .as_ref()
        {
            let named = self.claims.read_private(&claim_name(&self.record))?;
            if crate::platform::FileIdentity::of(&claim.file).map_err(|e| e.to_string())?
                != crate::platform::FileIdentity::of(&named).map_err(|e| e.to_string())?
                || file_version(&claim.file)? != claim.version
                || file_version(&named)? != claim.version
            {
                return Err("Original Claude appearance claim changed".into());
            }
        }
        helper::boundary(self.until)
    }
    fn until(&self) -> Instant {
        self.until
    }
    fn offer(&self, call: &helper::Call) -> Result<()> {
        self.actor()?;
        match call {
            helper::Call::Capture(pane) if pane == string(&self.record, "pane") => {}
            helper::Call::Pane(pane) if pane == string(&self.record, "pane") => {}
            helper::Call::Layout(terminal) if terminal == terminal_name(&self.record) => {}
            helper::Call::Process(pid) if Some(u64::from(*pid)) == self.record["pid"].as_u64() => {}
            helper::Call::Enter(pane)
                if pane == string(&self.record, "pane")
                    && self.permit.swap(false, Ordering::AcqRel) =>
            {
                self.may_have_sent.store(true, Ordering::Release);
            }
            _ => return Err("Unreviewed Claude appearance helper/target refused".into()),
        }
        helper::Source::check(self)
    }
}

pub(super) fn completed(done: mpsc::Receiver<()>) {
    loop {
        match done.recv_timeout(std::time::Duration::from_millis(20)) {
            Ok(()) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // Loss of the completion witness is not original resource drain.
            Err(mpsc::RecvTimeoutError::Disconnected) => loop {
                std::thread::park_timeout(std::time::Duration::from_secs(1));
            },
        }
    }
}
