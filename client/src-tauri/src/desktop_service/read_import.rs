//! One native startup producer; no JS source paths/rows, second read worker,
//! writer activation, source cleanup or delivery acknowledgement.
use super::*;
use crate::actions::{Actions, CapturedReads};
use std::time::Instant;
use zerus_client_core::desktop::pending_reads::Source;

#[derive(Default)]
pub(super) struct State {
    actions: Option<Actions>,
    captured: Option<Arc<CapturedReads>>,
    source: Option<Source>,
    configuration: Option<String>,
    after: usize,
    attempted: bool,
    running: bool,
    complete: bool,
    pages_done: bool,
    code: &'static str,
}
impl State {
    pub fn blocked(&self) -> bool {
        self.actions.is_some() && !self.complete
    }
    pub fn status(&self) -> Value {
        json!({"required":self.actions.is_some(),"complete":self.complete,
            "running":self.running,"source":self.source,"through":self.after,
            "code":self.code,"delivered":false})
    }
}
impl Service {
    pub(crate) fn start_pending_read_import(self: &Arc<Self>, actions: Actions) {
        {
            let mut state = self
                .shared
                .read_import
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state.actions.is_some() {
                return;
            }
            state.actions = Some(actions);
            state.code = "pending_read_import_waiting";
        }
        retry(self.shared.clone());
    }
}

fn publish(shared: &Shared) {
    let status = shared
        .read_import
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .status();
    let mut snapshot = shared
        .snapshot
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    snapshot.pending_read_import = Some(status);
    shared.pump.service(snapshot.clone());
}
fn finish(shared: &Shared, code: &'static str) {
    {
        let mut state = shared
            .read_import
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.running = false;
        state.code = code;
    }
    publish(shared);
}

pub(super) fn retry(shared: Arc<Shared>) {
    let Ok(charge) = shared.rpc.admit(Role::ReadDelivery) else {
        return;
    };
    let (actions, captured) = {
        let mut state = shared
            .read_import
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if shared.stopped.load(Ordering::Acquire)
            || state.running
            || state.complete
            || (state.attempted && state.captured.is_none())
        {
            return;
        }
        let Some(actions) = state.actions.clone() else {
            return;
        };
        state.attempted = true;
        state.running = true;
        (actions, state.captured.clone())
    };
    publish(&shared);
    tauri::async_runtime::spawn(async move {
        // The charge remains inside every original checkpoint/native worker,
        // even if its receiver times out or the UI requests shutdown.
        let hold: Arc<dyn Send + Sync> = Arc::new(charge);
        let captured = match captured {
            Some(captured) => captured,
            None => {
                let retained = hold.clone();
                let capture_deadline = Instant::now() + Duration::from_secs(30);
                let result = tauri::async_runtime::spawn_blocking(move || {
                    let _retained = retained;
                    actions
                        .freeze_reads()
                        .and_then(|frozen| frozen.capture(capture_deadline))
                })
                .await;
                match result {
                    Ok(Ok(captured)) => {
                        let captured = Arc::new(captured);
                        {
                            let mut state = shared
                                .read_import
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner);
                            state.source = Some(captured.capture.source().clone());
                            state.captured = Some(captured.clone());
                        }
                        captured
                    }
                    _ => {
                        finish(&shared, "pending_read_checkpoint_unavailable");
                        return;
                    }
                }
            }
        };
        if !shared.allowed {
            finish(&shared, "frontend_route_unavailable");
            return;
        }
        let resources: Arc<dyn Send + Sync> = Arc::new((hold, captured.clone()));
        let deadline = Instant::now() + Duration::from_secs(30);
        let configuration = loop {
            if shared.stopped.load(Ordering::Acquire) {
                finish(&shared, "pending_read_import_stopped");
                return;
            }
            let connected = shared
                .snapshot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .connected;
            let configuration = shared
                .responder
                .as_ref()
                .and_then(|r| r.cached_configuration().map(str::to_owned));
            if let Some(configuration) = configuration.filter(|_| connected) {
                break configuration;
            }
            if Instant::now() >= deadline {
                finish(&shared, "pending_read_import_waiting_for_service");
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        {
            let mut state = shared
                .read_import
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state
                .configuration
                .as_ref()
                .is_some_and(|old| old != &configuration)
            {
                drop(state);
                finish(&shared, "pending_read_import_configuration_changed");
                return;
            }
            state.configuration = Some(configuration);
        }
        if !unchanged(captured.clone(), resources.clone()).await {
            finish(&shared, "pending_read_import_source_changed");
            return;
        }
        loop {
            if shared.stopped.load(Ordering::Acquire) {
                finish(&shared, "pending_read_import_stopped");
                return;
            }
            if shared
                .read_import
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pages_done
            {
                break;
            }
            let after = shared
                .read_import
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .after;
            let page = match captured.capture.page(after) {
                Ok(page) if captured.capture.matches(&page) => page,
                _ => {
                    finish(&shared, "pending_read_import_page_changed");
                    return;
                }
            };
            let through = page.through();
            let total = page.source.rows;
            let response = frontend::import_page(shared.clone(), page, resources.clone()).await;
            if response["accepted"] != true
                || response["durable"] != true
                || response["pending"] != false
            {
                finish(&shared, "pending_read_import_unconfirmed");
                return;
            }
            {
                let mut state = shared
                    .read_import
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                state.after = through;
                state.pages_done = through == total;
            }
            publish(&shared);
            if through == total {
                break;
            }
            tokio::task::yield_now().await;
        }
        if !unchanged(captured, resources).await {
            finish(&shared, "pending_read_import_source_changed");
            return;
        }
        {
            let mut state = shared
                .read_import
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.complete = true;
            state.captured = None;
        }
        finish(&shared, "complete");
    });
}
async fn unchanged(captured: Arc<CapturedReads>, hold: Arc<dyn Send + Sync>) -> bool {
    tauri::async_runtime::spawn_blocking(move || {
        let _hold = hold;
        captured.unchanged().is_ok()
    })
    .await
    .unwrap_or(false)
}
