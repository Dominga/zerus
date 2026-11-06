//! Original read-only pipeline custody, separate from each mutation's I/O.
//! Construction is native-only and precedes manifest confirmation. The holder
//! and its frames are DATA; none of these types supplies program-stop consent.
use super::*;
use std::{process::Child, time::Instant};
use zerus_client_core::worktree_programs::{
    custody::{self, Control, NativeChild, Owner, Reply},
    Discovery, Manifest,
};
use zerus_client_core::Target;

pub(crate) struct Source {
    context: Arc<Origin>,
    owner: Mutex<Owner>,
    manifest: Manifest,
    pipeline: String,
    holder: NativeChild,
    socket: String,
    not_after: Instant,
    normal_consumed: AtomicBool,
    closed: AtomicBool,
}

enum Origin {
    Service(Arc<Context>),
    Native(Arc<super::native::NativeContext>),
}
impl Origin {
    fn scope(&self) -> zerus_client_core::worktree_programs::stop::Source {
        match self {
            Self::Service(c) => zerus_client_core::worktree_programs::stop::Source::Service {
                epoch: c.scope.service_epoch.clone(),
            },
            Self::Native(c) => c.scope(),
        }
    }
    fn current(&self) -> bool {
        match self {
            Self::Service(c) => c.current(),
            Self::Native(c) => c.current(),
        }
    }
    fn configuration(&self) -> &str {
        match self {
            Self::Service(c) => &c.scope.configuration,
            Self::Native(c) => c.configuration(),
        }
    }
    fn environment(&self) -> &FrozenEnvironment {
        match self {
            Self::Service(c) => &c.environment,
            Self::Native(c) => c.environment(),
        }
    }
    fn executable(&self) -> &std::path::Path {
        match self {
            Self::Service(c) => c.image.executable(),
            Self::Native(c) => c.executable(),
        }
    }
    fn prepare_stop(
        &self,
        intent: zerus_client_core::worktree_programs::invocation::Intent,
    ) -> Result<Registration, String> {
        if !self.current()
            || intent.input.source != self.scope()
            || intent.input.manifest.configuration != self.configuration()
        {
            return Err(UNCONFIRMED.into());
        }
        let request = Request::WorktreeProgramStop {
            intent: Box::new(intent),
        };
        match self {
            Self::Service(c) => c
                .service
                .shared
                .responder
                .as_ref()
                .ok_or(UNCONFIRMED)?
                .prepare_ui_request(uuid::Uuid::new_v4().to_string(), request)
                .map_err(|_| UNCONFIRMED.into()),
            Self::Native(c) => c.prepare_stop(request),
        }
    }
    fn check_stop_capability(&self) -> Result<(), String> {
        let args = vec!["--help".into()];
        let bytes = match self {
            Self::Service(c) => match c.read(&Invocation {
                args,
                stdin: Some(Vec::new()),
                timeout: Duration::from_secs(5),
                request_id: None,
            }) {
                Exit::Code {
                    code: 0, stdout, ..
                } if stdout.len() <= 256 << 10 => stdout,
                _ => return Err(UNCONFIRMED.into()),
            },
            Self::Native(c) => c.read(args, 256 << 10)?,
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| UNCONFIRMED)?;
        if !text
            .lines()
            .any(|line| line.trim() == zerus_client_core::worktree_programs::stop::HELP_MARKER)
            || !self.current()
        {
            return Err("termination_program_stop_unavailable".into());
        }
        Ok(())
    }
    fn spawn(
        &self,
        command: &mut std::process::Command,
    ) -> std::io::Result<(Child, Option<NativeChild>)> {
        match self {
            Self::Service(c) => c.spawn_program_child(command),
            Self::Native(c) => c.spawn(command),
        }
    }
}

impl Selected {
    /// The native host must supply its actual Actions Running hold before
    /// discovery and keep this opaque Source through the confirmation dialog.
    /// A remote SSH wrapper's PID/token is never a remote native helper proof.
    pub(crate) fn program_source(
        &self,
        capture: &Capture,
        hold: Arc<dyn Send + Sync>,
    ) -> Result<Arc<Source>, String> {
        Source::open(Arc::new(Origin::Service(self.0.with_hold(hold)?)), capture)
    }
}
impl Source {
    pub(crate) fn inspect_native(
        source: crate::desktop_ownership::NativeSource,
        target: &Target,
        machine: &str,
        hold: Arc<dyn Send + Sync>,
    ) -> Result<Option<(Capture, Arc<Self>)>, String> {
        let context = super::native::NativeContext::selected(source, hold)?;
        let Some(capture) = context.inspect(target, machine)? else {
            return Ok(None);
        };
        let original = Self::open(Arc::new(Origin::Native(context)), &capture)?;
        Ok(Some((capture, original)))
    }
    /// Only the actual guarded Native startup/takeover result constructs this
    /// origin. Mode/status/receipt JSON cannot reconstruct it after restart.
    pub(crate) fn native(
        ready: &crate::desktop_ownership::NativeReady,
        capture: &Capture,
        hold: Arc<dyn Send + Sync>,
    ) -> Result<Arc<Self>, String> {
        // Refuse a remote route before even probing the local helper. A local
        // SSH process is never adopted as a remote native custody holder.
        if !capture.target.computer.is_empty() {
            return Err("termination_programs_remote_custody_unavailable".into());
        }
        let native = super::native::NativeContext::new(ready, hold)?;
        Self::open(Arc::new(Origin::Native(native)), capture)
    }
    fn open(context: Arc<Origin>, capture: &Capture) -> Result<Arc<Self>, String> {
        if !capture.target.computer.is_empty() {
            return Err("termination_programs_remote_custody_unavailable".into());
        }
        context.check_stop_capability()?;
        let pipeline = uuid::Uuid::new_v4().to_string();
        let original_holder = Arc::new(Mutex::new(None));
        let observed = original_holder.clone();
        let selected = context.clone();
        let mut owner = Owner::begin(
            context.executable(),
            Control::Open {
                pipeline: pipeline.clone(),
                discovery: Discovery {
                    capture: capture.clone(),
                },
            },
            context.configuration().into(),
            context.environment().clone(),
            context.clone(),
            Arc::new(move |command| {
                let (child, identity) = selected.spawn(command)?;
                // Owner builds only the fixed help/holder commands. Help must
                // also pass observation, but its exited identity is not reused.
                if command.get_args().any(|arg| arg == "program-holder") {
                    *observed.lock().unwrap_or_else(PoisonError::into_inner) = identity;
                }
                Ok(child)
            }),
        )
        .map_err(|_| UNCONFIRMED)?;
        let not_after = owner.deadline();
        let Reply::Ready {
            manifest,
            holder,
            socket,
            ..
        } = owner.ready().map_err(|_| UNCONFIRMED)?
        else {
            return Err(UNCONFIRMED.into());
        };
        // Self-reported PID/birth/audit is insufficient. Match the data to the
        // SAME Child observed before its original input was released.
        if original_holder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            != Some(&holder)
            || !context.current()
        {
            owner.cancel();
            return Err(UNCONFIRMED.into());
        }
        Ok(Arc::new(Source {
            context,
            owner: Mutex::new(owner),
            manifest: *manifest,
            pipeline,
            holder,
            socket,
            not_after,
            normal_consumed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }))
    }
}

impl Context {
    /// The callback ALWAYS returns the SAME successfully spawned Child. A
    /// refused image, identity or final Context closes input, then the existing
    /// owned runner retains its actual child/writer/readers through settlement.
    fn spawn_program_child(
        &self,
        command: &mut std::process::Command,
    ) -> std::io::Result<(Child, Option<NativeChild>)> {
        if !self.current() {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        self.image.before_spawn(command)?;
        let mut child = command.spawn()?;
        let identity = self.image.observe_custody_child(&mut child).ok();
        if identity.is_none() || !self.same_context() {
            self.reject_child(&mut child);
            Ok((child, None))
        } else {
            Ok((child, identity))
        }
    }
}

impl Source {
    pub(super) fn current(&self) -> bool {
        let current = !self.closed.load(Ordering::Acquire)
            && Instant::now() < self.not_after
            && self.context.current();
        if !current {
            self.closed.store(true, Ordering::Release);
            self.owner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .cancel();
        }
        current
    }
    pub(super) fn service_selected(&self) -> Option<Selected> {
        match self.context.as_ref() {
            Origin::Service(c) => Some(Selected(c.clone())),
            Origin::Native(_) => None,
        }
    }
    pub(super) fn executable(&self) -> &std::path::Path {
        self.context.executable()
    }
    pub(super) fn check_native_scope(&self) -> Result<(), String> {
        match self.context.as_ref() {
            Origin::Native(c) => c.check_scope(),
            Origin::Service(_) => Err(UNCONFIRMED.into()),
        }
    }
    pub(super) fn read(&self, invocation: &Invocation) -> Exit {
        if !self.current() {
            return Exit::StartFailed(UNCONFIRMED.into());
        }
        match self.context.as_ref() {
            Origin::Service(c) => c.read(invocation),
            Origin::Native(c) => match c.read(
                invocation.args.clone(),
                zerus_client_core::worktree_review::REVIEW_LIMIT,
            ) {
                Ok(stdout) => Exit::Code {
                    code: 0,
                    stdout,
                    stderr: String::new(),
                },
                Err(detail) => Exit::StartFailed(detail),
            },
        }
    }
    pub(super) fn run_native(
        self: &Arc<Self>,
        invocation: &Invocation,
        resources: Arc<dyn Send + Sync>,
    ) -> Exit {
        let Origin::Native(context) = self.context.as_ref() else {
            return Exit::StartFailed(UNCONFIRMED.into());
        };
        if !self.current() {
            return Exit::StartFailed(UNCONFIRMED.into());
        }
        let selected = context.clone();
        let command = helper_invocation(invocation);
        run::run_accounted_frozen(
            context.executable(),
            &command,
            context.environment(),
            NATIVE_RECEIPT_LIMIT,
            &context.cancel,
            resources,
            move |command| selected.spawn(command).map(|(child, _)| child),
        )
        .unwrap_or_else(|_| Exit::StartFailed(UNCONFIRMED.into()))
    }
    pub(super) fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub(super) fn not_after(&self) -> Instant {
        self.not_after
    }
    /// Read-only list for a subsequent native dialog; the original Source and
    /// checkout pins stay owned here. This never reconstructs a stop permit.
    pub(super) fn observe(&self, until: Instant) -> Result<Manifest, String> {
        if !self.current() {
            return Err(UNCONFIRMED.into());
        }
        let manifest = self
            .owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observe(until.min(self.not_after))
            .map_err(|_| UNCONFIRMED)?;
        if !self.current() {
            return Err(UNCONFIRMED.into());
        }
        Ok(manifest)
    }
    /// Close is available only after the prior mutator's separate actual-join
    /// witness. Owner also refuses to close an unretired phase independently.
    pub(super) fn close(&self) -> Result<(), String> {
        self.closed.store(true, Ordering::Release);
        self.owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .close()
            .map_err(|_| UNCONFIRMED.into())
    }
}

impl Source {
    /// Only the original consumed Remove confirmation can produce OriginalDone.
    /// Bind that SAME held Source/Actions before minting the normal-stop IDs;
    /// a report, a new selected CLI, or restored JSON cannot enter this method.
    pub(super) fn normal(self: &Arc<Self>, done: &OriginalDone) -> Result<StopAttempt, String> {
        let Origin::Service(context) = self.context.as_ref() else {
            return Err(UNCONFIRMED.into());
        };
        if !self.current()
            || !done.current()
            || !done.original_call_joined()
            || self.manifest.capture != *done.capture()
            || self.not_after != done.not_after
            || !Arc::ptr_eq(&context.image, &done.context.image)
            || !Arc::ptr_eq(&context.routing, &done.context.routing)
            || !Arc::ptr_eq(&context.hold, &done.context.hold)
        {
            return Err(UNCONFIRMED.into());
        }
        self.consume_normal()?;
        let input = zerus_client_core::worktree_programs::stop::Input {
            request_id: uuid::Uuid::new_v4().to_string(),
            confirmation_id: done.intent.confirmation_id.clone(),
            source: self.context.scope(),
            manifest: self.manifest.clone(),
            original: done.intent.original.clone(),
            archive_id: done.archive().into(),
            stop: zerus_client_core::worktree_programs::stop::Kind::Normal,
        };
        if !input.valid() {
            return Err(UNCONFIRMED.into());
        }
        self.run_stop(input)
    }
    /// The caller is the native one-shot normal/Force controller, never IPC.
    /// All resources, including the original cache slot and every output pipe,
    /// stay charged independently of this bounded return or parsed result.
    pub(super) fn consume_normal(&self) -> Result<(), String> {
        if !self.current() {
            return Err(UNCONFIRMED.into());
        }
        self.normal_consumed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| UNCONFIRMED.into())
    }
    pub(super) fn scope(&self) -> zerus_client_core::worktree_programs::stop::Source {
        self.context.scope()
    }
    pub(super) fn run_stop(
        self: &Arc<Self>,
        input: zerus_client_core::worktree_programs::stop::Input,
    ) -> Result<StopAttempt, String> {
        use zerus_client_core::worktree_programs::{invocation::Intent, stop::Report};
        if !self.current()
            || input.source != self.context.scope()
            || input.manifest.capture != self.manifest.capture
            || input.manifest.configuration != self.manifest.configuration
            || input.manifest.checkout != self.manifest.checkout
            || input.manifest.boot != self.manifest.boot
            || input.manifest.issued != self.manifest.issued
            || input.manifest.expires != self.manifest.expires
            || !self.manifest.contains_originals(&input.manifest.programs)
        {
            return Err(UNCONFIRMED.into());
        }
        let until =
            (Instant::now() + Duration::from_millis(custody::PHASE_MILLIS)).min(self.not_after);
        let intent = Intent {
            input: Box::new(input.clone()),
            pipeline: self.pipeline.clone(),
            holder: self.holder.clone(),
            socket: self.socket.clone(),
        };
        let registration = self.context.prepare_stop(intent)?;
        let bytes = serde_json::to_vec(registration.envelope()).map_err(|_| UNCONFIRMED)?;
        if bytes.len() > custody::FRAME_BYTES {
            return Err(UNCONFIRMED.into());
        }
        let (mutation, charge) = Mutation::new(self.clone(), &input);
        mutation.armed.store(false, Ordering::Release);
        let armed = mutation.armed.clone();
        let resource: Arc<dyn Send + Sync> = Arc::new((charge, registration.resources()));
        let rejected = Arc::new(AtomicBool::new(false));
        let refusal = rejected.clone();
        let selected = self.clone();
        let frozen = input.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let observe_stop = stop.clone();
        let observe_finished = finished.clone();
        let observe_source = self.clone();
        let observe_charge = resource.clone();
        let watcher = std::thread::Builder::new()
            .name("program-stop-source".into())
            .spawn(move || {
                let _charge = observe_charge;
                while !observe_finished.load(Ordering::Acquire) {
                    if Instant::now() >= until || !observe_source.current() {
                        observe_stop.store(true, Ordering::Release);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .map_err(|_| UNCONFIRMED)?;
        let invocation = Invocation {
            args: vec!["worktrees".into(), "program-stop".into(), "--json".into()],
            stdin: Some(bytes),
            timeout: until.saturating_duration_since(Instant::now()),
            request_id: Some(input.request_id.clone()),
        };
        #[cfg(target_os = "macos")]
        let invocation = run::rendezvous::prepare(&invocation);
        // Linux must keep the exact closed argv; its handler already waits for
        // input. Machine identity is checked by that own-helper before effects.
        let exit = run::run_accounted_frozen(
            self.context.executable(),
            &invocation,
            self.context.environment(),
            zerus_client_core::worktree_programs::MAX_BYTES,
            &stop,
            resource,
            move |command| {
                let (mut child, native) = selected.context.spawn(command)?;
                let mut refused = native.is_none() || !registration.bind_original(&child);
                if !refused {
                    armed.store(true, Ordering::Release); // missing ACK cannot reopen a phase
                    refused = selected
                        .owner
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .arm(&frozen, native.expect("original observed child"), until)
                        .is_err();
                }
                if refused || !selected.current() {
                    refusal.store(true, Ordering::Release);
                    drop(child.stdin.take());
                }
                Ok(child) // SAME child, even on image/registration/arm refusal
            },
        );
        finished.store(true, Ordering::Release);
        let observed = watcher.join().is_ok();
        let result = if observed && !rejected.load(Ordering::Acquire) {
            match exit {
                Ok(Exit::Code {
                    code: 0, stdout, ..
                }) => Report::decode(&stdout, &input).map_err(|_| UNCONFIRMED.into()),
                _ => Err(UNCONFIRMED.into()),
            }
        } else {
            Err(UNCONFIRMED.into())
        };
        Ok(StopAttempt {
            input,
            result,
            mutation,
        })
    }
}

pub(super) struct StopAttempt {
    pub(super) input: zerus_client_core::worktree_programs::stop::Input,
    pub(super) result: Result<zerus_client_core::worktree_programs::stop::Report, String>,
    pub(super) mutation: Mutation,
}

// Each mutating call will own a fresh instance. The read-only Source/holder is
// retained here but is NOT counted as that call's mutating reader/loan charge.
struct MutatorResources {
    _source: Arc<Source>,
}
pub(super) struct Mutation {
    source: Arc<Source>,
    binding: custody::Binding,
    resources: Weak<MutatorResources>,
    retired: bool,
    armed: Arc<AtomicBool>,
}
impl Mutation {
    /// A private native call obtains this while arming its ORIGINAL helper;
    /// this bookkeeping is not the one-shot native stop/Force permit.
    fn new(
        source: Arc<Source>,
        input: &zerus_client_core::worktree_programs::stop::Input,
    ) -> (Self, Arc<MutatorResources>) {
        let charge = Arc::new(MutatorResources {
            _source: source.clone(),
        });
        let mutation = Self {
            binding: custody::Binding::for_input(
                // This is the already-armed phase's exact pipeline binding.
                // The caller checks it against the holder when arming.
                &source.pipeline,
                input,
            ),
            source,
            resources: Arc::downgrade(&charge),
            retired: false,
            armed: Arc::new(AtomicBool::new(true)),
        };
        (mutation, charge)
    }
    pub(super) fn try_retire(&mut self) -> Result<bool, String> {
        if self.retired {
            return Ok(true);
        }
        // A parsed result, process exit, socket close or release ACK cannot
        // bypass these actual owned pipe/worker/loan resource references.
        if self.resources.upgrade().is_some() {
            return Ok(false);
        }
        if self.armed.load(Ordering::Acquire) {
            self.source
                .owner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .joined(&self.binding)
                .map_err(|_| UNCONFIRMED)?;
        }
        self.retired = true;
        Ok(true)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
