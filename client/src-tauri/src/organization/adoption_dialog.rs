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

struct NativeContext {
    generation: u64,
    digest: String,
    configuration: String,
    source_card: (String, String),
    destination_card: (String, String),
    ticket: usize,
    request_id: String,
    drained: bool,
    resources: Weak<Resources>,
}

impl NativeContext {
    fn current(&self, hub: &Hub, context: &Context) -> Result<(), String> {
        let preview = context.preview.as_ref().ok_or(EXTERNAL_CHANGED)?;
        let ticket = context.ticket.as_deref().ok_or(EXTERNAL_CHANGED)?;
        let adoption = context.adoption.as_ref().ok_or(EXTERNAL_CHANGED)?;
        let local = hub.sync.observed_value().ok_or(EXTERNAL_CHANGED)?;
        if !preview.eligible()
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
        }
    }

    fn active(&self) -> bool {
        !matches!(self.stage, Stage::Idle | Stage::Source)
    }

    fn view(&self, _hub: &Hub) -> Result<View, String> {
        let can_resume = self.ticket.is_some() && self.adoption.is_some() && !self.active();
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

    fn queue(&mut self, hub: &mut Hub, action: Action) -> Result<(), String> {
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
                    ticket: std::ptr::from_ref(ticket.as_ref()) as usize,
                    request_id: adoption.request_id().into(),
                    drained: false,
                    resources: Weak::new(),
                });
                self.ticket = Some(ticket);
                self.adoption = Some(adoption);
                self.changing = true;
                self.status = "Copying the complete project catalog…".into();
                self.stage = Stage::Queued(read_work(Step::AdoptConfig, Read::Machines)?);
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
                    .current(hub, self)?;
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
                    .current(hub, self)?;
                self.stage = Stage::Queued(Work {
                    step: Step::Mutation,
                    invocation,
                });
            }
            Step::Mutation => {
                self.native
                    .as_ref()
                    .ok_or(EXTERNAL_CHANGED)?
                    .current(hub, self)?;
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
                self.outcome = Some(Err(format!(
                    "Original adoption status: {phase}. Review manually; this status is read-only."
                )));
                self.receipt = Some(reply);
                self.stage = Stage::Queued(read_work(Step::Fresh, Read::Swarm)?);
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
                context.queue(&mut hub, Action::Config)?;
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
                if context.changing && matches!(context.stage, Stage::Queued(_) | Stage::Running) {
                    return Err(BUSY.into());
                }
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
                    context.queue(&mut hub, action)?;
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
            let Some(context) = hub.adoption.as_mut() else {
                return false;
            };
            if !self.0.allowed && context.ticket.is_some() {
                return false;
            }
            match std::mem::replace(&mut context.stage, Stage::Idle) {
                Stage::Queued(work) => {
                    context.stage = Stage::Running;
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
                                native.current(&hub, &context)
                            })
                    } else {
                        Ok(())
                    };
                    let result = checked.and_then(|()| context.result(work.step, result, &hub));
                    if let Err(error) = result {
                        if needs_fresh {
                            context.outcome = Some(Err(error));
                            context.stage = read_work(Step::Fresh, Read::Swarm)
                                .map(Stage::Queued)
                                .unwrap_or(Stage::Idle);
                        } else {
                            context.failed(error);
                        }
                    }
                    hub.adoption = Some(context);
                    hub.shown = None;
                    self.publish(&mut hub);
                    self.release(hub);
                    self.adoption_settle_source();
                    return true;
                }
                other => {
                    context.stage = other;
                    None
                }
            }
        };
        let Some(work) = work else {
            return false;
        };
        let resources = Arc::new(Resources(Arc::downgrade(&self.0)));
        let weak = Arc::downgrade(&resources);
        let result = zerus_client_core::run::run_accounted(
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
            context.stage = Stage::Draining {
                work,
                result,
                resources: weak,
            };
        }
        hub.shown = None;
        self.publish(&mut hub);
        self.release(hub);
        true
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
        if adoption_settle(&mut hub, &source, self.now()) {
            hub.shown = None;
            self.publish(&mut hub);
            self.release(hub);
            self.0.wake.notify_all();
        }
    }
}

fn adoption_settle(hub: &mut Hub, source: &OrganizationSnapshot, now: u64) -> bool {
    let Some(mut context) = hub.adoption.take() else {
        return false;
    };
    let ready = matches!(context.stage, Stage::Source)
        && context.native.as_ref().is_some_and(|native| {
            native.drained
                && native.resources.upgrade().is_none()
                && native.current(hub, &context).is_ok()
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
