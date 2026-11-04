//! Native confirmation only. Retention is data admission, never Terminate
//! consent. A consumed live confirmation can invoke its frozen native call once.
use super::*;
mod image;
use image::Image;
use serde::Deserialize;
use zerus_client_core::{
    desktop::{
        relay, termination_outcome as wire,
        ui_requests::{Registration, Request},
    },
    run::{self, FrozenEnvironment},
    termination_cleanup::Capture,
    Exit, Invocation,
};

pub(crate) const UNCONFIRMED: &str =
    "The termination is unconfirmed. Check the original session before continuing.";
const RPC_LIMIT: usize = 64 << 10;
// Capture::completed accepts at most this many original receipt bytes.
const NATIVE_RECEIPT_LIMIT: usize = 64 << 10;

/// Bounded correlation/bookkeeping only. No controller can be rebuilt from
/// these records. Durable client recovery is a separate follow-up.
#[derive(Default)]
pub(crate) struct Pending(Mutex<BTreeMap<String, Arc<Retained>>>);
struct Retained {
    original: wire::Intent,
    stage: std::sync::atomic::AtomicU8,
}
fn source_key(capture: &Capture) -> String {
    // Renaming a session/alias or clearing its conversation cannot bypass an
    // unconfirmed Terminate of this SAME original native run.
    wire::hash(
        &serde_json::to_vec(&(&capture.machine, &capture.target.run_id))
            .expect("fixed native source fields"),
    )
}

/// Read-only offer routing. Cloneable DATA, never a native confirmation.
#[derive(Clone)]
pub(crate) struct Selected(Arc<Context>);
impl Selected {
    pub(crate) fn inspect(
        service: Arc<Service>,
        target: &zerus_client_core::Target,
        machine: &str,
        read: Arc<dyn Send + Sync>,
    ) -> Result<(Option<Capture>, Self), String> {
        let context = Context::new(service, read)?;
        context.probe_abi()?;
        context.check_scope()?;
        let helper = context.helper.clone();
        let runner = |_: &std::path::Path, invocation: &Invocation| context.read(invocation);
        crate::termination_cleanup::probe(&helper, target, &runner)?;
        let value = crate::termination_cleanup::inspection(&helper, target, machine, &runner)?;
        let capture = Capture::from_inspection(target, machine, &value)?;
        // The offer keeps its immutable selection/pins, not an idle I/O charge.
        let selected = context.with_hold(Arc::new(()))?;
        Ok((capture, Self(selected)))
    }
}

/// Constructed only while consuming the original native Offer/Remove choice.
/// Neither this object nor its one-way stage has a serialized representation.
pub(crate) struct Confirmation {
    id: String,
    capture: Capture,
    invocation: Invocation,
    selected: Selected,
    pending: Arc<Pending>,
}
impl Confirmation {
    pub(crate) fn new(
        id: String,
        capture: Capture,
        selected: Selected,
        pending: Arc<Pending>,
    ) -> Result<Self, String> {
        let key = source_key(&capture);
        if pending
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&key)
        {
            return Err(UNCONFIRMED.into());
        }
        let request = uuid::Uuid::new_v4().to_string();
        let mut invocation = capture.invocation(&request)?;
        zerus_client_core::worktrees::guard(&mut invocation.args, &capture.machine);
        Ok(Self {
            id,
            capture,
            invocation,
            selected,
            pending,
        })
    }
    pub(crate) fn capture(&self) -> &Capture {
        &self.capture
    }
    pub(crate) fn invocation(&self) -> &Invocation {
        &self.invocation
    }
    pub(crate) fn bind(self, hold: Arc<dyn Send + Sync>) -> Result<Controller, String> {
        let context = self.selected.0.with_hold(hold)?;
        let intent = wire::Intent {
            scope: context.scope.clone(),
            confirmation_id: self.id,
            choice: wire::Choice::RemoveAfterTerminate,
            request_id: self.invocation.request_id.clone().ok_or(UNCONFIRMED)?,
            capture: self.capture,
            original: String::from_utf8(self.invocation.stdin.clone().ok_or(UNCONFIRMED)?)
                .map_err(|_| UNCONFIRMED)?,
        };
        if !intent.valid() {
            return Err(UNCONFIRMED.into());
        }
        let key = source_key(&intent.capture);
        let retained = Arc::new(Retained {
            original: intent.clone(),
            stage: std::sync::atomic::AtomicU8::new(Stage::Fresh as u8),
        });
        {
            let mut pending = self
                .pending
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if pending.len() >= wire::RECORDS || pending.contains_key(&key) {
                return Err(UNCONFIRMED.into());
            }
            pending.insert(key.clone(), retained.clone());
        }
        Ok(Controller {
            context,
            intent,
            invocation: self.invocation,
            stage: Stage::Fresh,
            key,
            retained,
            pending: self.pending,
        })
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Fresh,
    Offered,
    Retained,
    DispatchBegun,
}
pub(crate) struct Controller {
    context: Arc<Context>,
    intent: wire::Intent,
    invocation: Invocation,
    stage: Stage,
    key: String,
    retained: Arc<Retained>,
    pending: Arc<Pending>,
}
impl Drop for Controller {
    fn drop(&mut self) {
        // Only a proved never-offered intent may be discarded. Lost offered
        // results and begun dispatch remain fenced for this original GUI.
        if self.stage == Stage::Fresh {
            let mut pending = self
                .pending
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if pending
                .get(&self.key)
                .is_some_and(|v| Arc::ptr_eq(v, &self.retained))
            {
                pending.remove(&self.key);
            }
        }
    }
}
impl Controller {
    pub(crate) fn executable(&self) -> &std::path::Path {
        &self.context.helper
    }
    pub(crate) fn invocation(&self) -> &Invocation {
        &self.invocation
    }
    pub(crate) fn capture(&self) -> &Capture {
        &self.intent.capture
    }
    pub(crate) fn current(&self) -> bool {
        self.context.current()
    }
    pub(crate) fn read(&self, invocation: &Invocation) -> Exit {
        if self.stage == Stage::DispatchBegun {
            return Exit::StartFailed(UNCONFIRMED.into());
        }
        self.context.read(invocation)
    }
    pub(crate) fn retain(&mut self) -> Result<(), String> {
        if self.stage != Stage::Fresh {
            return Err(UNCONFIRMED.into());
        }
        self.context.probe_abi()?;
        self.context.check_scope()?;
        let registration = self
            .context
            .service
            .shared
            .responder
            .as_ref()
            .ok_or(UNCONFIRMED)?
            .prepare_ui_request(
                uuid::Uuid::new_v4().to_string(),
                Request::TerminationOutcome {
                    intent: Box::new(self.intent.clone()),
                },
            )
            .map_err(|_| UNCONFIRMED)?;
        let input = serde_json::to_vec(registration.envelope()).map_err(|_| UNCONFIRMED)?;
        self.stage = Stage::Offered; // Never resubmit, including after a lost reply.
        self.retained
            .stage
            .store(Stage::Offered as u8, Ordering::Release);
        let result = self.context.rpc(
            vec!["desktop".into(), "frontend-request".into(), "--json".into()],
            Some(input),
            Some(registration),
        );
        let accepted = result
            .as_ref()
            .ok()
            .is_some_and(|bytes| acknowledgement(bytes, &self.intent));
        if !accepted {
            // rpc refuses while ANY old child/pipe custody remains. This is a
            // read of the SAME original ID, not another native request.
            self.context.current_or_refuse()?;
            let query = uuid::Uuid::new_v4().to_string();
            let bytes = self.context.rpc(vec!["desktop".into(), "request".into(), "--json".into()],
                Some(serde_json::to_vec(&json!({"op":"termination_outcome_status","request_id":query,
                    "original_request_id":self.intent.request_id,"configuration":self.intent.scope.configuration})).map_err(|_| UNCONFIRMED)?), None)?;
            if !lookup(&bytes, &query, &self.intent) {
                return Err(UNCONFIRMED.into());
            }
        }
        self.context.current_or_refuse()?;
        self.stage = Stage::Retained;
        self.retained
            .stage
            .store(Stage::Retained as u8, Ordering::Release);
        Ok(())
    }
    /// Last slow configuration check. The dispatcher follows this with its
    /// final Capture inspection and then current routing/cancellation checks.
    pub(crate) fn recheck_scope(&self) -> Result<(), String> {
        if self.stage != Stage::Retained {
            return Err(UNCONFIRMED.into());
        }
        self.context.check_scope()
    }
    /// One-way transition BEFORE the existing native runner is called. An
    /// error, failed spawn, timeout, dropped future or exit never restores it.
    pub(crate) fn begin_dispatch(&mut self, invocation: &Invocation) -> Result<(), String> {
        if self.stage != Stage::Retained
            || !same_invocation(invocation, &self.invocation)
            || self.retained.original != self.intent
        {
            return Err(UNCONFIRMED.into());
        }
        self.context.current_or_refuse()?;
        self.stage = Stage::DispatchBegun;
        self.retained
            .stage
            .store(Stage::DispatchBegun as u8, Ordering::Release);
        Ok(())
    }
    pub(crate) fn run_native(self, invocation: &Invocation) -> Exit {
        if self.stage != Stage::DispatchBegun || !same_invocation(invocation, &self.invocation) {
            return Exit::StartFailed(UNCONFIRMED.into());
        }
        // This is the original accounted runner and ORIGINAL invocation
        // timeout (45 local / 60 peer), not run_owned's 30-second RPC cap.
        let current = self.context.clone();
        run::run_accounted_frozen(
            self.context.image.executable(),
            invocation,
            &self.context.environment,
            NATIVE_RECEIPT_LIMIT,
            &AtomicBool::new(false),
            Arc::new((self.context.hold.clone(), self.context.clone())),
            move |command| current.spawn(command),
        )
        .ok()
        .filter(|_| !self.context.rejected.load(Ordering::Acquire))
        .unwrap_or_else(|| Exit::StartFailed(UNCONFIRMED.into()))
    }
}
fn same_invocation(a: &Invocation, b: &Invocation) -> bool {
    a.args == b.args && a.stdin == b.stdin && a.timeout == b.timeout && a.request_id == b.request_id
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    op: String,
    request_id: String,
    accepted: bool,
    durable: bool,
    phase: String,
    status: wire::Status,
    cleanup_authorized: bool,
}
fn acknowledgement(bytes: &[u8], intent: &wire::Intent) -> bool {
    bytes.len() <= RPC_LIMIT
        && serde_json::from_slice::<Ack>(bytes).is_ok_and(|v| {
            v.op == "termination_outcome"
                && v.request_id == intent.request_id
                && v.accepted
                && v.durable
                && v.phase == "retained"
                && !v.cleanup_authorized
                && v.status.matches(intent)
                && v.status.original_service_epoch == intent.scope.service_epoch
        })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lookup {
    op: String,
    request_id: String,
    original_request_id: String,
    accepted: bool,
    durable: bool,
    found: bool,
    status: Option<wire::Status>,
    cleanup_authorized: bool,
}
fn lookup(bytes: &[u8], query: &str, intent: &wire::Intent) -> bool {
    bytes.len() <= RPC_LIMIT
        && serde_json::from_slice::<Lookup>(bytes).is_ok_and(|v| {
            v.op == "termination_outcome_status"
                && v.request_id == query
                && v.original_request_id == intent.request_id
                && v.accepted
                && !v.durable
                && v.found
                && !v.cleanup_authorized
                && v.status.is_some_and(|s| {
                    s.matches(intent) && s.original_service_epoch == intent.scope.service_epoch
                })
        })
}
struct Context {
    service: Arc<Service>,
    scope: wire::Scope,
    generation: u64,
    routing: Arc<relay::Connection>,
    image: Arc<Image>,
    helper: PathBuf,
    cancel: Arc<AtomicBool>,
    environment: FrozenEnvironment,
    hold: Arc<dyn Send + Sync>,
    busy: Arc<AtomicBool>,
    rejected: Arc<AtomicBool>,
}
struct CallLifetime {
    hold: Option<Arc<dyn Send + Sync>>,
    rpc: Option<zerus_client_core::desktop::drain::Charge>,
    busy: Arc<AtomicBool>,
}
impl Drop for CallLifetime {
    fn drop(&mut self) {
        drop(self.hold.take());
        drop(self.rpc.take());
        self.busy.store(false, Ordering::Release);
    }
}
impl Context {
    fn new(service: Arc<Service>, hold: Arc<dyn Send + Sync>) -> Result<Arc<Self>, String> {
        image::prepare(|| Self::new_supported(service, hold))
    }
    fn new_supported(
        service: Arc<Service>,
        hold: Arc<dyn Send + Sync>,
    ) -> Result<Arc<Self>, String> {
        let snapshot = service.snapshot();
        let shared = &service.shared;
        if !shared.allowed || !snapshot.connected {
            return Err(UNCONFIRMED.into());
        }
        let configuration = shared
            .configuration
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(UNCONFIRMED)?;
        let cancel = shared
            .cancellation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(UNCONFIRMED)?;
        let routing = shared
            .relay
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(UNCONFIRMED)?;
        let helper = routing.executable()?.to_owned();
        let image = Arc::new(Image::capture(&helper).map_err(|_| UNCONFIRMED)?);
        let context = Arc::new(Self {
            service,
            scope: wire::Scope {
                configuration,
                service_epoch: snapshot.epoch.ok_or(UNCONFIRMED)?,
            },
            generation: snapshot.generation.ok_or(UNCONFIRMED)?,
            routing,
            image,
            helper,
            cancel,
            environment: FrozenEnvironment::capture().map_err(|_| UNCONFIRMED)?,
            hold,
            busy: Arc::new(AtomicBool::new(false)),
            rejected: Arc::new(AtomicBool::new(false)),
        });
        context.current_or_refuse()?;
        Ok(context)
    }
    fn with_hold(self: &Arc<Self>, hold: Arc<dyn Send + Sync>) -> Result<Arc<Self>, String> {
        self.current_or_refuse()?;
        Ok(Arc::new(Self {
            service: self.service.clone(),
            scope: self.scope.clone(),
            generation: self.generation,
            routing: self.routing.clone(),
            image: self.image.clone(),
            helper: self.helper.clone(),
            cancel: self.cancel.clone(),
            environment: self.environment.clone(),
            hold,
            busy: Arc::new(AtomicBool::new(false)),
            rejected: self.rejected.clone(),
        }))
    }
    fn current(&self) -> bool {
        let shared = &self.service.shared;
        let snapshot = self.service.snapshot();
        !shared.stopped.load(Ordering::Acquire)
            && !self.rejected.load(Ordering::Acquire)
            && !self.cancel.load(Ordering::Acquire)
            && snapshot.connected
            && snapshot.epoch.as_deref() == Some(&self.scope.service_epoch)
            && snapshot.generation == Some(self.generation)
            && self.environment.current()
            && shared
                .configuration
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_deref()
                == Some(&self.scope.configuration)
            && shared
                .cancellation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|v| Arc::ptr_eq(v, &self.cancel))
            && shared
                .relay
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|v| Arc::ptr_eq(v, &self.routing))
            && self
                .routing
                .executable()
                .is_ok_and(|p| p == self.helper && self.image.current(p))
    }
    fn current_or_refuse(&self) -> Result<(), String> {
        if !self.current() || self.busy.load(Ordering::Acquire) {
            Err(UNCONFIRMED.into())
        } else {
            Ok(())
        }
    }
    fn spawn(&self, command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
        if !self.current() {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        self.image.before_spawn(command)?;
        let mut child = command.spawn()?;
        if !self.image.matches_child(&child) {
            self.rejected.store(true, Ordering::Release);
            drop(child.stdin.take());
        }
        // Never lose custody by returning Err after spawning. Both runners
        // retain/reap THIS child; rejected cannot become a successful receipt.
        Ok(child)
    }
    fn start_call(&self) -> Result<Arc<dyn Send + Sync>, String> {
        self.current_or_refuse()?;
        let rpc = self
            .service
            .shared
            .rpc
            .admit(Role::Fleet)
            .map_err(|_| UNCONFIRMED)?;
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| UNCONFIRMED)?;
        Ok(Arc::new(CallLifetime {
            hold: Some(self.hold.clone()),
            rpc: Some(rpc),
            busy: self.busy.clone(),
        }))
    }
    fn rpc(
        self: &Arc<Self>,
        args: Vec<String>,
        stdin: Option<Vec<u8>>,
        registration: Option<Registration>,
    ) -> Result<Vec<u8>, String> {
        if stdin
            .as_ref()
            .is_some_and(|s| s.len() > zerus_watch_protocol::CONTROL_LINE_BYTES)
        {
            return Err(UNCONFIRMED.into());
        }
        let resources = self.start_call()?;
        let resources: Arc<dyn Send + Sync> = Arc::new((
            self.clone(),
            registration.as_ref().map(Registration::resources),
            resources,
        ));
        let original = self.clone();
        let environment = self.environment.clone();
        let invocation = Invocation {
            args,
            stdin: Some(stdin.unwrap_or_default()), // Explicit EOF, including help/status.
            timeout: RPC_WAIT,
            request_id: None,
        };
        let result = run::run_owned(
            self.image.executable(),
            &invocation,
            &[],
            &self.cancel,
            move |command| {
                environment.apply(command);
                let mut child = original.spawn(command)?;
                if registration
                    .as_ref()
                    .is_some_and(|registration| !registration.bind_original(&child))
                {
                    // Withhold the envelope but keep THIS child in the owned
                    // runner's stop/reap/pipe custody, including on refusal.
                    drop(child.stdin.take());
                    #[cfg(all(test, target_os = "linux"))]
                    tests::observe_refused_binding(&mut child);
                }
                Ok(child)
            },
            resources,
        );
        self.finish_call()?;
        let result = result.map_err(|_| UNCONFIRMED)?;
        self.current_or_refuse()?;
        if result.len() > RPC_LIMIT {
            return Err(UNCONFIRMED.into());
        }
        Ok(result)
    }
    fn read(self: &Arc<Self>, invocation: &Invocation) -> Exit {
        let Ok(resources) = self.start_call() else {
            return Exit::StartFailed(UNCONFIRMED.into());
        };
        let original = self.clone();
        let exit = run::run_accounted_frozen(
            self.image.executable(),
            invocation,
            &self.environment,
            zerus_client_core::worktree_review::REVIEW_LIMIT,
            &self.cancel,
            Arc::new((self.clone(), resources)),
            move |command| original.spawn(command),
        );
        if self.finish_call().is_err() || self.current_or_refuse().is_err() {
            return Exit::StartFailed(UNCONFIRMED.into());
        }
        exit.unwrap_or_else(|_| Exit::StartFailed(UNCONFIRMED.into()))
    }
    fn finish_call(&self) -> Result<(), String> {
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while self.busy.load(Ordering::Acquire) {
            if std::time::Instant::now() >= until {
                return Err(UNCONFIRMED.into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
    fn probe_abi(self: &Arc<Self>) -> Result<(), String> {
        let help = self.rpc(vec!["--help".into()], None, None)?;
        let text = std::str::from_utf8(&help).map_err(|_| UNCONFIRMED)?;
        for marker in [
            HELP,
            wire::HELP_MARKER,
            zerus_client_core::desktop::ui_requests::HELP_MARKER,
        ] {
            if !text.lines().any(|line| line.trim() == marker) {
                return Err(UNCONFIRMED.into());
            }
        }
        Ok(())
    }
    fn check_scope(self: &Arc<Self>) -> Result<(), String> {
        let bytes = self.rpc(
            vec!["desktop".into(), "status".into(), "--json".into()],
            None,
            None,
        )?;
        let status: Value = serde_json::from_slice(&bytes).map_err(|_| UNCONFIRMED)?;
        if status["abi"] != zerus_client_core::desktop::service::ABI
            || status["running"] != true
            || status["configuration"] != self.scope.configuration
            || status["epoch"] != self.scope.service_epoch
        {
            return Err(UNCONFIRMED.into());
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
