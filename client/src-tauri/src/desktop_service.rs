//! One read-only desktop subscriber. No daemon launch, legacy fallback or writer admission.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
        Arc, Mutex, OnceLock, PoisonError, Weak,
    },
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    sync::{oneshot, watch, Notify},
};
use zerus_client_core::{
    desktop::{
        drain::{Gate, Role},
        service::{self, CatalogPage, ChangePage, Revision, Snapshot},
    },
    Change, ComputerId, ComputerStatus,
};

const RPC_WAIT: Duration = Duration::from_secs(5);
const HELP: &str = "Desktop service ABI: desktop-v1";
const SOURCES: usize = 64;
const KEY_BYTES: usize = 32 << 20;
const CATALOG_KEYS: usize = 1_000_000;

mod catalog;
mod catalog_relay;
use catalog::catalog_loop;
mod details;
pub mod frontend;

/// Registered at setup before start_source. Some contains a verified service
/// watch frame; None invalidates the prior projection. Callbacks must not block.
pub type Projection = Arc<dyn Fn(Option<&Value>) + Send + Sync>;

/// Captured before startup exclusion and before any producer is constructed.
pub fn mode() -> bool {
    static MODE: OnceLock<bool> = OnceLock::new();
    *MODE.get_or_init(|| service::enabled(std::env::var(service::MODE_ENV).ok().as_deref()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Retry(u64),
    Stop,
}

#[derive(Default)]
struct Dirty {
    sources: BTreeMap<String, SourceVersion>,
    heads: BTreeMap<String, Revision>,
    progress: BTreeMap<String, Revision>,
    pending: BTreeSet<String>,
    halted: BTreeSet<String>,
    running: Option<String>,
    next: u64,
}
impl Dirty {
    fn restart(&mut self, computer: &str) -> Result<Option<SourceVersion>, &'static str> {
        let Some(source) = self.sources.get_mut(computer) else {
            return Ok(None);
        };
        self.next = self
            .next
            .checked_add(1)
            .ok_or("source_generation_exhausted")?;
        source.serial = self.next;
        self.heads.remove(computer);
        self.progress.remove(computer);
        self.halted.remove(computer);
        self.pending.insert(computer.into());
        Ok(Some(source.clone()))
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
struct SourceVersion {
    incarnation: u64,
    generation: u64,
    projection: Option<String>,
    online: bool,
    serial: u64,
}
impl SourceVersion {
    fn from(value: &Value) -> Self {
        Self {
            incarnation: value["incarnation"].as_u64().unwrap_or(0),
            generation: value["generation"].as_u64().unwrap_or(0),
            projection: value["projection"].as_str().map(str::to_owned),
            online: value["online"] == true,
            serial: 0,
        }
    }
    fn identity(&self) -> service::CatalogIdentity {
        service::CatalogIdentity {
            incarnation: self.incarnation,
            generation: self.generation,
            projection: self.projection.clone(),
        }
    }
    fn matches(&self, current: &Self) -> bool {
        self.serial == current.serial
            && self.incarnation == current.incarnation
            && self.generation == current.generation
            && self.online == current.online
            && self
                .projection
                .as_ref()
                .is_none_or(|p| current.projection.as_ref() == Some(p))
    }
}

pub enum Data {
    Change(Box<Change>),
    Catalog(CatalogDelivery),
    Detail(details::Delivery),
}

/// Recheck identity at the view drain, not just when an RPC finishes. Frames
/// already queued before a removal, reconnect or new generation cannot reappear.
pub struct CatalogDelivery {
    shared: Weak<Shared>,
    computer: String,
    epoch: String,
    source: SourceVersion,
    stopped: Arc<AtomicBool>,
    value: CatalogValue,
}
enum CatalogValue {
    Row(Box<Change>),
    Finish {
        through: Option<Revision>,
        keys: Option<BTreeSet<String>>,
        done: oneshot::Sender<()>,
    },
}
impl CatalogDelivery {
    pub fn apply(self, pump: &crate::pump::Pump, observe: &impl Fn(&Change)) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        // Same lock order as Shared::current. Hold admission through the push so
        // an invalidation cannot race the last membership decision.
        let snapshot = shared
            .snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut dirty = shared.dirty.lock().unwrap_or_else(PoisonError::into_inner);
        if self.stopped.load(Ordering::Acquire)
            || shared.stopped.load(Ordering::Acquire)
            || snapshot.epoch.as_deref() != Some(&self.epoch)
            || dirty.sources.get(&self.computer) != Some(&self.source)
        {
            return;
        }
        match self.value {
            CatalogValue::Row(change) => {
                observe(&change);
                pump.push(*change);
            }
            CatalogValue::Finish {
                through,
                keys,
                done,
            } => {
                // Only baseline PLUS a fully consumed contiguous delta walk
                // establishes membership, even when a newer hint is pending.
                let clean = through.is_some_and(|through| {
                    dirty
                        .heads
                        .get(&self.computer)
                        .is_none_or(|head| *head <= through)
                });
                if let Some(through) = through {
                    dirty.progress.insert(self.computer.clone(), through);
                }
                if clean || through.is_none() {
                    dirty.pending.remove(&self.computer);
                } else {
                    dirty.pending.insert(self.computer.clone());
                    shared.changed.notify_one();
                }
                let computer = ComputerId(self.computer);
                if let (Some(keys), Some(_)) = (keys, through) {
                    pump.service_catalog_finish(&computer, &keys);
                }
                pump.push(Change::Computer {
                    computer,
                    // A contiguous delta range remains live even when a newer
                    // head needs another catch-up. Pending display changes do
                    // not mean this computer lost its connection.
                    status: if through.is_some() {
                        ComputerStatus::Live
                    } else {
                        ComputerStatus::Reconnecting { attempt: 0 }
                    },
                });
                let _ = done.send(());
            }
        }
    }
}

struct Shared {
    hgs: PathBuf,
    relay: Mutex<Option<Arc<zerus_client_core::desktop::relay::Connection>>>,
    pump: crate::pump::Pump,
    sink: Mutex<Option<SyncSender<Data>>>,
    snapshot: Mutex<Snapshot>,
    dirty: Mutex<Dirty>,
    changed: Notify,
    rpc: Gate,
    allowed: bool,
    stopped: AtomicBool,
    cancellation: Mutex<Option<Arc<AtomicBool>>>,
    responder: Option<Arc<zerus_client_core::desktop::Responder>>,
    reads: Mutex<BTreeMap<String, ReadIntent>>,
    details: Mutex<details::Hub>,
    details_changed: Notify,
    organization: Mutex<Option<zerus_client_core::OrganizationSnapshot>>,
    frontend: frontend::Frontend,
    projections: Vec<Projection>,
}

struct ReadIntent {
    id: String,
    in_flight: bool,
    uncertain: bool,
}

pub struct Service {
    shared: Arc<Shared>,
    control: watch::Sender<Control>,
    join: Mutex<Option<oneshot::Receiver<()>>>,
    drain: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Service {
    pub fn start(
        hgs: PathBuf,
        pump: crate::pump::Pump,
        sink: SyncSender<Data>,
        allowed: bool,
        drain: std::thread::JoinHandle<()>,
        responder: Option<Arc<zerus_client_core::desktop::Responder>>,
        projections: Vec<Projection>,
    ) -> Arc<Self> {
        let shared = Arc::new(Shared {
            hgs,
            relay: Mutex::new(None),
            pump,
            sink: Mutex::new(Some(sink)),
            snapshot: Mutex::new(Snapshot::default()),
            dirty: Mutex::new(Dirty::default()),
            changed: Notify::new(),
            rpc: Gate::new(4).unwrap(),
            allowed,
            stopped: AtomicBool::new(false),
            cancellation: Mutex::new(None),
            responder,
            reads: Mutex::new(BTreeMap::new()),
            details: Mutex::new(details::Hub::default()),
            details_changed: Notify::new(),
            organization: Mutex::new(None),
            frontend: frontend::Frontend::default(),
            projections,
        });
        let (control, receiver) = watch::channel(Control::Retry(0));
        let (finished, join) = oneshot::channel();
        let service = Arc::new(Self {
            shared: shared.clone(),
            control,
            join: Mutex::new(Some(join)),
            drain: Mutex::new(Some(drain)),
        });
        shared.publish(Snapshot::default());
        tauri::async_runtime::spawn(async move {
            run(shared, receiver).await;
            let _ = finished.send(());
        });
        service
    }
    pub fn snapshot(&self) -> Snapshot {
        self.shared
            .snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub fn retry(&self) {
        if self.shared.stopped.load(Ordering::Acquire) {
            return;
        }
        if let Some(cancel) = self
            .shared
            .cancellation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            cancel.store(true, Ordering::Release);
        }
        self.control.send_modify(|control| {
            if let Control::Retry(n) = control {
                *control = n.checked_add(1).map_or(Control::Stop, Control::Retry);
            }
        });
    }
    /// Nudge a non-live catalog, preserving healthy and already-pending reads.
    /// The local Refresh entry point also reaches halted peers: service mode
    /// has no legacy GUI peer listing for the tray/UI Refresh loops to visit.
    pub fn reconnect(&self, computer: &str) -> bool {
        if self.shared.stopped.load(Ordering::Acquire) {
            return false;
        }
        let snapshot = self
            .shared
            .snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !snapshot.connected {
            drop(snapshot);
            self.retry();
            return true;
        }
        let mut dirty = self
            .shared
            .dirty
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let targets: Vec<_> = dirty
            .sources
            .keys()
            .filter(|target| {
                (computer.is_empty() || target.as_str() == computer)
                    && !dirty.progress.contains_key(*target)
                    && !dirty.pending.contains(*target)
                    && dirty.running.as_ref() != Some(*target)
            })
            .cloned()
            .collect();
        let mut changed = false;
        for target in targets {
            changed |= matches!(dirty.restart(&target), Ok(Some(_)));
        }
        if changed {
            self.shared.changed.notify_one();
        }
        changed
    }
    pub async fn read_marks(
        &self,
        marks: Vec<zerus_client_core::readmarks::ReadMark>,
    ) -> Result<Vec<zerus_client_core::readmarks::ReadMark>, String> {
        if marks.len() > service::CATALOG_ROWS {
            return Err("Select at most 128 read marks at once.".into());
        }
        let mut committed = Vec::new();
        for mark in marks {
            if !self.snapshot().component_ready("reads") {
                return Err(
                    "Read marks are unavailable until the background service is ready.".into(),
                );
            }
            let key = serde_json::to_string(&mark).map_err(|_| "Invalid read intent.")?;
            if key.len() > zerus_watch_protocol::CONTROL_LINE_BYTES / 2 || mark.is_empty() {
                return Err("Invalid read intent.".into());
            }
            let (id, uncertain) = {
                let mut reads = self
                    .shared
                    .reads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if reads.len() >= 256 && !reads.contains_key(&key) {
                    return Err(
                        "Too many unconfirmed read intents. Retry after the service reconnects."
                            .into(),
                    );
                }
                let intent = reads.entry(key.clone()).or_insert_with(|| ReadIntent {
                    id: uuid::Uuid::new_v4().to_string(),
                    in_flight: false,
                    uncertain: false,
                });
                if intent.in_flight {
                    return Err("This read intent is already being submitted.".into());
                }
                intent.in_flight = true;
                (intent.id.clone(), intent.uncertain)
            };
            let stop = self
                .shared
                .cancellation
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .ok_or("Background service is unavailable.")?;
            let result = async {
                if uncertain {
                    let query = uuid::Uuid::new_v4().to_string();
                    let receipt = request(
                        &self.shared,
                        json!({"op":"delivery_result","request_id":query,"original_request_id":id}),
                        stop.clone(),
                    )
                    .await?;
                    if receipt["request_id"] != query
                        || receipt["op"] != "delivery_result"
                        || receipt["original_request_id"] != id
                        || receipt["accepted"] != true
                    {
                        return Err("invalid_service_receipt");
                    }
                    if service::read_committed(&receipt["result"], &id) {
                        return Ok(());
                    }
                    if receipt["found"] != false {
                        return Err("read_commit_unconfirmed");
                    }
                }
                // Reissuing a read uses its SAME durable UUID, including after
                // an uncertain result. Policy-setting requests never use this lane.
                let receipt = request(
                    &self.shared,
                    json!({"op":"read_intent","request_id":id,"mark":mark}),
                    stop,
                )
                .await?;
                if service::read_committed(&receipt, &id) {
                    Ok(())
                } else {
                    Err("read_commit_unconfirmed")
                }
            }
            .await;
            {
                let mut reads = self
                    .shared
                    .reads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if result.is_ok() {
                    reads.remove(&key);
                } else if let Some(intent) = reads.get_mut(&key) {
                    intent.in_flight = false;
                    intent.uncertain = true;
                }
            }
            result.map_err(|_| "The background service has not confirmed this read mark. Retry keeps its request identity.".to_owned())?;
            committed.push(mark);
        }
        Ok(committed)
    }
    pub fn stop(&self) {
        self.shared.stopped.store(true, Ordering::Release);
        if let Some(cancel) = self
            .shared
            .cancellation
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            cancel.store(true, Ordering::Release);
        }
        self.shared
            .sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.shared.rpc.close();
        let _ = self.control.send(Control::Stop);
    }
    pub async fn join(&self) -> Result<(), String> {
        self.stop();
        let receiver = self
            .join
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(receiver) = receiver {
            receiver
                .await
                .map_err(|_| "The service connection did not stop.".to_owned())?;
        }
        let drain = self
            .drain
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(drain) = drain {
            join_view(drain, RPC_WAIT).await?;
        }
        let closed = self.shared.rpc.close();
        let drained = tauri::async_runtime::spawn_blocking(move || {
            closed.wait_until(std::time::Instant::now() + RPC_WAIT)
        })
        .await
        .map_err(|error| error.to_string())?;
        if drained != zerus_client_core::desktop::drain::Observation::Idle {
            return Err("The service connection still has unconfirmed helper work.".into());
        }
        Ok(())
    }
}
async fn join_view(drain: std::thread::JoinHandle<()>, wait: Duration) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + wait;
    while !drain.is_finished() {
        if tokio::time::Instant::now() >= deadline {
            return Err("The service view did not stop before the quit deadline.".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drain
        .join()
        .map_err(|_| "The service view did not stop.".to_owned())
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    fn publish(&self, snapshot: Snapshot) {
        *self.snapshot.lock().unwrap_or_else(PoisonError::into_inner) = snapshot.clone();
        self.pump.service(snapshot);
    }
    fn unavailable(&self, code: &str) {
        for projection in &self.projections {
            projection(None);
        }
        self.publish(Snapshot {
            code: code.into(),
            ..Snapshot::default()
        });
    }
    fn dirty(&self, computer: &str, source: SourceVersion) -> Result<bool, &'static str> {
        self.catalog_hint(computer, source, None, false)
    }
    fn catalog_hint(
        &self,
        computer: &str,
        mut source: SourceVersion,
        head: Option<Revision>,
        reset: bool,
    ) -> Result<bool, &'static str> {
        if !source.identity().valid() {
            return Err("invalid_catalog_scope");
        }
        let mut dirty = self.dirty.lock().unwrap_or_else(PoisonError::into_inner);
        if !dirty.sources.contains_key(computer) && dirty.sources.len() >= SOURCES {
            return Err("source_capacity");
        }
        let scope_changed = dirty.sources.get(computer).is_none_or(|old| {
            old.incarnation != source.incarnation
                || old.generation != source.generation
                || old.online != source.online
                || (old.projection.is_some() && old.projection != source.projection)
        });
        // Repeated hints (including incomplete/reset-required hints) cannot
        // spend another retry budget for the same failed source scope.
        if !scope_changed && dirty.halted.contains(computer) {
            return Ok(false);
        }
        let replaced = reset || scope_changed;
        if replaced {
            dirty.next = dirty
                .next
                .checked_add(1)
                .ok_or("source_generation_exhausted")?;
            source.serial = dirty.next;
            dirty.heads.remove(computer);
            dirty.progress.remove(computer);
            dirty.halted.remove(computer);
        } else {
            source.serial = dirty.sources[computer].serial;
        }
        dirty.sources.insert(computer.into(), source);
        if let Some(head) = head {
            dirty
                .heads
                .entry(computer.into())
                .and_modify(|old| *old = (*old).max(head))
                .or_insert(head);
        }
        let caught = dirty
            .progress
            .get(computer)
            .is_some_and(|since| dirty.heads.get(computer).is_none_or(|head| head <= since));
        if !caught {
            dirty.pending.insert(computer.into());
            self.changed.notify_one();
        }
        Ok(replaced)
    }
    fn current(&self, computer: &str, source: &SourceVersion, epoch: &str) -> bool {
        self.snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .epoch
            .as_deref()
            == Some(epoch)
            && self
                .dirty
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .sources
                .get(computer)
                .is_some_and(|current| source.matches(current))
    }
    fn invalidate(&self) -> Vec<String> {
        let mut dirty = self.dirty.lock().unwrap_or_else(PoisonError::into_inner);
        let computers = dirty.sources.keys().cloned().collect();
        dirty.heads.clear();
        dirty.progress.clear();
        dirty.sources.clear();
        dirty.pending.clear();
        dirty.halted.clear();
        dirty.running = None;
        computers
    }
}

async fn emit(shared: &Shared, change: Change, stop: &AtomicBool) -> Result<(), &'static str> {
    emit_data(shared, Data::Change(Box::new(change)), stop).await
}
async fn emit_data(
    shared: &Shared,
    mut change: Data,
    stop: &AtomicBool,
) -> Result<(), &'static str> {
    loop {
        if stop.load(Ordering::Acquire) || shared.stopped.load(Ordering::Acquire) {
            return Err("stopped");
        }
        let sink = shared
            .sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or("frontend_closed")?;
        match sink.try_send(change) {
            Ok(()) => return Ok(()),
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return Err("frontend_closed"),
            Err(std::sync::mpsc::TrySendError::Full(value)) => change = value,
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn command(
    shared: &Arc<Shared>,
    args: Vec<String>,
    stdin: Option<String>,
    stop: Arc<AtomicBool>,
) -> Result<Vec<u8>, &'static str> {
    if !shared.allowed {
        return Err("replay_transport_unavailable");
    }
    if shared.stopped.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
        return Err("stopped");
    }
    let charge = shared
        .rpc
        .admit(Role::Fleet)
        .map_err(|_| "service_transport_busy")?;
    let relay = shared
        .relay
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .ok_or("service_unavailable")?;
    let state = shared.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let hgs = relay.executable()?;
        if state.stopped.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
            return Err("stopped");
        }
        let inv = zerus_client_core::Invocation {
            args,
            stdin: stdin.map(String::into_bytes),
            timeout: RPC_WAIT,
            request_id: None,
        };
        let guard: Arc<dyn Send + Sync> = Arc::new(charge);
        match zerus_client_core::run::run_accounted(
            hgs,
            &inv,
            &[],
            service::FRAME_BYTES,
            &stop,
            guard,
        ) {
            Ok(zerus_client_core::Exit::Code {
                code: 0, stdout, ..
            }) => Ok(stdout),
            _ => Err("service_request_unconfirmed"),
        }
    })
    .await
    .map_err(|_| "service_request_unconfirmed")?
}

async fn request(
    shared: &Arc<Shared>,
    value: Value,
    stop: Arc<AtomicBool>,
) -> Result<Value, &'static str> {
    // This lane never acquires frontend approval-setting authority.
    if matches!(
        value["op"].as_str(),
        Some("automatic_approvals" | "maintenance_configure")
    ) {
        return Err("frontend_route_required");
    }
    let input = serde_json::to_string(&value).map_err(|_| "invalid_service_request")?;
    if input.len() > zerus_watch_protocol::CONTROL_LINE_BYTES {
        return Err("service_request_too_large");
    }
    let bytes = command(
        shared,
        vec!["desktop".into(), "request".into(), "--json".into()],
        Some(input),
        stop,
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid_service_response")
}

async fn preflight(shared: &Arc<Shared>, stop: Arc<AtomicBool>) -> Result<String, &'static str> {
    let help = command(shared, vec!["--help".into()], None, stop.clone()).await?;
    if !std::str::from_utf8(&help).ok().is_some_and(|help| {
        help.lines().any(|line| line.trim() == HELP)
            && help
                .lines()
                .any(|line| line.trim() == service::CHANGES_HELP)
            && help.lines().any(|line| line.trim() == service::RELAY_HELP)
    }) {
        return Err("incompatible_service_cli");
    }
    let bytes = command(
        shared,
        vec!["desktop".into(), "status".into(), "--json".into()],
        None,
        stop,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid_service_response")?;
    let configuration = value["configuration"]
        .as_str()
        .filter(|value| zerus_client_core::desktop::digest(value))
        .ok_or("incompatible_service")?;
    if value["catalog_reads"]["changes_abi"] != service::CHANGES_ABI {
        return Err("incompatible_catalog_changes");
    }
    let mut state = service::State::new(configuration.into())?;
    state.status(&value)?;
    if let Some(responder) = &shared.responder {
        responder.configure_once(configuration.into());
    }
    shared.publish(state.snapshot().clone());
    Ok(configuration.into())
}

async fn run(shared: Arc<Shared>, mut control: watch::Receiver<Control>) {
    loop {
        let requested = *control.borrow_and_update();
        if requested == Control::Stop || shared.stopped.load(Ordering::Acquire) {
            break;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *shared
            .cancellation
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(stop.clone());
        let result = connection(shared.clone(), &mut control, stop.clone()).await;
        *shared.relay.lock().unwrap_or_else(PoisonError::into_inner) = None;
        stop.store(true, Ordering::Release);
        let computers = shared.invalidate();
        let changes = shared
            .details
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .disconnect();
        *shared
            .organization
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        for change in changes {
            let _ = emit(&shared, change, &AtomicBool::new(false)).await;
        }
        shared.unavailable(result.err().unwrap_or("service_unavailable"));
        for computer in computers {
            let _ = emit(
                &shared,
                Change::Computer {
                    computer: ComputerId(computer),
                    status: ComputerStatus::Reconnecting { attempt: 0 },
                },
                &AtomicBool::new(false),
            )
            .await;
        }
        if *control.borrow() == Control::Stop {
            break;
        }
        // Retry is explicit and reuses this one worker after its children join.
        if *control.borrow() != requested {
            continue;
        }
        if control.changed().await.is_err() {
            break;
        }
    }
}

async fn connection(
    shared: Arc<Shared>,
    control: &mut watch::Receiver<Control>,
    stop: Arc<AtomicBool>,
) -> Result<(), &'static str> {
    let ordinary = shared.hgs.clone();
    let relay = Arc::new(
        tauri::async_runtime::spawn_blocking(move || {
            zerus_client_core::desktop::relay::Connection::select(&ordinary)
        })
        .await
        .map_err(|_| "paired_desktop_cli_unavailable")??,
    );
    if shared.stopped.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
        return Err("stopped");
    }
    *shared.relay.lock().unwrap_or_else(PoisonError::into_inner) = Some(relay.clone());
    let configuration = preflight(&shared, stop.clone()).await?;
    if shared.stopped.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
        return Err("stopped");
    }
    // Retain/join the synchronous native verifier off the UI/runtime actor,
    // including when a stop arrives. No child starts after that stop.
    let hgs = tauri::async_runtime::spawn_blocking(move || {
        relay.executable().map(std::path::Path::to_owned)
    })
    .await
    .map_err(|_| "paired_desktop_cli_unavailable")??;
    if shared.stopped.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
        return Err("stopped");
    }
    let mut command = tokio::process::Command::new(hgs);
    command
        .args(["desktop", "watch", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::child_environment::restore(command.as_std_mut());
    command.env_remove("HGS_SESSION").env_remove("HGS_RUN_ID");
    let mut child = command.spawn().map_err(|_| "service_unavailable")?;
    // Keep stdin open: EOF is the proxy's explicit detach instruction.
    let stdin = child.stdin.take();
    let mut stdout = child.stdout.take().ok_or("service_unavailable")?;
    let mut stderr = child.stderr.take().ok_or("service_unavailable")?;
    let mut state = service::State::new(configuration.clone())?;
    let mut buffer = Vec::new();
    let (mut out, mut err) = ([0u8; 8192], [0u8; 4096]);
    let (mut error_bytes, mut stderr_open) = (0usize, true);
    let catalog_shared = shared.clone();
    let catalog_stop = stop.clone();
    let mut catalogs = tauri::async_runtime::spawn(async move {
        catalog_loop(catalog_shared, catalog_stop, configuration).await
    });
    let detail_shared = shared.clone();
    let detail_stop = stop.clone();
    let mut detail_worker =
        tauri::async_runtime::spawn(async move { details::run(detail_shared, detail_stop).await });
    let mut last_frame = tokio::time::Instant::now();
    let result = async {
        loop {
            tokio::select! {
                biased;
                _ = control.changed() => return Err("service_reconnecting"),
                _ = tokio::time::sleep_until(last_frame + Duration::from_secs(30)) => return Err("service_stale"),
                value = &mut catalogs => return value.map_err(|_| "catalog_unavailable")?,
                value = &mut detail_worker => return value.map_err(|_| "details_unavailable")?,
                count = stdout.read(&mut out) => {
                    let count = count.map_err(|_| "service_unavailable")?;
                    if count == 0 { return Err("service_unavailable"); }
                    for part in out[..count].split_inclusive(|byte| *byte == b'\n') {
                        if buffer.len() + part.len() > service::FRAME_BYTES { return Err("service_frame_too_large"); }
                        buffer.extend_from_slice(part);
                        if !part.ends_with(b"\n") { continue; }
                        let frame: Value = serde_json::from_slice(&buffer).map_err(|_| "invalid_service_frame")?;
                        buffer.clear();
                        let update = state.frame(&frame)?;
                        details::frame(&shared,&frame,&stop).await?;
                        for projection in &shared.projections { projection(Some(&frame)); }
                        last_frame = tokio::time::Instant::now();
                        shared.publish(state.snapshot().clone());
                        match update {
                            service::Update::Status => {
                                if let Some(computers) = state.sources() {
                                    for computer in computers {
                                        // Generation zero has not greeted the service yet.
                                        if computer.generation == 0 { continue; }
                                        let name = &computer.computer;
                                        let known = shared.dirty.lock().unwrap_or_else(PoisonError::into_inner).sources.contains_key(name);
                                        if !known {
                                            shared.dirty(name, SourceVersion {incarnation:computer.incarnation,generation:computer.generation,
                                                online:computer.online,..SourceVersion::default()})?;
                                        }
                                        // This roster discovers sources. Catalog hints/pages
                                        // bind their scope and coverage; transport readiness
                                        // alone cannot repeatedly re-baseline cached rows.
                                        shared.details.lock().unwrap_or_else(PoisonError::into_inner).source(name,computer.generation)?;
                                        shared.details_changed.notify_one();
                                    }
                                }
                            }
                            service::Update::Catalog { computer, removed } => {
                                if removed {
                                    { let mut dirty = shared.dirty.lock().unwrap_or_else(PoisonError::into_inner); dirty.progress.remove(&computer); dirty.heads.remove(&computer); dirty.sources.remove(&computer); dirty.pending.remove(&computer); dirty.halted.remove(&computer); }
                                    emit(&shared, Change::Computer { computer: ComputerId(computer.clone()), status: ComputerStatus::Ended }, &stop).await?;
                                    emit(&shared, Change::Replace { computer: ComputerId(computer), rows: vec![] }, &stop).await?;
                                } else if shared.catalog_hint(&computer, SourceVersion::from(&frame["value"]),
                                    serde_json::from_value(frame["value"]["revision"].clone()).map_err(|_| "invalid_catalog_revision")?, frame["value"]["reset_required"] == true)? {
                                    emit(&shared, Change::Computer { computer: ComputerId(computer), status: ComputerStatus::Probing }, &stop).await?;
                                }
                            }
                            service::Update::Other => (),
                        }
                    }
                }
                count = stderr.read(&mut err), if stderr_open => {
                    let count = count.map_err(|_| "service_unavailable")?;
                    stderr_open = count != 0;
                    error_bytes += count;
                    if error_bytes > 64 << 10 { return Err("service_error_output_limit"); }
                }
            }
        }
    }.await;
    stop.store(true, Ordering::Release);
    shared.changed.notify_one();
    shared.details_changed.notify_one();
    drop(stdin);
    // Only this owned local proxy is stopped; the desktop service and native agents remain untouched.
    if child.kill().await.is_err() && child.wait().await.is_err() {
        shared.stopped.store(true, Ordering::Release);
        if !catalogs.inner().is_finished() {
            let _ = catalogs.await;
        }
        if !detail_worker.inner().is_finished() {
            let _ = detail_worker.await;
        }
        return Err("service_transport_unverified");
    }
    if !catalogs.inner().is_finished() {
        let _ = catalogs.await;
    }
    if !detail_worker.inner().is_finished() {
        let _ = detail_worker.await;
    }
    result
}

#[tauri::command]
pub fn desktop_service_mode() -> bool {
    mode()
}

#[tauri::command]
pub fn desktop_service_retry(
    source: tauri::State<'_, crate::commands::AppState>,
) -> Result<(), String> {
    if !mode() {
        return Err("Background service mode is not enabled.".into());
    }
    if let Some(service) = source.desktop_service() {
        service.retry();
    } else {
        source.start_source()?;
    }
    Ok(())
}

#[tauri::command]
pub async fn desktop_service_read_marks(
    source: tauri::State<'_, crate::commands::AppState>,
    marks: Vec<zerus_client_core::readmarks::ReadMark>,
) -> Result<Vec<zerus_client_core::readmarks::ReadMark>, String> {
    if !mode() {
        return Err("Background service mode is not enabled.".into());
    }
    let service = source
        .desktop_service()
        .ok_or("Background service is unavailable.")?;
    service.read_marks(marks).await
}

/// The existing confirmation calls the dedicated native frontend route.
#[tauri::command(rename_all = "snake_case")]
pub async fn desktop_service_automatic_approvals(
    source: tauri::State<'_, crate::commands::AppState>,
    request_id: String,
    enabled: bool,
) -> Result<Value, String> {
    frontend::approval_command(
        source.desktop_service().filter(|_| mode()),
        request_id,
        enabled,
    )
    .await
}

#[cfg(all(test, unix))]
mod tests;
