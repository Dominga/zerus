//! One-time, add-only migration on the existing organization worker. This lane
//! never enters the ordinary retrying draft: reserve durably, dispatch at most
//! once, then observe. Every CLI step keeps its I/O charge through actual drain.
use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use zerus_client_core::reads;
use zerus_client_core::settings::{CapturedQt, QtStore, Settings};
use zerus_client_core::swarm::legacy_projects::{self as model, Binding, Code, Plan};
use zerus_client_core::swarm::sync::{self, ExternalTicket};
use zerus_client_core::{ComputerId, Invocation};

const MACHINES: usize = 256;
const READ_BACKOFF_MS: [u64; 5] = [250, 1_000, 3_000, 10_000, 30_000];
const READ_BLOCKED_AFTER: u8 = 3;
const READ_BLOCKED_CODE: &str = "legacy_projects_read_blocked";
pub(super) const READ_BLOCKED: &str = zerus_client_core::source!(
    "Project import verification is blocked. Retrying reads automatically."
);
fn read_delay(failures: u8) -> u64 {
    READ_BACKOFF_MS[usize::from(failures.saturating_sub(1)).min(READ_BACKOFF_MS.len() - 1)]
}

#[derive(Clone)]
pub(super) struct Report {
    settings: Settings,
    codes: Arc<Mutex<Vec<Code>>>,
    read_blocked: Arc<AtomicBool>,
}
impl Report {
    fn problem(&self, code: Code) {
        let mut codes = self.codes.lock().unwrap_or_else(PoisonError::into_inner);
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    pub(super) fn value(&self) -> Value {
        let mut value = self.settings.legacy_project_report();
        let codes = self.codes.lock().unwrap_or_else(PoisonError::into_inner);
        let read_blocked = self.read_blocked.load(Ordering::Acquire);
        if !codes.is_empty() || read_blocked {
            let reasons = value["reasons"].as_array_mut().unwrap();
            for code in codes.iter() {
                let reason = json!(code.as_str());
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
            }
            if read_blocked {
                reasons.push(json!(READ_BLOCKED_CODE));
            }
            value["state"] = json!("blocked");
            value["keys"] = json!(["projects", "folders"]);
        }
        value
    }
}

pub(super) struct Context {
    report: Report,
    qt: Option<QtStore>,
    captured: Option<CapturedQt>,
    imported: Option<BTreeSet<String>>,
    refused: bool,
    seen: BTreeSet<String>,
    job: Option<Job>,
}
#[derive(Clone)]
struct Target {
    computer: ComputerId,
    machine: String,
    name: String,
    platform: String,
    owner: String,
    swarm: String,
    generation: u64,
    epoch: u64,
    owner_epoch: u64,
}
struct Job {
    target: Target,
    stage: Stage,
    revision: Option<String>,
    binding: Option<Binding>,
    rows: Option<Value>,
    before: Option<Value>,
    plan: Option<Plan>,
    ticket: Option<ExternalTicket>,
    /// Existing durable reservations are observation-only, including restart.
    pending: bool,
    /// Latches before the original call; no error path queues Apply again.
    sent: bool,
    last_read_digest: String,
    read_failures: u8,
}
#[derive(Clone, Copy)]
enum Step {
    Help,
    Config,
    Projects,
    Before,
    Check,
    Apply,
    After,
    Confirm,
}
enum Stage {
    Command(Step, Invocation),
    Draining {
        step: Step,
        result: Result<Exit, Code>,
        resources: Weak<Resources>,
    },
    Source(Value),
    /// Only fresh read/confirmation is retried, never preparation or Apply.
    RetryRead {
        at: u64,
    },
    Done,
}
struct Resources(Weak<Shared>);
impl Drop for Resources {
    fn drop(&mut self) {
        if let Some(shared) = self.0.upgrade() {
            shared.wake.notify_all();
        }
    }
}

impl Organization {
    /// Called only by the production switch. An absent Qt file is supported;
    /// a development/replay identity supplies no source and never starts this.
    pub fn import_legacy_projects(&self, qt: Option<QtStore>, settings: Settings) {
        if !self.0.allowed || qt.is_none() {
            return;
        }
        let mut hub = self.lock();
        if hub.legacy_report.is_some() {
            return;
        }
        let report = Report {
            settings,
            codes: Arc::new(Mutex::new(Vec::new())),
            read_blocked: Arc::new(AtomicBool::new(false)),
        };
        hub.legacy_epochs.insert(String::new(), 0);
        hub.legacy_report = Some(report.clone());
        hub.legacy = Some(Context {
            report,
            qt,
            captured: None,
            imported: None,
            refused: false,
            seen: BTreeSet::new(),
            job: None,
        });
        drop(hub);
        self.0.wake.notify_all();
    }
    // Runs before Pump publishes a disconnect, changed hello or failed
    // organization. Only registered targets occupy this bounded map.
    pub(super) fn legacy_observe(&self, change: &Change) {
        let computer = match change {
            Change::Machine { computer, .. } | Change::Computer { computer, .. } => computer,
            Change::Organization {
                computer,
                update: OrganizationUpdate::Failed { .. } | OrganizationUpdate::Ended,
            } if computer.is_local() => computer,
            _ => return,
        };
        if !self.lock().legacy_epochs.contains_key(&computer.0) {
            return;
        }
        let invalid = match change {
            Change::Machine { machine, .. } => self.0.pump.machine(computer).is_none_or(|old| {
                old.machine_id != machine.machine_id
                    || old.host != machine.host
                    || old.platform != machine.platform
                    || old.ok != machine.ok
                    || old.local_sessions != machine.local_sessions
            }),
            Change::Computer { status, .. } => !matches!(status.name(), "live" | "polled"),
            _ => true,
        };
        if invalid {
            let mut hub = self.lock();
            if let Some(epoch) = hub.legacy_epochs.get_mut(&computer.0) {
                if let Some(next) = epoch.checked_add(1) {
                    *epoch = next;
                } else {
                    hub.stopped = true;
                }
            }
        }
    }
    pub fn legacy_project_report(&self) -> Value {
        let report = self.lock().legacy_report.clone();
        report.map_or_else(
            || json!({"family":"legacy-projects","state":"empty","keys":[],"reasons":[]}),
            |r| r.value(),
        )
    }
    pub(super) fn legacy_projects_pass(&self) -> bool {
        let context = {
            let mut hub = self.lock();
            if hub.stopped || self.0.hub.is_poisoned() {
                return false;
            }
            hub.legacy.take()
        };
        let Some(mut context) = context else {
            return false;
        };
        let progress = context.pass(self);
        self.lock().legacy = Some(context);
        progress
    }
}

impl Target {
    fn root_current(&self, org: &Organization) -> bool {
        let hub = org.lock();
        let same = !hub.stopped
            && hub
                .sync
                .observed_value()
                .is_some_and(|s| s["node_id"] == self.owner && s["swarm_id"] == self.swarm);
        drop(hub);
        same && online(org, &ComputerId::local())
            .is_some_and(|m| m.machine_id.as_deref() == Some(&self.owner))
    }
    fn identity_current(&self, org: &Organization) -> bool {
        self.root_current(org)
            && online(org, &self.computer).is_some_and(|m| {
                m.machine_id.as_deref() == Some(&self.machine)
                    && m.host.as_deref() == Some(&self.name)
                    && m.platform.as_deref() == Some(&self.platform)
            })
    }
    fn current(&self, org: &Organization) -> bool {
        let hub = org.lock();
        let same = hub.legacy_epochs.get(&self.computer.0) == Some(&self.epoch)
            && hub.legacy_epochs.get("") == Some(&self.owner_epoch)
            && hub.source_generation == self.generation;
        drop(hub);
        same && self.identity_current(org)
    }
    fn read(&self, read: Read) -> Result<Invocation, Code> {
        let computer = if read == Read::Projects {
            &self.computer
        } else {
            &ComputerId::local()
        };
        let mut inv = reads::invocation(computer, &read).map_err(|_| Code::Invalid)?;
        zerus_client_core::worktrees::guard(
            &mut inv.args,
            if read == Read::Projects {
                &self.machine
            } else {
                &self.owner
            },
        );
        Ok(inv)
    }
}
fn online(org: &Organization, computer: &ComputerId) -> Option<zerus_client_core::MachineMeta> {
    let entry = org
        .0
        .pump
        .fleet()
        .into_iter()
        .find(|e| &e.computer == computer)?;
    if !matches!(entry.status.as_str(), "live" | "polled") {
        return None;
    }
    let meta = org.0.pump.machine(computer)?;
    (meta.ok == Some(true)
        && meta
            .machine_id
            .as_deref()
            .is_some_and(zerus_client_core::creation::canonical_uuid))
    .then_some(meta)
}
fn cache(org: &Organization) -> Option<OrganizationSnapshot> {
    let source = org.lock().source.clone()?;
    source()
}
fn same_source(source: &OrganizationSnapshot, value: &Value) -> bool {
    source.node_id == value["node_id"].as_str().unwrap_or_default()
        && source.swarm_id == value["swarm_id"].as_str().unwrap_or_default()
        && source.digest == value["digest"].as_str().unwrap_or_default()
        && source.base == value["organization"]
        && source.versions == value["versions"]
}
fn configuration(value: &Value, computer: &ComputerId) -> Result<String, Code> {
    if let Some(alias) = computer.alias() {
        return swarm::connection::configuration_revision(value, alias).map_err(|_| Code::Changed);
    }
    let revision = value["revision"].as_str().ok_or(Code::Invalid)?;
    if revision.len() != 64
        || !revision
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(Code::Invalid);
    }
    Ok(revision.into())
}
fn decode(read: Read, exit: Exit) -> Result<Value, Code> {
    reads::decode(&read, exit).map_err(|e| {
        if matches!(e, ReadError::TooLarge { .. }) {
            Code::Capacity
        } else {
            Code::Invalid
        }
    })
}
fn store_error(error: String) -> Code {
    [
        Code::Capacity,
        Code::Changed,
        Code::History,
        Code::InvalidFolders,
        Code::Pending,
    ]
    .into_iter()
    .find(|c| error == c.as_str())
    .unwrap_or(Code::Invalid)
}

impl Context {
    fn pass(&mut self, org: &Organization) -> bool {
        if self.refused {
            return false;
        }
        if self.imported.is_none() {
            let loaded = self
                .qt
                .as_ref()
                .unwrap()
                .capture_for_migration(Arc::new(()), &AtomicBool::new(false));
            match loaded {
                Ok(captured) => {
                    match model::qt_imported_machines(captured.as_ref().map(CapturedQt::snapshot)) {
                        Ok(imported) => {
                            self.imported = Some(imported);
                            self.captured = captured;
                        }
                        Err(code) => {
                            self.report.problem(code);
                            self.refused = true;
                        }
                    }
                }
                Err(_) => {
                    self.report.problem(Code::Invalid);
                    self.refused = true;
                }
            }
            return true;
        }
        let Some(mut job) = self.job.take() else {
            return self.discover(org);
        };
        let stage = std::mem::replace(&mut job.stage, Stage::Done);
        let progress = match stage {
            Stage::Command(step, invocation) => {
                if matches!(step, Step::Apply)
                    && (job.sent
                        || !job
                            .before
                            .as_ref()
                            .is_some_and(|v| cache(org).is_some_and(|s| same_source(&s, v)))
                        || !job.target.current(org))
                {
                    self.report.problem(Code::Changed);
                    job.queue(Step::After, Read::Swarm);
                } else {
                    if matches!(step, Step::Apply) {
                        job.sent = true;
                    }
                    let resource = Arc::new(Resources(Arc::downgrade(&org.0)));
                    let weak = Arc::downgrade(&resource);
                    let limit = if matches!(step, Step::Help) {
                        128 << 10
                    } else if matches!(step, Step::Projects) {
                        model::MAX_BYTES
                    } else {
                        Read::Swarm.limit()
                    };
                    let result = zerus_client_core::run::run_accounted(
                        &org.0.hgs,
                        &invocation,
                        &[],
                        limit,
                        &AtomicBool::new(false),
                        resource.clone(),
                    )
                    .map_err(|e| {
                        if matches!(e, zerus_client_core::run::Stopped::TooLarge(_)) {
                            Code::Capacity
                        } else {
                            Code::Invalid
                        }
                    });
                    drop(resource);
                    job.stage = Stage::Draining {
                        step,
                        result,
                        resources: weak,
                    };
                }
                true
            }
            Stage::Draining {
                step,
                result,
                resources,
            } => {
                if resources.upgrade().is_some() {
                    job.stage = Stage::Draining {
                        step,
                        result,
                        resources,
                    };
                    false
                } else {
                    match self.result(org, &mut job, step, result) {
                        Err(_) if matches!(step, Step::After | Step::Confirm) => {
                            self.retry_read(org, &mut job);
                        }
                        Err(code) => {
                            self.report.problem(code);
                            if job.ticket.is_some() {
                                job.queue(Step::After, Read::Swarm);
                            }
                        }
                        Ok(()) if matches!(job.stage, Stage::Done) => {
                            self.read_status(org, false, false)
                        }
                        Ok(()) => {}
                    }
                    true
                }
            }
            Stage::Source(value) => {
                if job.target.root_current(org)
                    && cache(org).is_some_and(|s| same_source(&s, &value))
                {
                    job.rows = Some(value);
                    job.queue(Step::Confirm, Read::Machines);
                    true
                } else if !job.target.root_current(org) {
                    if job.ticket.is_some() {
                        self.retry_read(org, &mut job);
                    } else {
                        self.report.problem(Code::Changed);
                    }
                    false
                } else if let Some(source) = cache(org).filter(|s| s.digest != job.last_read_digest)
                {
                    // A newer watch state may have passed the exact fresh read.
                    // Read once for that new digest; never poll the old one.
                    job.last_read_digest = source.digest;
                    job.queue(Step::After, Read::Swarm);
                    true
                } else {
                    job.stage = Stage::Source(value);
                    false
                }
            }
            Stage::RetryRead { at } => {
                if org.now() >= at {
                    job.queue(Step::After, Read::Swarm);
                    true
                } else {
                    job.stage = Stage::RetryRead { at };
                    false
                }
            }
            Stage::Done => false,
        };
        if !matches!(job.stage, Stage::Done) {
            self.job = Some(job);
        }
        progress
    }
    fn read_status(&self, org: &Organization, blocked: bool, fenced: bool) {
        self.report.read_blocked.store(blocked, Ordering::Release);
        let mut hub = org.lock();
        if hub.legacy_read_blocked != (blocked && fenced) {
            hub.legacy_read_blocked = blocked && fenced;
            org.publish(&mut hub);
            org.release(hub);
        }
    }
    fn retry_read(&self, org: &Organization, job: &mut Job) {
        job.read_failures = job.read_failures.saturating_add(1);
        job.stage = Stage::RetryRead {
            at: org.now().saturating_add(read_delay(job.read_failures)),
        };
        // A transient transport error is not a permanent import refusal.
        // Keep retrying only the read lane, with a 30 s delay cap; after three
        // failures, Projects and the report explain why the fence is held.
        self.read_status(
            org,
            job.read_failures >= READ_BLOCKED_AFTER,
            job.ticket.is_some(),
        );
    }
    pub(super) fn next_due(&self) -> Option<u64> {
        match self.job.as_ref()?.stage {
            Stage::RetryRead { at } => Some(at),
            _ => None,
        }
    }
    fn discover(&mut self, org: &Organization) -> bool {
        let hub = org.lock();
        if !hub.sync.ready() || hub.sync.busy() || hub.sync.pending() {
            return false;
        }
        let Some(snapshot) = hub.sync.observed_value() else {
            return false;
        };
        let (Some(owner), Some(swarm)) =
            (snapshot["node_id"].as_str(), snapshot["swarm_id"].as_str())
        else {
            return false;
        };
        if !zerus_client_core::creation::canonical_uuid(owner)
            || !zerus_client_core::creation::canonical_uuid(swarm)
        {
            return false;
        }
        let generation = hub.source_generation;
        let owner_epoch = hub.legacy_epochs.get("").copied().unwrap_or_default();
        drop(hub);
        for entry in org.0.pump.fleet() {
            if entry.computer.is_local() && !entry.local_sessions {
                continue;
            }
            let Some(meta) = online(org, &entry.computer) else {
                continue;
            };
            let machine = meta.machine_id.unwrap();
            if self.seen.contains(&machine) {
                continue;
            }
            if self.seen.len() == MACHINES {
                self.report.problem(Code::Capacity);
                return false;
            }
            let (Some(name), Some(platform)) = (meta.host, meta.platform) else {
                continue;
            };
            if !matches!(platform.as_str(), "linux" | "macos" | "windows") {
                continue;
            }
            self.seen.insert(machine.clone());
            let epoch = *org
                .lock()
                .legacy_epochs
                .entry(entry.computer.0.clone())
                .or_default();
            let target = Target {
                epoch,
                owner_epoch,
                computer: entry.computer,
                machine,
                name,
                platform,
                owner: owner.into(),
                swarm: swarm.into(),
                generation,
            };
            if !target.current(org) {
                self.report.problem(Code::Changed);
                return true;
            }
            let existing = match self.report.settings.legacy_project_import(&target.machine) {
                Ok(Some(record)) if record.complete() => return true,
                Ok(record) => record,
                Err(error) => {
                    self.report.problem(store_error(error));
                    return true;
                }
            };
            if target.platform == "windows"
                && existing.is_none()
                && !self.imported.as_ref().unwrap().contains(&target.name)
                && !target
                    .computer
                    .alias()
                    .is_some_and(|a| self.imported.as_ref().unwrap().contains(a))
            {
                self.report.problem(Code::WindowsUnsupported);
                return true;
            }
            let help =
                zerus_client_core::worktrees::guard_probe(&target.read(Read::Machines).unwrap());
            self.job = Some(Job {
                target,
                stage: Stage::Command(Step::Help, help),
                revision: None,
                binding: None,
                rows: None,
                before: None,
                plan: None,
                ticket: None,
                pending: existing.is_some(),
                sent: false,
                last_read_digest: snapshot["digest"].as_str().unwrap_or_default().into(),
                read_failures: 0,
            });
            return true;
        }
        false
    }
    fn result(
        &mut self,
        org: &Organization,
        job: &mut Job,
        step: Step,
        result: Result<Exit, Code>,
    ) -> Result<(), Code> {
        // An Apply result (including transport loss) is never an admission.
        if matches!(step, Step::Apply) {
            job.queue(Step::After, Read::Swarm);
            return Ok(());
        }
        let exit = result?;
        let current = if matches!(step, Step::After | Step::Confirm) {
            job.target.root_current(org)
        } else {
            job.target.current(org)
        };
        if !current {
            return Err(Code::Changed);
        }
        match step {
            Step::Help => {
                zerus_client_core::worktrees::guard_supported(exit).map_err(|_| Code::Invalid)?;
                job.queue(Step::Config, Read::Machines);
            }
            Step::Config => {
                let config = decode(Read::Machines, exit)?;
                let revision = configuration(&config, &job.target.computer)?;
                let source = format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&json!([
                            org.0.hgs,
                            revision,
                            job.target.computer.0,
                            job.target.machine
                        ]))
                        .map_err(|_| Code::Invalid)?
                    )
                );
                let binding = Binding {
                    machine: job.target.machine.clone(),
                    node: job.target.owner.clone(),
                    swarm: job.target.swarm.clone(),
                    source,
                };
                if let Some(record) = self
                    .report
                    .settings
                    .legacy_project_import(&binding.machine)
                    .map_err(store_error)?
                {
                    if record.complete() {
                        return Ok(());
                    }
                    if record.binding() != &binding {
                        return Err(Code::Changed);
                    }
                    job.pending = true;
                }
                job.binding = Some(binding);
                job.revision = Some(revision);
                let imported = self.imported.as_ref().unwrap();
                if job.pending {
                    job.queue(Step::After, Read::Swarm);
                } else if imported.contains(&job.target.name)
                    || job
                        .target
                        .computer
                        .alias()
                        .is_some_and(|a| imported.contains(a))
                {
                    job.rows = Some(json!([]));
                    job.queue(Step::Before, Read::Swarm);
                } else {
                    job.queue(Step::Projects, Read::Projects);
                }
            }
            Step::Projects => {
                job.rows = Some(decode(Read::Projects, exit)?);
                job.queue(Step::Before, Read::Swarm);
            }
            Step::Before => {
                let snapshot = decode(Read::Swarm, exit)?;
                if !cache(org).is_some_and(|s| same_source(&s, &snapshot)) {
                    return Err(Code::Changed);
                }
                let plan = model::plan(
                    &snapshot,
                    job.binding.clone().ok_or(Code::Invalid)?,
                    &job.target.name,
                    job.rows.as_ref().ok_or(Code::Invalid)?,
                    zerus_client_core::paths::Style::Posix,
                )?;
                let mut hub = org.lock();
                if !hub.sync.observed_value().is_some_and(|v| {
                    v["digest"] == snapshot["digest"]
                        && v["organization"] == snapshot["organization"]
                }) {
                    return Err(Code::Changed);
                }
                let revision = hub.sync.revision();
                let ticket = hub
                    .sync
                    .begin_external(revision)
                    .map_err(|_| Code::Changed)?;
                job.ticket = Some(ticket);
                job.before = Some(snapshot);
                drop(hub);
                // A refusal or uncertain flush retains the original reservation;
                // it NEVER grants dispatch. Recheck after this slow operation.
                let admitted = self
                    .report
                    .settings
                    .prepare_legacy_project_import(&plan)
                    .map_err(store_error)?;
                job.plan = Some(plan);
                if !admitted {
                    job.pending = true;
                    job.queue(Step::After, Read::Swarm);
                } else {
                    job.queue(Step::Check, Read::Machines);
                }
            }
            Step::Check => {
                if configuration(&decode(Read::Machines, exit)?, &job.target.computer)?
                    != *job.revision.as_ref().ok_or(Code::Invalid)?
                {
                    return Err(Code::Changed);
                }
                if let Some(source) = &self.captured {
                    source
                        .recheck(Arc::new(()), &AtomicBool::new(false))
                        .map_err(|_| Code::Changed)?;
                } else if self
                    .qt
                    .as_ref()
                    .unwrap()
                    .capture_for_migration(Arc::new(()), &AtomicBool::new(false))
                    .map_err(|_| Code::Changed)?
                    .is_some()
                {
                    return Err(Code::Changed);
                }
                if !job.target.current(org) {
                    return Err(Code::Changed);
                }
                let plan = job.plan.as_ref().ok_or(Code::Invalid)?;
                if !cache(org).is_some_and(|s| same_source(&s, job.before.as_ref().unwrap())) {
                    return Err(Code::Changed);
                }
                if plan.changed() {
                    let mut invocation = sync::invocation(plan.input());
                    zerus_client_core::worktrees::guard(&mut invocation.args, &job.target.owner);
                    job.stage = Stage::Command(Step::Apply, invocation);
                } else {
                    job.queue(Step::After, Read::Swarm);
                }
            }
            Step::After => {
                let value = decode(Read::Swarm, exit)?;
                let binding = job.binding.as_ref().ok_or(Code::Invalid)?;
                if value["node_id"] != binding.node
                    || value["swarm_id"] != binding.swarm
                    || value["initialized"] != true
                {
                    return Err(Code::Changed);
                }
                job.stage = Stage::Source(value);
            }
            Step::Confirm => {
                let same_route =
                    configuration(&decode(Read::Machines, exit)?, &job.target.computer)
                        .is_ok_and(|revision| job.revision.as_ref() == Some(&revision))
                        && job.target.identity_current(org);
                if !same_route {
                    self.report.problem(Code::Changed);
                }
                let value = job.rows.as_ref().ok_or(Code::Invalid)?;
                let (generation, source) = {
                    let hub = org.lock();
                    (hub.source_generation, hub.source.clone())
                };
                if !source
                    .and_then(|s| s())
                    .is_some_and(|s| same_source(&s, value))
                {
                    job.queue(Step::After, Read::Swarm);
                    return Ok(());
                }
                if let Some(ticket) = job.ticket.as_ref() {
                    let observation =
                        Observation::from_reply(value.clone()).ok_or(Code::Invalid)?;
                    let mut hub = org.lock();
                    if hub.source_generation != generation {
                        return Err(Code::Changed);
                    }
                    // Releasing the catalog lane after a drained, refused call
                    // is distinct from adopting its durable import evidence.
                    // A changed route never completes or replays that record.
                    hub.sync
                        .finish_external(ticket, observation, org.now())
                        .map_err(|_| Code::Changed)?;
                    hub.source_generation =
                        hub.source_generation.checked_add(1).ok_or(Code::Changed)?;
                    hub.legacy_read_blocked = false;
                    hub.shown = None;
                    org.publish(&mut hub);
                    org.release(hub);
                    job.ticket = None;
                }
                if same_route {
                    self.report
                        .settings
                        .observe_legacy_project_import(
                            job.binding.as_ref().ok_or(Code::Invalid)?,
                            value,
                        )
                        .map_err(store_error)?;
                }
            }
            Step::Apply => unreachable!(),
        }
        Ok(())
    }
}
impl Job {
    fn command(&self, step: Step, read: Read) -> Stage {
        self.target
            .read(read)
            .map_or(Stage::Done, |inv| Stage::Command(step, inv))
    }
    fn queue(&mut self, step: Step, read: Read) {
        self.stage = self.command(step, read);
    }
}

#[cfg(all(test, unix))]
impl Context {
    pub(super) fn draining_for_test(&self) -> bool {
        self.job.as_ref().is_some_and(|job| matches!(&job.stage, Stage::Draining { result: Err(Code::Capacity), resources, .. } if resources.upgrade().is_some()))
    }
    pub(super) fn idle_for_test(&self) -> bool {
        self.imported.is_some() && self.job.is_none() && !self.seen.is_empty()
    }
}

#[cfg(all(test, unix))]
mod retry_tests {
    use super::*;
    #[test]
    fn fresh_read_retries_back_off_and_stay_capped_without_overflow() {
        assert_eq!(
            (1..=6).map(read_delay).collect::<Vec<_>>(),
            vec![250, 1_000, 3_000, 10_000, 30_000, 30_000]
        );
        assert_eq!(read_delay(u8::MAX), 30_000);
    }
}
