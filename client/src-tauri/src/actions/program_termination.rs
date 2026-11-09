//! The new explicit Remove path reuses its pre-discovery Actions claim. No
//! second claim, default CLI selection or reconstructed invocation is allowed.
use super::*;
use crate::desktop_service::termination_outcome::programs::pipeline::{Confirmation, Continuation};

pub(crate) async fn dispatch_program_termination(
    state: &Actions,
    confirmation: Confirmation,
    delayed: Option<(zerus_client_core::end_action::Captured, Arc<Claim>)>,
) -> Result<(Outcome, Option<Continuation>), String> {
    let target = confirmation.target().clone();
    let action = confirmation.action();
    if blocked() {
        return Ok((failed(REPLAY_NEEDS_HGS), None));
    }
    if let Some(reason) = refused_before_claim(&target, &action) {
        return Ok((failed(reason), None));
    }
    if delayed
        .as_ref()
        .is_some_and(|d| !confirmation.check_delay(d))
    {
        return Ok((failed(zerus_client_core::end_action::STATE_CHANGED), None));
    }
    let termination = state
        .termination_pump
        .get()
        .and_then(crate::pump::WeakPump::upgrade)
        .and_then(|pump| pump.capture_termination(&target));
    let (outcome, continuation) = tauri::async_runtime::spawn_blocking(move || {
        if let Some((captured, _)) = &delayed {
            if let Err(error) = confirmation.delayed_check(captured) {
                return (failed(error), None);
            }
        }
        confirmation.run()
    })
    .await
    .map_err(|error| error.to_string())?;
    state.finished(&target, &action, &outcome);
    if matches!(outcome, Outcome::Done { .. }) {
        if let (Some(pump), Some(original)) = (
            state
                .termination_pump
                .get()
                .and_then(crate::pump::WeakPump::upgrade),
            termination,
        ) {
            pump.termination_succeeded(original);
        }
    }
    Ok((outcome, continuation))
}
