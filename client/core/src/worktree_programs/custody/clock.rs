//! Fixed host-local monotonic bounds for the closed custody protocol.
//! A timestamp is DATA, not permission. Peers and the original native phase
//! are admitted independently. No receipt may restart a remaining duration.
use std::{
    io,
    time::{Duration, Instant},
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "original native custody deadline is unavailable",
    )
}
pub fn now() -> io::Result<u64> {
    // Match Rust Instant, including Apple's suspend behavior. No clock/epoch
    // selector enters the wire or environment, and there is no wall fallback.
    // https://doc.rust-lang.org/src/std/sys/time/unix.rs.html
    #[cfg(target_os = "macos")]
    let clock = libc::CLOCK_UPTIME_RAW;
    #[cfg(target_os = "linux")]
    let clock = libc::CLOCK_MONOTONIC;
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(clock, &mut time) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if time.tv_sec < 0 || !(0..1_000_000_000).contains(&time.tv_nsec) {
        return Err(invalid());
    }
    (time.tv_sec as u64)
        .checked_mul(1000)
        .and_then(|v| v.checked_add(time.tv_nsec as u64 / 1_000_000))
        .ok_or_else(invalid)
}
/// Observe native time BEFORE sampling the sender's remaining Instant budget.
/// Rounding goes down, so conversion cannot add transport or scheduling time.
pub fn freeze(until: Instant) -> io::Result<u64> {
    let native = now()?;
    let remaining = until
        .checked_duration_since(Instant::now())
        .ok_or_else(invalid)?;
    if remaining > Duration::from_millis(super::PHASE_MILLIS) {
        return Err(invalid());
    }
    let millis = u64::try_from(remaining.as_millis()).map_err(|_| invalid())?;
    if millis == 0 {
        return Err(invalid());
    }
    native.checked_add(millis).ok_or_else(invalid)
}
fn at(deadline: u64, native: u64, before: Instant) -> io::Result<Instant> {
    let remaining = deadline
        .checked_sub(native)
        .and_then(|v| v.checked_sub(1))
        .ok_or_else(invalid)?;
    // Account for the native millisecond rounding quantum, also on reception.
    if remaining == 0 || remaining > super::PHASE_MILLIS {
        return Err(invalid());
    }
    before
        .checked_add(Duration::from_millis(remaining))
        .ok_or_else(invalid)
}
pub fn restore(deadline: u64) -> io::Result<Instant> {
    let before = Instant::now();
    at(deadline, now()?, before)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivery_delay_never_restarts_the_original_phase_budget() {
        let origin = Instant::now();
        let first = at(10_000, 9_000, origin).unwrap();
        let delayed = at(10_000, 9_400, origin + Duration::from_millis(400)).unwrap();
        assert_eq!(first, delayed);
        assert!(at(10_000, 10_000, origin + Duration::from_secs(1)).is_err());
        assert!(at(0, 0, origin).is_err());
        assert!(at(super::super::PHASE_MILLIS + 2, 0, origin).is_err());
        assert!(at(u64::MAX, 0, origin).is_err());
    }
    #[test]
    fn the_actual_native_bridge_is_conservative_and_has_no_clock_fallback() {
        let original = Instant::now() + Duration::from_secs(2);
        let frozen = freeze(original).unwrap();
        assert!(restore(frozen).unwrap() <= original);
        assert!(freeze(Instant::now()).is_err());
        assert!(freeze(Instant::now() + Duration::from_secs(16)).is_err());
        assert!(restore(now().unwrap().saturating_sub(1)).is_err());
    }
}
