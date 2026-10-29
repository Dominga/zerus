//! The Processes tab's hub, ported from `tray/src/ProcessView.cpp`: the
//! selected session's processes from the watch topic (`Fleet::follow_processes`),
//! projected with the core's `process_view` rules; the selected process's
//! output, read again at Qt's cadence only while the tab shows; and the stop
//! queue, sequential and never retried, cancelled by a change of the session's
//! identity (host, name, archive, run, conversation, host generation).
//!
//! Selections and stops name the view they were made in (its key and, for a
//! stop, its selection), so a late click never acts on another view. An
//! archived session is shown without a follow: the topic has no archived
//! sessions (`ARCHIVED`).
//!
//! The five settings come from the app's one settings store
//! ([`ProcessHub::settings_from`], `processes/*` in `<data dir>/settings.json`).
//! The hub reads no file and writes none.

use std::collections::{BTreeMap, HashMap};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zerus_client_core::actions::{Action, Outcome, Target};
use zerus_client_core::process_view::{
    self as rules, Group, Next, Settings, StopContext, StopControl, StopQueue, Tone,
};
use zerus_client_core::reads::Read;
use zerus_client_core::{Batch, ComputerId};

pub type Job = Box<dyn FnOnce() + Send>;
pub type Spawn = Arc<dyn Fn(Job) + Send + Sync>;
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// How often the ticker runs while the tab shows: the output cadence, the
/// busy indicator's delay and the duration column.
pub const TICK: Duration = Duration::from_millis(250);
/// Qt keeps the views of this many sessions (`saveView`, `ProcessView.cpp:107`).
const VIEWS: usize = 30;
/// Qt's notice while stop requests go out (`stopSelected`, 196).
const SENDING: &str = "Sending stop requests…";
/// An archived session is never followed: its catalog key (`archive:ID`)
/// is no live session's, and its recorded processes need the archive
/// inventory read (core request zerus-5wmh.63).
pub const ARCHIVED: &str =
    "Recorded processes of archived sessions arrive with the archive inventory read.";

/// The session the tab follows, as the webview names it. `live` is Qt's
/// `!recorded` (`SessionsWindow.cpp:3783`): online, not archived or paused,
/// a running process and no inspect error.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessTarget {
    pub computer: String,
    /// The catalog key the topic follows.
    pub key: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
    #[serde(default)]
    pub archive_id: Option<String>,
    /// The row's machine: the peer's alias, or this computer's host name.
    pub machine: String,
    pub online: bool,
    pub live: bool,
    /// The native host generation of the session's details
    /// (`host_generation`, Qt's `inspection` at `ProcessView.cpp:111`). The
    /// processes topic does not carry it (`src/state/watch_source.rs:48`), and
    /// a native job's output and stop need it.
    #[serde(default)]
    pub generation: Option<String>,
}

impl ProcessTarget {
    /// Qt's host for the details line (`ProcessView.cpp:151`): empty for this
    /// computer ("This machine"), else the peer's machine.
    fn host(&self) -> &str {
        if self.computer.is_empty() {
            ""
        } else if self.machine.is_empty() {
            &self.computer
        } else {
            &self.machine
        }
    }

    /// An archived session (`archive:ID`), which the topic cannot follow.
    fn archived(&self) -> bool {
        self.key.starts_with("archive:")
            || self.archive_id.as_deref().is_some_and(|id| !id.is_empty())
    }

    /// The topic's follow: another session ref is another subscription.
    fn same_follow(&self, other: &ProcessTarget) -> bool {
        self.computer == other.computer
            && self.key == other.key
            && self.name == other.name
            && self.run_id == other.run_id
            && self.conversation_id == other.conversation_id
    }
}

/// One row of the list: Command, Agent, State and Duration.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RowView {
    pub id: String,
    /// The command with its whitespace collapsed (Qt's `simplified()`).
    pub command: String,
    /// The command as recorded, the row's tooltip.
    pub command_full: String,
    pub owner: String,
    pub status: String,
    pub tooltip: String,
    pub tone: Tone,
    pub duration: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GroupView {
    pub group: Group,
    pub title: String,
    pub rows: Vec<RowView>,
}

/// The current process's details and output.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DetailView {
    pub id: String,
    /// The whole command, for Copy command.
    pub command: String,
    pub details: String,
    /// None until an output was read for this selection.
    pub output: Option<String>,
    pub notice: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProcessesView {
    /// Qt's view key: host, name, archive, run, conversation and host
    /// generation, one per line; empty without a session.
    pub key: String,
    pub summary: String,
    pub summary_tooltip: String,
    pub notes: String,
    /// Live, Recorded and Finished, each only with rows. Rows keep Qt's tree
    /// order: a row stays where it first appeared, and one that changes group
    /// goes to the end of its new group.
    pub groups: Vec<GroupView>,
    pub selected: Vec<String>,
    pub current: Option<String>,
    pub stop: StopControl,
    pub stop_notice: String,
    pub busy: bool,
    pub failed: Option<String>,
    pub detail: Option<DetailView>,
    /// The tab's count: 0 for a recorded session (`SessionsWindow.cpp:3786`).
    pub count: usize,
}

/// Milliseconds on a monotonic clock.
pub type ClockMs = Arc<dyn Fn() -> u64 + Send + Sync>;
/// Seconds since the epoch.
pub type Epoch = Arc<dyn Fn() -> f64 + Send + Sync>;
/// Follows a session's processes topic; the error is the refusal's text.
pub type Follow = Arc<dyn Fn(&ProcessTarget) -> Result<(), String> + Send + Sync>;
pub type Unfollow = Arc<dyn Fn(&ProcessTarget) + Send + Sync>;
/// Update an existing topic subscription; no inventory is dropped.
pub type SetInterval = Arc<dyn Fn(&ProcessTarget, u64) + Send + Sync>;
/// Request one inventory read after a stop or mismatched output reply.
pub type Refresh = Arc<dyn Fn(&ProcessTarget) + Send + Sync>;
/// One `hgs processes NAME --output ID ...`; blocking.
pub type ReadOutput = Arc<dyn Fn(&ComputerId, &Read) -> Result<Value, String> + Send + Sync>;
/// One guarded stop (`actions::dispatch_as`); blocking.
pub type Stop = Arc<dyn Fn(Target, Action) -> Result<Outcome, String> + Send + Sync>;

pub struct ProcessDeps {
    pub spawn: Spawn,
    pub notify: Notify,
    /// The output cadence and the busy delay (Qt's `QElapsedTimer` and `QTimer`).
    pub clock_ms: ClockMs,
    /// The duration column (`refreshDurations`).
    pub epoch: Epoch,
    pub follow: Follow,
    pub unfollow: Unfollow,
    pub set_interval: SetInterval,
    pub refresh: Refresh,
    pub read: ReadOutput,
    pub stop: Stop,
}

/// The selected process's output pane (`m_output`, `m_notice`).
#[derive(Default)]
struct OutputState {
    text: Option<String>,
    notice: String,
    /// When the last read started (`m_outputAge`).
    read_at: Option<u64>,
    /// The read in flight (`m_request`), by its token.
    reading: Option<u64>,
    /// The output revision changed, or the selection is new (`m_outputDirty`).
    dirty: bool,
}

/// One session's view, restored when it is shown again (`saveView`).
#[derive(Clone, Default)]
struct Saved {
    current: Option<String>,
    selection: Vec<String>,
    output: Option<String>,
}

const GROUPS: [Group; 3] = [Group::Live, Group::Recorded, Group::Finished];

fn slot(group: Group) -> usize {
    match group {
        Group::Live => 0,
        Group::Recorded => 1,
        Group::Finished => 2,
    }
}

#[derive(Default)]
struct Hub {
    settings: Settings,
    shown: bool,
    /// The session the webview follows; shown only while Processes is enabled.
    wanted: Option<ProcessTarget>,
    /// The topic follow that succeeded, if any.
    subscribed: Option<ProcessTarget>,
    /// Last cadence sent for the current subscription (one entry, no history).
    interval: Option<(String, String, u64)>,
    /// The (computer, key) whose inventory `value` holds.
    value_key: Option<(String, String)>,
    /// Qt's view key of the session shown.
    identity: String,
    value: Option<Value>,
    failed: Option<String>,
    /// Normalized records, one per ID in first-seen order (`m_jobs`).
    records: Vec<Value>,
    index: HashMap<String, usize>,
    /// The rows of each group in Qt's tree order (`m_rows` under their parents).
    rows: [Vec<String>; 3],
    selected: Vec<String>,
    current: Option<String>,
    /// After a change of identity, until its first inventory: the view to restore.
    fresh: Option<Saved>,
    views: BTreeMap<String, Saved>,
    output: OutputState,
    queue: StopQueue,
    stop_notice: String,
    /// The queue's stop (`m_stopRequest`): sent, or held in `held`.
    stopping: Option<InFlight>,
    /// The queue's stop while a stop of an earlier identity still holds its
    /// lane: it goes out when that one answers, so it never finds the lane busy.
    held: Option<StopCall>,
    /// Stops of earlier identities still under way; their answers only free
    /// their lane.
    draining: Vec<InFlight>,
    /// Since when a request has run while the tab shows (`m_busyDelay`).
    busy_since: Option<u64>,
    busy_seen: bool,
    tokens: u64,
    second: i64,
    /// The view may differ from `last`.
    changed: bool,
    last: Option<ProcessesView>,
    closed: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// An output read to start, off the hub's lock.
struct OutputCall {
    token: u64,
    computer: ComputerId,
    read: Read,
    process: String,
}

/// A stop under way, by its token, and its lane in `actions` (one stop per
/// session name and computer, `Lane::Process`).
struct InFlight {
    token: u64,
    computer: String,
    name: String,
}

impl InFlight {
    fn lane_of(&self, call: &StopCall) -> bool {
        self.computer == call.target.computer && self.name == call.target.name
    }
}

/// A stop to dispatch, off the hub's lock.
struct StopCall {
    token: u64,
    target: Target,
    action: Action,
    process: String,
}

impl Hub {
    /// The followed session while Processes is enabled.
    fn target(&self) -> Option<&ProcessTarget> {
        self.wanted.as_ref().filter(|_| self.settings.enabled)
    }

    fn live(&self) -> bool {
        self.target().is_some_and(|t| t.live)
    }

    fn record(&self, id: &str) -> Option<&Value> {
        self.index.get(id).map(|&at| &self.records[at])
    }

    /// Qt's view key (`setSession`, `ProcessView.cpp:112`).
    fn identity_of(target: Option<&ProcessTarget>) -> String {
        target
            .map(|t| {
                [
                    t.computer.as_str(),
                    &t.name,
                    t.archive_id.as_deref().unwrap_or(""),
                    &t.run_id,
                    &t.conversation_id,
                    t.generation.as_deref().unwrap_or(""),
                ]
                .join("\n")
            })
            .unwrap_or_default()
    }

    fn selected_ids(&self) -> Vec<String> {
        self.selected
            .iter()
            .filter(|id| self.index.contains_key(*id))
            .cloned()
            .collect()
    }

    fn context(&self) -> StopContext {
        StopContext {
            enabled: self.settings.enabled,
            online: self.target().is_some_and(|t| t.online),
            live: self.live(),
            sending: self.queue.sending(),
            pending: self.queue.pending.clone(),
        }
    }

    /// The Stop selected button (`updateActions`, 161-177).
    fn control(&self) -> StopControl {
        let selected: Vec<(&str, &Value)> = self
            .selected
            .iter()
            .filter_map(|id| self.record(id).map(|record| (id.as_str(), record)))
            .collect();
        rules::stop_control(&selected, &self.context())
    }

    /// `updateBusy` (178-182): the delay starts with the first request while
    /// the tab shows and ends when none runs or the tab hides.
    fn update_busy(&mut self, now: u64) {
        let running = self.output.reading.is_some() || self.stopping.is_some();
        if running && self.shown {
            self.busy_since.get_or_insert(now);
        } else {
            self.busy_since = None;
        }
    }

    fn busy(&self, now: u64) -> bool {
        self.shown
            && self
                .busy_since
                .is_some_and(|since| now.saturating_sub(since) >= self.settings.loading_ms())
    }

    /// A new selection (`select`, 145-147): the read in flight is dropped
    /// and the output is read again at once.
    fn reset_output(&mut self, now: u64) {
        self.output = OutputState {
            dirty: true,
            ..OutputState::default()
        };
        self.update_busy(now);
    }

    /// A new identity (`setSession` "changed", 113): the old view is saved,
    /// and the read, the stops and the selection start over.
    fn reset(&mut self, identity: String, now: u64) {
        if !self.identity.is_empty() {
            // A session left before its inventory came keeps the view it had.
            let saved = self.fresh.take().unwrap_or_else(|| Saved {
                current: self.current.clone(),
                selection: self.selected_ids(),
                output: self.output.text.clone(),
            });
            self.views.insert(std::mem::take(&mut self.identity), saved);
            while self.views.len() > VIEWS {
                self.views.pop_first();
            }
        }
        self.output = OutputState::default();
        self.queue.reset();
        // A stop that went out still holds its lane until it answers; a held
        // one never went out.
        if let Some(stop) = self.stopping.take() {
            if self
                .held
                .as_ref()
                .is_none_or(|held| held.token != stop.token)
            {
                self.draining.push(stop);
            }
        }
        self.held = None;
        self.stop_notice.clear();
        self.selected.clear();
        self.current = None;
        self.fresh = Some(self.views.get(&identity).cloned().unwrap_or_default());
        self.identity = identity;
        self.update_busy(now);
        self.changed = true;
    }

    /// Follows what the webview selected while Processes is enabled, and
    /// resets the view on a new identity. Returns whether the current
    /// process changed.
    fn retarget(&mut self, deps: &ProcessDeps, now: u64) -> bool {
        let next = self.target().cloned();
        let key = next.as_ref().map(|t| (t.computer.clone(), t.key.clone()));
        if key != self.value_key {
            // Another session's inventory is not this one's.
            self.value = None;
            self.failed = None;
            self.value_key = key;
        }
        // An archived session is shown without a follow (`ARCHIVED`).
        let follows = next.as_ref().filter(|target| !target.archived());
        let keep =
            matches!((&self.subscribed, follows), (Some(old), Some(new)) if old.same_follow(new));
        if !keep {
            self.interval = None;
            if let Some(old) = self.subscribed.take() {
                let same_key =
                    follows.is_some_and(|new| new.computer == old.computer && new.key == old.key);
                if !same_key {
                    (deps.unfollow)(&old);
                }
            }
            if let Some(new) = follows {
                match (deps.follow)(new) {
                    Ok(()) => self.subscribed = Some(new.clone()),
                    Err(error) => self.failed = Some(error),
                }
            }
        }
        self.sync_interval(deps);
        let identity = Self::identity_of(next.as_ref());
        if identity != self.identity {
            self.reset(identity, now);
        }
        self.changed = true;
        self.place(now)
    }

    /// Qt polls faster while the tab shows (SessionsWindow.cpp:2015,3473).
    /// Changing cadence leaves the topic and the displayed inventory intact.
    fn sync_interval(&mut self, deps: &ProcessDeps) {
        let Some(target) = &self.subscribed else {
            self.interval = None;
            return;
        };
        let ms = if self.shown {
            self.settings.active_ms()
        } else {
            self.settings.background_ms()
        };
        let interval = (target.computer.clone(), target.key.clone(), ms);
        if self.interval.as_ref() != Some(&interval) {
            (deps.set_interval)(target, ms);
            self.interval = Some(interval);
        }
    }

    /// Takes the inventory as `setSession` does (116-143): normalized
    /// records, Qt's tree order, the selection and the current process, and
    /// the output's revision. Returns whether the current process changed.
    fn place(&mut self, now: u64) -> bool {
        let live = self.live();
        let mut records: Vec<Value> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for item in self
            .value
            .as_ref()
            .and_then(|value| value["processes"]["items"].as_array())
            .into_iter()
            .flatten()
        {
            let id = text(item, "id");
            if id.is_empty() {
                continue;
            }
            let mut record = item.clone();
            rules::normalize(&mut record, live);
            match index.get(id) {
                Some(&at) => records[at] = record,
                None => {
                    index.insert(id.to_owned(), records.len());
                    records.push(record);
                }
            }
        }
        let prior_records = std::mem::replace(&mut self.records, records);
        let prior_index = std::mem::replace(&mut self.index, index);
        let fresh = if self.value.is_some() {
            self.fresh.take()
        } else {
            None
        };

        // Rows stay where they are; a new one goes to the end of its group,
        // and one whose group changed moves to the end of its new group.
        let placed: HashMap<String, usize> = self
            .rows
            .iter()
            .enumerate()
            .flat_map(|(g, rows)| rows.iter().map(move |row| (row.clone(), g)))
            .collect();
        let mut moved_rows: HashMap<String, usize> = HashMap::new();
        for record in &self.records {
            let id = text(record, "id");
            let group = slot(rules::group(record, live));
            match placed.get(id) {
                Some(&g) if g == group => {}
                Some(&g) => {
                    moved_rows.insert(id.to_owned(), g);
                    self.rows[group].push(id.to_owned());
                }
                None => self.rows[group].push(id.to_owned()),
            }
        }
        // A moved row leaves its old group.
        for (g, rows) in self.rows.iter_mut().enumerate() {
            rows.retain(|row| moved_rows.get(row) != Some(&g));
        }
        // A removed current row hands the current to its nearest remaining
        // sibling, above first (QItemSelectionModel's row removal).
        let sibling = self.current.as_ref().and_then(|current| {
            if self.index.contains_key(current) {
                return None;
            }
            let group = self.rows.iter().find(|rows| rows.contains(current))?;
            let at = group.iter().position(|row| row == current)?;
            let remains = |row: &&String| self.index.contains_key(*row);
            group[..at]
                .iter()
                .rev()
                .find(remains)
                .or_else(|| group[at + 1..].iter().find(remains))
                .cloned()
        });
        for rows in &mut self.rows {
            rows.retain(|row| self.index.contains_key(row));
        }

        let (selection, desired) = match &fresh {
            Some(saved) => (saved.selection.clone(), saved.current.clone()),
            None => (std::mem::take(&mut self.selected), self.current.clone()),
        };
        self.selected = selection
            .into_iter()
            .filter(|id| self.index.contains_key(id))
            .collect();
        let current = match desired.filter(|id| self.index.contains_key(id)) {
            Some(id) => Some(id),
            None if !self.records.is_empty() && (fresh.is_some() || prior_records.is_empty()) => {
                // The first live row, else the first recorded, else the first finished (131).
                let first = self.rows.iter().find_map(|rows| rows.first()).cloned();
                self.selected = first.iter().cloned().collect();
                first
            }
            None if fresh.is_none() => sibling,
            None => None,
        };
        let moved = current != self.current;
        if moved {
            self.current = current;
            self.reset_output(now);
        }
        // The saved output shows until the new read answers (142).
        if let Some(saved) = fresh.as_ref().filter(|saved| saved.current == self.current) {
            if saved.output.is_some() {
                self.output.text = saved.output.clone();
            }
        }
        if !moved && fresh.is_none() {
            if let Some(current) = &self.current {
                let before = prior_index
                    .get(current)
                    .map(|&at| rules::output_revision(&prior_records[at]));
                let after = self.record(current).map(rules::output_revision);
                if before != after {
                    self.output.dirty = true;
                }
            }
        }
        let Hub {
            queue,
            records,
            index,
            ..
        } = self;
        queue.settle(|id| index.get(id).map(|&at| records[at].clone()));
        self.changed = true;
        moved
    }

    /// `requestOutput` (183-192): the current process's output, when due.
    fn output_call(&mut self, now: u64, forced: bool, allowed: bool) -> Option<OutputCall> {
        let target = self.target()?.clone();
        if !self.shown || !target.online || self.output.reading.is_some() {
            return None;
        }
        let id = self.current.clone()?;
        let record = self.record(&id)?.clone();
        if target.run_id.is_empty() || target.conversation_id.is_empty() {
            return None;
        }
        if let Some(notice) = rules::output_unavailable(&record) {
            if self.output.notice != notice {
                self.output.notice = notice.into();
                self.changed = true;
            }
            return None;
        }
        let age = self.output.read_at.map(|at| now.saturating_sub(at));
        if !rules::output_due(&record, &self.settings, forced, self.output.dirty, age) {
            return None;
        }
        self.output.dirty = false;
        self.output.read_at = Some(now);
        if !allowed {
            if self.output.notice != crate::actions::REPLAY_NEEDS_HGS {
                self.output.notice = crate::actions::REPLAY_NEEDS_HGS.into();
                self.changed = true;
            }
            return None;
        }
        self.tokens += 1;
        self.output.reading = Some(self.tokens);
        self.update_busy(now);
        Some(OutputCall {
            token: self.tokens,
            computer: ComputerId(target.computer.clone()),
            read: Read::ProcessOutput {
                name: target.name.clone(),
                process_id: id.clone(),
                run_id: target.run_id.clone(),
                conversation_id: target.conversation_id.clone(),
                archive_id: target.archive_id.clone(),
                generation: target.generation.clone().filter(|g| !g.is_empty()),
            },
            process: id,
        })
    }

    /// `stopNext` (198-215): the next stop of the queue, or its summary.
    fn next_stop(&mut self, now: u64) -> Option<StopCall> {
        let target = self.target().cloned();
        let available = target.as_ref().is_some_and(|t| t.online && t.live);
        let identity = self.identity.clone();
        let Hub {
            queue,
            records,
            index,
            ..
        } = self;
        let next = queue.next(
            &identity,
            |id| index.get(id).map(|&at| records[at].clone()),
            available,
        )?;
        self.changed = true;
        match next {
            Next::Done(message) => {
                self.stop_notice = message;
                None
            }
            Next::Stop(id) => {
                // `available` holds a target.
                let target = target?;
                self.tokens += 1;
                self.stopping = Some(InFlight {
                    token: self.tokens,
                    computer: target.computer.clone(),
                    name: target.name.clone(),
                });
                self.update_busy(now);
                let call = StopCall {
                    token: self.tokens,
                    target: Target {
                        computer: target.computer.clone(),
                        name: target.name.clone(),
                        run_id: target.run_id.clone(),
                        conversation_id: target.conversation_id.clone(),
                    },
                    action: Action::StopProcess {
                        process_id: id.clone(),
                        archive_id: target.archive_id.clone(),
                        generation: target.generation.clone().filter(|g| !g.is_empty()),
                    },
                    process: id,
                };
                if self.draining.iter().any(|stop| stop.lane_of(&call)) {
                    self.held = Some(call);
                    return None;
                }
                Some(call)
            }
        }
    }

    /// The held stop, once no earlier stop holds its lane.
    fn release_held(&mut self) -> Option<StopCall> {
        let held = self.held.as_ref()?;
        if self.draining.iter().any(|stop| stop.lane_of(held)) {
            return None;
        }
        self.held.take()
    }

    fn view(&self, now: u64, epoch: f64) -> ProcessesView {
        let live = self.live();
        let groups = GROUPS
            .iter()
            .filter_map(|&group| {
                let rows: Vec<RowView> = self.rows[slot(group)]
                    .iter()
                    .filter_map(|id| self.record(id).map(|record| row_view(id, record, epoch)))
                    .collect();
                (!rows.is_empty()).then(|| GroupView {
                    group,
                    title: rules::group_title(group, live).into(),
                    rows,
                })
            })
            .collect();
        let processes = self
            .value
            .as_ref()
            .map_or(&Value::Null, |value| &value["processes"]);
        let (notes, summary_tooltip) = if self.target().is_some_and(ProcessTarget::archived) {
            (ARCHIVED.to_owned(), ARCHIVED.to_owned())
        } else {
            rules::notes(processes)
        };
        let host = self.target().map_or("", ProcessTarget::host);
        let detail = self.current.as_ref().and_then(|id| {
            let record = self.record(id)?;
            Some(DetailView {
                id: id.clone(),
                command: text(record, "command").to_owned(),
                details: rules::details(record, host),
                output: self.output.text.clone(),
                notice: self.output.notice.clone(),
            })
        });
        let number = |key: &str| processes[key].as_u64().unwrap_or(0) as usize;
        let count = if self.target().is_some_and(|t| t.online && t.live) {
            number("live_count").max(number("active_count"))
        } else {
            0
        };
        ProcessesView {
            key: self.identity.clone(),
            summary: rules::summary(&self.records, live),
            summary_tooltip,
            notes,
            groups,
            selected: self.selected_ids(),
            current: self.current.clone(),
            stop: self.control(),
            stop_notice: self.stop_notice.clone(),
            busy: self.busy(now),
            failed: self.failed.clone(),
            detail,
            count,
        }
    }
}

fn row_view(id: &str, record: &Value, epoch: f64) -> RowView {
    let command = text(record, "command");
    RowView {
        id: id.to_owned(),
        command: command.split_whitespace().collect::<Vec<_>>().join(" "),
        command_full: command.to_owned(),
        owner: text(record, "owner").to_owned(),
        status: rules::status_label(record),
        tooltip: rules::status_tooltip(record),
        tone: rules::tone(record),
        duration: rules::duration(record, epoch),
    }
}

/// The five settings as the app's settings store gives them (Qt's
/// `ProcessSettings`, already clamped by the store; the rules clamp again).
pub fn stored(store: &zerus_client_core::settings::Settings) -> Settings {
    Settings {
        enabled: store.flag("processes/enabled"),
        active_seconds: store.number("processes/activeSeconds"),
        background_seconds: store.number("processes/backgroundSeconds"),
        unknown_seconds: store.number("processes/unknownSeconds"),
        loading_seconds: store.number("processes/loadingSeconds"),
    }
}

struct Shared {
    deps: ProcessDeps,
    hgs_allowed: bool,
    hub: Mutex<Hub>,
    /// Wakes the ticker when the tab shows a session.
    wake: Condvar,
    /// Store reads apply in the order they were made.
    settings_order: Mutex<()>,
}

/// The hub; clones share it.
#[derive(Clone)]
pub struct ProcessHub(Arc<Shared>);

impl ProcessHub {
    /// `hgs_allowed` is false in a replay without `ZERUS_CLIENT_HGS`: output
    /// reads and stops then refuse with the replay notice.
    pub fn new(deps: ProcessDeps, hgs_allowed: bool) -> Self {
        ProcessHub(Arc::new(Shared {
            deps,
            hgs_allowed,
            hub: Mutex::new(Hub::default()),
            wake: Condvar::new(),
            settings_order: Mutex::new(()),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, Hub> {
        lock(&self.0.hub)
    }

    fn now(&self) -> u64 {
        (self.0.deps.clock_ms)()
    }

    /// Ends a call that changed the hub: the pump wakes, the ticker too,
    /// and a read that came due starts.
    fn finish(&self, hub: MutexGuard<'_, Hub>, read: Option<OutputCall>, stop: Option<StopCall>) {
        let changed = hub.changed;
        drop(hub);
        self.0.wake.notify_all();
        if changed {
            (self.0.deps.notify)();
        }
        if let Some(read) = read {
            self.run_read(read);
        }
        if let Some(stop) = stop {
            self.run_stop(stop);
        }
    }

    /// The session the tab shows, or none (`setSession`). The same session
    /// again follows its topic again when its follow was refused or ended.
    pub fn follow(&self, target: Option<ProcessTarget>) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.closed {
            return;
        }
        hub.wanted = target;
        let moved = hub.retarget(&self.0.deps, now);
        let read = hub.output_call(now, moved, self.0.hgs_allowed);
        self.finish(hub, read, None);
    }

    /// Settings > Processes (`applyPreferences`, 101-105, and
    /// `SessionsWindow.cpp:1684-1689`). Disabling unfollows the session and
    /// drops its view, its read and its unsent stops.
    pub fn settings(&self, settings: Settings) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.closed || hub.settings == settings {
            return;
        }
        hub.settings = settings;
        let moved = hub.retarget(&self.0.deps, now);
        let read = hub.output_call(now, moved, self.0.hgs_allowed);
        self.finish(hub, read, None);
    }

    /// Reads the five settings from the app's store and applies them; reads
    /// apply in their order, so a later change never loses to an earlier one.
    pub fn settings_from(&self, store: &zerus_client_core::settings::Settings) {
        let _order = lock(&self.0.settings_order);
        self.settings(stored(store));
    }

    /// The tab shows or hides (`showEvent`, `hideEvent`, 217-218): showing
    /// reads the output at once.
    pub fn shown(&self, shown: bool) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.closed || hub.shown == shown {
            return;
        }
        hub.shown = shown;
        hub.sync_interval(&self.0.deps);
        hub.changed = true;
        hub.update_busy(now);
        let read = if shown {
            hub.output_call(now, true, self.0.hgs_allowed)
        } else {
            None
        };
        self.finish(hub, read, None);
    }

    /// The webview's selection and current row (`select`, 145-157), for the
    /// view it showed as `key`: a selection made in another session's view
    /// changes nothing. A new current process drops the read in flight and
    /// reads its output at once.
    pub fn select(&self, key: &str, ids: Vec<String>, current: Option<String>) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.closed || hub.identity != key {
            return;
        }
        let mut selected: Vec<String> = Vec::new();
        for id in ids {
            if hub.index.contains_key(&id) && !selected.contains(&id) {
                selected.push(id);
            }
        }
        hub.selected = selected;
        let current = current.filter(|id| hub.index.contains_key(id));
        let moved = current != hub.current;
        if moved {
            hub.current = current;
            hub.reset_output(now);
        }
        hub.changed = true;
        let read = hub.output_call(now, moved, self.0.hgs_allowed);
        self.finish(hub, read, None);
    }

    /// Stop selected (`stopSelected`, 193-197) for the view the click saw:
    /// only when `key` is still the session's identity and `ids` still its
    /// selection (Qt's `key==m_key && ids==selectedIds()`, 74), and only while
    /// the button is enabled. The stops go out one at a time.
    pub fn stop(&self, key: &str, ids: &[String]) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.closed || hub.identity != key || hub.selected_ids() != ids || !hub.control().enabled
        {
            return;
        }
        hub.changed = true;
        if !self.0.hgs_allowed {
            hub.stop_notice = crate::actions::REPLAY_NEEDS_HGS.into();
            return self.finish(hub, None, None);
        }
        let identity = hub.identity.clone();
        let ids = hub.selected_ids();
        hub.queue.start(&identity, ids);
        hub.stop_notice = SENDING.into();
        let stop = hub.next_stop(now);
        self.finish(hub, None, stop);
    }

    /// Takes the followed session's entries out of the batch: a `state`
    /// replaces the inventory, `failed` shows its message and `ended` drops
    /// the inventory and the follow (a later follow subscribes again).
    pub fn observe(&self, batch: &mut Batch) {
        if batch.processes.is_empty() {
            return;
        }
        let now = self.now();
        let mut hub = self.lock();
        let Some((computer, key)) = hub.target().map(|t| (t.computer.clone(), t.key.clone()))
        else {
            return;
        };
        let mut mine = Vec::new();
        batch.processes.retain(|entry| {
            let followed = entry.computer.0 == computer && entry.key == key;
            if followed {
                mine.push(entry.clone());
            }
            !followed
        });
        if mine.is_empty() {
            return;
        }
        for entry in mine {
            match entry.kind.as_str() {
                "state" => {
                    hub.value = Some(entry.payload["value"].clone());
                    hub.failed = None;
                }
                "failed" => {
                    hub.failed = Some(text(&entry.payload, "message").to_owned());
                }
                "ended" => {
                    hub.value = None;
                    hub.subscribed = None;
                }
                _ => {}
            }
        }
        let moved = hub.place(now);
        let read = hub.output_call(now, moved, self.0.hgs_allowed);
        self.finish(hub, read, None);
    }

    /// The view when it changed since the last one taken.
    pub fn take_view(&self) -> Option<ProcessesView> {
        let now = self.now();
        let epoch = (self.0.deps.epoch)();
        let mut hub = self.lock();
        if !hub.changed {
            return None;
        }
        hub.changed = false;
        let view = hub.view(now, epoch);
        hub.busy_seen = view.busy;
        if hub.last.as_ref() == Some(&view) {
            return None;
        }
        hub.last = Some(view.clone());
        Some(view)
    }

    /// A reloaded webview gets the view again.
    pub fn resend(&self) {
        let mut hub = self.lock();
        hub.last = None;
        hub.changed = true;
        drop(hub);
        (self.0.deps.notify)();
    }

    /// Every 250 ms while the tab shows: the output when due (Qt's timer,
    /// 98), the busy indicator after its delay, and each new second for the
    /// duration column (`refreshDurations`).
    pub fn tick(&self) {
        let now = self.now();
        let epoch = (self.0.deps.epoch)();
        let mut hub = self.lock();
        if hub.closed {
            return;
        }
        let before = hub.changed;
        if hub.shown {
            let second = epoch.floor() as i64;
            if second != hub.second {
                hub.second = second;
                hub.changed = true;
            }
        }
        if hub.busy(now) != hub.busy_seen {
            hub.changed = true;
        }
        let read = hub.output_call(now, false, self.0.hgs_allowed);
        let changed = hub.changed && !before;
        drop(hub);
        if changed {
            (self.0.deps.notify)();
        }
        if let Some(read) = read {
            self.run_read(read);
        }
    }

    /// The app quits: no stop or read starts any more, and unsent stops are
    /// dropped, as Qt's view goes with its window.
    pub fn close(&self) {
        let mut hub = self.lock();
        hub.closed = true;
        hub.queue.reset();
        hub.held = None;
        drop(hub);
        self.0.wake.notify_all();
    }

    /// Starts the ticker, which sleeps while the tab shows no session and
    /// ends when the hub closes.
    pub fn start_ticker(&self) {
        let me = self.clone();
        let started = std::thread::Builder::new()
            .name("zerus-processes".into())
            .spawn(move || {
                while me.wait_shown() {
                    std::thread::sleep(TICK);
                    me.tick();
                }
            });
        if let Err(error) = started {
            eprintln!("zerus-client: the Processes ticker did not start: {error}");
        }
    }

    /// False once the hub closes.
    fn wait_shown(&self) -> bool {
        let hub = self
            .0
            .wake
            .wait_while(self.lock(), |hub| {
                !hub.closed && !(hub.shown && hub.target().is_some())
            })
            .unwrap_or_else(PoisonError::into_inner);
        !hub.closed
    }

    fn run_read(&self, call: OutputCall) {
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                (me.0.deps.read)(&call.computer, &call.read)
            }))
            .unwrap_or_else(|_| Err("The output read failed.".into()));
            me.output_read(call, result);
        }));
    }

    /// The output reply (`ProcessView.cpp:89-96`).
    fn refresh_inventory(&self, hub: &Hub) {
        if self.0.hgs_allowed && !hub.closed {
            if let Some(target) = &hub.subscribed {
                (self.0.deps.refresh)(target);
            }
        }
    }

    fn output_read(&self, call: OutputCall, result: Result<Value, String>) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.output.reading != Some(call.token) {
            return;
        }
        hub.output.reading = None;
        hub.update_busy(now);
        hub.changed = true;
        if hub.current.as_deref() == Some(call.process.as_str()) {
            match result {
                Err(error) => hub.output.notice = error,
                Ok(reply) => {
                    let (run, conversation) = hub
                        .target()
                        .map(|t| (t.run_id.clone(), t.conversation_id.clone()))
                        .unwrap_or_default();
                    // Qt re-inspects after a mismatched reply (ProcessView.cpp:93).
                    // The token guard above prevents an old selection refreshing a new one.
                    let shown = rules::output(&reply, &call.process, &run, &conversation);
                    if shown.refresh {
                        self.refresh_inventory(&hub);
                    }
                    if let Some(text) = shown.text {
                        hub.output.text = Some(text);
                    }
                    hub.output.notice = shown.notice;
                }
            }
        }
        self.finish(hub, None, None);
    }

    fn run_stop(&self, call: StopCall) {
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                (me.0.deps.stop)(call.target.clone(), call.action.clone())
            }))
            .unwrap_or_else(|_| Err(String::new()));
            me.stopped(call, outcome);
        }));
    }

    /// A stop's answer (`ProcessView.cpp:79-87`): its result, then the next
    /// stop. An answer of a dropped stop changes nothing.
    fn stopped(&self, call: StopCall, outcome: Result<Outcome, String>) {
        let (run, conversation) = (&call.target.run_id, &call.target.conversation_id);
        let result = match &outcome {
            Ok(Outcome::Done { receipt, .. }) => {
                rules::stop_result(Ok(receipt), &call.process, run, conversation)
            }
            Ok(Outcome::Failed { detail } | Outcome::Uncertain { detail }) | Err(detail) => {
                rules::stop_result(Err(detail), &call.process, run, conversation)
            }
        };
        let now = self.now();
        let mut hub = self.lock();
        let next = if hub
            .stopping
            .as_ref()
            .is_some_and(|stop| stop.token == call.token)
        {
            hub.stopping = None;
            hub.queue.finished(result);
            hub.changed = true;
            // Every current stop answer, including uncertain or failed, asks
            // for one read; the stop itself is never replayed (ProcessView.cpp:87).
            self.refresh_inventory(&hub);
            if hub.closed {
                None
            } else {
                hub.next_stop(now)
            }
        } else if let Some(at) = hub
            .draining
            .iter()
            .position(|stop| stop.token == call.token)
        {
            // An earlier identity's stop: its lane is free now.
            hub.draining.remove(at);
            if hub.closed {
                None
            } else {
                hub.release_held()
            }
        } else {
            return;
        };
        hub.update_busy(now);
        self.finish(hub, None, next);
    }
}

#[tauri::command]
pub fn processes_follow(hub: tauri::State<'_, ProcessHub>, target: Option<ProcessTarget>) {
    hub.follow(target);
}

#[tauri::command]
pub fn processes_shown(hub: tauri::State<'_, ProcessHub>, shown: bool) {
    hub.shown(shown);
}

/// `key` is the `ProcessesView.key` the selection was made in.
#[tauri::command]
pub fn processes_select(
    hub: tauri::State<'_, ProcessHub>,
    key: String,
    ids: Vec<String>,
    current: Option<String>,
) {
    hub.select(&key, ids, current);
}

/// `key` and `ids` are the `ProcessesView.key` and `selected` the click saw.
#[tauri::command]
pub fn processes_stop(hub: tauri::State<'_, ProcessHub>, key: String, ids: Vec<String>) {
    hub.stop(&key, &ids);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU64, Ordering};
    use zerus_client_core::ProcessesOut;

    struct Fake {
        now: AtomicU64,
        reads: Mutex<Vec<String>>,
        stops: Mutex<Vec<String>>,
        stop_ok: Mutex<bool>,
        follows: Mutex<Vec<String>>,
        intervals: Mutex<Vec<(String, u64)>>,
        refreshes: Mutex<Vec<String>>,
        follow_error: Mutex<Option<String>>,
        /// Jobs held for the test to run, when `hold` is set.
        held: Mutex<Vec<Job>>,
        hold: Mutex<bool>,
        output: Mutex<Value>,
        /// Every read and stop as the hub asked for it.
        calls: Mutex<Vec<String>>,
        /// The conversation an output reply names.
        reply_conversation: Mutex<String>,
        /// `actions`' process lanes: one stop per computer and session name
        /// at a time, `Err("busy")` otherwise (`actions::dispatch_with`).
        lanes: Mutex<HashSet<String>>,
        /// Runs while the next stop holds its lane, as a stop in flight.
        during: Mutex<Option<Job>>,
    }

    fn hub_with(allowed: bool) -> (ProcessHub, Arc<Fake>) {
        let fake = Arc::new(Fake {
            now: AtomicU64::new(0),
            reads: Mutex::new(vec![]),
            stops: Mutex::new(vec![]),
            stop_ok: Mutex::new(true),
            follows: Mutex::new(vec![]),
            intervals: Mutex::new(vec![]),
            refreshes: Mutex::new(vec![]),
            follow_error: Mutex::new(None),
            held: Mutex::new(vec![]),
            hold: Mutex::new(false),
            output: Mutex::new(json!("running 42 tests")),
            calls: Mutex::new(vec![]),
            reply_conversation: Mutex::new("conv-1".into()),
            lanes: Mutex::new(HashSet::new()),
            during: Mutex::new(None),
        });
        let (a, b, c, d, e, f) = (
            fake.clone(),
            fake.clone(),
            fake.clone(),
            fake.clone(),
            fake.clone(),
            fake.clone(),
        );
        let intervals = Arc::clone(&fake);
        let refreshes = Arc::clone(&fake);
        let hub = ProcessHub::new(
            ProcessDeps {
                spawn: Arc::new(move |job| {
                    if *lock(&f.hold) {
                        lock(&f.held).push(job);
                    } else {
                        job();
                    }
                }),
                notify: Arc::new(|| {}),
                clock_ms: Arc::new(move || a.now.load(Ordering::SeqCst)),
                epoch: Arc::new(|| 1_793_000_200.0),
                follow: Arc::new(move |target| {
                    lock(&d.follows).push(format!("+{}", target.key));
                    lock(&d.follow_error).clone().map_or(Ok(()), Err)
                }),
                unfollow: Arc::new(move |target| lock(&e.follows).push(format!("-{}", target.key))),
                set_interval: Arc::new(move |target, ms| {
                    lock(&intervals.intervals).push((target.key.clone(), ms))
                }),
                refresh: Arc::new(move |target| {
                    lock(&refreshes.refreshes).push(target.key.clone())
                }),
                read: Arc::new(move |computer, read| {
                    lock(&b.calls).push(format!("{computer:?} {read:?}"));
                    let Read::ProcessOutput {
                        process_id, run_id, ..
                    } = read
                    else {
                        panic!()
                    };
                    lock(&b.reads).push(process_id.clone());
                    let conversation = lock(&b.reply_conversation).clone();
                    Ok(
                        json!({"id": process_id, "run_id": run_id, "conversation_id": conversation,
                               "output": lock(&b.output).clone(), "truncated": false}),
                    )
                }),
                stop: Arc::new(move |target, action| {
                    lock(&c.calls).push(format!("{target:?} {action:?}"));
                    let Action::StopProcess { process_id, .. } = action else {
                        panic!()
                    };
                    let lane = format!("{}\n{}", target.computer, target.name);
                    if !lock(&c.lanes).insert(lane.clone()) {
                        return Err("busy".into());
                    }
                    lock(&c.stops).push(process_id.clone());
                    let during = lock(&c.during).take();
                    if let Some(during) = during {
                        during();
                    }
                    lock(&c.lanes).remove(&lane);
                    Ok(if *lock(&c.stop_ok) {
                        Outcome::Done {
                            receipt: json!({"id": process_id, "run_id": target.run_id,
                                            "conversation_id": target.conversation_id, "status": "requested"}),
                            cleanup: None,
                        }
                    } else {
                        Outcome::Failed {
                            detail: "Stop was not confirmed".into(),
                        }
                    })
                }),
            },
            allowed,
        );
        (hub, fake)
    }

    fn hub() -> (ProcessHub, Arc<Fake>) {
        hub_with(true)
    }

    /// The view key the webview would send back.
    fn key_of(hub: &ProcessHub) -> String {
        lock(&hub.0.hub).identity.clone()
    }

    /// A click in the view as shown now.
    fn select(hub: &ProcessHub, ids: &[&str], current: &str) {
        let ids = ids.iter().map(|id| id.to_string()).collect();
        hub.select(&key_of(hub), ids, Some(current.into()));
    }

    /// Stop selected as the view shows it now.
    fn stop(hub: &ProcessHub) {
        let (key, ids) = {
            let hub = lock(&hub.0.hub);
            (hub.identity.clone(), hub.selected_ids())
        };
        hub.stop(&key, &ids);
    }

    /// Runs the jobs held so far, in order, and returns how many ran.
    fn run_held(fake: &Fake) -> usize {
        let jobs = std::mem::take(&mut *lock(&fake.held));
        let ran = jobs.len();
        for job in jobs {
            job();
        }
        ran
    }

    fn target() -> ProcessTarget {
        ProcessTarget {
            computer: "studio".into(),
            key: "run:run-1".into(),
            name: "claude/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            archive_id: None,
            machine: "studio".into(),
            online: true,
            live: true,
            generation: None,
        }
    }

    fn process(id: &str, status: &str, source: &str) -> Value {
        json!({"id": id, "status": status, "source": source, "command": "cargo   test --workspace", "owner": "claude",
               "cwd": "/home/example/src/sample-project", "started_at": 100.0, "capabilities": {"stop": true, "output": true}})
    }

    fn state_of(key: &str, items: Vec<Value>) -> Batch {
        Batch {
            processes: vec![ProcessesOut {
                computer: ComputerId::from("studio"),
                key: key.into(),
                kind: "state".into(),
                payload: json!({"value": {"processes": {"items": items, "active_count": 2, "live_count": 1, "complete": true}}, "rev": 1}),
            }],
            ..Batch::default()
        }
    }

    fn state(items: Vec<Value>) -> Batch {
        state_of("run:run-1", items)
    }

    fn enabled(hub: &ProcessHub) {
        hub.settings(Settings {
            enabled: true,
            ..Settings::default()
        });
        hub.follow(Some(target()));
    }

    fn snapshot(hub: &ProcessHub) -> ProcessesView {
        lock(&hub.0.hub).changed = true;
        hub.take_view()
            .unwrap_or_else(|| lock(&hub.0.hub).last.clone().unwrap())
    }

    #[test]
    fn the_inventory_interval_tracks_visibility_and_settings_without_a_new_follow() {
        let (hub, fake) = hub();
        enabled(&hub);
        assert_eq!(*lock(&fake.intervals), [("run:run-1".into(), 30_000)]);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        select(&hub, &["p1"], "p1");
        hub.shown(true);
        hub.shown(true);
        hub.settings(Settings {
            enabled: true,
            active_seconds: 1.25,
            background_seconds: 40.0,
            ..Settings::default()
        });
        hub.shown(false);
        hub.follow(Some(target()));
        assert_eq!(
            *lock(&fake.intervals),
            [
                ("run:run-1".into(), 30_000),
                ("run:run-1".into(), 2_500),
                ("run:run-1".into(), 1_250),
                ("run:run-1".into(), 40_000),
            ]
        );
        assert_eq!(
            *lock(&fake.follows),
            ["+run:run-1"],
            "a cadence change keeps the subscription"
        );
        assert_eq!(snapshot(&hub).current.as_deref(), Some("p1"));
        hub.settings(Settings::default());
        let changes = lock(&fake.intervals).len();
        hub.shown(true);
        assert_eq!(
            lock(&fake.intervals).len(),
            changes,
            "disabled has no subscription"
        );
        enabled(&hub);
        assert_eq!(
            lock(&fake.intervals).last(),
            Some(&("run:run-1".into(), 2_500))
        );
    }

    #[test]
    fn every_current_stop_answer_requests_one_inventory_read_without_replaying_the_stop() {
        for confirmed in [true, false] {
            let (hub, fake) = hub();
            enabled(&hub);
            hub.observe(&mut state(vec![
                process("p1", "running", "hook"),
                process("p2", "running", "hook"),
            ]));
            select(&hub, &["p1", "p2"], "p1");
            *lock(&fake.stop_ok) = confirmed;
            stop(&hub);
            assert_eq!(*lock(&fake.refreshes), ["run:run-1", "run:run-1"]);
            hub.tick();
            assert_eq!(lock(&fake.stops).len(), 2);
            assert_eq!(lock(&fake.refreshes).len(), 2);
        }
    }

    #[test]
    fn a_stop_answer_after_close_never_refreshes_the_inventory() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        select(&hub, &["p1"], "p1");
        *lock(&fake.hold) = true;
        stop(&hub);
        hub.close();
        assert_eq!(run_held(&fake), 1);
        assert!(lock(&fake.refreshes).is_empty());
    }

    #[test]
    fn the_view_groups_labels_and_takes_its_entries_out_of_the_batch() {
        let (hub, _) = hub();
        enabled(&hub);
        let mut batch = state(vec![
            process("p1", "running", "process_tree"),
            process("p2", "unknown", "hook"),
            process("p3", "finished", "hook"),
        ]);
        hub.observe(&mut batch);
        assert!(batch.processes.is_empty());
        let view = hub.take_view().unwrap();
        let titles: Vec<&str> = view.groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Live shells / processes",
                "Recorded commands / unconfirmed",
                "Finished"
            ]
        );
        assert_eq!(view.groups[0].rows[0].command, "cargo test --workspace");
        assert_eq!(view.groups[1].rows[0].status, "Unconfirmed");
        assert_eq!(
            view.summary,
            "1 live    0 active    1 unconfirmed    2 recorded"
        );
        assert_eq!(
            view.current.as_deref(),
            Some("p1"),
            "the first live process is selected"
        );
        assert_eq!(view.count, 2);
        assert_eq!(
            view.detail.unwrap().details,
            "cargo   test --workspace\nstudio  /home/example/src/sample-project  (Running)"
        );
        assert!(
            hub.take_view().is_none(),
            "an unchanged view is not sent again"
        );
    }

    #[test]
    fn output_is_read_while_shown_at_qts_cadence() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        hub.tick();
        assert!(lock(&fake.reads).is_empty(), "a hidden tab reads no output");
        hub.shown(true);
        hub.tick();
        assert_eq!(*lock(&fake.reads), ["p1"]);
        fake.now.store(1000, Ordering::SeqCst);
        hub.tick();
        assert_eq!(
            lock(&fake.reads).len(),
            1,
            "within 2.5 s nothing is read again"
        );
        fake.now.store(2600, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 2);
        let detail = hub.take_view().unwrap().detail.unwrap();
        assert_eq!(detail.output.as_deref(), Some("running 42 tests"));
    }

    #[test]
    fn stops_run_one_at_a_time_and_are_never_retried() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        select(&hub, &["p1", "p2"], "p1");
        let view = hub.take_view().unwrap();
        assert_eq!(view.stop.label, "Stop selected (2)");
        assert!(view.stop.enabled);
        *lock(&fake.stop_ok) = false;
        stop(&hub);
        assert_eq!(*lock(&fake.stops), ["p1", "p2"]);
        let view = hub.take_view().unwrap();
        assert!(view
            .stop_notice
            .contains("2 not confirmed. No automatic retries."));
        hub.tick();
        assert_eq!(lock(&fake.stops).len(), 2);
    }

    #[test]
    fn a_new_identity_cancels_unsent_stops() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        select(&hub, &["p1"], "p1");
        let mut other = target();
        other.run_id = "run-2".into();
        other.key = "run:run-2".into();
        hub.follow(Some(other));
        stop(&hub);
        assert!(lock(&fake.stops).is_empty());
    }

    #[test]
    fn the_next_stop_waits_for_the_answer_of_the_one_in_flight() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
            process("p3", "running", "hook"),
        ]));
        select(&hub, &["p3", "p1", "p2"], "p1");
        *lock(&fake.hold) = true;
        stop(&hub);
        let view = snapshot(&hub);
        assert_eq!(view.stop_notice, "Sending stop requests…");
        assert!(!view.stop.enabled);
        assert_eq!(view.stop.tooltip, "Sending stop requests…");
        assert_eq!(run_held(&fake), 1);
        assert_eq!(
            *lock(&fake.stops),
            ["p3"],
            "the selection's order, one at a time"
        );
        stop(&hub);
        hub.tick();
        assert_eq!(run_held(&fake), 1, "a second Stop sends nothing more");
        assert_eq!(*lock(&fake.stops), ["p3", "p1"]);
        assert_eq!(run_held(&fake), 1);
        assert_eq!(run_held(&fake), 0);
        assert_eq!(*lock(&fake.stops), ["p3", "p1", "p2"]);
        let view = snapshot(&hub);
        assert_eq!(
            view.stop_notice,
            "Stop requested for 3 process(es). Waiting for completion."
        );
        assert_eq!(
            view.stop.tooltip,
            "A stop request is already pending for a selected process."
        );
        // The inventory ends them; the marks go.
        hub.observe(&mut state(vec![
            process("p1", "finished", "hook"),
            process("p2", "finished", "hook"),
            process("p3", "finished", "hook"),
        ]));
        assert!(lock(&hub.0.hub).queue.pending.is_empty());
    }

    #[test]
    fn a_generation_change_drops_the_queue_and_the_late_answer() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        select(&hub, &["p1", "p2"], "p1");
        *lock(&fake.hold) = true;
        stop(&hub);
        // The host restarted: the same session under a new generation.
        let mut restarted = target();
        restarted.generation = Some("gen-2".into());
        hub.follow(Some(restarted));
        assert_eq!(
            *lock(&fake.follows),
            ["+run:run-1"],
            "the topic follow stays"
        );
        let view = snapshot(&hub);
        assert_eq!(view.stop_notice, "");
        assert_eq!(
            view.key,
            "studio\nclaude/sample-project/review\n\nrun-1\nconv-1\ngen-2"
        );
        assert_eq!(view.current.as_deref(), Some("p1"));
        assert!(view.stop.enabled, "the old stop no longer counts");
        // A stop of the new generation waits until the old one frees the
        // session's stop lane, so it never answers busy.
        select(&hub, &["p1", "p2"], "p1");
        stop(&hub);
        assert_eq!(snapshot(&hub).stop_notice, "Sending stop requests…");
        assert_eq!(lock(&fake.held).len(), 1, "only the old stop is out");
        assert_eq!(run_held(&fake), 1, "the old stop answers");
        assert!(
            lock(&fake.refreshes).is_empty(),
            "an old generation never refreshes the new inventory"
        );
        assert_eq!(
            snapshot(&hub).stop_notice,
            "Sending stop requests…",
            "the old answer neither ends nor steps the new queue"
        );
        assert_eq!(lock(&fake.held).len(), 1, "the new stop goes out now");
        assert_eq!(run_held(&fake), 1);
        assert_eq!(run_held(&fake), 1);
        assert_eq!(run_held(&fake), 0);
        assert_eq!(*lock(&fake.stops), ["p1", "p1", "p2"]);
        assert_eq!(
            snapshot(&hub).stop_notice,
            "Stop requested for 2 process(es). Waiting for completion."
        );
        // The new stops name the new generation.
        assert_eq!(
            lock(&fake.calls)[1],
            "Target { computer: \"studio\", name: \"claude/sample-project/review\", run_id: \"run-1\", conversation_id: \"conv-1\" } \
             StopProcess { process_id: \"p1\", archive_id: None, generation: Some(\"gen-2\") }"
        );
        assert!(lock(&fake.calls)[0].contains("generation: None"));
        *lock(&fake.hold) = false;
        hub.shown(true);
        assert_eq!(
            lock(&fake.calls).last().unwrap(),
            "ComputerId(\"studio\") ProcessOutput { name: \"claude/sample-project/review\", process_id: \"p1\", \
             run_id: \"run-1\", conversation_id: \"conv-1\", archive_id: None, generation: Some(\"gen-2\") }"
        );
    }

    #[test]
    fn every_identity_field_resets_the_view() {
        let fields: [fn(&mut ProcessTarget); 5] = [
            |t| t.computer = "mac".into(),
            |t| t.name = "claude/sample-project/renamed".into(),
            |t| t.archive_id = Some("archive-1".into()),
            |t| t.conversation_id = "conv-2".into(),
            |t| t.generation = Some("gen-2".into()),
        ];
        let two = || {
            state(vec![
                process("p1", "running", "hook"),
                process("p2", "running", "hook"),
            ])
        };
        for change in fields {
            let (hub, fake) = hub();
            enabled(&hub);
            hub.observe(&mut two());
            select(&hub, &["p1", "p2"], "p2");
            *lock(&fake.hold) = true;
            stop(&hub);
            let before = snapshot(&hub).key;
            let mut other = target();
            change(&mut other);
            hub.follow(Some(other));
            let view = snapshot(&hub);
            assert_ne!(view.key, before);
            assert_eq!(view.stop_notice, "", "{:?}", view.key);
            assert_ne!(view.selected, ["p1", "p2"], "{:?}", view.key);
            assert_eq!(run_held(&fake), 1, "the stop in flight ends");
            assert_eq!(run_held(&fake), 0, "the unsent one is cancelled");
            assert_eq!(*lock(&fake.stops), ["p1"]);
            assert_eq!(snapshot(&hub).stop_notice, "");
        }
        // Online and live are not part of it: the queue goes on (as unavailable).
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut two());
        select(&hub, &["p1", "p2"], "p2");
        *lock(&fake.hold) = true;
        stop(&hub);
        let mut offline = target();
        offline.online = false;
        hub.follow(Some(offline));
        assert_eq!(snapshot(&hub).selected, ["p1", "p2"]);
        assert_eq!(run_held(&fake), 1);
        assert_eq!(*lock(&fake.stops), ["p1"]);
        assert_eq!(
            snapshot(&hub).stop_notice,
            "Stop requested for 1 process(es). Waiting for completion. 1 not confirmed. No automatic retries.\nMachine or session became unavailable; remaining requests were cancelled."
        );
    }

    #[test]
    fn a_finished_process_is_read_again_only_when_its_output_changed() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        let mut done = process("p1", "finished", "hook");
        hub.observe(&mut state(vec![done.clone()]));
        assert_eq!(lock(&fake.reads).len(), 1, "a new selection reads at once");
        fake.now.store(60_000, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 1, "finished and unchanged");
        hub.observe(&mut state(vec![done.clone()]));
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 1, "the same revision");
        done["output_at"] = json!(5.0);
        hub.observe(&mut state(vec![done.clone()]));
        assert_eq!(lock(&fake.reads).len(), 2, "a new output revision");
        // Unconfirmed: max(unconfirmed, active) = 10 s.
        let (hub, fake) = hub_with(true);
        enabled(&hub);
        hub.shown(true);
        hub.observe(&mut state(vec![process("p1", "unknown", "hook")]));
        fake.now.store(9_999, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 1);
        fake.now.store(10_000, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 2);
    }

    #[test]
    fn the_settings_are_clamped_as_qt_clamps_them() {
        let (hub, fake) = hub();
        hub.settings(Settings {
            enabled: true,
            active_seconds: 0.2,
            loading_seconds: 9.0,
            ..Settings::default()
        });
        hub.follow(Some(target()));
        hub.shown(true);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        assert_eq!(lock(&fake.reads).len(), 1);
        fake.now.store(999, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 1, "at least 1 s apart");
        fake.now.store(1000, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 2);
        // The loading indicator waits at most 5 s.
        *lock(&fake.hold) = true;
        fake.now.store(2000, Ordering::SeqCst);
        hub.tick();
        assert!(!snapshot(&hub).busy);
        fake.now.store(6999, Ordering::SeqCst);
        hub.tick();
        assert!(!snapshot(&hub).busy);
        fake.now.store(7000, Ordering::SeqCst);
        hub.tick();
        assert!(snapshot(&hub).busy);
        run_held(&fake);
        assert!(!snapshot(&hub).busy);
    }

    #[test]
    fn the_store_gives_the_five_settings() {
        let dir =
            std::env::temp_dir().join(format!("zerus-processes-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = zerus_client_core::settings::Settings::open(dir.join("settings.json"), None);
        assert_eq!(stored(&store), Settings::default());
        store.set("processes/enabled", json!(true)).unwrap();
        store.set("processes/activeSeconds", json!(0.2)).unwrap();
        store.set("processes/unknownSeconds", json!(700)).unwrap();
        store.set("processes/loadingSeconds", json!(2)).unwrap();
        let read = stored(&store);
        assert!(read.enabled);
        assert_eq!(read.active_ms(), 1000);
        assert_eq!(read.unknown_ms(), 600_000);
        assert_eq!(read.loading_ms(), 2000);
        assert_eq!(read.background_ms(), 30_000);
        let (hub, fake) = hub();
        hub.follow(Some(target()));
        assert!(
            lock(&fake.follows).is_empty(),
            "disabled: nothing is followed"
        );
        hub.settings_from(&store);
        assert_eq!(*lock(&fake.follows), ["+run:run-1"]);
        store.set("processes/enabled", json!(false)).unwrap();
        hub.settings_from(&store);
        assert_eq!(*lock(&fake.follows), ["+run:run-1", "-run:run-1"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabling_drops_the_view_and_its_requests() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        assert_eq!(lock(&fake.reads).len(), 1);
        hub.settings(Settings::default());
        assert_eq!(*lock(&fake.follows), ["+run:run-1", "-run:run-1"]);
        let view = snapshot(&hub);
        assert!(view.groups.is_empty() && view.current.is_none());
        assert_eq!(
            view.stop.tooltip,
            "Enable Processes in Settings to use process control."
        );
        assert_eq!(view.count, 0);
        fake.now.store(60_000, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 1);
        let mut batch = state(vec![process("p1", "running", "hook")]);
        hub.observe(&mut batch);
        assert_eq!(batch.processes.len(), 1, "no longer followed");
    }

    #[test]
    fn rows_keep_qts_tree_order_and_a_removed_current_row_passes_to_its_sibling() {
        let (hub, _) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![
            process("a", "running", "hook"),
            process("b", "running", "hook"),
            process("c", "running", "hook"),
        ]));
        select(&hub, &["b"], "b");
        // The inventory sorts new work first; the tree keeps first-seen order.
        hub.observe(&mut state(vec![
            process("d", "running", "hook"),
            process("a", "finished", "hook"),
            process("c", "running", "hook"),
        ]));
        let view = snapshot(&hub);
        let ids = |group: usize| -> Vec<&str> {
            view.groups[group]
                .rows
                .iter()
                .map(|row| row.id.as_str())
                .collect()
        };
        assert_eq!(ids(0), ["c", "d"]);
        assert_eq!(ids(1), ["a"]);
        assert_eq!(
            view.current.as_deref(),
            Some("c"),
            "the row above went to Finished first"
        );
        assert!(view.selected.is_empty(), "the current row is not selected");
    }

    #[test]
    fn a_session_shown_again_gets_its_selection_and_output_back() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        select(&hub, &["p1", "p2"], "p2");
        assert_eq!(*lock(&fake.reads), ["p1", "p2"]);
        let mut other = target();
        other.key = "run:run-2".into();
        other.run_id = "run-2".into();
        hub.follow(Some(other.clone()));
        assert!(snapshot(&hub).current.is_none(), "no inventory yet");
        hub.observe(&mut state_of(
            "run:run-2",
            vec![process("q1", "finished", "hook")],
        ));
        assert_eq!(snapshot(&hub).current.as_deref(), Some("q1"));
        *lock(&fake.hold) = true;
        hub.follow(Some(target()));
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        let view = snapshot(&hub);
        assert_eq!(view.selected, ["p1", "p2"]);
        assert_eq!(view.current.as_deref(), Some("p2"));
        assert_eq!(
            view.detail.unwrap().output.as_deref(),
            Some("running 42 tests"),
            "the saved output shows while it is read again"
        );
        assert_eq!(lock(&fake.held).len(), 1);
        // A session left before its inventory came keeps the view it had.
        hub.follow(Some(other.clone()));
        hub.follow(Some(target()));
        hub.follow(Some(other));
        hub.observe(&mut state_of(
            "run:run-2",
            vec![
                process("q0", "finished", "hook"),
                process("q1", "finished", "hook"),
            ],
        ));
        assert_eq!(snapshot(&hub).current.as_deref(), Some("q1"));
    }

    #[test]
    fn a_new_selection_drops_the_read_in_flight() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        *lock(&fake.hold) = true;
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        select(&hub, &["p2"], "p2");
        let mut jobs = std::mem::take(&mut *lock(&fake.held)).into_iter();
        assert_eq!(jobs.len(), 2, "p1's read, then p2's at once");
        *lock(&fake.output) = json!("p1 output");
        (jobs.next().unwrap())();
        let detail = snapshot(&hub).detail.unwrap();
        assert_eq!(
            (detail.id.as_str(), detail.output),
            ("p2", None),
            "p1's late answer is dropped"
        );
        *lock(&fake.output) = json!("p2 output");
        (jobs.next().unwrap())();
        assert_eq!(*lock(&fake.reads), ["p1", "p2"]);
        let detail = snapshot(&hub).detail.unwrap();
        assert_eq!(detail.output.as_deref(), Some("p2 output"));
    }

    #[test]
    fn an_answer_of_another_session_keeps_the_output_and_says_so() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        assert_eq!(
            snapshot(&hub).detail.unwrap().output.as_deref(),
            Some("running 42 tests")
        );
        // The session moved on: the next answer names another conversation.
        *lock(&fake.reply_conversation) = "conv-2".into();
        *lock(&fake.output) = json!("another conversation's output");
        fake.now.store(2500, Ordering::SeqCst);
        hub.tick();
        assert_eq!(lock(&fake.reads).len(), 2);
        let view = snapshot(&hub);
        let detail = view.detail.unwrap();
        assert_eq!(detail.notice, "Session changed. Refreshing processes…");
        assert_eq!(
            detail.output.as_deref(),
            Some("running 42 tests"),
            "the shown output stays"
        );
        assert_eq!(view.groups.len(), 1, "and so does the inventory");
        assert_eq!(*lock(&fake.refreshes), ["run:run-1"]);
    }

    #[test]
    fn one_output_read_runs_at_a_time() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        *lock(&fake.hold) = true;
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        assert_eq!(lock(&fake.held).len(), 1);
        for at in [2500, 5000, 60_000] {
            fake.now.store(at, Ordering::SeqCst);
            hub.tick();
        }
        // Showing again forces a read, but not beside the one under way.
        hub.shown(false);
        hub.shown(true);
        assert_eq!(lock(&fake.held).len(), 1, "none starts while one runs");
        assert_eq!(run_held(&fake), 1);
        assert_eq!(*lock(&fake.reads), ["p1"]);
        hub.tick();
        assert_eq!(lock(&fake.held).len(), 1, "the next one after the answer");
    }

    #[test]
    fn the_queue_stops_when_the_session_stops_being_live() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        select(&hub, &["p1", "p2"], "p1");
        *lock(&fake.hold) = true;
        stop(&hub);
        let mut stopped = target();
        stopped.live = false;
        hub.follow(Some(stopped));
        assert_eq!(run_held(&fake), 1);
        assert_eq!(run_held(&fake), 0, "p2 is never sent");
        assert_eq!(*lock(&fake.stops), ["p1"]);
        assert_eq!(
            snapshot(&hub).stop_notice,
            "Stop requested for 1 process(es). Waiting for completion. 1 not confirmed. No automatic retries.\nMachine or session became unavailable; remaining requests were cancelled."
        );
    }

    #[test]
    fn busy_clears_on_a_new_identity() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        *lock(&fake.hold) = true;
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        fake.now.store(600, Ordering::SeqCst);
        hub.tick();
        assert!(snapshot(&hub).busy, "a read past the 0.5 s delay");
        let mut other = target();
        other.key = "run:run-2".into();
        other.run_id = "run-2".into();
        hub.follow(Some(other));
        assert!(!snapshot(&hub).busy);
        fake.now.store(5000, Ordering::SeqCst);
        hub.tick();
        assert!(!snapshot(&hub).busy, "the old read no longer counts");
        assert_eq!(run_held(&fake), 1);
        assert!(
            snapshot(&hub).detail.is_none(),
            "and its answer shows nowhere"
        );
    }

    #[test]
    fn a_stale_stop_click_never_stops_the_auto_selected_process() {
        // The review's probe: the user selected p2, then the host restarted
        // and the new view selected p1 by default before the click arrived.
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        select(&hub, &["p2"], "p2");
        let (clicked, ids) = (key_of(&hub), vec!["p2".to_owned()]);
        let mut restarted = target();
        restarted.generation = Some("gen-2".into());
        hub.follow(Some(restarted));
        assert_eq!(snapshot(&hub).selected, ["p1"]);
        hub.stop(&clicked, &ids);
        assert!(lock(&fake.stops).is_empty(), "another view's click");
        let now = key_of(&hub);
        hub.stop(&now, &ids);
        assert!(lock(&fake.stops).is_empty(), "another selection");
        assert_eq!(snapshot(&hub).stop_notice, "");
        hub.stop(&now, &["p1".to_owned()]);
        assert_eq!(*lock(&fake.stops), ["p1"]);
    }

    #[test]
    fn a_click_in_another_identitys_view_stops_nothing_even_for_the_same_ids() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        select(&hub, &["p1"], "p1");
        let clicked = key_of(&hub);
        let mut moved = target();
        moved.conversation_id = "conv-2".into();
        hub.follow(Some(moved));
        assert_eq!(snapshot(&hub).selected, ["p1"], "selected again by default");
        hub.stop(&clicked, &["p1".to_owned()]);
        assert!(lock(&fake.stops).is_empty());
        assert_eq!(snapshot(&hub).stop_notice, "");
        hub.stop(&key_of(&hub), &["p1".to_owned()]);
        assert_eq!(*lock(&fake.stops), ["p1"]);
    }

    #[test]
    fn a_selection_made_in_another_sessions_view_changes_nothing() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        let old = key_of(&hub);
        let mut restarted = target();
        restarted.generation = Some("gen-2".into());
        hub.follow(Some(restarted));
        let reads = lock(&fake.reads).len();
        hub.select(&old, vec!["p2".into()], Some("p2".into()));
        let view = snapshot(&hub);
        assert_eq!(
            (view.selected, view.current.as_deref()),
            (vec!["p1".to_owned()], Some("p1"))
        );
        assert_eq!(lock(&fake.reads).len(), reads, "nothing is read for it");
        select(&hub, &["p2"], "p2");
        assert_eq!(snapshot(&hub).current.as_deref(), Some("p2"));
    }

    #[test]
    fn a_new_identitys_stop_waits_for_the_old_stops_lane() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![
            process("p1", "running", "hook"),
            process("p2", "running", "hook"),
        ]));
        select(&hub, &["p1"], "p1");
        let during = hub.clone();
        *lock(&fake.during) = Some(Box::new(move || {
            // While p1's stop holds the session's lane, the host restarts and
            // the user stops p1 again.
            let mut restarted = target();
            restarted.generation = Some("gen-2".into());
            during.follow(Some(restarted));
            select(&during, &["p1"], "p1");
            stop(&during);
        }));
        stop(&hub);
        let view = snapshot(&hub);
        assert_eq!(
            view.stop_notice, "Stop requested for 1 process(es). Waiting for completion.",
            "nothing answers busy"
        );
        assert_eq!(*lock(&fake.stops), ["p1", "p1"]);
        assert!(lock(&fake.calls)[1].contains("generation: Some(\"gen-2\")"));
        assert!(lock(&fake.lanes).is_empty());
        assert!(!view.busy && lock(&hub.0.hub).draining.is_empty());
    }

    #[test]
    fn an_archived_session_is_never_followed() {
        let (hub, fake) = hub();
        hub.settings(Settings {
            enabled: true,
            ..Settings::default()
        });
        let archived = ProcessTarget {
            key: "archive:a1".into(),
            archive_id: Some("a1".into()),
            live: false,
            ..target()
        };
        hub.follow(Some(target()));
        hub.follow(Some(archived.clone()));
        assert_eq!(*lock(&fake.follows), ["+run:run-1", "-run:run-1"]);
        hub.follow(Some(archived));
        assert_eq!(lock(&fake.follows).len(), 2, "never followed");
        let view = snapshot(&hub);
        assert!(view.groups.is_empty() && view.failed.is_none());
        assert_eq!(view.notes, ARCHIVED);
        assert_eq!(view.summary, "No shell commands recorded yet");
        assert_eq!(view.count, 0);
    }

    #[test]
    fn output_less_processes_and_replays_read_nothing() {
        let (hub, fake) = hub();
        enabled(&hub);
        hub.shown(true);
        let mut tree = process("p1", "running", "process_tree");
        tree["capabilities"]["output"] = json!(false);
        hub.observe(&mut state(vec![tree]));
        assert!(lock(&fake.reads).is_empty());
        assert_eq!(
            snapshot(&hub).detail.unwrap().notice,
            "Live OS process. Output remains in the recorded command or Terminal."
        );
        let (hub, fake) = hub_with(false);
        enabled(&hub);
        hub.shown(true);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        select(&hub, &["p1"], "p1");
        stop(&hub);
        assert!(lock(&fake.reads).is_empty() && lock(&fake.stops).is_empty());
        assert!(lock(&fake.refreshes).is_empty());
        let view = snapshot(&hub);
        assert_eq!(
            view.detail.unwrap().notice,
            "Actions in replay mode need ZERUS_CLIENT_HGS."
        );
        assert_eq!(
            view.stop_notice,
            "Actions in replay mode need ZERUS_CLIENT_HGS."
        );
    }

    #[test]
    fn follows_failures_and_endings() {
        let (hub, fake) = hub();
        *lock(&fake.follow_error) =
            Some("the computer already follows as many as its stream allows".into());
        enabled(&hub);
        assert_eq!(
            snapshot(&hub).failed.as_deref(),
            Some("the computer already follows as many as its stream allows")
        );
        *lock(&fake.follow_error) = None;
        hub.follow(Some(target()));
        assert_eq!(*lock(&fake.follows), ["+run:run-1", "+run:run-1"]);
        hub.follow(Some(target()));
        assert_eq!(
            lock(&fake.follows).len(),
            2,
            "a followed session is not followed twice"
        );
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        let mut failed = state(vec![]);
        failed.processes[0].kind = "failed".into();
        failed.processes[0].payload = json!({"code": "inspect_failed", "message": "ps failed"});
        hub.observe(&mut failed);
        let shown = snapshot(&hub);
        assert_eq!(shown.failed.as_deref(), Some("ps failed"));
        assert_eq!(shown.groups.len(), 1, "the last inventory stays");
        let mut ended = state(vec![]);
        ended.processes[0].kind = "ended".into();
        ended.processes[0].payload = Value::Null;
        hub.observe(&mut ended);
        assert!(snapshot(&hub).groups.is_empty());
        // The same session follows again on the next follow.
        hub.follow(Some(target()));
        assert_eq!(
            *lock(&fake.follows),
            ["+run:run-1", "+run:run-1", "+run:run-1"]
        );
    }

    #[test]
    fn the_count_is_zero_for_a_recorded_session() {
        let (hub, _) = hub();
        enabled(&hub);
        hub.observe(&mut state(vec![process("p1", "running", "hook")]));
        assert_eq!(snapshot(&hub).count, 2);
        let mut recorded = target();
        recorded.live = false;
        hub.follow(Some(recorded));
        let view = snapshot(&hub);
        assert_eq!(view.count, 0);
        assert_eq!(view.groups[0].rows[0].status, "Unconfirmed");
        assert_eq!(view.groups[0].rows[0].tone, Tone::Muted);
    }
}
