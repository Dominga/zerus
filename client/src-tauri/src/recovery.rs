//! Automatic recovery's shared policy (`zerus_client_core::recovery_settings`):
//! a background service, as Qt's RecoverySync is. Once a second it follows
//! this computer's machine card and the peers; every 15 seconds, as Qt's
//! timer, it reads every machine. Nothing is read or written before the card
//! says whether this computer runs agents (or, without one for 30 s, Qt's
//! default that it does), and a replay never drives the CLI. The newest
//! policy is copied to machines behind it; each answer sends
//! `recovery-changed` with the page's view. Calls run on their own threads.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use zerus_client_core::recovery_settings::{
    self, assumed_card, form, policy, tick, Call, Effect, Form, RecoverySync, REFRESH,
    SHARED_POLICY_KEY,
};
use zerus_client_core::{default_hgs, ComputerId};

use crate::commands::AppState;
use crate::settings::Store;

/// How often the service looks at the fleet.
const FOLLOW: Duration = Duration::from_secs(1);

/// The service's state.
pub struct Recovery(Arc<Inner>);

struct Inner {
    sync: Mutex<RecoverySync>,
    /// Orders the stored policy's writes, which run outside `sync`.
    persist: Mutex<()>,
    /// A replay without an explicit CLI: nothing is read or written.
    blocked: bool,
}

impl Inner {
    fn sync(&self) -> MutexGuard<'_, RecoverySync> {
        self.sync.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores the newest policy known now. Writes take turns, and each
    /// reads the policy once it has its turn, so a slower thread never
    /// stores an older one; the store and its listeners run outside `sync`.
    fn persist(&self, store: &Store) {
        let _turn = self.persist.lock().unwrap_or_else(PoisonError::into_inner);
        let newest = Value::Object(self.sync().envelope().clone()).to_string();
        let _ = store.0.set(SHARED_POLICY_KEY, json!(newest));
    }
}

/// What the page shows of the service.
fn view(sync: &RecoverySync) -> Value {
    json!({
        "status": sync.status(),
        "error": sync.error(),
        "busy": sync.busy(),
        "can_save": sync.can_save(),
        "known": sync.known(),
        "has_machines": sync.has_machines(),
        "has_policy": !sync.envelope().is_empty(),
        "saves": sync.saves(),
    })
}

/// Whether the service may read and write machines: never in a replay
/// without an explicit CLI (actions::blocked), as for read marks.
fn allowed(blocked: bool) -> Result<(), String> {
    if blocked {
        Err("Automatic recovery is not synchronized in a replay.".into())
    } else {
        Ok(())
    }
}

/// Starts the service from the newest policy kept from before. Its first
/// look at the fleet comes a second later, and its first timed read 15 s
/// later, as Qt's timer; a replay starts nothing.
pub fn start(app: &AppHandle) -> Recovery {
    let stored = app.state::<Store>().0.text(SHARED_POLICY_KEY);
    let inner = Arc::new(Inner {
        sync: Mutex::new(RecoverySync::new(&stored)),
        persist: Mutex::new(()),
        blocked: crate::actions::blocked(),
    });
    if !inner.blocked {
        let (handle, service) = (app.clone(), Arc::clone(&inner));
        let periods = (REFRESH.as_secs() / FOLLOW.as_secs()).max(1);
        let started = std::thread::Builder::new()
            .name("recovery-sync".into())
            .spawn(move || {
                let mut second = 0u64;
                loop {
                    std::thread::sleep(FOLLOW);
                    second += 1;
                    let waited = FOLLOW * u32::try_from(second).unwrap_or(u32::MAX);
                    step(&handle, &service, waited, second.is_multiple_of(periods));
                }
            });
        if let Err(error) = started {
            eprintln!("zerus-client: automatic recovery settings: {error}");
        }
    }
    Recovery(inner)
}

/// One look at the fleet `waited` after the service started: this
/// computer's card and the peers, and a read of every machine when `due`.
fn step(app: &AppHandle, inner: &Arc<Inner>, waited: Duration, due: bool) {
    let state = app.state::<AppState>();
    let card = assumed_card(state.pump().machine(&ComputerId::local()), waited);
    let peers = state.peers().map(|peers| peers.into_iter().collect());
    let (calls, persist) = {
        let mut sync = inner.sync();
        settle(tick(&mut sync, card.as_ref(), peers, due))
    };
    if persist {
        inner.persist(&app.state::<Store>());
    }
    if due || !calls.is_empty() {
        changed(app, inner);
    }
    spawn(app, inner, calls);
}

/// The calls to run, and whether the stored policy is to be written (with
/// [`Inner::persist`], once the service's lock is released). A Persist's own
/// text is not what is stored: the newest policy at the time of the write is.
fn settle(effects: Vec<Effect>) -> (Vec<Call>, bool) {
    let mut calls = Vec::new();
    let mut persist = false;
    for effect in effects {
        match effect {
            Effect::Call(call) => calls.push(call),
            Effect::Persist(_) => persist = true,
        }
    }
    (calls, persist)
}

fn changed(app: &AppHandle, inner: &Arc<Inner>) {
    let view = view(&inner.sync());
    let _ = app.emit("recovery-changed", view);
}

/// Feeds one answer and runs what follows.
fn answered(
    app: &AppHandle,
    inner: &Arc<Inner>,
    id: u64,
    reply: Result<Map<String, Value>, String>,
) {
    let (calls, persist) = settle(inner.sync().answered(id, reply));
    if persist {
        inner.persist(&app.state::<Store>());
    }
    spawn(app, inner, calls);
    changed(app, inner);
}

/// Runs each call on a thread of its own; a thread that cannot start
/// answers its call as failed, so nothing stays pending for good.
fn spawn(app: &AppHandle, inner: &Arc<Inner>, calls: Vec<Call>) {
    for call in calls {
        let id = call.id;
        let (handle, service) = (app.clone(), Arc::clone(inner));
        let started = std::thread::Builder::new()
            .name("recovery-call".into())
            .spawn(move || {
                let reply = recovery_settings::call(&default_hgs(), &call);
                answered(&handle, &service, call.id, reply);
            });
        if let Err(error) = started {
            answered(app, inner, id, Err(format!("Could not start hgs: {error}")));
        }
    }
}

/// The page's view.
#[tauri::command]
pub fn recovery_view(recovery: State<'_, Recovery>) -> Value {
    view(&recovery.0.sync())
}

/// The form for the newest policy and that policy (`base`), which the page
/// keeps with the form and hands back to Save (RecoverySettings::load);
/// both null before any policy.
#[tauri::command]
pub fn recovery_load(recovery: State<'_, Recovery>) -> Value {
    let envelope = recovery.0.sync().envelope().clone();
    let loaded = form(&envelope);
    let base = loaded.as_ref().map(|_| Value::Object(envelope));
    json!({ "form": loaded, "base": base })
}

/// Save recovery settings: the form's delays checked as Qt checks them,
/// then the policy sent on the base the form was loaded with. Answers the
/// view, Qt's text when the form is refused, and whether it was sent.
#[tauri::command(async)]
pub fn recovery_save(
    app: AppHandle,
    recovery: State<'_, Recovery>,
    form: Form,
    base: Map<String, Value>,
) -> Result<Value, String> {
    allowed(recovery.0.blocked)?;
    let (calls, persist, message, view) = {
        let mut sync = recovery.0.sync();
        let (effects, message) = match policy(&base, &form) {
            Err(message) => (Vec::new(), Some(message)),
            Ok(policy) => (sync.save(policy, &base), None),
        };
        let (calls, persist) = settle(effects);
        (calls, persist, message, view(&sync))
    };
    if persist {
        recovery.0.persist(&app.state::<Store>());
    }
    let sent = !calls.is_empty();
    spawn(&app, &recovery.0, calls);
    Ok(json!({ "view": view, "message": message, "sent": sent }))
}

/// Reload, and opening the section: every machine read again.
#[tauri::command(async)]
pub fn recovery_reload(app: AppHandle, recovery: State<'_, Recovery>) -> Result<Value, String> {
    allowed(recovery.0.blocked)?;
    let (calls, persist, view) = {
        let mut sync = recovery.0.sync();
        let (calls, persist) = settle(sync.refresh());
        (calls, persist, view(&sync))
    };
    if persist {
        recovery.0.persist(&app.state::<Store>());
    }
    spawn(&app, &recovery.0, calls);
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_neither_reloads_nor_saves() {
        assert!(allowed(true).is_err());
        assert!(allowed(false).is_ok());
    }

    #[test]
    fn the_stored_policy_is_the_newest_at_the_time_it_is_written() {
        let dir =
            std::env::temp_dir().join(format!("zerus-recovery-settle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store(zerus_client_core::settings::Settings::open(
            dir.join("settings.json"),
            None,
        ));
        let newest = json!({"order": [9, "w"], "policy": {"revision": 9}});
        let inner = Inner {
            sync: Mutex::new(RecoverySync::new(&newest.to_string())),
            persist: Mutex::new(()),
            blocked: false,
        };
        // A slower thread's Persist carries an older policy; the newest is kept.
        let older = json!({"order": [8, "w"], "policy": {"revision": 8}}).to_string();
        let (calls, persist) = settle(vec![Effect::Persist(older)]);
        assert!(calls.is_empty() && persist);
        inner.persist(&store);
        assert_eq!(store.0.text(SHARED_POLICY_KEY), newest.to_string());
        assert!(store.0.flush_within(Duration::from_secs(5)));
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
