//! New session's background host (SessionCreationController; SessionsWindow.cpp:1509-1593,
//! 3020-3070). One worker owns the core queue. Journal writes are ordered on
//! another thread; calls, launches and bounded binding reads run on blocking
//! jobs. Every acknowledgement returns to the queue, never to a second owner.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use zerus_client_core::creation::{
    self, BindingListing, BindingSession, Creations, Effect, Journal, Remember,
};
use zerus_client_core::launch::{self, Launch, Project};
use zerus_client_core::paths::{self, Style};
use zerus_client_core::swarm::{self, edit::Intent};
use zerus_client_core::{Change, ComputerId};

const PARK: Duration = Duration::from_millis(100);
const FLUSH: Duration = Duration::from_secs(2);
const STOPPED: &str = "Session creation stopped.";
type Job = Box<dyn FnOnce() + Send>;
type JournalWriter = Arc<dyn Fn(&Path, &[u8]) -> bool + Send + Sync>;
type LaunchSession = Arc<dyn Fn(&Value) -> Result<(), String> + Send + Sync>;
type ReadBinding = Arc<dyn Fn(&ComputerId) -> Result<BindingListing, String> + Send + Sync>;
type Resolve = Arc<dyn Fn(&mut Value, &Value) -> Result<(), String> + Send + Sync>;
type Bind = Arc<dyn Fn(&Value, &BindingSession) -> Result<bool, String> + Send + Sync>;

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs() as i64)
}

/// The wire shapes are shared with New session Task 6.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Admission {
    Admitted { admitted: String },
    Refused { refused: String },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreationAction {
    Resume,
    Check,
    Stop,
    Hide,
    Show,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CreationCard {
    id: String,
    tag: String,
    state: String,
    caption: String,
    error: String,
    created: i64,
    group: String,
    host: String,
    machine: String,
    cmd: String,
    account: String,
    target: String,
    show_pending: bool,
    launch_dispatched: bool,
    stop_requested: bool,
    worktree_receipts: bool,
    can_resume: bool,
    can_edit: bool,
}

impl CreationCard {
    fn of(id: &str, operation: &Value, can_edit: bool) -> Self {
        Self {
            id: id.into(),
            tag: text(operation, "tag").into(),
            state: text(operation, "state").into(),
            caption: creation::caption(text(operation, "state")).into(),
            error: text(operation, "error").into(),
            created: operation["created"].as_i64().unwrap_or_default(),
            group: text(operation, "group").into(),
            host: text(operation, "host").into(),
            machine: text(operation, "machine").into(),
            cmd: text(operation, "cmd").into(),
            account: text(operation, "account").into(),
            target: text(operation, "target").into(),
            show_pending: flag(operation, "show_pending"),
            launch_dispatched: flag(operation, "launch_dispatched"),
            stop_requested: flag(operation, "stop_requested"),
            worktree_receipts: flag(operation, "worktree_receipts"),
            can_resume: creation::can_resume(operation),
            can_edit,
        }
    }
}

/// All effects are replaceable in tests; production uses the core's real
/// guarded calls and terminal launcher, never a stand-in.
struct Services {
    allowed: bool,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
    ids: Arc<dyn Fn() -> String + Send + Sync>,
    write: JournalWriter,
    call: Arc<dyn Fn(&creation::Call) -> creation::Answer + Send + Sync>,
    launch: LaunchSession,
    binding: ReadBinding,
    online: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    resolve: Resolve,
    bind: Bind,
    remember: Arc<dyn Fn(&Remember) + Send + Sync>,
    emit: Arc<dyn Fn(Value) + Send + Sync>,
    spawn: Arc<dyn Fn(Job) + Send + Sync>,
}

#[derive(Default)]
struct Cache {
    operations: BTreeMap<String, Value>,
    cards: BTreeMap<String, CreationCard>,
}

enum Input {
    Submit(Value, SyncSender<Admission>),
    Action(String, CreationAction, SyncSender<Result<(), String>>),
    Catalog,
    Flush(SyncSender<()>),
}

enum Done {
    Written(u64, bool),
    Called(u64, creation::Answer),
    Launched(String, String, Result<(), String>),
    Binding(String, Result<BindingListing, String>),
}

enum Write {
    Journal(u64, Vec<u8>, Option<Remember>),
    Flush(SyncSender<()>),
}

struct Shared {
    input: SyncSender<Input>,
    cache: Arc<Mutex<Cache>>,
    catalog: Arc<AtomicBool>,
    quitting: Arc<AtomicBool>,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

/// Clones share the sole queue worker. The cached reads never do I/O.
#[derive(Clone)]
pub struct CreationHost(Arc<Shared>);

impl CreationHost {
    fn new(file: PathBuf, services: Services) -> Self {
        let services = Arc::new(services);
        let (input, received) = mpsc::sync_channel(128);
        let (done, finished) = mpsc::channel();
        let (writer, writes) = mpsc::sync_channel(8);
        let cache = Arc::new(Mutex::new(Cache::default()));
        let catalog = Arc::new(AtomicBool::new(false));
        let quitting = Arc::new(AtomicBool::new(false));
        let host = Self(Arc::new(Shared {
            input,
            cache: Arc::clone(&cache),
            catalog: Arc::clone(&catalog),
            quitting: Arc::clone(&quitting),
            clock: Arc::clone(&services.clock),
        }));
        let writing = Arc::clone(&services);
        let written = done.clone();
        let path = file.clone();
        std::thread::Builder::new()
            .name("zerus-creation-journal".into())
            .spawn(move || {
                for message in writes {
                    match message {
                        Write::Journal(write, bytes, remember) => {
                            let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                (writing.write)(&path, &bytes)
                            }))
                            .unwrap_or(false);
                            if ok {
                                if let Some(remember) = remember {
                                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                        || (writing.remember)(&remember),
                                    ));
                                }
                            }
                            let _ = written.send(Done::Written(write, ok));
                        }
                        Write::Flush(answer) => {
                            let _ = answer.send(());
                        }
                    }
                }
            })
            .expect("the creation journal thread starts");
        std::thread::Builder::new()
            .name("zerus-creations".into())
            .spawn(move || {
                let clock = Arc::clone(&services.clock);
                let ids = Arc::clone(&services.ids);
                let mut worker = Worker {
                    queue: Creations::new(move || clock(), move || ids()),
                    services,
                    done,
                    writer,
                    cache,
                    catalog,
                    polls: BTreeMap::new(),
                    shutdown: quitting,
                    quitting: false,
                };
                let effects = worker.queue.load(load(&file));
                worker.effects(effects);
                loop {
                    worker.stop_if_requested();
                    while let Ok(done) = finished.try_recv() {
                        worker.stop_if_requested();
                        if !worker.quitting {
                            worker.finished(done);
                        }
                    }
                    match received.recv_timeout(PARK) {
                        Ok(message) => worker.input(message),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            worker.queue.quit();
                            break;
                        }
                    }
                    if !worker.quitting {
                        // Catalog wakeups coalesce; reads and reconciliation are
                        // bounded by the 128-operation queue, never the full fleet.
                        worker.catalog.swap(false, Ordering::AcqRel);
                        worker.reconcile();
                        worker.poll();
                    }
                }
            })
            .expect("the creation worker starts");
        host
    }

    /// The source drain only sets a coalesced flag and tries a bounded send.
    /// It never waits for the worker, journal or an organization lock.
    pub fn observe(&self, change: &Change) {
        if matches!(
            change,
            Change::Upsert { .. }
                | Change::Replace { .. }
                | Change::Remove { .. }
                | Change::Computer { .. }
                | Change::Machine { .. }
        ) && !self.0.catalog.swap(true, Ordering::AcqRel)
        {
            let _ = self.0.input.try_send(Input::Catalog);
        }
    }

    fn submit(&self, plan: Value) -> Result<Admission, String> {
        let (send, answer) = mpsc::sync_channel(1);
        self.0
            .input
            .send(Input::Submit(plan, send))
            .map_err(|_| STOPPED.to_owned())?;
        answer.recv().map_err(|_| STOPPED.to_owned())
    }

    pub fn view(&self) -> Vec<CreationCard> {
        let mut cards: Vec<_> = lock(&self.0.cache)
            .cards
            .values()
            .filter(|card| card.state != "bound")
            .cloned()
            .collect();
        cards.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
        cards
    }

    fn details(&self, id: &str) -> Option<String> {
        let operation = lock(&self.0.cache).operations.get(id)?.clone();
        Some(creation::details(
            &operation,
            (self.0.clock)(),
            &|at, stamp| {
                Local
                    .timestamp_opt(at, 0)
                    .single()
                    .map(|time| match stamp {
                        creation::Stamp::Created => time.format("%Y-%m-%dT%H:%M:%S").to_string(),
                        creation::Stamp::Step => time.format("%H:%M:%S").to_string(),
                    })
                    .unwrap_or_default()
            },
        ))
    }

    fn plan(&self, id: &str) -> Option<Value> {
        let cache = lock(&self.0.cache);
        cache
            .cards
            .get(id)?
            .can_edit
            .then(|| cache.operations.get(id).cloned())
            .flatten()
    }

    fn action(&self, id: String, action: CreationAction) -> Result<(), String> {
        let (send, answer) = mpsc::sync_channel(1);
        self.0
            .input
            .send(Input::Action(id, action, send))
            .map_err(|_| STOPPED.to_owned())?;
        answer.recv().map_err(|_| STOPPED.to_owned())?
    }

    fn flush(&self, limit: Duration) -> bool {
        self.0.quitting.store(true, Ordering::Release);
        let (send, answer) = mpsc::sync_channel(1);
        // Exit must remain bounded even when the worker is stalled. A full
        // channel leaves the durable barrier and recovery journal in charge.
        self.0.input.try_send(Input::Flush(send)).is_ok() && answer.recv_timeout(limit).is_ok()
    }
}

fn load(path: &Path) -> Journal {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Journal::Missing,
        Err(_) => return Journal::Unreadable,
    };
    let mut bytes = Vec::new();
    match file
        .take(creation::JOURNAL_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
    {
        Ok(_) => Journal::Bytes(bytes),
        Err(_) => Journal::Unreadable,
    }
}

#[derive(Default)]
struct Poll {
    running: bool,
    force: bool,
    last: Option<Instant>,
    listing: Option<BindingListing>,
}

struct Worker {
    queue: Creations,
    services: Arc<Services>,
    done: Sender<Done>,
    writer: SyncSender<Write>,
    cache: Arc<Mutex<Cache>>,
    catalog: Arc<AtomicBool>,
    polls: BTreeMap<String, Poll>,
    shutdown: Arc<AtomicBool>,
    quitting: bool,
}

struct Placement(Resolve);
impl creation::Placement for Placement {
    fn resolve(&mut self, plan: &mut Value, evidence: &Value) -> Result<(), String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.0)(plan, evidence)))
            .unwrap_or_else(|_| Err("Project placement stopped.".into()))
    }
}

impl Worker {
    fn stop_if_requested(&mut self) {
        if self.shutdown.load(Ordering::Acquire) {
            self.quitting = true;
            self.queue.quit();
        }
    }

    fn input(&mut self, message: Input) {
        self.stop_if_requested();
        match message {
            Input::Submit(plan, answer) => {
                let (result, effects) = if self.quitting || !self.services.allowed {
                    (
                        Err(if self.quitting {
                            STOPPED
                        } else {
                            crate::newsession::REPLAY_NEEDS_HGS
                        }
                        .into()),
                        Vec::new(),
                    )
                } else {
                    self.queue.submit(plan)
                };
                self.effects(effects);
                let _ = answer.send(match result {
                    Ok(admitted) => Admission::Admitted { admitted },
                    Err(refused) => Admission::Refused { refused },
                });
            }
            Input::Action(id, action, answer) => {
                if self.quitting
                    || self
                        .queue
                        .operation(&id)
                        .is_none_or(|operation| text(operation, "state") == "bound")
                {
                    let _ = answer.send(Err("This creation is no longer listed.".into()));
                    return;
                }
                if !self.services.allowed
                    && matches!(action, CreationAction::Resume | CreationAction::Check)
                {
                    let _ = answer.send(Err(crate::newsession::REPLAY_NEEDS_HGS.into()));
                    return;
                }
                if matches!(action, CreationAction::Check) {
                    let operation = self.queue.operation(&id).unwrap();
                    if flag(operation, "launch_dispatched") {
                        self.polls
                            .entry(text(operation, "host").into())
                            .or_default()
                            .force = true;
                    }
                }
                let effects = match action {
                    CreationAction::Resume => self.queue.resume(&id),
                    CreationAction::Check => self.queue.check(&id),
                    CreationAction::Stop => self.queue.stop(&id),
                    CreationAction::Hide => self.queue.hide(&id),
                    CreationAction::Show => self.queue.show(&id),
                };
                self.effects(effects);
                let _ = answer.send(Ok(()));
            }
            Input::Catalog => {}
            Input::Flush(answer) => {
                self.quitting = true;
                self.queue.quit();
                let _ = self.writer.send(Write::Flush(answer));
            }
        }
    }

    fn refresh_cache(&self) {
        let mut cache = lock(&self.cache);
        cache.operations = self.queue.operations().clone();
        cache.cards = self
            .queue
            .operations()
            .iter()
            .map(|(id, operation)| {
                (
                    id.clone(),
                    CreationCard::of(id, operation, self.queue.can_edit(id)),
                )
            })
            .collect();
    }

    fn effects(&mut self, effects: Vec<Effect>) {
        self.refresh_cache();
        for effect in effects {
            match effect {
                Effect::Write {
                    write,
                    bytes,
                    remember,
                } => {
                    let _ = self.writer.send(Write::Journal(write, bytes, remember));
                }
                Effect::Changed { id } => {
                    let card = lock(&self.cache).cards.get(&id).cloned();
                    if let Some(card) = card {
                        (self.services.emit)(serde_json::to_value(card).unwrap());
                    }
                }
                Effect::Removed { id } => {
                    (self.services.emit)(json!({"id": id, "removed": true}));
                }
                Effect::Call { flight, call } => {
                    if self.shutdown.load(Ordering::Acquire) || !self.services.allowed {
                        continue;
                    }
                    let shutdown = Arc::clone(&self.shutdown);
                    let services = Arc::clone(&self.services);
                    let done = self.done.clone();
                    (self.services.spawn)(Box::new(move || {
                        if shutdown.load(Ordering::Acquire) {
                            return;
                        }
                        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            (services.call)(&call)
                        }))
                        .unwrap_or_else(|_| creation::Answer {
                            ok: false,
                            data: Value::Null,
                            error: "The creation step stopped.".into(),
                        });
                        let _ = done.send(Done::Called(flight, answer));
                    }));
                }
                Effect::Launch { operation, .. } => {
                    if self.shutdown.load(Ordering::Acquire) || !self.services.allowed {
                        continue;
                    }
                    let shutdown = Arc::clone(&self.shutdown);
                    let services = Arc::clone(&self.services);
                    let done = self.done.clone();
                    (self.services.spawn)(Box::new(move || {
                        if shutdown.load(Ordering::Acquire) {
                            return;
                        }
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            (services.launch)(&operation)
                        }))
                        .unwrap_or_else(|_| Err("The session launch stopped.".into()));
                        let _ = done.send(Done::Launched(
                            text(&operation, "host").into(),
                            text(&operation, "launch_id").into(),
                            result,
                        ));
                    }));
                }
            }
        }
    }

    fn finished(&mut self, done: Done) {
        let effects = match done {
            Done::Written(write, ok) => self.queue.written(write, ok),
            Done::Called(flight, answer) => self.queue.answered(
                flight,
                answer,
                &mut Placement(Arc::clone(&self.services.resolve)),
            ),
            Done::Launched(host, launch, result) => {
                self.polls.entry(host).or_default().force = true;
                self.queue.launch_finished(&launch, result)
            }
            Done::Binding(host, result) => {
                if let Some(poll) = self.polls.get_mut(&host) {
                    poll.running = false;
                    poll.listing = result.ok();
                }
                Vec::new()
            }
        };
        self.effects(effects);
    }

    fn waiting(&self) -> BTreeSet<String> {
        self.queue
            .operations()
            .values()
            .filter(|operation| matches!(text(operation, "state"), "launching" | "confirming"))
            .map(|operation| text(operation, "host").to_owned())
            .collect()
    }

    fn poll(&mut self) {
        if !self.services.allowed {
            return;
        }
        let waiting = self.waiting();
        self.polls
            .retain(|host, poll| waiting.contains(host) || poll.force || poll.running);
        for host in &waiting {
            self.polls.entry(host.clone()).or_default();
        }
        for (host, poll) in &mut self.polls {
            let gap = Duration::from_secs(if host.is_empty() { 2 } else { 60 });
            if poll.running || (!poll.force && poll.last.is_some_and(|last| last.elapsed() < gap)) {
                continue;
            }
            poll.force = false;
            poll.running = true;
            poll.last = Some(Instant::now());
            let host = host.clone();
            let shutdown = Arc::clone(&self.shutdown);
            let services = Arc::clone(&self.services);
            let done = self.done.clone();
            (self.services.spawn)(Box::new(move || {
                if shutdown.load(Ordering::Acquire) {
                    return;
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (services.binding)(&ComputerId(host.clone()))
                }))
                .unwrap_or_else(|_| Err("The session listing stopped.".into()));
                let _ = done.send(Done::Binding(host, result));
            }));
        }
    }

    fn reconcile(&mut self) {
        let listings: Vec<_> = self
            .polls
            .iter()
            .filter_map(|(host, poll)| {
                poll.listing
                    .as_ref()
                    .map(|listing| (host.clone(), listing.clone()))
            })
            .collect();
        for (host, listing) in listings {
            if !listing.online || !(self.services.online)(&host) {
                continue;
            }
            for session in &listing.sessions {
                if session.launch_id.is_empty() || session.state == "archived" {
                    continue;
                }
                let Some(id) = self.queue.for_launch(&session.launch_id) else {
                    continue;
                };
                let Some(operation) = self.queue.operation(&id).cloned() else {
                    continue;
                };
                if text(&operation, "state") == "bound"
                    || text(&operation, "launch_id") != session.launch_id
                    || text(&operation, "host") != host
                    || text(&operation, "cmd") != session.cmd
                    || text(&operation, "tag")
                        != session.name.splitn(3, '/').nth(2).unwrap_or_default()
                {
                    continue;
                }
                match (self.services.bind)(&operation, session) {
                    Ok(true) => {
                        let effects = self.queue.complete(&id);
                        self.effects(effects);
                    }
                    Ok(false) => {}
                    Err(error) => {
                        let effects = self.queue.launch_finished(&session.launch_id, Err(error));
                        self.effects(effects);
                    }
                }
            }
            for ended in listing.ended_launches {
                if ended.launch_id.is_empty()
                    || listing
                        .sessions
                        .iter()
                        .any(|session| session.launch_id == ended.launch_id)
                {
                    continue;
                }
                let Some(id) = self.queue.for_launch(&ended.launch_id) else {
                    continue;
                };
                let Some(operation) = self.queue.operation(&id) else {
                    continue;
                };
                if text(operation, "host") != host {
                    continue;
                }
                let tag = text(operation, "tag");
                let reason = match ended.code {
                    Some(code) => {
                        format!("Session {tag} ended before it appeared (exit code {code}).")
                    }
                    None => format!("Session {tag} ended before it appeared."),
                } + "\n"
                    + &ended.output.chars().take(4000).collect::<String>();
                let effects = self.queue.ended(&id, &ended.launch_id, &reason);
                self.effects(effects);
            }
        }
    }
}

/// Qt's project catalog, with this computer's name mapped to the empty alias
/// expected by launch::ProjectFolder (SessionsWindow.cpp:1539-1569).
fn projects(organization: &crate::organization::Organization, local: &str) -> Vec<Project> {
    organization.creation_projects(local)
}

fn style(path: &str) -> Style {
    if paths::windows_shaped(path) {
        Style::Windows
    } else {
        Style::Posix
    }
}

fn resolve(
    organization: &crate::organization::Organization,
    local: &str,
    plan: &mut Value,
    evidence: &Value,
) -> Result<(), String> {
    let placed = creation::place(plan, evidence, &projects(organization, local))?;
    if let Some((id, name)) = placed.create {
        organization.accept(move |_| Intent::Create {
            id,
            name,
            members: Vec::new(),
        })?;
    }
    if let Some(path) = placed.add_folder {
        let project = placed.group;
        let machine = text(plan, "machine").to_owned();
        let style = style(&path);
        organization.accept(move |_| Intent::AddFolder {
            project,
            machine,
            path,
            name: String::new(),
            style,
        })?;
    }
    Ok(())
}

fn bind(
    organization: &crate::organization::Organization,
    pump: &crate::pump::Pump,
    operation: &Value,
    session: &BindingSession,
) -> Result<bool, String> {
    let host = text(operation, "host");
    if !pump.online(host) {
        return Ok(false);
    }
    let requested = text(operation, "group");
    if flag(operation, "implicit_inbox") && requested == swarm::INBOX {
        return Ok(true);
    }
    let Some(row) = pump.row_named(host, &session.name) else {
        return Ok(false);
    };
    let local = pump
        .machine(&ComputerId::local())
        .and_then(|machine| machine.host)
        .unwrap_or_default();
    let project = if projects(organization, &local)
        .iter()
        .any(|project| project.id == requested)
    {
        requested.to_owned()
    } else {
        pump.default_project()
    };
    if flag(operation, "add_folder") {
        let path = operation
            .get("folder")
            .and_then(Value::as_str)
            .unwrap_or_else(|| text(operation, "target"))
            .to_owned();
        let style = paths::of(
            pump.machine(&ComputerId(host.into()))
                .and_then(|machine| machine.platform)
                .as_deref(),
            host.is_empty(),
        );
        if paths::absolute(&path, style) {
            let group = project.clone();
            let machine = row.machine.clone();
            organization.accept(move |_| Intent::AddFolder {
                project: group,
                machine,
                path,
                name: String::new(),
                style,
            })?;
        }
    }
    let member = swarm::member(&row.machine, &row.name);
    let group = project.clone();
    organization.accept(move |_| Intent::Move {
        members: vec![member],
        project: group,
        before: None,
    })?;
    pump.set_collapsed(&project, false);
    Ok(true)
}

fn remember(settings: &zerus_client_core::settings::Settings, choices: &Remember) {
    let _ = settings.set("workspace/launchAgent", json!(choices.agent));
    let _ = settings.set("workspace/launchOpenTerminal", json!(choices.terminal));
    if !choices.account.is_empty() {
        let mut accounts =
            serde_json::from_str::<Value>(&settings.text("workspace/launchAccounts"))
                .unwrap_or_else(|_| json!({}));
        launch::remember_account(
            &mut accounts,
            &choices.host,
            &choices.agent,
            &choices.account,
        );
        let _ = settings.set("workspace/launchAccounts", json!(accounts.to_string()));
    }
}

fn launch_invocation(operation: &Value) -> zerus_client_core::Invocation {
    let computer = ComputerId(text(operation, "host").into());
    let launch = Launch {
        agent: text(operation, "cmd").into(),
        target: text(operation, "target").into(),
        name: text(operation, "tag").into(),
        account: text(operation, "account").into(),
        launch_id: text(operation, "launch_id").into(),
        model: text(operation, "model").into(),
        effort: text(operation, "effort").into(),
    };
    let mut invocation =
        launch::launch_invocation(&computer, &launch, text(operation, "expected_machine"));
    if flag(operation, "terminal") {
        // Remove only the option, even when a tag or account is also "-d".
        let at = usize::from(!computer.is_local())
            + if text(operation, "expected_machine").is_empty() {
                0
            } else {
                2
            }
            + 7;
        if invocation
            .args
            .get(at)
            .is_some_and(|argument| argument == "-d")
        {
            invocation.args.remove(at);
        }
    }
    invocation
}

fn detached(hgs: &Path, invocation: &zerus_client_core::Invocation) -> Result<(), String> {
    let exit = zerus_client_core::worktrees::run_guarded(hgs, invocation, |hgs, invocation| {
        zerus_client_core::run_limited(hgs, invocation, &[], zerus_client_core::reads::READ_LIMIT)
            .unwrap_or(zerus_client_core::Exit::Code {
                code: 1,
                stdout: Vec::new(),
                stderr: "The launch response exceeded its limit.".into(),
            })
    })?;
    launch::launched(exit)
}

/// The app's host: settings, the one organization Controller, real guarded
/// calls and the same terminal launcher Qt's New session uses.
pub fn start(app: &AppHandle, dir: &Path, replay: bool, allowed: bool) -> CreationHost {
    let hgs = zerus_client_core::default_hgs();
    let call_hgs = hgs.clone();
    let binding_hgs = hgs.clone();
    let organization = app
        .state::<crate::organization::Organization>()
        .inner()
        .clone();
    let placement = organization.clone();
    let pump = app.state::<crate::commands::AppState>().pump();
    let placement_pump = pump.clone();
    let remembered = app.state::<crate::settings::Store>().0.clone();
    let online_pump = pump.clone();
    let emit = app.clone();
    let notices = app.clone();
    let terminal_app = app.clone();
    let terminal_dir = dir.to_path_buf();
    let launcher = Arc::new(Mutex::new(
        zerus_client_core::terminal_launch::Launcher::new(Default::default()),
    ));
    CreationHost::new(
        dir.join(if replay {
            "creations-replay.json"
        } else {
            "creations.json"
        }),
        Services {
            allowed,
            clock: Arc::new(seconds),
            ids: Arc::new(|| uuid::Uuid::new_v4().to_string()),
            write: Arc::new(|file, bytes| {
                zerus_client_core::fsx::write_private_atomic(file, bytes, true).is_ok()
            }),
            call: Arc::new(move |call| creation::run_call(&call_hgs, call)),
            binding: Arc::new(move |computer| {
                creation::binding_listing(
                    zerus_client_core::run_limited(
                        &binding_hgs,
                        &creation::binding_invocation(computer),
                        &[],
                        zerus_client_core::reads::READ_LIMIT,
                    )
                    .map_err(|error| format!("{error:?}"))?,
                )
            }),
            online: Arc::new(move |host| online_pump.online(host)),
            resolve: Arc::new(move |plan, evidence| {
                let local = placement_pump
                    .machine(&ComputerId::local())
                    .and_then(|machine| machine.host)
                    .unwrap_or_default();
                resolve(&placement, &local, plan, evidence)
            }),
            bind: Arc::new(move |operation, session| {
                bind(&organization, &pump, operation, session)
            }),
            remember: Arc::new(move |choices| remember(&remembered, choices)),
            emit: Arc::new(move |value| {
                let _ = emit.emit("creations", value);
            }),
            spawn: Arc::new(|job| {
                tauri::async_runtime::spawn_blocking(job);
            }),
            launch: Arc::new(move |operation| {
                let invocation = launch_invocation(operation);
                if flag(operation, "terminal") {
                    let conf = crate::desk::tray_conf();
                    let mut terminal = lock(&launcher);
                    let mut mode = terminal.settings().clone();
                    mode.template = conf
                        .as_deref()
                        .and_then(|conf| {
                            zerus_client_core::menubar::tray_conf_value(conf, "terminal")
                        })
                        .unwrap_or_default();
                    mode.open_mode = if cfg!(target_os = "macos") {
                        serde_json::to_value(crate::desk::load_mode(&terminal_dir, conf.as_deref()))
                            .ok()
                            .and_then(|value| value.as_str().map(str::to_owned))
                            .unwrap_or_default()
                    } else {
                        conf.as_deref()
                            .and_then(|conf| {
                                zerus_client_core::menubar::tray_conf_value(conf, "open-mode")
                            })
                            .unwrap_or_default()
                    };
                    terminal.set_settings(mode);
                    let command = zerus_client_core::shell::command(
                        &hgs.to_string_lossy(),
                        &invocation.args,
                        zerus_client_core::terminal_launch::Platform::current().shell(),
                    );
                    match terminal.open_new_session(&hgs.to_string_lossy(), &invocation.args) {
                        Ok(advisory) => {
                            if let Some(advisory) = advisory {
                                crate::desk::window_notice(&terminal_app, "Zerus", &advisory);
                            }
                            Ok(())
                        }
                        Err(error) => {
                            crate::desk::window_notice(
                                &notices,
                                "Zerus: the terminal did not open",
                                &format!("{error}\n\n{command}"),
                            );
                            Err(error)
                        }
                    }
                } else {
                    detached(&hgs, &invocation)
                }
            }),
        },
    )
}

pub fn flush_on_exit(app: &AppHandle) {
    let started = Instant::now();
    if let Some(host) = app.try_state::<CreationHost>() {
        host.flush(FLUSH);
    }
    if let Some(settings) = app.try_state::<crate::settings::Store>() {
        settings
            .0
            .flush_within(FLUSH.saturating_sub(started.elapsed()));
    }
}

#[tauri::command]
pub async fn creation_submit(
    state: State<'_, CreationHost>,
    plan: Value,
) -> Result<Admission, String> {
    let host = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host.submit(plan))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn creations_view(state: State<'_, CreationHost>) -> Vec<CreationCard> {
    state.view()
}

#[tauri::command]
pub fn creation_details(state: State<'_, CreationHost>, id: String) -> Option<String> {
    state.details(&id)
}

#[tauri::command]
pub async fn creation_action(
    state: State<'_, CreationHost>,
    id: String,
    action: CreationAction,
) -> Result<(), String> {
    let host = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host.action(id, action))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn creation_plan(state: State<'_, CreationHost>, id: String) -> Option<Value> {
    state.plan(&id)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Condvar;
    use zerus_client_core::launch::Choices;

    const MACHINE: &str = "11111111-2222-4333-8444-555555555555";
    static NEXT: AtomicUsize = AtomicUsize::new(1);

    #[derive(Default)]
    struct Probe {
        listing: Mutex<BindingListing>,
        events: Mutex<Vec<Value>>,
        remembered: Mutex<Vec<Remember>>,
        placements: Mutex<Vec<creation::Placed>>,
        projects: Mutex<Vec<Project>>,
        bound: Mutex<Vec<String>>,
        jobs: Mutex<Vec<std::thread::JoinHandle<()>>>,
        reads: AtomicUsize,
    }

    struct Fake {
        dir: PathBuf,
        hgs: PathBuf,
        probe: Arc<Probe>,
        _tree: std::sync::RwLockReadGuard<'static, ()>,
    }

    fn wait(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn choices() -> Choices {
        Choices {
            machine: "workstation".into(),
            agent: "codex".into(),
            name: "created".into(),
            project: "p".into(),
            project_name: "Project".into(),
            path: "/srv/repo".into(),
            branch: "work-1".into(),
            listed_folder: true,
            ..Choices::default()
        }
    }

    fn plan() -> Value {
        serde_json::to_value(launch::plan(&choices())).unwrap()
    }

    impl Fake {
        fn new() -> Self {
            let tree = crate::PROCESS_TREE
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            let dir = std::env::temp_dir().join(format!(
                "zerus-creation-host-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let hgs = dir.join("hgs");
            std::os::unix::fs::symlink(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../core/tests/support/fake_hgs_creation.py"),
                &hgs,
            )
            .unwrap();
            let config = json!({"help": "guarded-create-v1 repository-clone-v1", "replies": {
                "dirs": {"stdout": {"path": "/srv/repo", "directories": [],
                    "creation_identity_v1": true, "computer_id": MACHINE}, "echo": {"path": "--creation"}},
                "codex": {"exit": 0},
                "worktrees --creation": {"stdout": {"state": "ok", "common_dir": "/srv/repo/.git",
                    "selected_root": "/srv/repo", "worktrees": [{"path": "/srv/repo-work-1", "branch": "work-1", "available": true}], "creation_receipts_v1": true}},
                "worktrees create": {"stdout": {"status": "created", "common_dir": "/srv/repo/.git"},
                    "echo": {"request_id": "--request-id", "branch": "--branch", "path": "--destination"}},
                "worktrees status": {"stdout": {"schema": 1, "status": "created", "path": "/srv/repo-work-1", "branch": "work-1", "common_dir": "/srv/repo/.git"}, "echo": {"request_id": "--request-id"}},
                "repository check": {"stdout": {"status": "ready", "name": "repo", "parent": "/srv", "path": "/srv/repo"}},
                "repository clone": {"stdout": {"status": "cloned", "parent": "/srv", "path": "/srv/repo"},
                    "echo": {"request_id": "--request-id"}}
            }});
            std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
            Self {
                dir,
                hgs,
                probe: Arc::new(Probe::default()),
                _tree: tree,
            }
        }

        fn services(&self) -> Services {
            let call = self.hgs.clone();
            let launch = self.hgs.clone();
            let p = Arc::clone(&self.probe);
            let binding = Arc::clone(&p);
            let remember = Arc::clone(&p);
            let resolve = Arc::clone(&p);
            let bind = Arc::clone(&p);
            let emit = Arc::clone(&p);
            let ids = AtomicUsize::new(1);
            Services {
                allowed: true,
                clock: Arc::new(|| 1_000),
                ids: Arc::new(move || {
                    format!(
                        "00000000-0000-4000-8000-{:012x}",
                        ids.fetch_add(1, Ordering::SeqCst)
                    )
                }),
                write: Arc::new(|file, bytes| {
                    zerus_client_core::fsx::write_private_atomic(file, bytes, true).is_ok()
                }),
                call: Arc::new(move |step| creation::run_call(&call, step)),
                launch: Arc::new(move |operation| detached(&launch, &launch_invocation(operation))),
                binding: Arc::new(move |_| {
                    binding.reads.fetch_add(1, Ordering::SeqCst);
                    Ok(lock(&binding.listing).clone())
                }),
                online: Arc::new(|_| true),
                resolve: Arc::new(move |operation, evidence| {
                    let mut projects = lock(&resolve.projects);
                    if projects.is_empty() {
                        projects.push(Project {
                            id: "p".into(),
                            name: "Project".into(),
                            folders: Vec::new(),
                        });
                    }
                    let placed = creation::place(operation, evidence, &projects)?;
                    if let Some((id, name)) = &placed.create {
                        projects.push(Project {
                            id: id.clone(),
                            name: name.clone(),
                            folders: Vec::new(),
                        });
                    }
                    lock(&resolve.placements).push(placed);
                    Ok(())
                }),
                bind: Arc::new(move |_, session| {
                    lock(&bind.bound).push(session.name.clone());
                    Ok(true)
                }),
                remember: Arc::new(move |value| {
                    lock(&remember.remembered).push(value.clone());
                }),
                emit: Arc::new(move |value| {
                    lock(&emit.events).push(value);
                }),
                spawn: Arc::new(move |job| {
                    lock(&p.jobs).push(std::thread::spawn(job));
                }),
            }
        }

        fn host(&self) -> CreationHost {
            CreationHost::new(self.dir.join("creations.json"), self.services())
        }

        fn log(&self) -> Vec<Vec<String>> {
            std::fs::read_to_string(self.dir.join("log.ndjson"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }

        fn stop(&self, host: CreationHost) {
            assert!(host.flush(Duration::from_secs(2)));
            let worker = Arc::downgrade(&host.0.cache);
            drop(host);
            wait("the queue worker to end", || worker.strong_count() == 0);
            for job in lock(&self.probe.jobs).drain(..) {
                job.join().unwrap();
            }
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn admitted(host: &CreationHost, plan: Value) -> String {
        match host.submit(plan).unwrap() {
            Admission::Admitted { admitted } => admitted,
            other => panic!("refused: {other:?}"),
        }
    }

    fn operation(host: &CreationHost, id: &str) -> Value {
        lock(&host.0.cache)
            .operations
            .get(id)
            .cloned()
            .unwrap_or(Value::Null)
    }

    fn state(host: &CreationHost, id: &str, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while text(&operation(host, id), "state") != expected {
            assert!(
                Instant::now() < deadline,
                "expected {expected}, got {}",
                operation(host, id)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn session(launch: &str) -> BindingSession {
        BindingSession {
            launch_id: launch.into(),
            cmd: "codex".into(),
            name: "codex/repo/created".into(),
            state: "running".into(),
        }
    }

    fn check(host: &CreationHost, fake: &Fake, id: &str, listing: BindingListing) {
        let reads = fake.probe.reads.load(Ordering::SeqCst);
        *lock(&fake.probe.listing) = listing;
        host.action(id.into(), CreationAction::Check).unwrap();
        wait("the forced listing read", || {
            fake.probe.reads.load(Ordering::SeqCst) > reads
        });
        for job in lock(&fake.probe.jobs).drain(..) {
            job.join().unwrap();
        }
        // The completion is now queued. Two worker inputs bracket the drain
        // and reconciliation even if it was already waiting for an input.
        let _ = host.action(id.into(), CreationAction::Show);
        let _ = host.action(id.into(), CreationAction::Show);
    }

    #[test]
    fn an_ordinary_folder_launch_is_guarded_and_waits_for_binding() {
        let fake = Fake::new();
        let host = fake.host();
        let id = admitted(&host, plan());
        state(&host, &id, "confirming");
        let operation = operation(&host, &id);
        let commands = fake.log();
        let dirs = commands
            .iter()
            .position(|args| args.first().is_some_and(|arg| arg == "dirs"))
            .unwrap();
        let launch = commands
            .iter()
            .position(|args| args.contains(&"codex".into()))
            .unwrap();
        assert!(dirs < launch);
        assert!(!commands
            .iter()
            .any(|args| args.contains(&"worktrees".into())));
        assert_eq!(
            &commands[launch][..3],
            ["--expect-machine", MACHINE, "codex"]
        );
        assert!(commands[launch].contains(&"-d".into()));
        assert!(lock(&fake.probe.bound).is_empty());
        assert!(!lock(&fake.probe.remembered).is_empty());
        let launch_id = text(&operation, "launch_id");
        check(
            &host,
            &fake,
            &id,
            BindingListing {
                online: true,
                sessions: vec![session(launch_id)],
                ended_launches: Vec::new(),
            },
        );
        state(&host, &id, "bound");
        assert!(host.view().is_empty());
        assert_eq!(lock(&fake.probe.bound).as_slice(), ["codex/repo/created"]);
        assert!(host.plan(&id).is_none());
        assert!(host.action(id, CreationAction::Resume).is_err());
        fake.stop(host);
    }

    #[test]
    fn auto_verifies_then_creates_once_and_launches_in_the_receipted_checkout() {
        let fake = Fake::new();
        let host = fake.host();
        let mut choices = choices();
        choices.auto_preferred = true;
        choices.automatic_available = true;
        let id = admitted(&host, serde_json::to_value(launch::plan(&choices)).unwrap());
        state(&host, &id, "confirming");
        let commands = fake.log();
        let catalog = commands
            .iter()
            .position(|args| {
                args.contains(&"--creation".into()) && args.contains(&"worktrees".into())
            })
            .unwrap();
        let create = commands
            .iter()
            .position(|args| args.contains(&"create".into()))
            .unwrap();
        let launch = commands
            .iter()
            .position(|args| args.contains(&"codex".into()))
            .unwrap();
        assert!(catalog < create && create < launch);
        assert_eq!(
            commands
                .iter()
                .filter(|args| args.contains(&"create".into()))
                .count(),
            1
        );
        assert!(commands[create].contains(&"--receipt".into()));
        assert!(commands[launch].contains(&"/srv/repo-work-1".into()));
        assert!(flag(&operation(&host, &id), "worktree_done"));
        fake.stop(host);
    }

    #[test]
    fn a_repository_is_checked_then_cloned_before_project_placement() {
        let fake = Fake::new();
        let host = fake.host();
        let mut choices = choices();
        choices.project.clear();
        choices.repository = Some(launch::RepositoryChoice {
            url: "https://example.com/repo.git".into(),
            parent: "/srv".into(),
            name: "repo".into(),
            destination: "/srv/repo".into(),
            new_project: true,
            project_name: "Repository".into(),
            ..Default::default()
        });
        let id = admitted(&host, serde_json::to_value(launch::plan(&choices)).unwrap());
        state(&host, &id, "confirming");
        let commands = fake.log();
        let check = commands
            .iter()
            .position(|args| args.contains(&"check".into()))
            .unwrap();
        let clone = commands
            .iter()
            .position(|args| args.contains(&"clone".into()))
            .unwrap();
        let launch = commands
            .iter()
            .position(|args| args.contains(&"codex".into()))
            .unwrap();
        assert!(check < clone && clone < launch);
        let placements = lock(&fake.probe.placements);
        assert!(placements
            .iter()
            .any(|placed| placed.create.is_some() && placed.add_folder.is_some()));
        drop(placements);
        fake.stop(host);
    }

    #[test]
    fn a_failed_journal_write_sends_no_call_and_keeps_the_save_failure_text() {
        let fake = Fake::new();
        let mut services = fake.services();
        services.write = Arc::new(|_, _| false);
        let host = CreationHost::new(fake.dir.join("creations.json"), services);
        let id = admitted(&host, plan());
        state(&host, &id, "storage_error");
        assert_eq!(
            operation(&host, &id)["error"],
            "Could not save creation progress. No further setup step was sent."
        );
        assert!(fake.log().is_empty());
        assert!(lock(&fake.probe.remembered).is_empty());
        assert_eq!(fake.probe.reads.load(Ordering::SeqCst), 0);
        fake.stop(host);
    }

    #[test]
    fn restart_recovers_without_calls_and_only_resume_continues_setup() {
        let fake = Fake::new();
        let mut services = fake.services();
        services.spawn = Arc::new(|_| {});
        let host = CreationHost::new(fake.dir.join("creations.json"), services);
        let id = admitted(&host, plan());
        state(&host, &id, "checking");
        fake.stop(host);
        let host = fake.host();
        state(&host, &id, "recover");
        assert!(fake.log().is_empty());
        assert_eq!(fake.probe.reads.load(Ordering::SeqCst), 0);
        host.action(id.clone(), CreationAction::Resume).unwrap();
        state(&host, &id, "confirming");
        fake.stop(host);
    }

    #[test]
    fn replay_recovery_and_admission_cannot_run_the_cli() {
        let fake = Fake::new();
        let mut services = fake.services();
        services.spawn = Arc::new(|_| {});
        let host = CreationHost::new(fake.dir.join("creations.json"), services);
        let id = admitted(&host, plan());
        state(&host, &id, "checking");
        fake.stop(host);
        let mut services = fake.services();
        services.allowed = false;
        let host = CreationHost::new(fake.dir.join("creations.json"), services);
        state(&host, &id, "recover");
        assert_eq!(
            host.submit(plan()).unwrap(),
            Admission::Refused {
                refused: crate::newsession::REPLAY_NEEDS_HGS.into()
            }
        );
        for action in [CreationAction::Resume, CreationAction::Check] {
            assert_eq!(
                host.action(id.clone(), action),
                Err(crate::newsession::REPLAY_NEEDS_HGS.into())
            );
        }
        assert!(fake.log().is_empty());
        fake.stop(host);
    }

    #[test]
    fn mismatched_offline_archived_or_missing_rows_do_not_bind_or_relaunch() {
        let fake = Fake::new();
        let host = fake.host();
        let id = admitted(&host, plan());
        state(&host, &id, "confirming");
        let launch = text(&operation(&host, &id), "launch_id").to_owned();
        let original_launches = fake
            .log()
            .iter()
            .filter(|args| args.contains(&"codex".into()))
            .count();
        let mut wrong_agent = session(&launch);
        wrong_agent.cmd = "claude".into();
        let mut wrong_tag = session(&launch);
        wrong_tag.name = "codex/repo/other".into();
        let mut archived = session(&launch);
        archived.state = "archived".into();
        for listing in [
            BindingListing {
                online: true,
                sessions: vec![session("another-launch"), wrong_agent, wrong_tag, archived],
                ..Default::default()
            },
            BindingListing {
                online: false,
                sessions: vec![session(&launch)],
                ..Default::default()
            },
            BindingListing {
                online: true,
                ..Default::default()
            },
        ] {
            check(&host, &fake, &id, listing);
        }
        assert_eq!(text(&operation(&host, &id), "state"), "confirming");
        assert!(lock(&fake.probe.bound).is_empty());
        assert_eq!(
            fake.log()
                .iter()
                .filter(|args| args.contains(&"codex".into()))
                .count(),
            original_launches
        );
        fake.stop(host);
    }

    #[test]
    fn a_retired_launch_cannot_bind_the_retried_creation() {
        let fake = Fake::new();
        let host = fake.host();
        let id = admitted(&host, plan());
        state(&host, &id, "confirming");
        let old = text(&operation(&host, &id), "launch_id").to_owned();
        check(
            &host,
            &fake,
            &id,
            BindingListing {
                online: true,
                sessions: Vec::new(),
                ended_launches: vec![creation::EndedLaunch {
                    launch_id: old.clone(),
                    ..Default::default()
                }],
            },
        );
        state(&host, &id, "failed");
        host.action(id.clone(), CreationAction::Resume).unwrap();
        state(&host, &id, "confirming");
        check(
            &host,
            &fake,
            &id,
            BindingListing {
                online: true,
                sessions: vec![session(&old)],
                ended_launches: Vec::new(),
            },
        );
        assert_eq!(text(&operation(&host, &id), "state"), "confirming");
        assert!(lock(&fake.probe.bound).is_empty());
        fake.stop(host);
    }

    #[test]
    fn an_ended_launch_without_a_row_uses_qts_text_and_bounded_output() {
        for code in [None, Some(7)] {
            let fake = Fake::new();
            let host = fake.host();
            let id = admitted(&host, plan());
            state(&host, &id, "confirming");
            let launch = text(&operation(&host, &id), "launch_id").to_owned();
            let ended = creation::EndedLaunch {
                launch_id: launch,
                name: "codex/repo/created".into(),
                code,
                output: "é".repeat(4001),
            };
            check(
                &host,
                &fake,
                &id,
                BindingListing {
                    online: true,
                    sessions: Vec::new(),
                    ended_launches: vec![ended],
                },
            );
            state(&host, &id, "failed");
            let suffix = code.map_or(String::new(), |code| format!(" (exit code {code})"));
            let operation = operation(&host, &id);
            let error = text(&operation, "error");
            let prefix = format!("Session created ended before it appeared{suffix}.\n");
            assert!(error.starts_with(&prefix));
            let output = error.strip_prefix(&prefix).unwrap();
            assert!(!output.is_empty() && output.chars().count() <= 4000);
            assert!(output.chars().all(|character| character == 'é'));
            // The core also bounds the entire sanitized diagnostic to 4000.
            assert_eq!(error.chars().count(), 4000);
            assert!(host.plan(&id).is_some());
            fake.stop(host);
        }
    }

    #[test]
    fn a_held_journal_does_not_stall_catalog_observation_or_exit() {
        let fake = Fake::new();
        let held = Arc::new((Mutex::new(false), Condvar::new()));
        let gate = Arc::clone(&held);
        let entered = Arc::new(AtomicBool::new(false));
        let writing = Arc::clone(&entered);
        let mut services = fake.services();
        let write = services.write.clone();
        services.write = Arc::new(move |file, bytes| {
            writing.store(true, Ordering::Release);
            let (lock_gate, wake) = &*gate;
            let _guard = wake
                .wait_while(lock(lock_gate), |released| !*released)
                .unwrap();
            write(file, bytes)
        });
        let host = CreationHost::new(fake.dir.join("creations.json"), services);
        admitted(&host, plan());
        wait("the held journal", || entered.load(Ordering::Acquire));
        let start = Instant::now();
        for _ in 0..10_000 {
            host.observe(&Change::Remove {
                computer: ComputerId::local(),
                key: "gone".into(),
                reason: None,
            });
        }
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!host.flush(Duration::from_millis(20)));
        *lock(&held.0) = true;
        held.1.notify_all();
        fake.stop(host);
        assert!(
            fake.log().is_empty(),
            "a late write acknowledgement cannot start setup after exit"
        );
    }

    #[test]
    fn admission_reports_refusals_and_a_computer_without_waiting_work_is_not_polled() {
        let fake = Fake::new();
        let mut services = fake.services();
        services.spawn = Arc::new(|_| {});
        let host = CreationHost::new(fake.dir.join("creations.json"), services);
        for n in 0..creation::UNSETTLED {
            let mut plan = plan();
            plan["tag"] = json!(format!("pending-{n}"));
            admitted(&host, plan);
        }
        let refusal = host.submit(plan()).unwrap();
        assert!(matches!(refusal, Admission::Refused { .. }));
        assert!(serde_json::to_value(refusal).unwrap()["refused"]
            .as_str()
            .unwrap()
            .contains("32"));
        assert_eq!(fake.probe.reads.load(Ordering::SeqCst), 0);
        assert!(host.view().len() <= 128);
        fake.stop(host);
    }

    #[test]
    fn the_source_drain_flows_while_the_creation_worker_is_blocked() {
        let fake = Fake::new();
        let held = Arc::new((Mutex::new(false), Condvar::new()));
        let gate = Arc::clone(&held);
        let entered = Arc::new(AtomicBool::new(false));
        let placing = Arc::clone(&entered);
        let mut services = fake.services();
        let resolve = services.resolve.clone();
        services.resolve = Arc::new(move |plan, evidence| {
            placing.store(true, Ordering::Release);
            let (gate, wake) = &*gate;
            let _guard = wake.wait_while(lock(gate), |released| !*released).unwrap();
            resolve(plan, evidence)
        });
        let host = CreationHost::new(fake.dir.join("creations.json"), services);
        admitted(&host, plan());
        wait("the blocked creation worker", || {
            entered.load(Ordering::Acquire)
        });
        let pump = crate::pump::Pump::default();
        let (send, receive) = mpsc::channel();
        let observing = host.clone();
        pump.drain_with(receive, move |change| observing.observe(change));
        for _ in 0..10_000 {
            send.send(Change::Upsert {
                computer: ComputerId::local(),
                key: "flowing".into(),
                row: zerus_client_core::ViewRow {
                    name: "codex/repo/flowing".into(),
                    ..Default::default()
                },
            })
            .unwrap();
        }
        drop(send);
        let deadline = Instant::now() + Duration::from_secs(2);
        while pump.row_named("", "codex/repo/flowing").is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let flowed = pump.row_named("", "codex/repo/flowing").is_some();
        *lock(&held.0) = true;
        held.1.notify_all();
        fake.stop(host);
        assert!(flowed, "the drain waited for the creation worker");
    }

    #[test]
    fn work_reserved_before_exit_cannot_start_from_a_delayed_executor() {
        let fake = Fake::new();
        let jobs = Arc::new(Mutex::new(Vec::<Job>::new()));
        let executor = Arc::clone(&jobs);
        let mut services = fake.services();
        services.spawn = Arc::new(move |job| lock(&executor).push(job));
        let host = CreationHost::new(fake.dir.join("creations.json"), services);
        admitted(&host, plan());
        wait("the reserved setup call", || !lock(&jobs).is_empty());
        assert!(host.flush(Duration::from_secs(2)));
        for job in lock(&jobs).drain(..) {
            job();
        }
        assert!(fake.log().is_empty());
        fake.stop(host);
    }

    #[test]
    fn remembered_choices_keep_other_computers_and_only_replace_invalid_text_on_a_choice() {
        let fake = Fake::new();
        let settings =
            zerus_client_core::settings::Settings::open(fake.dir.join("settings.json"), None);
        settings
            .set("workspace/launchAccounts", json!("invalid JSON"))
            .unwrap();
        let mut choice = Remember {
            host: "studio".into(),
            agent: "codex".into(),
            account: String::new(),
            terminal: true,
        };
        remember(&settings, &choice);
        assert_eq!(settings.text("workspace/launchAccounts"), "invalid JSON");
        assert_eq!(settings.text("workspace/launchAgent"), "codex");
        assert!(settings.flag("workspace/launchOpenTerminal"));
        choice.account = "remote".into();
        remember(&settings, &choice);
        choice.host.clear();
        choice.agent = "claude".into();
        choice.account = "local".into();
        remember(&settings, &choice);
        let accounts: Value =
            serde_json::from_str(&settings.text("workspace/launchAccounts")).unwrap();
        assert_eq!(
            accounts,
            json!({"studio": {"codex": "remote"}, "": {"claude": "local"}})
        );
        assert!(settings.flush_within(Duration::from_secs(2)));
    }

    #[test]
    fn terminal_launch_removes_only_the_detached_option() {
        for host in ["", "studio"] {
            let mut operation = plan();
            operation["terminal"] = json!(true);
            operation["host"] = json!(host);
            operation["tag"] = json!("-d");
            operation["account"] = json!("-d");
            operation["expected_machine"] = json!(MACHINE);
            let invocation = launch_invocation(&operation);
            assert_eq!(invocation.args.iter().filter(|arg| *arg == "-d").count(), 2);
            assert!(invocation
                .args
                .windows(2)
                .any(|pair| pair == ["--expect-machine", MACHINE]));
            operation["terminal"] = json!(false);
            assert_eq!(
                launch_invocation(&operation)
                    .args
                    .iter()
                    .filter(|arg| *arg == "-d")
                    .count(),
                3
            );
        }
    }
}
