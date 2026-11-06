//! Original read-only pipeline custody, separate from each mutation's I/O.
//! Construction is native-only and precedes manifest confirmation. The holder
//! and its frames are DATA; none of these types supplies program-stop consent.
use super::*;
use std::{process::Child, time::Instant};
use zerus_client_core::worktree_programs::{
    custody::{self, Control, NativeChild, Owner, Reply},
    Discovery, Manifest,
};

pub(crate) struct Source {
    context: Arc<Origin>,
    owner: Mutex<Owner>,
    manifest: Manifest,
    pipeline: String,
    holder: NativeChild,
    socket: String,
    not_after: Instant,
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
        let current = Instant::now() < self.not_after && self.context.current();
        if !current {
            self.owner
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .cancel();
        }
        current
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
        self.owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .close()
            .map_err(|_| UNCONFIRMED.into())
    }
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
        self.source
            .owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .joined(&self.binding)
            .map_err(|_| UNCONFIRMED)?;
        self.retired = true;
        Ok(true)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
