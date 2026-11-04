//! A bounded observation continuation. It never re-enters the confirmed
//! choice path, reconstructs an intent, or makes work eligible for scheduling.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

impl Controller {
    pub(in crate::desktop_service) fn scheduling_permitted(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .business
            .as_ref()
            .is_some_and(|b| b.observed.is_none())
    }

    pub(in crate::desktop_service) async fn restore_business(
        self: &Arc<Self>,
        ownership: Arc<crate::desktop_ownership::Controller>,
        snapshot: service::Snapshot,
        cancel: Arc<AtomicBool>,
    ) {
        // Preserve the already retained explicit continuation at its SAME
        // endpoint. This is not cold recovery and creates no new consent.
        if self.scheduling_permitted()
            && self.business_permitted(
                &snapshot,
                ownership.view().status.as_ref(),
                ownership.granted(),
            )
        {
            return;
        }
        // Any other previous binding cannot lend eligibility or scheduling
        // authority to this connection's observation callbacks.
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .business = None;
        if !snapshot.connected || cancel.load(Ordering::Acquire) {
            return;
        }
        let (Some(epoch), Some(generation)) = (snapshot.epoch, snapshot.generation) else {
            return;
        };
        let display = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .review
                .as_ref()
                .filter(|r| {
                    r.view.disposition == Disposition::ReviewedUnknown
                        && state.choice_capable
                        && !state.view.pending
                        && !state
                            .intent
                            .as_ref()
                            .is_some_and(|i| !i.complete || !i.joined)
                })
                .map(|r| r.view.clone())
        };
        let Some(display) = display else {
            return;
        };
        let Ok(charge) = self.gate.admit(Role::Fleet) else {
            return;
        };
        let owner = self.clone();
        let current_display = display.clone();
        let current_cancel = cancel.clone();
        let worker =
            tauri::async_runtime::spawn_blocking(move || -> Option<(Business, Resources)> {
                let route = owner.checked_target(&display, Action::Background).ok()?;
                let pin = RoutePin::new(route.clone()).ok()?;
                let resources: Resources = Arc::new((charge, pin));
                owner
                    .state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .view
                    .running = true;
                let _busy = Busy(owner.clone());
                let before =
                    owner.status(route.clone(), &display.configuration, resources.clone())?;
                owner.checked_target(&display, Action::Background).ok()?;
                if cancel.load(Ordering::Acquire) {
                    return None;
                }
                let historical =
                    Choice::historical(&before, &display.request_id, &display.configuration)?;
                // Query THIS historical incarnation against the independently
                // observed current endpoint, never the receipt's old service epoch.
                let observed = ownership
                    .observe_current(
                        zerus_client_core::desktop::ownership::observation::Query {
                            request_id: uuid::Uuid::new_v4().to_string(),
                            configuration: display.configuration.clone(),
                            journal: historical.scope.journal.clone(),
                            owner_epoch: historical.scope.owner_epoch.clone(),
                            service_epoch: epoch.clone(),
                            generation,
                        },
                        cancel.clone(),
                        resources.clone(),
                    )
                    .ok()?;
                #[cfg(windows)]
                if observed.image_id() != route.image_id {
                    return None;
                }
                #[cfg(not(windows))]
                if observed.helper() != route.cli {
                    return None;
                }
                if Choice::applicable(
                    &before,
                    &display.request_id,
                    &display.configuration,
                    observed.status(),
                    &epoch,
                    generation,
                )
                .as_ref()
                    != Some(&historical)
                {
                    return None;
                }
                // The exact original result, not merely a latest-status receipt.
                let bytes = owner.call_bounded(
                    route.clone(),
                    route.arguments(Some(&historical.request), false, false),
                    resources.clone(),
                    Duration::from_secs(3),
                )?;
                let result: Value = serde_json::from_slice(&bytes).ok()?;
                if !historical.original(&result, route.windows, &display.configuration) {
                    return None;
                }
                let after =
                    owner.status(route.clone(), &display.configuration, resources.clone())?;
                owner.checked_target(&display, Action::Background).ok()?;
                if cancel.load(Ordering::Acquire)
                    || !observed.matches(observed.status(), &epoch, generation)
                    || Choice::applicable(
                        &after,
                        &display.request_id,
                        &display.configuration,
                        observed.status(),
                        &epoch,
                        generation,
                    )
                    .as_ref()
                        != Some(&historical)
                {
                    return None;
                }
                Some((
                    Business {
                        choice: historical,
                        configuration: display.configuration,
                        epoch,
                        generation,
                        observed: Some(observed),
                    },
                    resources,
                ))
            });
        // An abandoned read keeps its original resources on that worker. It
        // can never publish eligibility late after this bounded caller returns.
        let Ok(Ok(Some((business, resources)))) =
            tokio::time::timeout(Duration::from_secs(12), worker).await
        else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.stop.load(Ordering::Acquire)
            && !current_cancel.load(Ordering::Acquire)
            && state
                .review
                .as_ref()
                .is_some_and(|r| r.view == current_display)
            && !state.view.pending
        {
            state.business = Some(business);
        }
        drop(state);
        drop(resources);
    }
}
