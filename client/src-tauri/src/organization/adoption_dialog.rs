use super::*;
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use zerus_client_core::{
    reads,
    swarm::{
        adoption,
        sync::{ExternalTicket, EXTERNAL_CHANGED},
    },
    ComputerId, Invocation,
};

const BUSY: &str = "An adoption request is already running.";
const EXPIRED: &str = "This adoption dialog expired. Open it again.";
const SETTLING: &str = "The original adoption call is still settling. Its result stays unknown until its connection closes; no new adoption will start.";
/// Past the CLI's own 25 s original-call deadline, a still running mutation
/// is only settling its original resources.
#[cfg(not(test))]
const SETTLING_AFTER: std::time::Duration = std::time::Duration::from_secs(30);
#[cfg(test)]
const SETTLING_AFTER: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Serialize)]
pub struct View {
    id: String,
    aliases: Vec<String>,
    preview: Option<PreviewView>,
    status: String,
    error: bool,
    busy: bool,
    changing: bool,
    can_preview: bool,
    can_adopt: bool,
    can_resume: bool,
    mode: &'static str,
    drive_status: &'static str,
}

#[derive(Serialize)]
struct PreviewView {
    request_id: String,
    alias: String,
    eligible: bool,
    reason: String,
}

struct Resources(Weak<Shared>);
impl Drop for Resources {
    fn drop(&mut self) {
        if let Some(shared) = self.0.upgrade() {
            shared.wake.notify_all();
        }
    }
}

/// Independently observed native card data, never a preview-derived witness.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeCardBinding {
    computer: ComputerId,
    host: String,
    machine_id: String,
}

impl NativeCardBinding {
    fn capture(pump: &Pump, computer: ComputerId, expected_node: &str) -> Result<Self, String> {
        let card = pump.live_machine(&computer).ok_or(EXTERNAL_CHANGED)?;
        let machine_id = card.machine_id.ok_or(EXTERNAL_CHANGED)?;
        let host = card.host.ok_or(EXTERNAL_CHANGED)?;
        if card.ok != Some(true)
            || host.is_empty()
            || !zerus_client_core::creation::canonical_uuid(&machine_id)
            || machine_id != expected_node
        {
            return Err(EXTERNAL_CHANGED.into());
        }
        Ok(Self {
            computer,
            host,
            machine_id,
        })
    }

    fn current(&self, pump: &Pump) -> Result<(), String> {
        let actual = Self::capture(pump, self.computer.clone(), &self.machine_id)?;
        if &actual != self {
            return Err(EXTERNAL_CHANGED.into());
        }
        Ok(())
    }
}

struct NativeContext {
    generation: u64,
    digest: String,
    configuration: String,
    source_card: (String, String),
    destination_card: (String, String),
    source_machine: NativeCardBinding,
    destination_machine: NativeCardBinding,
    ticket: usize,
    request_id: String,
    drained: bool,
    resources: Weak<Resources>,
    /// Sticky: once any currentness check refused this original, a later
    /// restored card, generation or configuration never revives it. Only
    /// read-only Status and Fresh remain; the ticket stays held.
    revoked: std::cell::Cell<bool>,
}

impl NativeContext {
    fn current(&self, hub: &Hub, context: &Context, pump: &Pump) -> Result<(), String> {
        if self.revoked.get() {
            return Err(EXTERNAL_CHANGED.into());
        }
        let checked = self.check(hub, context, pump);
        if checked.is_err() {
            self.revoked.set(true);
        }
        checked
    }

    fn check(&self, hub: &Hub, context: &Context, pump: &Pump) -> Result<(), String> {
        let preview = context.preview.as_ref().ok_or(EXTERNAL_CHANGED)?;
        let ticket = context.ticket.as_deref().ok_or(EXTERNAL_CHANGED)?;
        let adoption = context.adoption.as_ref().ok_or(EXTERNAL_CHANGED)?;
        let local = hub.sync.observed_value().ok_or(EXTERNAL_CHANGED)?;
        if hub.stopped
            || !preview.eligible()
            || hub.source_generation != self.generation
            || self.configuration != context.configuration
            || self.source_card != preview.source_card()
            || self.destination_card != preview.destination_card()
            || self.digest != local["digest"]
            || self.ticket != std::ptr::from_ref(ticket) as usize
            || self.request_id != adoption.request_id()
            || !preview.matches_source(&local)
            || local["node_id"] != ticket.snapshot()["node_id"]
            || local["swarm_id"] != ticket.snapshot()["swarm_id"]
            || (self.drained && self.resources.upgrade().is_some())
        {
            return Err(EXTERNAL_CHANGED.into());
        }
        self.source_machine.current(pump)?;
        self.destination_machine.current(pump)?;
        Ok(())
    }
}

#[derive(Clone)]
pub(super) enum Action {
    Config,
    Preview(String),
    Adopt,
    Resume,
    Status,
}

enum Step {
    Config,
    PreviewConfig(String),
    Preview(String),
    AdoptConfig,
    AdoptLocal,
    Mutation,
    Status,
    Fresh,
    /// After a restart: this computer's journal for the disclosed request.
    DisclosedLocal(String),
    /// Then, for a source original, the destination's read-only status.
    DisclosedRemote {
        request_id: String,
        local_phase: String,
    },
}

struct Work {
    step: Step,
    invocation: Invocation,
}

enum Stage {
    Idle,
    Queued(Work),
    Running,
    Draining {
        work: Work,
        result: Result<Exit, String>,
        resources: Weak<Resources>,
    },
    Source,
}

pub(super) struct Context {
    id: String,
    visible: bool,
    disabled: bool,
    stage: Stage,
    aliases: Vec<String>,
    preview: Option<adoption::Preview>,
    adoption: Option<adoption::Adoption>,
    configuration: String,
    status: String,
    error: bool,
    changing: bool,
    ticket: Option<Box<ExternalTicket>>,
    native: Option<NativeContext>,
    expected: Option<Observation>,
    outcome: Option<Result<String, String>>,
    receipt: Option<Value>,
    /// When the running call is the original mutation, since when.
    mutation_since: Option<std::time::Instant>,
}

impl Context {
    fn new(disabled: bool) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            visible: true,
            disabled,
            stage: Stage::Idle,
            aliases: Vec::new(),
            preview: None,
            adoption: None,
            configuration: String::new(),
            status: String::new(),
            error: false,
            changing: false,
            ticket: None,
            native: None,
            expected: None,
            outcome: None,
            receipt: None,
            mutation_since: None,
        }
    }

    fn active(&self) -> bool {
        !matches!(self.stage, Stage::Idle | Stage::Source)
    }

    /// After a restart this dialog holds no original; the organization read
    /// may still disclose one. That is data for a read-only status lookup,
    /// never a ticket, a continuation or a mutation.
    fn disclosed_review(&self, hub: &Hub) -> Option<(String, String)> {
        if self.disabled || self.ticket.is_some() || self.adoption.is_some() || self.active() {
            return None;
        }
        disclosed(hub)
    }

    fn view(&self, hub: &Hub) -> Result<View, String> {
        let can_resume = (self.ticket.is_some() && self.adoption.is_some() && !self.active())
            || self.disclosed_review(hub).is_some();
        Ok(View {
            id: self.id.clone(),
            aliases: self.aliases.clone(),
            preview: self.preview.as_ref().map(|preview| PreviewView {
                request_id: preview.request_id().into(),
                alias: preview.alias().into(),
                eligible: preview.eligible(),
                reason: preview.reason(),
            }),
            status: self.status.clone(),
            error: self.error,
            busy: self.active(),
            changing: self.changing,
            can_preview: !self.disabled
                && self.ticket.is_none()
                && !self.active()
                && !self.aliases.is_empty(),
            can_adopt: !self.disabled
                && self.ticket.is_none()
                && !self.active()
                && self
                    .preview
                    .as_ref()
                    .is_some_and(|preview| preview.eligible()),
            can_resume,
            mode: if self.receipt.is_some() {
                "reconcile"
            } else {
                "preview"
            },
            drive_status: adoption::DRIVE_UNSUPPORTED,
        })
    }

    fn queue(&mut self, hub: &mut Hub, pump: &Pump, action: Action) -> Result<(), String> {
        if self.active() {
            return Err(BUSY.into());
        }
        self.error = false;
        match action {
            Action::Config => {
                if self.ticket.is_some() {
                    return Err(EXTERNAL_CHANGED.into());
                }
                self.stage = Stage::Queued(read_work(Step::Config, Read::Machines)?);
            }
            Action::Preview(alias) => {
                if self.ticket.is_some() || !self.aliases.contains(&alias) {
                    return Err(EXTERNAL_CHANGED.into());
                }
                adoption::preview_invocation(&alias)?;
                self.preview = None;
                self.receipt = None;
                self.status = "Checking whether the other computer can adopt this catalog…".into();
                self.stage = Stage::Queued(read_work(Step::PreviewConfig(alias), Read::Machines)?);
            }
            Action::Adopt => {
                if self.ticket.is_some() || hub.presentation_due || hub.recovery_due.is_some() {
                    return Err(zerus_client_core::swarm::sync::EXTERNAL_BUSY.into());
                }
                let preview = self.preview.as_ref().ok_or(EXTERNAL_CHANGED)?;
                if !preview.eligible() {
                    return Err(adoption::INVALID.into());
                }
                let local = hub.sync.observed_value().ok_or(EXTERNAL_CHANGED)?;
                if !preview.matches_source(&local) {
                    return Err(EXTERNAL_CHANGED.into());
                }
                let source_machine = NativeCardBinding::capture(
                    pump,
                    ComputerId::local(),
                    &preview.source_card().0,
                )?;
                let destination_machine = NativeCardBinding::capture(
                    pump,
                    ComputerId(preview.alias().to_owned()),
                    &preview.destination_card().0,
                )?;
                let generation = hub.source_generation.checked_add(1).ok_or(STOPPED)?;
                let ticket = Box::new(hub.sync.begin_external(hub.sync.revision())?);
                hub.source_generation = generation;
                let adoption = preview.adopt(uuid::Uuid::new_v4().to_string(), generation)?;
                self.native = Some(NativeContext {
                    generation,
                    digest: local["digest"].as_str().ok_or(EXTERNAL_CHANGED)?.into(),
                    configuration: self.configuration.clone(),
                    source_card: preview.source_card(),
                    destination_card: preview.destination_card(),
                    source_machine,
                    destination_machine,
                    ticket: std::ptr::from_ref(ticket.as_ref()) as usize,
                    request_id: adoption.request_id().into(),
                    drained: false,
                    resources: Weak::new(),
                    revoked: std::cell::Cell::new(false),
                });
                self.ticket = Some(ticket);
                self.adoption = Some(adoption);
                self.changing = true;
                self.status = "Copying the complete project catalog…".into();
                self.stage = Stage::Queued(read_work(Step::AdoptConfig, Read::Machines)?);
            }
            Action::Resume | Action::Status if self.adoption.is_none() => {
                let (request_id, _) = self.disclosed_review(hub).ok_or(EXTERNAL_CHANGED)?;
                self.status =
                    "Reviewing the disclosed original adoption status. No new mutation will be sent."
                        .into();
                self.stage = Stage::Queued(Work {
                    invocation: adoption::local_status_invocation(&request_id)?,
                    step: Step::DisclosedLocal(request_id),
                });
            }
            Action::Resume | Action::Status => {
                let adoption = self.adoption.as_ref().ok_or(EXTERNAL_CHANGED)?;
                self.status =
                    "Reviewing the original adoption status. No new mutation will be sent.".into();
                self.stage = Stage::Queued(Work {
                    step: Step::Status,
                    invocation: adoption::status_invocation(
                        preview_alias(self)?,
                        adoption.request_id(),
                    )?,
                });
            }
        }
        Ok(())
    }

    #[cfg(all(test, unix))]
    pub(super) fn drift_configuration(&mut self) {
        self.configuration = "changed".into();
    }

    /// No queued, running or draining call remains for this dialog.
    #[cfg(all(test, unix))]
    pub(super) fn idle_for_tests(&self) -> bool {
        !self.active()
    }

    fn failed(&mut self, error: String) {
        self.status = error;
        self.error = true;
        self.changing = false;
        self.stage = Stage::Idle;
    }

    fn result(
        &mut self,
        step: Step,
        result: Result<Exit, String>,
        hub: &Hub,
        pump: &Pump,
    ) -> Result<(), String> {
        let exit = result?;
        match step {
            Step::Config => {
                let config = reads::decode(&Read::Machines, exit).map_err(|e| e.to_string())?;
                self.aliases = adoption::configured_aliases(&config)?;
                self.stage = Stage::Idle;
            }
            Step::PreviewConfig(alias) => {
                let config = reads::decode(&Read::Machines, exit).map_err(|e| e.to_string())?;
                self.configuration = adoption::revision(&config, &alias)?;
                self.aliases = adoption::configured_aliases(&config)?;
                self.stage = Stage::Queued(Work {
                    step: Step::Preview(alias.clone()),
                    invocation: adoption::preview_invocation(&alias)?,
                });
            }
            Step::Preview(alias) => {
                let preview = adoption::Preview::checked(&alias, object(exit)?)?;
                let local = hub.sync.observed_value().ok_or(EXTERNAL_CHANGED)?;
                if preview.configuration_revision() != self.configuration
                    || !preview.matches_source(&local)
                {
                    return Err(EXTERNAL_CHANGED.into());
                }
                self.status = preview.reason();
                self.preview = Some(preview);
                self.stage = Stage::Idle;
            }
            Step::AdoptConfig => {
                let config = reads::decode(&Read::Machines, exit).map_err(|e| e.to_string())?;
                self.native
                    .as_ref()
                    .ok_or(EXTERNAL_CHANGED)?
                    .current(hub, self, pump)?;
                let preview = self.preview.as_ref().ok_or(EXTERNAL_CHANGED)?;
                if adoption::revision(&config, preview.alias())? != self.configuration
                    || preview.configuration_revision() != self.configuration
                {
                    return Err(zerus_client_core::machine_connections::CHANGED.into());
                }
                self.stage = Stage::Queued(read_work(Step::AdoptLocal, Read::Swarm)?);
            }
            Step::AdoptLocal => {
                let local = reads::decode(&Read::Swarm, exit).map_err(|e| e.to_string())?;
                let preview = self.preview.as_ref().ok_or(EXTERNAL_CHANGED)?;
                let ticket = self.ticket.as_ref().ok_or(EXTERNAL_CHANGED)?;
                if !preview.matches_source(&local)
                    || local["node_id"] != ticket.snapshot()["node_id"]
                    || local["swarm_id"] != ticket.snapshot()["swarm_id"]
                {
                    return Err(EXTERNAL_CHANGED.into());
                }
                let invocation = self
                    .adoption
                    .as_ref()
                    .ok_or(EXTERNAL_CHANGED)?
                    .invocation()?;
                self.native
                    .as_ref()
                    .ok_or(EXTERNAL_CHANGED)?
                    .current(hub, self, pump)?;
                self.stage = Stage::Queued(Work {
                    step: Step::Mutation,
                    invocation,
                });
            }
            Step::Mutation => {
                self.native
                    .as_ref()
                    .ok_or(EXTERNAL_CHANGED)?
                    .current(hub, self, pump)?;
                let reply = self
                    .adoption
                    .as_ref()
                    .ok_or(EXTERNAL_CHANGED)?
                    .receipt(&object(exit)?)?;
                self.outcome = Some(match reply["phase"].as_str() {
                    Some("settled") => Ok("Adoption completed. Drive folders remain uninitialized without reverse enrollment.".into()),
                    Some("refused_not_started") => Err("The destination refused before changing its catalog. Review the original request before continuing.".into()),
                    Some("remote_committed") => Err("The destination committed, but the original call did not settle local membership. Review manually; no new mutation will be sent.".into()),
                    _ => Err(adoption::INVALID.into()),
                });
                self.receipt = Some(reply);
                self.stage = Stage::Queued(read_work(Step::Fresh, Read::Swarm)?);
            }
            Step::Status => {
                let reply = object(exit)?;
                let adoption = self.adoption.as_ref().ok_or(EXTERNAL_CHANGED)?;
                if reply["request_id"] != adoption.request_id() {
                    return Err(adoption::INVALID.into());
                }
                let phase = reply["journal"]["phase"]
                    .as_str()
                    .ok_or(adoption::INVALID)?;
                if phase == "remote_committed" {
                    adoption.receipt(&reply["journal"]["receipts"]["remote"])?;
                }
                let status = format!(
                    "Original adoption status: {phase}. Review manually; this status is read-only."
                );
                // Status is retained data even when a stale native binding
                // forbids finish_external. It cannot grant another mutation.
                self.status = status.clone();
                self.error = true;
                self.outcome = Some(Err(status));
                self.receipt = Some(reply);
                self.stage = Stage::Queued(read_work(Step::Fresh, Read::Swarm)?);
            }
            Step::DisclosedLocal(request_id) => {
                let reply = object(exit)?;
                let journal = &reply["journal"];
                if reply["schema"] != 1
                    || reply["request_id"] != request_id.as_str()
                    || journal["original"]["request_id"] != request_id.as_str()
                {
                    return Err(adoption::INVALID.into());
                }
                let phase = disclosed_phase(&journal["phase"])?;
                match journal["role"].as_str() {
                    Some("source") => {
                        let alias = journal["original"]["source_alias"]
                            .as_str()
                            .filter(|alias| self.aliases.iter().any(|known| known == alias))
                            .ok_or("The disclosed original names a computer that is not configured here.")?
                            .to_owned();
                        self.stage = Stage::Queued(Work {
                            invocation: adoption::status_invocation(&alias, &request_id)?,
                            step: Step::DisclosedRemote {
                                request_id,
                                local_phase: phase,
                            },
                        });
                    }
                    Some("destination") => {
                        self.status = format!(
                            "Disclosed original adoption: this computer {phase}. Review manually; this status is read-only."
                        );
                        self.error = true;
                        self.stage = Stage::Idle;
                    }
                    _ => return Err(adoption::INVALID.into()),
                }
            }
            Step::DisclosedRemote {
                request_id,
                local_phase,
            } => {
                let reply = object(exit)?;
                if reply["request_id"] != request_id.as_str() {
                    return Err(adoption::INVALID.into());
                }
                let remote = disclosed_phase(&reply["journal"]["phase"])?;
                self.status = format!(
                    "Disclosed original adoption: this computer {local_phase}, destination {remote}. Review manually; this status is read-only."
                );
                self.error = true;
                self.stage = Stage::Idle;
            }
            Step::Fresh => {
                let value = reads::decode(&Read::Swarm, exit).map_err(|e| e.to_string())?;
                self.expected = Some(Observation::from_reply(value).ok_or(adoption::INVALID)?);
                self.stage = Stage::Source;
                self.changing = false;
            }
        }
        Ok(())
    }
}

/// Only the three fixed adoption calls run as a private helper group
/// (the disclosed remote status is adoption-status too).
fn group_custody(step: &Step) -> bool {
    matches!(
        step,
        Step::Preview(_) | Step::Mutation | Step::Status | Step::DisclosedRemote { .. }
    )
}

/// The request and role the organization read discloses (`adoption`), if any.
fn disclosed(hub: &Hub) -> Option<(String, String)> {
    let local = hub.sync.observed_value()?;
    let adoption = local.get("adoption")?;
    let request_id = adoption["request_id"].as_str()?;
    let role = adoption["role"].as_str()?;
    (uuid::Uuid::parse_str(request_id).is_ok()
        && request_id == request_id.to_ascii_lowercase()
        && matches!(role, "source" | "destination"))
    .then(|| (request_id.to_owned(), role.to_owned()))
}

/// A recorded phase name, shown only as bounded snake_case text.
fn disclosed_phase(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .filter(|phase| {
            (1..=40).contains(&phase.len())
                && phase
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        })
        .map(str::to_owned)
        .ok_or_else(|| adoption::INVALID.into())
}

#[cfg(all(test, unix))]
pub(super) fn group_custody_for_tests() -> Vec<(&'static str, bool)> {
    [
        ("config", Step::Config),
        ("preview-config", Step::PreviewConfig("peer".into())),
        ("preview", Step::Preview("peer".into())),
        ("adopt-config", Step::AdoptConfig),
        ("adopt-local", Step::AdoptLocal),
        ("mutation", Step::Mutation),
        ("status", Step::Status),
        ("fresh", Step::Fresh),
        ("disclosed-local", Step::DisclosedLocal(String::new())),
        (
            "disclosed-remote",
            Step::DisclosedRemote {
                request_id: String::new(),
                local_phase: String::new(),
            },
        ),
    ]
    .into_iter()
    .map(|(name, step)| (name, group_custody(&step)))
    .collect()
}

fn preview_alias(context: &Context) -> Result<&str, String> {
    Ok(context.preview.as_ref().ok_or(EXTERNAL_CHANGED)?.alias())
}

fn read_work(step: Step, read: Read) -> Result<Work, String> {
    Ok(Work {
        step,
        invocation: reads::invocation(&ComputerId::local(), &read).map_err(|e| e.to_string())?,
    })
}

fn object(exit: Exit) -> Result<Value, String> {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => serde_json::from_slice::<Value>(&stdout)
            .map_err(|_| adoption::INVALID.to_owned())
            .and_then(|value| {
                value
                    .is_object()
                    .then_some(value)
                    .ok_or_else(|| adoption::INVALID.into())
            }),
        Exit::Code { stderr, .. } | Exit::Abnormal { stderr } => Err(if stderr.trim().is_empty() {
            adoption::INVALID.into()
        } else {
            stderr.chars().take(2048).collect()
        }),
        Exit::TimedOut => Err(ReadError::TimedOut.to_string()),
        Exit::StartFailed(message) => Err(message.chars().take(2048).collect()),
        Exit::WriteFailed => Err("hgs did not take the request".into()),
    }
}

impl Organization {
    fn adoption_lock(&self) -> Result<MutexGuard<'_, Hub>, String> {
        self.0.hub.lock().map_err(|_| STOPPED.into())
    }

    pub(super) fn adoption_request(
        &self,
        id: Option<&str>,
        action: Option<Action>,
        close: bool,
    ) -> Result<View, String> {
        let mut hub = self.adoption_lock()?;
        if hub.stopped {
            return Err(STOPPED.into());
        }
        if id.is_none() && hub.adoption.is_none() {
            let mut context = Context::new(!self.0.allowed);
            if self.0.allowed {
                context.queue(&mut hub, &self.0.pump, Action::Config)?;
            } else {
                context.status = REPLAY_NEEDS_HGS.into();
                context.error = true;
            }
            hub.adoption = Some(context);
        }
        let current = hub.adoption.as_ref().ok_or(EXPIRED)?;
        if id.is_some_and(|id| id != current.id) {
            return Err(EXPIRED.into());
        }
        if id.is_some() && action.is_none() && !close {
            return current.view(&hub);
        }
        let mut context = hub.adoption.take().ok_or(EXPIRED)?;
        let result = (|| {
            if close {
                // Hiding never cancels or forgets a queued or running original:
                // the worker keeps it, and reopening attaches to the same one.
                context.visible = false;
            } else {
                if id.is_none() {
                    context.visible = true;
                }
                if let Some(action) = action {
                    if !context.visible {
                        return Err(EXPIRED.into());
                    }
                    if !self.0.allowed {
                        return Err(REPLAY_NEEDS_HGS.into());
                    }
                    context.queue(&mut hub, &self.0.pump, action)?;
                }
            }
            context.view(&hub)
        })();
        hub.adoption = Some(context);
        hub.shown = None;
        self.publish(&mut hub);
        self.release(hub);
        self.0.wake.notify_all();
        result
    }

    pub(super) fn adoption_pass(&self) -> bool {
        if self.0.hub.is_poisoned() {
            return false;
        }
        let work = {
            let mut hub = self.lock();
            // Recheck the original independent card/generation binding at the
            // actual queued invocation barrier. Read-only original Status and
            // Fresh remain available after drift; no ticket is discarded.
            let refused = hub.adoption.as_ref().and_then(|context| {
                if matches!(
                    &context.stage,
                    Stage::Queued(Work {
                        step: Step::AdoptConfig | Step::AdoptLocal | Step::Mutation,
                        ..
                    })
                ) {
                    context
                        .native
                        .as_ref()
                        .ok_or_else(|| EXTERNAL_CHANGED.to_owned())
                        .and_then(|native| native.current(&hub, context, &self.0.pump))
                        .err()
                } else {
                    None
                }
            });
            if let Some(error) = refused {
                hub.adoption.as_mut().unwrap().failed(error);
                hub.shown = None;
                self.publish(&mut hub);
                self.release(hub);
                return true;
            }
            let Some(context) = hub.adoption.as_mut() else {
                return false;
            };
            if !self.0.allowed && context.ticket.is_some() {
                return false;
            }
            match std::mem::replace(&mut context.stage, Stage::Idle) {
                Stage::Queued(work) => {
                    context.stage = Stage::Running;
                    context.mutation_since =
                        matches!(work.step, Step::Mutation).then(std::time::Instant::now);
                    Some(work)
                }
                Stage::Draining {
                    work,
                    result,
                    resources,
                } if resources.upgrade().is_none() => {
                    let mut context = hub.adoption.take().unwrap();
                    let needs_fresh = context.ticket.is_some() && !matches!(work.step, Step::Fresh);
                    let drained = matches!(work.step, Step::Mutation);
                    if drained {
                        if let Some(native) = context.native.as_mut() {
                            native.drained = true;
                            native.resources = resources;
                        }
                    }
                    let checked = if drained {
                        context
                            .native
                            .as_ref()
                            .map_or(Err(EXTERNAL_CHANGED.into()), |native| {
                                native.current(&hub, &context, &self.0.pump)
                            })
                    } else {
                        Ok(())
                    };
                    let result = checked
                        .and_then(|()| context.result(work.step, result, &hub, &self.0.pump));
                    if let Err(error) = result {
                        if needs_fresh {
                            context.status = error.clone();
                            context.error = true;
                            context.outcome = Some(Err(error));
                            context.stage = read_work(Step::Fresh, Read::Swarm)
                                .map(Stage::Queued)
                                .unwrap_or(Stage::Idle);
                        } else {
                            context.failed(error);
                        }
                    }
                    #[cfg(test)]
                    let queued_mutation = matches!(
                        context.stage,
                        Stage::Queued(Work {
                            step: Step::Mutation,
                            ..
                        })
                    );
                    hub.adoption = Some(context);
                    hub.shown = None;
                    self.publish(&mut hub);
                    self.release(hub);
                    #[cfg(test)]
                    if let Some(hook) = self
                        .0
                        .adoption_queued
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .as_ref()
                    {
                        hook(queued_mutation);
                    }
                    self.adoption_settle_source();
                    return true;
                }
                other => {
                    let settling = matches!(other, Stage::Running)
                        && context
                            .mutation_since
                            .is_some_and(|since| since.elapsed() >= SETTLING_AFTER)
                        && context.status != SETTLING;
                    context.stage = other;
                    if settling {
                        // Still unknown: the card says so, nothing else changes.
                        context.status = SETTLING.into();
                        hub.shown = None;
                        self.publish(&mut hub);
                        self.release(hub);
                    }
                    None
                }
            }
        };
        let Some(work) = work else {
            return false;
        };
        // The call runs beside the organization worker, which keeps serving
        // every other pass while an original is retained. Only this dialog's
        // admission stays closed (Running) until the result is recorded.
        let organization = self.clone();
        let started = std::thread::Builder::new()
            .name("zerus-adoption-call".into())
            .spawn(move || organization.adoption_call(work));
        if let Err(error) = started {
            // The closure, and the work in it, never ran: no call started.
            let mut hub = self.lock();
            if let Some(context) = hub.adoption.as_mut() {
                context.mutation_since = None;
                context.failed(format!("The adoption request could not start: {error}"));
            }
            hub.shown = None;
            self.publish(&mut hub);
            self.release(hub);
        }
        true
    }

    fn adoption_call(&self, work: Work) {
        let resources = Arc::new(Resources(Arc::downgrade(&self.0)));
        let weak = Arc::downgrade(&resources);
        // Preview, Mutation and Status are the three fixed adoption calls
        // (adoption-preview, adopt, adoption-status). Their CLI leads a
        // private process group created for that call only; the runner
        // signals that exact group while its leader is pinned unreaped, so
        // the original ssh is stopped and reaped even if the CLI was killed.
        // The dialog's local reads keep the ordinary runner.
        let run = if group_custody(&work.step) {
            zerus_client_core::run::run_accounted_group
        } else {
            zerus_client_core::run::run_accounted
        };
        let result = run(
            &self.0.hgs,
            &work.invocation,
            &[],
            Read::Swarm.limit(),
            &AtomicBool::new(false),
            resources.clone(),
        )
        .map_err(|e| match e {
            zerus_client_core::run::Stopped::TooLarge(too) => {
                ReadError::TooLarge { limit: too.cap }.to_string()
            }
            zerus_client_core::run::Stopped::Cancelled => "hgs request cancelled".into(),
        });
        drop(resources);
        let mut hub = self.lock();
        if let Some(context) = hub.adoption.as_mut() {
            // Only this call leaves Running; nothing else can change the
            // stage meanwhile, so the work and its resources are retained.
            debug_assert!(matches!(context.stage, Stage::Running));
            context.mutation_since = None;
            context.stage = Stage::Draining {
                work,
                result,
                resources: weak,
            };
        }
        hub.shown = None;
        self.publish(&mut hub);
        self.release(hub);
        self.0.wake.notify_all();
    }

    /// A later settlement attempt, as any future trigger would make it.
    #[cfg(all(test, unix))]
    pub(super) fn adoption_settle_for_tests(&self) {
        self.adoption_settle_source();
    }

    fn adoption_settle_source(&self) {
        if self.0.hub.is_poisoned() || !self.0.allowed {
            return;
        }
        let source = {
            let hub = self.lock();
            if hub
                .adoption
                .as_ref()
                .is_none_or(|context| !matches!(context.stage, Stage::Source))
            {
                return;
            }
            hub.source.clone()
        };
        let Some(source) = source.and_then(|source| source()) else {
            return;
        };
        let mut hub = self.lock();
        if adoption_settle(&mut hub, &source, self.now(), &self.0.pump) {
            hub.shown = None;
            self.publish(&mut hub);
            self.release(hub);
            self.0.wake.notify_all();
        }
    }
}

fn adoption_settle(hub: &mut Hub, source: &OrganizationSnapshot, now: u64, pump: &Pump) -> bool {
    let Some(mut context) = hub.adoption.take() else {
        return false;
    };
    let ready = matches!(context.stage, Stage::Source)
        && context.native.as_ref().is_some_and(|native| {
            native.drained
                && native.resources.upgrade().is_none()
                && native.current(hub, &context, pump).is_ok()
        })
        && context.expected.as_ref().is_some_and(|expected| {
            source.node_id == expected.value["node_id"].as_str().unwrap_or_default()
                && source.swarm_id == expected.value["swarm_id"].as_str().unwrap_or_default()
                && source.digest == expected.digest
                && source.base == expected.value["organization"]
                && source.versions == expected.versions
        });
    let changed = if ready {
        if context
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt["phase"] == "settled")
        {
            match hub.source_generation.checked_add(1) {
                Some(generation) => {
                    let expected = context.expected.take().unwrap();
                    match hub
                        .sync
                        .finish_external(context.ticket.as_ref().unwrap(), expected, now)
                    {
                        Ok(()) => {
                            hub.source_generation = generation;
                            context.ticket = None;
                            context.native = None;
                            context.adoption = None;
                            context.stage = Stage::Idle;
                            match context
                                .outcome
                                .take()
                                .unwrap_or_else(|| Err(adoption::INVALID.into()))
                            {
                                Ok(status) => {
                                    context.status = status;
                                    context.error = false;
                                }
                                Err(error) => context.failed(error),
                            }
                        }
                        Err(error) => context.failed(error),
                    }
                }
                None => context.failed(STOPPED.into()),
            }
        } else {
            context.stage = Stage::Idle;
            context.changing = false;
            if let Some(outcome) = context.outcome.take() {
                match outcome {
                    Ok(status) => context.status = status,
                    Err(error) => {
                        context.status = error;
                        context.error = true;
                    }
                }
            }
        }
        true
    } else {
        false
    };
    hub.adoption = Some(context);
    changed
}

fn main(window: &tauri::Webview) -> Result<(), String> {
    if window.label() == crate::MAIN_WINDOW {
        Ok(())
    } else {
        Err("Open Project adoption from the main window.".into())
    }
}

async fn command(
    org: Organization,
    id: Option<String>,
    action: Option<Action>,
    close: bool,
) -> Result<View, String> {
    tauri::async_runtime::spawn_blocking(move || org.adoption_request(id.as_deref(), action, close))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn project_adoption_open(
    window: tauri::Webview,
    state: State<'_, Organization>,
) -> Result<View, String> {
    main(&window)?;
    command(state.inner().clone(), None, None, false).await
}

#[tauri::command]
pub async fn project_adoption_view(
    window: tauri::Webview,
    state: State<'_, Organization>,
    id: String,
) -> Result<View, String> {
    main(&window)?;
    command(state.inner().clone(), Some(id), None, false).await
}

#[tauri::command]
pub async fn project_adoption_preview(
    window: tauri::Webview,
    state: State<'_, Organization>,
    id: String,
    alias: String,
) -> Result<View, String> {
    main(&window)?;
    command(
        state.inner().clone(),
        Some(id),
        Some(Action::Preview(alias)),
        false,
    )
    .await
}

#[tauri::command]
pub async fn project_adoption_adopt(
    window: tauri::Webview,
    state: State<'_, Organization>,
    id: String,
) -> Result<View, String> {
    main(&window)?;
    command(state.inner().clone(), Some(id), Some(Action::Adopt), false).await
}

#[tauri::command]
pub async fn project_adoption_status(
    window: tauri::Webview,
    state: State<'_, Organization>,
    id: String,
) -> Result<View, String> {
    main(&window)?;
    command(state.inner().clone(), Some(id), Some(Action::Status), false).await
}

#[tauri::command]
pub async fn project_adoption_resume(
    window: tauri::Webview,
    state: State<'_, Organization>,
    id: String,
) -> Result<View, String> {
    main(&window)?;
    command(state.inner().clone(), Some(id), Some(Action::Resume), false).await
}

#[tauri::command]
pub async fn project_adoption_close(
    window: tauri::Webview,
    state: State<'_, Organization>,
    id: String,
) -> Result<View, String> {
    main(&window)?;
    command(state.inner().clone(), Some(id), None, true).await
}
