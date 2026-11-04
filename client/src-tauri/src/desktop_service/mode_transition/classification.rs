//! Classification failures restrict business, never the read connection.
use super::*;

impl Controller {
    pub(in crate::desktop_service) fn require_classification(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .classification_required = true;
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
    /// Existing explicit Resume only. Clearing a scheduling deferral grants no
    /// native authority: the unchanged caller/Grant/per-kind checks still run.
    pub(in crate::desktop_service) fn confirmed_resume(&self) {
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

/// One joined, observation-only worker for the original watch. No startup,
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
