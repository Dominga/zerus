//! One watch-v1 fleet per service epoch. The actor alone applies its packets.
//! Source permits cover queued and consumer-held frames, independently of UI
//! speed. Workers belong to computers (at most 64), never individual sessions.
use super::{feed, store::Location};
use crate::machines::retry_delays::Loaded;
use crate::{config::Config, platform};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    process::{Child, Command},
    sync::Arc,
    time::{Duration, Instant},
};
use zerus_client_core::{source, ComputerId, ComputerStatus, Retry, Timing};
use zerus_watch_protocol::{topic, Body, Control};

const POLL_ITEMS: usize = 16;
/// The least time between two early connection publications.
pub const CONNECTIONS_GAP: Duration = Duration::from_millis(250);
/// The most computers one `fleet_reconnect` request names.
pub const RECONNECT_COMPUTERS: usize = source::SOURCES;
struct Computer {
    handle: source::Handle,
    wire_generation: u64,
    generation: u64,
    accepting: bool,
    status: ComputerStatus,
    /// The supervisor's restart schedule, display only.
    retry: Retry,
    seq: Option<u64>,
}

/// Retain this through Broker::apply, including its SQLite transaction. Dropping
/// it releases source ingress. Synthetic retirements hold no payload permits.
pub struct FleetEvent {
    pub packet: feed::Packet,
    _permit: Option<source::IngressPermit>,
}
pub struct Runtime {
    epoch: String,
    hgs: PathBuf,
    timing: Timing,
    sender: source::Sender,
    receiver: Option<source::Receiver>,
    computers: BTreeMap<ComputerId, Computer>,
    retiring: Vec<source::Handle>,
    pending: VecDeque<feed::Packet>,
    desired: BTreeSet<String>,
    next: u64,
    error: Option<&'static str>,
    stopping: bool,
    /// Changes whenever the connection presentation may have changed.
    connections_revision: u64,
    /// The configuration folder holding the saved reconnect timings.
    config_dir: Option<PathBuf>,
    /// Where the restart delays in use came from, and why a saved file is
    /// not used.
    retry_source: &'static str,
    retry_diagnostic: Option<String>,
    /// A schedule was read in this runtime (a saved file, or the defaults for
    /// a missing one); only then does an unusable file keep the one in use.
    retry_read: bool,
}

fn spawn(command: &mut Command) -> std::io::Result<Child> {
    platform::spawn_service_child(command)
}

impl Runtime {
    #[cfg(test)]
    pub(super) fn empty_for_test(epoch: &str) -> Self {
        let (sender, receiver) = source::channel_with_legacy(
            spawn,
            Arc::new(|| Err(std::io::Error::other("empty fixture never spools"))),
        );
        Self {
            epoch: epoch.into(),
            hgs: PathBuf::new(),
            timing: Timing::default(),
            sender,
            receiver: Some(receiver),
            computers: BTreeMap::new(),
            retiring: Vec::new(),
            pending: VecDeque::new(),
            desired: BTreeSet::new(),
            next: 0,
            error: None,
            stopping: false,
            connections_revision: 0,
            config_dir: None,
            retry_source: "default",
            retry_diagnostic: None,
            retry_read: false,
        }
    }
    /// Bind service projection identity to this original handle's wire epoch.
    pub fn controls(&self, computer: &str, generation: u64, controls: &[Control]) -> bool {
        self.computers
            .get(&ComputerId::from(computer))
            .is_some_and(|source| {
                !self.stopping
                    && source.accepting
                    && source.generation == generation
                    && source.handle.controls(source.wire_generation, controls)
            })
    }
    /// Called only after lifecycle ownership, configuration lock and epoch.
    /// No standalone/GUI activation route is provided by this module.
    pub fn start(config: &Config, epoch: &str) -> Result<Self> {
        let hgs = crate::self_exec::current()?;
        let location = Location::new(config)?;
        let root = location.root;
        let factory: source::SpoolFactory = Arc::new(move || {
            ensure_private(&root)?;
            let file = tempfile::NamedTempFile::new_in(&root)?;
            if !platform::verify_private_file(file.as_file())? {
                return Err(std::io::Error::other("unsafe fleet spool"));
            }
            Ok(Box::new(file))
        });
        let mut runtime = Self::start_with(hgs, epoch, factory, Timing::default())?;
        runtime.configure_retry_delays(config.dir.clone());
        Ok(runtime)
    }

    fn start_with(
        hgs: PathBuf,
        epoch: &str,
        factory: source::SpoolFactory,
        timing: Timing,
    ) -> Result<Self> {
        ensure!(
            !epoch.is_empty() && epoch.len() <= 128,
            "invalid fleet epoch"
        );
        let (sender, receiver) = source::channel_with_legacy(spawn, factory);
        let local = sender.spawn(&hgs, &ComputerId::local(), timing.clone())?;
        let mut computers = BTreeMap::new();
        computers.insert(
            ComputerId::local(),
            Computer {
                handle: local,
                wire_generation: 0,
                generation: 0,
                accepting: false,
                status: ComputerStatus::Probing,
                retry: Retry::Idle,
                seq: None,
            },
        );
        Ok(Self {
            epoch: epoch.into(),
            hgs,
            timing,
            sender,
            receiver: Some(receiver),
            computers,
            retiring: Vec::new(),
            pending: VecDeque::new(),
            desired: BTreeSet::new(),
            next: 0,
            error: None,
            stopping: false,
            connections_revision: 0,
            config_dir: None,
            retry_source: "default",
            retry_diagnostic: None,
            retry_read: false,
        })
    }

    /// Bounded and nonblocking. At most one admitted payload is handed to the
    /// actor; discarded stale/status events have a separate fixed work budget.
    pub fn poll(&mut self) -> Option<FleetEvent> {
        self.reap();
        if self.stopping {
            return None;
        }
        if let Some(packet) = self.pending.pop_front() {
            return Some(FleetEvent {
                packet,
                _permit: None,
            });
        }
        self.admit_peers();
        for _ in 0..POLL_ITEMS {
            let item = self.receiver.as_ref()?.recv_timeout(Duration::ZERO).ok()?;
            let (event, permit) = item.into_parts();
            if let Some(packet) = self.convert(event) {
                return Some(FleetEvent {
                    packet,
                    _permit: Some(permit),
                });
            }
        }
        None
    }

    fn convert(&mut self, event: source::Event) -> Option<feed::Packet> {
        let (identity, wire, starting) = match &event {
            source::Event::Frame {
                source,
                generation,
                frame,
                ..
            } => (source, *generation, matches!(frame.body, Body::Hello(_))),
            source::Event::Legacy {
                source,
                generation,
                update,
                ..
            } => (
                source,
                *generation,
                matches!(**update, source::Legacy::Begin { .. }),
            ),
            source::Event::Status {
                source, generation, ..
            }
            | source::Event::Retry {
                source, generation, ..
            } => (source, *generation, false),
        };
        let computer = self.computers.get_mut(&identity.computer)?;
        if computer.handle.identity() != identity {
            return None;
        }
        if wire < computer.wire_generation || (starting && wire == computer.wire_generation) {
            return None;
        }
        let stale = match &event {
            source::Event::Frame { received_at, .. } => {
                received_at.elapsed() >= self.timing.silence
            }
            source::Event::Legacy { observed_at, .. } => {
                observed_at.elapsed() >= self.timing.silence
            }
            _ => false,
        };
        if stale {
            computer.handle.resnapshot(wire);
            let was_accepting = std::mem::replace(&mut computer.accepting, false);
            self.error = Some("fleet_resnapshot_required");
            return was_accepting.then(|| feed::Packet::Offline {
                computer: identity.computer.0.clone(),
                generation: computer.generation,
            });
        }
        if starting {
            if wire <= computer.wire_generation {
                return None;
            }
            let Some(next) = self.next.checked_add(1).filter(|n| *n <= i64::MAX as u64) else {
                self.error = Some("fleet_generation_exhausted");
                computer.handle.request_stop();
                return None;
            };
            self.next = next;
            computer.wire_generation = wire;
            computer.generation = next;
            computer.accepting = true;
            computer.seq = None;
            if matches!(
                self.error,
                Some("fleet_projection_refused" | "fleet_resnapshot_required")
            ) {
                self.error = None;
            }
        } else if wire < computer.wire_generation {
            return None;
        }
        let generation = computer.generation;
        let alias = identity.computer.0.clone();
        if let source::Event::Retry { retry, .. } = &event {
            // Display only: the exact source's schedule, never coverage.
            if computer.retry != *retry {
                computer.retry = *retry;
                self.connections_revision += 1;
            }
            return None;
        }
        if let source::Event::Status { status, .. } = &event {
            if computer.status != *status {
                self.connections_revision += 1;
            }
            computer.status = *status;
            if !matches!(status, ComputerStatus::Live | ComputerStatus::Polled)
                && computer.accepting
            {
                computer.accepting = false;
                return Some(feed::Packet::Offline {
                    computer: alias,
                    generation,
                });
            }
            return None;
        }
        if !computer.accepting || wire != computer.wire_generation {
            return None;
        }
        if let source::Event::Frame { frame, .. } = &event {
            let hole = computer.seq.is_some_and(|seq| frame.seq != seq + 1);
            let invalidated = matches!(&frame.body, Body::Gap(gap) if gap.topic == topic::CATALOG)
                || matches!(&frame.body, Body::Error(error) if error.fatal || error.topic.as_deref() == Some(topic::CATALOG));
            if hole || invalidated {
                computer.accepting = false;
                computer.handle.resnapshot(wire);
                self.error = Some("fleet_resnapshot_required");
                return Some(feed::Packet::Offline {
                    computer: alias,
                    generation,
                });
            }
            computer.seq = Some(frame.seq);
        }
        // Only the selected local stream may change the configured alias set.
        // Its host/machine_id are observations, never aliases or write authority.
        if identity.computer.is_local() {
            let metadata = match &event {
                source::Event::Frame { frame, .. } => match &frame.body {
                    Body::Snapshot(s) if s.topic == topic::CATALOG_META => s.value.as_ref(),
                    _ => None,
                },
                // Legacy metadata is provisional until Complete; peers are
                // adopted from a later watch/meta rather than partial listings.
                _ => None,
            };
            if let Some(peers) = metadata.and_then(|v| v["peers"].as_array()) {
                self.set_peers(peers);
            }
        }
        Some(match event {
            source::Event::Frame { frame, .. } => feed::Packet::Watch {
                computer: alias,
                generation,
                frame,
            },
            source::Event::Legacy { update, .. } => match *update {
                source::Legacy::Begin {
                    metadata,
                    rows,
                    pages,
                    ..
                } => feed::Packet::LegacyBegin {
                    computer: alias,
                    generation,
                    metadata,
                    rows,
                    pages,
                },
                source::Legacy::Page { index, items } => feed::Packet::LegacyPage {
                    computer: alias,
                    generation,
                    index,
                    items,
                },
                source::Legacy::Complete => feed::Packet::LegacyComplete {
                    computer: alias,
                    generation,
                },
            },
            _ => return None,
        })
    }

    fn set_peers(&mut self, peers: &[Value]) {
        let mut desired = BTreeSet::new();
        for value in peers {
            let Some(alias) = value
                .as_str()
                .filter(|alias| zerus_client_core::fleet::valid_alias(alias))
            else {
                self.error = Some("invalid_fleet_peer");
                return;
            };
            if desired.len() == source::SOURCES - 1 && !desired.contains(alias) {
                self.error = Some("fleet_source_capacity");
                return;
            }
            desired.insert(alias.to_owned());
        }
        self.desired = desired;
        let removed: Vec<_> = self
            .computers
            .keys()
            .filter(|id| !id.is_local() && !self.desired.contains(&id.0))
            .cloned()
            .collect();
        for id in removed {
            let old = self.computers.remove(&id).expect("selected computer");
            self.connections_revision += 1;
            old.handle.request_stop();
            if old.generation > 0 {
                self.queue_retirement(feed::Packet::Remove {
                    computer: id.0,
                    generation: old.generation,
                });
            }
            self.retiring.push(old.handle);
        }
        self.error = None;
    }

    fn admit_peers(&mut self) {
        for alias in &self.desired {
            let id = ComputerId(alias.clone());
            if self.computers.contains_key(&id) {
                continue;
            }
            // source::Sender additionally charges pending, cancelled workers.
            let Ok(handle) = self.sender.spawn(&self.hgs, &id, self.timing.clone()) else {
                break;
            };
            self.computers.insert(
                id,
                Computer {
                    handle,
                    wire_generation: 0,
                    generation: 0,
                    accepting: false,
                    status: ComputerStatus::Probing,
                    retry: Retry::Idle,
                    seq: None,
                },
            );
            self.connections_revision += 1;
        }
    }

    /// A failed actor apply retires only that exact source. Queued frames of
    /// its old generation cannot restore coverage while resnapshot reconnects.
    pub fn reject(&mut self, event: &FleetEvent) {
        let Some((alias, generation)) = event.packet.scope() else {
            return;
        };
        if matches!(event.packet, feed::Packet::Remove { .. }) {
            self.error = Some("fleet_removal_refused");
            self.queue_retirement(feed::Packet::Offline {
                computer: alias.into(),
                generation,
            });
            return;
        }
        let Some(computer) = self.computers.get_mut(&ComputerId(alias.into())) else {
            return;
        };
        if computer.generation != generation || !computer.accepting {
            return;
        }
        computer.accepting = false;
        computer.handle.resnapshot(computer.wire_generation);
        self.queue_retirement(feed::Packet::Offline {
            computer: alias.into(),
            generation,
        });
        self.error = Some("fleet_projection_refused");
    }

    fn queue_retirement(&mut self, packet: feed::Packet) {
        if let Some(old) = self
            .pending
            .iter_mut()
            .find(|old| old.scope() == packet.scope())
        {
            if matches!(packet, feed::Packet::Remove { .. }) {
                *old = packet;
            }
        } else if self.pending.len() < source::SOURCES {
            self.pending.push_back(packet);
        } else {
            self.error = Some("fleet_retirement_capacity");
        }
    }

    fn reap(&mut self) {
        self.retiring
            .retain_mut(|handle| !handle.join_if_finished());
    }

    /// Nonblocking cancellation plus a join of already finished supervisors.
    /// Call again while draining; false explicitly means shutdown is incomplete.
    pub fn stop(&mut self) -> bool {
        if !self.stopping {
            self.stopping = true;
            self.receiver.take(); // Cancels all original stream endpoints.
            self.pending.clear();
            for (_, computer) in std::mem::take(&mut self.computers) {
                computer.handle.request_stop();
                self.retiring.push(computer.handle);
            }
        }
        self.reap();
        let usage = self.sender.usage();
        self.retiring.is_empty()
            && usage.workers == 0
            && usage.readers == 0
            && usage.connecting == 0
            && usage.items == 0
            && usage.legacy_spools == 0
            && usage.controls == 0
    }

    /// Configured computers with their display state and restart schedule.
    /// `remaining_ms` is measured now from the supervisor's own monotonic
    /// deadline. Display only: no source generation or writer authority.
    fn connection_rows(&self, now: Instant) -> Vec<Value> {
        std::iter::once("")
            .chain(self.desired.iter().map(String::as_str))
            .map(|id| {
                let c = self.computers.get(&ComputerId(id.into()));
                let status = c.map(|c| c.status).unwrap_or(ComputerStatus::Probing);
                let online = c.is_some_and(|c| c.accepting)
                    && matches!(status, ComputerStatus::Live | ComputerStatus::Polled);
                let label =
                    if !online && matches!(status, ComputerStatus::Live | ComputerStatus::Polled) {
                        "reconnecting"
                    } else {
                        status.name()
                    };
                let mut row = json!({"id":id,"status":label,"attempt":status.attempt()});
                if let Some(c) = c {
                    row["incarnation"] = c.handle.identity().incarnation.into();
                    row["retry"] = retry_value(c.retry, now);
                }
                row
            })
            .collect()
    }

    /// Changes whenever a computer's status, schedule or membership changed.
    pub fn connections_revision(&self) -> u64 {
        self.connections_revision
    }

    /// The bounded connection presentation an early `fleet_connections`
    /// frame carries, in the shape the status `background_runtime` has.
    pub fn connections(&self) -> Value {
        json!({"epoch":self.epoch,
            "state":if self.stopping {"stopping"} else {"running"},
            "display_only":true,"authority":false,
            "connections":self.connection_rows(Instant::now()),
            "retry_delays":self.retry_delays()})
    }

    /// Ends the restart wait of each named computer's existing supervisor.
    /// Never starts, replaces or restarts a source, the service or an agent;
    /// rows, follows and generations stay. `incarnation` binds the request to
    /// the source the person saw. Codes: `requested`, `pending` (already
    /// requested, attempting or not waiting), `connected`, `not_configured`,
    /// `not_admitted`, `source_changed` and `stopping`.
    pub fn reconnect(&self, computer: &str, incarnation: Option<u64>) -> &'static str {
        if self.stopping {
            return "stopping";
        }
        let Some(c) = self.computers.get(&ComputerId(computer.into())) else {
            return if computer.is_empty() || self.desired.contains(computer) {
                "not_admitted"
            } else {
                "not_configured"
            };
        };
        if incarnation.is_some_and(|expected| expected != c.handle.identity().incarnation) {
            return "source_changed";
        }
        if c.accepting && matches!(c.status, ComputerStatus::Live | ComputerStatus::Polled) {
            return "connected";
        }
        if c.handle.reconnect() {
            "requested"
        } else {
            "pending"
        }
    }

    /// Replaces the restart delays of every supervisor of this runtime,
    /// including peers admitted later. Each applies them to its next wait.
    pub fn set_restart_delays(&mut self, delays: [Duration; 4]) {
        if self.restart_delays() != delays {
            self.timing.restart.set(delays);
            self.connections_revision += 1;
        }
    }

    /// The restart delays the next wait of every supervisor uses.
    fn restart_delays(&self) -> [Duration; 4] {
        self.timing.restart.get()
    }

    /// The cold start's timings: the saved file in `dir`, applied from the
    /// first wait. With no file, or one that cannot be used (reported), the
    /// defaults apply explicitly, whatever timing the runtime began with; the
    /// service always starts.
    fn configure_retry_delays(&mut self, dir: PathBuf) -> Value {
        self.config_dir = Some(dir);
        self.retry_read = false;
        self.reload_retry_delays()
    }

    /// Reads the saved reconnect timings again (`hgs machine retry-delays`).
    /// A missing file restores the defaults. An unreadable or invalid file is
    /// reported and keeps the last schedule read; before any was read, the
    /// defaults apply. The file is never changed.
    pub fn reload_retry_delays(&mut self) -> Value {
        let Some(loaded) = self
            .config_dir
            .as_deref()
            .map(crate::machines::retry_delays::load)
        else {
            return self.retry_delays();
        };
        match loaded {
            Loaded::Invalid(message) if self.retry_read => {
                self.retry_source = "kept";
                self.retry_diagnostic = Some(message);
            }
            Loaded::Invalid(message) => {
                self.retry_source = "default";
                self.retry_diagnostic = Some(message);
                self.set_restart_delays(Loaded::Missing.delays_or_default());
            }
            loaded => {
                self.retry_source = if loaded == Loaded::Missing {
                    "default"
                } else {
                    "file"
                };
                self.retry_diagnostic = None;
                self.retry_read = true;
                self.set_restart_delays(loaded.delays_or_default());
            }
        }
        self.connections_revision += 1;
        self.retry_delays()
    }

    /// The restart delays in use, in seconds, with their source: `default`,
    /// `file`, or `kept` (the last schedule read) when a saved file could not
    /// be used; `diagnostic` says why a saved file is not used.
    pub fn retry_delays(&self) -> Value {
        json!({"delays_seconds":self.restart_delays().map(|d| d.as_secs()),
            "source":self.retry_source,"diagnostic":self.retry_diagnostic})
    }

    pub fn status(&self) -> Value {
        let u = self.sender.usage();
        let drained = self.stopping
            && self.retiring.is_empty()
            && u.workers == 0
            && u.readers == 0
            && u.connecting == 0
            && u.items == 0
            && u.legacy_spools == 0;
        let drained = drained && u.controls == 0;
        // Fleet availability is per computer. One unavailable peer must not
        // degrade a healthy stream; complete coverage remains separate data.
        // Neither value supplies source identity or permission for a write.
        let ready = !self.stopping
            && self.computers.values().any(|c| {
                c.accepting && matches!(c.status, ComputerStatus::Live | ComputerStatus::Polled)
            });
        let all_sources_ready = !self.stopping
            && self.error.is_none()
            && self.computers.len() == self.desired.len() + 1
            && self.computers.values().all(|c| {
                c.accepting && matches!(c.status, ComputerStatus::Live | ComputerStatus::Polled)
            });
        json!({"epoch":self.epoch,"state":if drained {"drained"} else if self.stopping {"stopping"} else {"running"},
            "drained":drained,"transport_ready":ready,"all_sources_ready":all_sources_ready,
            "connections":self.connection_rows(Instant::now()),
            "display_only":true,"authority":false,"error":self.error,
            "workers":u.workers,"readers":u.readers,"connecting":u.connecting,
            "ingress_bytes":u.bytes,"ingress_items":u.items,"high_ingress_bytes":u.high_bytes,
            "control_batches":u.controls,"control_bytes":u.control_bytes,
            "retiring":self.retiring.len(),"pending_retirements":self.pending.len(),
            "legacy_spools":u.legacy_spools,"legacy_reserved_bytes":u.legacy_reserved_bytes,
            "computers":self.computers.iter().map(|(id,c)|json!({"computer":id.0,
                "incarnation":c.handle.identity().incarnation,"generation":c.generation,"status":c.status.name(),
                "attempt":c.status.attempt(),"accepting":c.accepting})).collect::<Vec<_>>(),
            "limits":{"sources":source::SOURCES,"connecting":source::CONNECTIONS,
                "ingress_bytes":source::INGRESS_BYTES,"ingress_items":source::INGRESS_ITEMS,
                "control_batches":source::CONTROL_BATCHES,"control_bytes":source::CONTROL_BYTES,
                "pending_retirements":source::SOURCES,
                "poll_items":POLL_ITEMS,"restart_ms":self.restart_delays().map(|d|d.as_millis())},
            "retry_delays":self.retry_delays()})
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
        // Early owner exit must not block the actor or claim that I/O drained.
        // The source's existing guards continue to account any unfinished work.
        for handle in self.retiring.drain(..) {
            handle.detach();
        }
    }
}
/// The service's connection requests. Display/connection observation only:
/// never authority, activation, ownership or a source/service restart.
///
/// - `fleet_reconnect` `{request_id, computers:[{computer, incarnation?}]}`
///   (1 to [`RECONNECT_COMPUTERS`], no duplicates; `""` is this computer)
///   ends each existing supervisor's restart wait; see [`Runtime::reconnect`].
/// - `fleet_retry_delays_reload` `{request_id}` re-reads the saved timings.
pub fn request(runtime: &mut Runtime, request: &Value) -> Value {
    let id = request["request_id"].clone();
    let refused = |code: &str| json!({"request_id":id,"accepted":false,"code":code});
    if !id
        .as_str()
        .is_some_and(|id| !id.is_empty() && id.len() <= 128)
    {
        return refused("invalid_request");
    }
    match request["op"].as_str() {
        Some("fleet_reconnect") => {
            let Some(items) = request["computers"]
                .as_array()
                .filter(|items| !items.is_empty() && items.len() <= RECONNECT_COMPUTERS)
            else {
                return refused("invalid_request");
            };
            let mut seen = BTreeSet::new();
            let mut targets = Vec::with_capacity(items.len());
            for item in items {
                let Some(computer) = item["computer"].as_str().filter(|computer| {
                    computer.is_empty() || zerus_client_core::fleet::valid_alias(computer)
                }) else {
                    return refused("invalid_request");
                };
                let incarnation = match &item["incarnation"] {
                    Value::Null => None,
                    value => match value.as_u64() {
                        Some(incarnation) => Some(incarnation),
                        None => return refused("invalid_request"),
                    },
                };
                if !seen.insert(computer) {
                    return refused("invalid_request");
                }
                targets.push((computer, incarnation));
            }
            let results: Vec<Value> = targets
                .into_iter()
                .map(|(computer, incarnation)| {
                    json!({"computer":computer,"code":runtime.reconnect(computer, incarnation)})
                })
                .collect();
            json!({"request_id":id,"accepted":true,"results":results,
                "connections":runtime.connections()})
        }
        Some("fleet_retry_delays_reload") => {
            let retry_delays = runtime.reload_retry_delays();
            json!({"request_id":id,"accepted":true,"retry_delays":retry_delays,
                "connections":runtime.connections()})
        }
        _ => refused("unknown_request"),
    }
}

/// A restart schedule for display: `phase`, and for a wait its `delay_ms` and
/// the `remaining_ms` left on the supervisor's monotonic deadline at `now`.
pub fn retry_value(retry: Retry, now: Instant) -> Value {
    match retry {
        Retry::Idle => json!({"phase":"idle"}),
        Retry::Waiting(next) => json!({"phase":"waiting","serial":next.serial,
            "attempt":next.attempt,"delay_ms":next.delay_ms,"remaining_ms":next.remaining_ms(now)}),
        Retry::Attempting { serial, attempt } => {
            json!({"phase":"attempting","serial":serial,"attempt":attempt})
        }
    }
}

/// Coalesces early `fleet_connections` frames: at most one per
/// [`CONNECTIONS_GAP`], only after the presentation's revision changed.
#[derive(Default)]
pub struct Publication {
    revision: u64,
    at: Option<Instant>,
}

impl Publication {
    pub fn due(&mut self, revision: u64, now: Instant) -> bool {
        if revision == self.revision
            || self
                .at
                .is_some_and(|at| now.saturating_duration_since(at) < CONNECTIONS_GAP)
        {
            return false;
        }
        self.revision = revision;
        self.at = Some(now);
        true
    }
}

fn ensure_private(root: &std::path::Path) -> std::io::Result<()> {
    if !platform::verify_private(root, platform::Private::Directory)? {
        return Err(std::io::Error::other("unsafe fleet spool directory"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
