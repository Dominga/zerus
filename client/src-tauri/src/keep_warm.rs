//! Keep cache warm: Qt's `TrayAgent::warmCaches` around the core's
//! [`Keepalive`]. One thread observes every computer's current listing, as
//! the tray agent does on each fleet poll, also while every window is closed,
//! and carries out what the state machine asks: a keepalive through the
//! ordinary guarded send (`actions::dispatch`, with the row's run and
//! conversation), one inspection for Claude's cache lifetime, and a read mark
//! for a reply hgs still counts. Nothing runs for a session whose keep-warm
//! the person did not turn on, and a send that failed or may have failed is
//! never repeated. Native mode only: the background service has no
//! keepalive, so in service mode the choice is kept but nothing is sent.

use crate::{actions::Actions, pump::Pump};
use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;
use zerus_client_core::cache_keepalive::{self as keepalive, Effect, Keepalive};
use zerus_client_core::readmarks::ReadMark;
use zerus_client_core::settings::Settings;
use zerus_client_core::{Action, Outcome, Target};

/// How often the current listings are observed. The rows follow the watch
/// streams; peers polled by Qt's rules change about once a minute.
const TICK: Duration = Duration::from_secs(5);

type Deliver = Arc<dyn Fn(Target) -> Result<Outcome, String> + Send + Sync>;
type Inspect = Arc<dyn Fn(&Target) -> Result<Value, String> + Send + Sync>;
type Read = Arc<dyn Fn(ReadMark) + Send + Sync>;

struct Calls {
    deliver: Deliver,
    inspect: Inspect,
    read: Read,
}

struct Shared {
    pump: Pump,
    settings: Settings,
    calls: Calls,
    engine: Mutex<Keepalive>,
    stopped: AtomicBool,
}

struct Worker(Arc<Shared>);

impl Drop for Worker {
    fn drop(&mut self) {
        self.0.stopped.store(true, Ordering::Release);
    }
}

/// The managed state: the choice is stored here; the worker runs only where
/// sends are allowed.
pub struct KeepWarm {
    pump: Pump,
    settings: Settings,
    _worker: Option<Worker>,
}

pub fn start(pump: Pump, actions: Actions, settings: Settings) -> KeepWarm {
    // A replay never reaches a real agent.
    let blocked = crate::actions::blocked()
        || std::env::var_os("ZERUS_CLIENT_REPLAY").is_some_and(|value| !value.is_empty());
    let worker = (!blocked && !crate::desktop_service::mode())
        .then(|| Worker::start(pump.clone(), actions, settings.clone()));
    KeepWarm {
        pump,
        settings,
        _worker: worker,
    }
}

impl Worker {
    fn start(pump: Pump, actions: Actions, settings: Settings) -> Self {
        let reads = actions.clone();
        let calls = Calls {
            deliver: Arc::new(move |target| {
                tauri::async_runtime::block_on(crate::actions::dispatch(
                    &actions,
                    target,
                    Action::Send {
                        text: keepalive::MESSAGE.into(),
                        attachments: Vec::new(),
                        agent_id: None,
                        expected_compaction_id: None,
                    },
                ))
            }),
            inspect: Arc::new(|target| {
                let hgs = zerus_client_core::default_hgs();
                let inv = zerus_client_core::details::details_invocation(target, None);
                crate::composer::send::inspection(
                    &hgs,
                    zerus_client_core::run_limited(&hgs, &inv, &[], 8 << 20),
                )
            }),
            read: Arc::new(move |mark| {
                let _ = reads.queue_marks(vec![mark]);
            }),
        };
        let shared = Arc::new(Shared {
            pump,
            settings,
            calls,
            engine: Mutex::new(Keepalive::new()),
            stopped: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&shared);
        std::thread::Builder::new()
            .name("zerus-cache-keepalive".into())
            .spawn(move || {
                while let Some(shared) = weak.upgrade() {
                    if shared.stopped.load(Ordering::Acquire) {
                        break;
                    }
                    let _ = catch_unwind(AssertUnwindSafe(|| tick(&shared)));
                    drop(shared);
                    std::thread::sleep(TICK);
                }
            })
            .expect("the cache keepalive worker starts");
        Self(shared)
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

fn engine(shared: &Shared) -> std::sync::MutexGuard<'_, Keepalive> {
    shared.engine.lock().unwrap_or_else(PoisonError::into_inner)
}

fn tick(shared: &Arc<Shared>) {
    let saved = keepalive::parse(&shared.settings.text(keepalive::KEY));
    if saved.is_empty() && engine(shared).is_empty() {
        return;
    }
    let listings = shared.pump.current_rows(|row| {
        row.state == "running"
            && row
                .conversation_id
                .as_deref()
                .is_some_and(|c| !c.is_empty())
    });
    let now = now();
    let effects: Vec<Effect> = {
        let mut engine = engine(shared);
        listings
            .iter()
            .flat_map(|(computer, rows)| engine.observe(computer, rows, &saved, now))
            .collect()
    };
    for effect in effects {
        carry_out(shared, effect);
    }
}

/// Each call on its own short-lived thread, so a slow peer holds up nothing.
fn carry_out(shared: &Arc<Shared>, effect: Effect) {
    let worker = shared.clone();
    let failed = shared.clone();
    let fallback = effect.clone();
    let started = std::thread::Builder::new()
        .name("zerus-cache-keepalive-call".into())
        .spawn(move || match effect {
            Effect::Send { request, target } => {
                let delivered = matches!(
                    catch_unwind(AssertUnwindSafe(|| (worker.calls.deliver)(target))),
                    Ok(Ok(Outcome::Done { .. }))
                );
                let mut engine = engine(&worker);
                if delivered {
                    engine.sent(request);
                } else {
                    engine.failed(request);
                }
            }
            Effect::Inspect { target } => {
                let details = catch_unwind(AssertUnwindSafe(|| (worker.calls.inspect)(&target)));
                let mut engine = engine(&worker);
                match details {
                    Ok(Ok(details)) => engine.inspected(&target.computer, &target.name, &details),
                    _ => engine.inspection_failed(&target.computer, &target.name),
                }
            }
            Effect::Read(mark) => {
                let _ = catch_unwind(AssertUnwindSafe(|| (worker.calls.read)(mark)));
            }
        });
    if started.is_err() {
        let mut engine = engine(&failed);
        match fallback {
            Effect::Send { request, .. } => engine.failed(request),
            Effect::Inspect { target } => engine.inspection_failed(&target.computer, &target.name),
            Effect::Read(_) => {}
        }
    }
}

/// Details' switch and slider and the session menu's toggle: the session's
/// hours, 0 to turn it off, kept on this device under Qt's identity. Returns
/// the hours now in effect.
#[tauri::command(rename_all = "snake_case")]
pub fn keep_warm_set(
    state: State<'_, KeepWarm>,
    computer: String,
    key: String,
    hours: i64,
) -> Result<i64, String> {
    let row = state
        .pump
        .row(&computer, &key)
        .ok_or("Refresh the session before changing Keep cache warm.")?;
    if row.state == "archived" || row.conversation_id.as_deref().is_none_or(str::is_empty) {
        return Err("Keep cache warm needs a session with a conversation.".into());
    }
    let id = keepalive::id(&computer, &row)
        .ok_or("Refresh the session before changing Keep cache warm.")?;
    let hours = hours.clamp(0, keepalive::MAX_HOURS);
    let mut saved = keepalive::parse(&state.settings.text(keepalive::KEY));
    if hours > 0 && !saved.contains_key(&id) && saved.len() >= keepalive::MAX_SESSIONS {
        return Err("Keep cache warm is already on for the most sessions it keeps.".into());
    }
    if keepalive::set_hours(&mut saved, &id, hours) {
        state
            .settings
            .set(keepalive::KEY, Value::String(keepalive::stored(&saved)))?;
    }
    Ok(keepalive::hours(&saved, &id))
}
