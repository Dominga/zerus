//! Classification failures restrict business, never the read connection.
use super::*;

/// Covers early main-CLI errors and cancellation before mode observation.
/// This guard can only close scheduling, never admit any operation.
pub(in crate::desktop_service) struct Preflight<'a>(&'a Controller);
impl Drop for Preflight<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.classification_required && state.selection.is_none() {
            state.observation_only = true;
        }
    }
}

impl Controller {
    pub(in crate::desktop_service) fn preflight_scope(&self) -> Preflight<'_> {
        Preflight(self)
    }
    pub(in crate::desktop_service) fn require_classification(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .classification_required = true;
    }
    /// This connection's own read must classify it before exposing a snapshot.
    /// A busy/stopped/skipped read cannot borrow an earlier selection, and no
    /// later successful observation may supply scheduling consent.
    pub(in crate::desktop_service) async fn observe_preflight(
        self: &Arc<Self>,
        ordinary: PathBuf,
        configuration: String,
    ) {
        let classified = self.exchange(ordinary, configuration, false, false).await;
        self.preflight_decision(classified);
    }
    fn preflight_decision(&self, classified: bool) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.classification_required && !classified {
            state.observation_only = true;
            state.selection = None;
        }
    }
    pub(in crate::desktop_service) fn classification_pending(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.classification_required && state.selection.is_none()
    }
    pub(in crate::desktop_service) fn startup_permitted(&self) -> bool {
        !self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observation_only
    }
    /// Only explicit Resume or the checked settlement of its SAME native-
    /// confirmed background choice. Clearing a scheduling deferral grants no
    /// native authority: unchanged caller/Grant/per-kind checks still run.
    pub(in crate::desktop_service) fn confirmed_scheduling(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observation_only = false;
    }
}

async fn pause(shared: &Shared, stop: &AtomicBool, stopping: &Notify, duration: Duration) -> bool {
    let cancelled = stopping.notified();
    tokio::pin!(cancelled);
    cancelled.as_mut().enable();
    if stop.load(Ordering::Acquire) || shared.stopped.load(Ordering::Acquire) {
        return false;
    }
    tokio::select! {
        biased;
        _ = &mut cancelled => false,
        _ = tokio::time::sleep(duration) => !stop.load(Ordering::Acquire) && !shared.stopped.load(Ordering::Acquire),
    }
}

/// One observation-only worker for the original watch. No startup,
/// Resume, Grant or scheduler continuation is entered on a successful read.
pub(in crate::desktop_service) async fn connected(
    shared: Arc<Shared>,
    configuration: String,
    stop: Arc<AtomicBool>,
    ready: Arc<Notify>,
    stopping: Arc<Notify>,
) {
    {
        let cancelled = stopping.notified();
        tokio::pin!(cancelled);
        cancelled.as_mut().enable();
        if stop.load(Ordering::Acquire) || shared.stopped.load(Ordering::Acquire) {
            return;
        }
        tokio::select! {
            biased;
            _ = &mut cancelled => return,
            _ = ready.notified() => (),
        }
    }
    let mut backoff = super::super::reconnect::Backoff::default();
    let mut restore = true;
    loop {
        if stop.load(Ordering::Acquire)
            || shared.stopped.load(Ordering::Acquire)
            || shared.read_checks_only()
        {
            return;
        }
        if shared.mode_transition.classification_pending() {
            restore = true;
            shared
                .mode_transition
                .observe(shared.hgs.clone(), configuration.clone())
                .await;
        }
        if stop.load(Ordering::Acquire)
            || shared.stopped.load(Ordering::Acquire)
            || shared.read_checks_only()
        {
            return;
        }
        if restore
            && !shared.mode_transition.classification_pending()
            && !startup_recovery()
            && shared.mode_transition.blocks_resume()
        {
            shared
                .mode_transition
                .restore_business(
                    shared.ownership.clone(),
                    shared.snapshot.copy(),
                    stop.clone(),
                )
                .await;
        }
        if restore {
            shared
                .publish_ownership(crate::desktop_ownership::Phase::Result)
                .await;
        }
        if !shared.mode_transition.classification_pending() {
            restore = false;
            backoff.reset();
            // Remain available if a later Check loses classification. This
            // idle poll performs no helper, ownership or scheduling operation.
            if !pause(&shared, &stop, &stopping, Duration::from_millis(250)).await {
                return;
            }
            continue;
        }
        // Reuse the transport's bounded 250 ms..5 s schedule, without ending
        // the watch or classifying a refusal as transport failure.
        let delay = backoff
            .next("service_unavailable", false)
            .expect("bounded read retry");
        if !pause(&shared, &stop, &stopping, delay).await {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_before_mode_observation_keeps_the_preflight_decision_closed() {
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        for required in [false, true] {
            let owner = Controller::new();
            if required {
                owner.require_classification();
            }
            let mut future = Box::pin(async {
                let _decision = owner.preflight_scope();
                std::future::pending::<()>().await;
            });
            assert!(future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            drop(future);
            assert_eq!(owner.startup_permitted(), !required);
        }
    }
    #[test]
    fn a_stopped_preflight_never_borrows_a_previous_classification() {
        tauri::async_runtime::block_on(async {
            let owner = Controller::new();
            owner.require_classification();
            owner.project_for_test(
                &json!({"selected_mode":"background","running_mode":"background",
                "mode_selection":"explicit","ipc_ready":true,"pending_restart":false}),
                false,
            );
            assert!(!owner.classification_pending());
            owner.stop.store(true, Ordering::Release);
            owner
                .observe_preflight(PathBuf::from("unused-stopped-cli"), "a".repeat(64))
                .await;
            assert!(owner.classification_pending());
            assert!(!owner.startup_permitted());
        });
    }
    #[test]
    fn a_latest_read_loss_clear_never_clears_a_preflight_scheduling_deferral() {
        let owner = Controller::new();
        owner.require_classification();
        // No failed() call: preflight's unknown decision owns the deferral.
        owner.preflight_decision(false);
        assert!(!owner.startup_permitted());
        let mut status = json!({"selected_mode":"passive","running_mode":"passive",
            "mode_selection":"default","ipc_ready":true,"pending_restart":false,
            "activation_automatic_blocked":true,"first_choice_lost":"missing"});
        owner.project_for_test(&status, true);
        assert!(owner.view().automatic_blocked);
        assert!(!owner.classification_pending());
        status["first_choice_lost"] = Value::Null;
        status["activation_automatic_blocked"] = false.into();
        owner.project_for_test(&status, true);
        assert!(
            !owner.view().automatic_blocked,
            "R4 loss is latest-read only"
        );
        assert!(
            !owner.startup_permitted(),
            "read recovery is never scheduling consent"
        );
        owner.preflight_decision(true); // A healthy reconnect cannot clear it.
        assert!(!owner.startup_permitted());
        owner.confirmed_scheduling(); // The existing explicit continuation is distinct.
        assert!(owner.startup_permitted());
    }
}
