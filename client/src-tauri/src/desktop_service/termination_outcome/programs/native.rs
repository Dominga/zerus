//! The actual Native startup origin, distinct from a Service Context. Fresh
//! reads here select custody DATA; neither a historical receipt nor an absent
//! service becomes program-stop permission. No fallback CLI or Service Grant.
use super::*;
use std::{
    process::{Child, Command},
    time::Instant,
};
use zerus_client_core::Target;
use zerus_client_core::{desktop::ownership, worktree_programs::custody::NativeChild};

pub(super) struct NativeContext {
    source: crate::desktop_ownership::NativeSource,
    connection: Arc<relay::Connection>,
    image: Arc<Image>,
    helper: PathBuf,
    environment: FrozenEnvironment,
    _hold: Arc<dyn Send + Sync>,
    pub(super) cancel: AtomicBool,
    rejected: AtomicBool,
    busy: Arc<AtomicBool>,
}
struct ReadLifetime {
    _context: Arc<NativeContext>,
    busy: Arc<AtomicBool>,
}
impl Drop for ReadLifetime {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::Release);
    }
}
impl NativeContext {
    pub(super) fn scope(&self) -> zerus_client_core::worktree_programs::stop::Source {
        let original = self.source.original_ownership();
        zerus_client_core::worktree_programs::stop::Source::Native {
            request_id: original.request_id.clone(),
            journal: original.journal.clone(),
            revision: original.revision,
        }
    }
    pub(super) fn new(
        ready: &crate::desktop_ownership::NativeReady,
        hold: Arc<dyn Send + Sync>,
    ) -> Result<Arc<Self>, String> {
        Self::selected(ready.program_source()?, hold)
    }
    pub(super) fn selected(
        source: crate::desktop_ownership::NativeSource,
        hold: Arc<dyn Send + Sync>,
    ) -> Result<Arc<Self>, String> {
        let connection = source.connection()?;
        let helper = connection.executable()?.to_owned();
        let image = Arc::new(Image::selected(connection.clone()).map_err(|_| UNCONFIRMED)?);
        let environment = source.environment().clone();
        let context = Arc::new(Self {
            source,
            connection,
            image,
            helper,
            environment,
            _hold: hold,
            cancel: AtomicBool::new(false),
            rejected: AtomicBool::new(false),
            busy: Arc::new(AtomicBool::new(false)),
        });
        if !context.current() {
            return Err(UNCONFIRMED.into());
        }
        // The SAME newly captured image is probed before its first new read.
        let help = context.read(vec!["--help".into()], 256 << 10)?;
        let text = std::str::from_utf8(&help).map_err(|_| UNCONFIRMED)?;
        for marker in [
            ownership::HELP_MARKER,
            zerus_client_core::worktree_programs::HELP_MARKER,
            zerus_client_core::worktree_programs::custody::HELP_MARKER,
        ] {
            if !text.lines().any(|line| line.trim() == marker) {
                return Err(UNCONFIRMED.into());
            }
        }
        context.check_scope()?;
        Ok(context)
    }
    pub(super) fn inspect(
        self: &Arc<Self>,
        target: &Target,
        machine: &str,
    ) -> Result<Option<Capture>, String> {
        if !target.computer.is_empty() {
            return Err("termination_programs_remote_custody_unavailable".into());
        }
        let runner = |_: &std::path::Path, invocation: &Invocation| match self.read(
            invocation.args.clone(),
            zerus_client_core::worktree_review::REVIEW_LIMIT,
        ) {
            Ok(stdout) => Exit::Code {
                code: 0,
                stdout,
                stderr: String::new(),
            },
            Err(detail) => Exit::StartFailed(detail),
        };
        crate::termination_cleanup::probe(&self.helper, target, &runner)?;
        let value = crate::termination_cleanup::inspection(&self.helper, target, machine, &runner)?;
        let capture = Capture::from_inspection(target, machine, &value)?;
        if !self.current() {
            return Err(UNCONFIRMED.into());
        }
        Ok(capture)
    }

    pub(super) fn configuration(&self) -> &str {
        self.source.configuration()
    }
    pub(super) fn environment(&self) -> &FrozenEnvironment {
        &self.environment
    }
    pub(super) fn executable(&self) -> &std::path::Path {
        self.image.executable()
    }
    pub(super) fn prepare_request(&self, request: Request) -> Result<Registration, String> {
        if !self.current() {
            return Err(UNCONFIRMED.into());
        }
        self.source
            .prepare_program_request(request)
            .map_err(str::to_owned)
    }
    fn same_context(&self) -> bool {
        !self.cancel.load(Ordering::Acquire)
            && !self.rejected.load(Ordering::Acquire)
            && self.source.route_current()
            && self.environment.current()
            && self
                .source
                .connection()
                .is_ok_and(|now| Arc::ptr_eq(&now, &self.connection))
            && self
                .connection
                .executable()
                .is_ok_and(|now| now == self.helper)
    }
    pub(super) fn current(&self) -> bool {
        self.same_context() && self.image.current(&self.helper)
    }
    pub(super) fn spawn(
        &self,
        command: &mut Command,
    ) -> std::io::Result<(Child, Option<NativeChild>)> {
        self.spawn_with_custody(command, None)
    }
    pub(super) fn spawn_with_custody(
        &self,
        command: &mut Command,
        custody: Option<&Arc<run::custody::InheritedLock>>,
    ) -> std::io::Result<(Child, Option<NativeChild>)> {
        if !self.current() {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        self.image.before_spawn(command)?;
        if let Some(custody) = custody {
            custody.inherit(command)?;
        }
        let mut child = command.spawn()?;
        let identity = self.image.observe_custody_child(&mut child).ok();
        if identity.is_none() || !self.same_context() {
            self.rejected.store(true, Ordering::Release);
            drop(child.stdin.take());
            return Ok((child, None)); // Preserve THIS original child and all I/O.
        }
        Ok((child, identity))
    }
    pub(super) fn read(self: &Arc<Self>, args: Vec<String>, cap: usize) -> Result<Vec<u8>, String> {
        if !self.current()
            || self
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(UNCONFIRMED.into());
        }
        let held = Arc::new(ReadLifetime {
            _context: self.clone(),
            busy: self.busy.clone(),
        });
        let context = self.clone();
        let invocation = run::rendezvous::prepare(&Invocation {
            args,
            stdin: Some(Vec::new()),
            timeout: Duration::from_secs(5),
            request_id: None,
        });
        let result = run::run_accounted_frozen(
            self.executable(),
            &invocation,
            &self.environment,
            cap,
            &self.cancel,
            held,
            move |command| context.spawn(command).map(|(child, _)| child),
        );
        let until = Instant::now() + Duration::from_secs(2);
        while self.busy.load(Ordering::Acquire) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        if self.busy.load(Ordering::Acquire) || !self.current() {
            return Err(UNCONFIRMED.into());
        }
        match result {
            Ok(Exit::Code {
                code: 0, stdout, ..
            }) if stdout.len() <= cap => Ok(stdout),
            _ => Err(UNCONFIRMED.into()),
        }
    }
    /// A fresh scope read, never conversion into NativeReady/Grant/consent.
    /// An absent journal remains absence; it cannot fabricate a durable row.
    pub(super) fn check_scope(self: &Arc<Self>) -> Result<(), String> {
        let bytes = self.read(
            vec!["desktop".into(), "ownership-status".into(), "--json".into()],
            ownership::BYTES,
        )?;
        let current: ownership::Status = serde_json::from_slice(&bytes).map_err(|_| UNCONFIRMED)?;
        if !same_scope(self.source.original_ownership(), &current) {
            return Err(UNCONFIRMED.into());
        }
        Ok(())
    }
}
fn same_scope(original: &ownership::Receipt, current: &ownership::Status) -> bool {
    if !original.native_ready() || !current.matches(&original.configuration) {
        return false;
    }
    match original.outcome {
        ownership::Outcome::NativeReady { .. } => current == &original.current,
        ownership::Outcome::NativeAbsent => {
            current.journal.is_none()
                && current.revision == 0
                && current.state == ownership::State::Passive
                && !current.rearm_required
                && current.request_id.is_none()
                && current.owner_epoch.is_none()
                && !current.identity_current
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn original(absent: bool) -> ownership::Receipt {
        let request = uuid::Uuid::new_v4().to_string();
        let journal = (!absent).then(|| uuid::Uuid::new_v4().to_string());
        ownership::Receipt {
            abi: ownership::ABI.into(),
            request_id: request.clone(),
            configuration: "a".repeat(64),
            journal: journal.clone(),
            fingerprint: "b".repeat(64),
            action: ownership::Action::NativeTakeover,
            revision: if absent { 0 } else { 3 },
            accepted: true,
            durable: !absent,
            outcome: if absent {
                ownership::Outcome::NativeAbsent
            } else {
                ownership::Outcome::NativeReady {
                    prior_owner_epoch: None,
                    revoked: true,
                    checkpointed: true,
                    drained: true,
                }
            },
            current: ownership::Status {
                abi: ownership::ABI.into(),
                configuration: "a".repeat(64),
                journal,
                revision: if absent { 0 } else { 3 },
                state: ownership::State::Native,
                rearm_required: false,
                request_id: Some(request),
                owner_epoch: None,
                identity_current: true,
            },
        }
    }
    #[test]
    fn current_native_scope_is_data_and_requires_the_same_original_revision() {
        let original = original(false);
        assert!(original.native_ready());
        assert!(same_scope(&original, &original.current));
        for key in [
            "configuration",
            "journal",
            "revision",
            "state",
            "rearm_required",
            "request_id",
            "owner_epoch",
            "identity_current",
        ] {
            let mut changed = serde_json::to_value(&original.current).unwrap();
            changed[key] = match key {
                "configuration" => json!("c".repeat(64)),
                "journal" | "request_id" | "owner_epoch" => json!(uuid::Uuid::new_v4().to_string()),
                "revision" => json!(4),
                "state" => json!("service"),
                "rearm_required" => json!(true),
                _ => json!(false),
            };
            assert!(
                !same_scope(&original, &serde_json::from_value(changed).unwrap()),
                "{key}"
            );
        }
    }
    #[test]
    fn original_absence_is_not_rewritten_into_a_current_native_grant() {
        let original = original(true);
        assert!(original.native_ready());
        let mut absent = original.current.clone();
        absent.state = ownership::State::Passive;
        absent.request_id = None;
        absent.identity_current = false;
        assert!(same_scope(&original, &absent));
        assert!(
            !same_scope(&original, &original.current),
            "a serialized Native flag is not the absent read"
        );
        absent.revision = 1;
        assert!(!same_scope(&original, &absent));
        absent.revision = 0;
        absent.configuration = "c".repeat(64);
        assert!(!same_scope(&original, &absent));
        let mut refused = original;
        refused.accepted = false;
        assert!(!same_scope(&refused, &refused.current));
    }
}
