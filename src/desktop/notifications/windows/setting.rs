//! Identity-less Win32 notifier readiness, under one original admitted post.
//! A getter failure never becomes permission: only ERROR_NOT_FOUND allows one
//! suppressed identity bootstrap, and an explicit Enabled recheck is required.
use super::Refusal;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const NOT_FOUND: i32 = 0x8007_0490_u32 as i32;
#[cfg(windows)]
pub(super) static ATTEMPTED: AtomicBool = AtomicBool::new(false);

pub(super) trait Platform {
    fn setting(&mut self) -> Result<bool, i32>;
    /// Suppressed, expiring, content-free toast and exact cleanup. Check the
    /// original cancellation/current registration again just before its Show.
    fn bootstrap(&mut self, cancelled: &dyn Fn() -> bool) -> Result<(), Refusal>;
}

pub(super) fn ensure_enabled(
    platform: &mut impl Platform,
    attempted: &AtomicBool,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Refusal> {
    if cancelled() {
        return Err(Refusal::Cancelled);
    }
    match platform.setting() {
        Ok(true) => return Ok(()),
        Ok(false) => return Err(Refusal::PermissionDenied),
        Err(NOT_FOUND) => (),
        Err(_) => return Err(Refusal::GetterFailed),
    }
    if cancelled() {
        return Err(Refusal::Cancelled);
    }
    // Spend before the effect. Failure, cancellation or a new worker never
    // resets this process-local gate or creates a bootstrap retry loop.
    attempted
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| Refusal::GetterFailed)?;
    platform.bootstrap(cancelled)?;
    if cancelled() {
        return Err(Refusal::Cancelled);
    }
    match platform.setting() {
        Ok(true) => Ok(()),
        Ok(false) => Err(Refusal::PermissionDenied),
        Err(_) => Err(Refusal::GetterFailed),
    }
}

#[cfg(test)]
mod tests;
