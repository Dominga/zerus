//! Automatic recovery's shared policy (`zerus_client_core::recovery_settings`):
//! a background service, as Qt's RecoverySync is. It reads every machine
//! every 15 seconds, follows the peers and whether this computer runs
//! agents, copies the newest policy to machines behind it, and sends
//! `recovery-changed` with the page's view. Calls run on their own threads.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use zerus_client_core::recovery_settings::{
    answer, form, invocation, policy, Effect, Form, RecoverySync, REFRESH, SHARED_POLICY_KEY,
};
use zerus_client_core::{default_hgs, ComputerId};

use crate::commands::AppState;
use crate::settings::Store;

/// The service's state: the shared policy and the policy the page loaded
/// its form from (Qt's `base`).
pub struct Recovery(Arc<Inner>);

struct Inner {
    sync: Mutex<RecoverySync>,
    base: Mutex<Map<String, Value>>,
}

impl Inner {
    fn sync(&self) -> MutexGuard<'_, RecoverySync> {
        self.sync.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn base(&self) -> MutexGuard<'_, Map<String, Value>> {
        self.base.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the page shows of the service.
fn view(sync: &RecoverySync) -> Value {
    json!({
        "status": sync.status(),
        "error": sync.error(),
        "busy": sync.busy(),
        "can_save": sync.can_save(),
        "has_machines": sync.has_machines(),
        "has_policy": !sync.envelope().is_empty(),
        "saves": sync.saves(),
    })
}

/// Starts the service: the newest policy kept from before, then a read of
/// every machine now and every 15 seconds.
pub fn start(app: &AppHandle) -> Recovery {
    let stored = app.state::<Store>().0.text(SHARED_POLICY_KEY);
    let inner = Arc::new(Inner {
        sync: Mutex::new(RecoverySync::new(&stored)),
        base: Mutex::new(Map::new()),
    });
    let (handle, service) = (app.clone(), Arc::clone(&inner));
    let started = std::thread::Builder::new()
        .name("recovery-sync".into())
        .spawn(move || loop {
            tick(&handle, &service);
            std::thread::sleep(REFRESH);
        });
    if let Err(error) = started {
        eprintln!("zerus-client: automatic recovery settings: {error}");
    }
    Recovery(inner)
}

/// One period: the peers and local sessions as the fleet has them now, then
/// a read of every machine.
fn tick(app: &AppHandle, inner: &Arc<Inner>) {
    let state = app.state::<AppState>();
    let peers = state.peers();
    let local = state
        .pump()
        .fleet()
        .into_iter()
        .find(|entry| entry.computer == ComputerId::local());
    let mut effects = Vec::new();
    {
        let mut sync = inner.sync();
        if let Some(local) = local {
            effects.extend(sync.set_local_sessions(local.local_sessions));
            // Machines offers to enable local sessions on a control console.
            sync.set_local_sessions_offered(!local.local_sessions);
        }
        if let Some(peers) = peers {
            effects.extend(sync.set_peers(peers.into_iter().collect()));
        }
        effects.extend(sync.refresh());
    }
    run(app, inner, effects);
    changed(app, inner);
}

fn changed(app: &AppHandle, inner: &Arc<Inner>) {
    let view = view(&inner.sync());
    let _ = app.emit("recovery-changed", view);
}

/// Runs what the service asked for; the caller sends the view after.
fn run(app: &AppHandle, inner: &Arc<Inner>, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::Call(call) => {
                let (app, inner) = (app.clone(), Arc::clone(inner));
                std::thread::spawn(move || {
                    let reply = answer(zerus_client_core::run::run(
                        &default_hgs(),
                        &invocation(&call),
                    ));
                    let next = inner.sync().answered(call.id, reply);
                    run(&app, &inner, next);
                    changed(&app, &inner);
                });
            }
            Effect::Persist(text) => {
                let _ = app.state::<Store>().0.set(SHARED_POLICY_KEY, json!(text));
            }
        }
    }
}

/// The page's view.
#[tauri::command]
pub fn recovery_view(recovery: State<'_, Recovery>) -> Value {
    view(&recovery.0.sync())
}

/// The form for the newest policy, which becomes the base a save is
/// checked against (RecoverySettings::load); null before any policy.
#[tauri::command]
pub fn recovery_load(recovery: State<'_, Recovery>) -> Value {
    let envelope = recovery.0.sync().envelope().clone();
    let loaded = form(&envelope);
    *recovery.0.base() = envelope;
    json!({ "form": loaded })
}

/// Save recovery settings: the form's delays checked as Qt checks them,
/// then the policy sent on the loaded base. Answers the view and Qt's text
/// when the form is refused.
#[tauri::command(async)]
pub fn recovery_save(app: AppHandle, recovery: State<'_, Recovery>, form: Form) -> Value {
    let base = recovery.0.base().clone();
    let built = policy(&base, &form);
    let (effects, message) = match built {
        Err(message) => (Vec::new(), Some(message)),
        Ok(policy) => (recovery.0.sync().save(policy, &base), None),
    };
    let sent = !effects.is_empty();
    let view = view(&recovery.0.sync());
    run(&app, &recovery.0, effects);
    json!({ "view": view, "message": message, "sent": sent })
}

/// Reload: every machine read again.
#[tauri::command(async)]
pub fn recovery_reload(app: AppHandle, recovery: State<'_, Recovery>) -> Value {
    let effects = recovery.0.sync().refresh();
    run(&app, &recovery.0, effects);
    view(&recovery.0.sync())
}
