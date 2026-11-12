//! Inactive native task-control support. Publication under the native writer
//! never opens the desktop database; import belongs after that writer drops.
#![allow(dead_code)] // Removed as the runtime entry points are connected.
pub(crate) mod observation;
mod stop_facts;
mod capture;
pub(crate) use capture::Capture;
#[cfg(windows)]
pub(in crate::state) use stop_facts::{CaptureFailed, SelectedCapture};
pub(crate) use stop_facts::{Captured, Fact, Source};
pub(in crate::state) mod writer;

/// A worker owns the native exclusion only for this capture. Returned evidence
/// retains unlocked original objects, never the writer's FileLock. No desktop
/// connection or actor transaction can enter this function.
pub(crate) fn observe_and_capture(
    config: &crate::config::Config,
    expected: &zerus_desktop_ledger::task_controls::Executor,
    until: std::time::Instant,
) -> Result<(observation::Observed, Capture), String> {
    if !zerus_desktop_ledger::task_controls::TASK_CONTROLS {
        return Err("task_controls_inactive".into());
    }
    let computer = super::lifecycle_receipt_machine(config)?;
    let original = writer::open(&config.state, until)?.ok_or("task_controls_inactive")?;
    let observed = observation::read(
        original.root()?,
        original.guard()?,
        &computer,
        expected,
        observation::Conversation::Exact,
    )?
    .ok_or("task_controls_inactive")?;
    #[cfg(not(windows))]
    let captured = {
        let captured = capture_stops(original.root()?, original.guard()?, &observed.executor)?
            .ok_or("task_controls_inactive")?;
        original.verify()?;
        drop(original);
        Capture::legacy(captured)
    };
    #[cfg(windows)]
    let captured = match capture_owned_stops(original, &observed.executor) {
        Ok(captured) => Capture::owned(captured),
        Err(failed) => {
            // This worker offered no actor/native operation. Explicitly retire
            // its retained failed custody before returning an error to the DB
            // owner; no native Writer may cross that boundary.
            let reason = format!("Owned Stop capture refused: {failed:?}");
            failed.retire();
            return Err(reason);
        }
    };
    captured.with_current(&observed.executor, |_, _, check| check())?;
    Ok((observed, captured))
}

/// Existing-run capture only, under the caller's original native exclusion.
/// Missing history refuses; inspection never creates an empty outbox.
pub(crate) fn capture_stops(
    root: &std::path::Path,
    writer: &super::storage::Guard,
    executor: &zerus_desktop_ledger::task_controls::Executor,
) -> Result<Option<Captured>, String> {
    stop_facts::open(root, writer, executor, false)?
        .map(stop_facts::Publisher::capture)
        .transpose()
}

/// Windows production capture consumes the same original native owner/deadline.
/// Outside a typed current read scope, the result is data custody only.
#[cfg(windows)]
pub(in crate::state) fn capture_owned_stops(
    owned: writer::Writer,
    executor: &zerus_desktop_ledger::task_controls::Executor,
) -> Result<SelectedCapture, CaptureFailed> {
    stop_facts::capture_owned(owned, executor)
}
