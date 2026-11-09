//! The overall deadline of one multi-child desktop operation (zerus-qw7wp):
//! fixed once, at the operation's own first turn, when it gets its actor's
//! one work slot, and never renewed. Time queued behind another operation
//! does not count, and nothing of the operation starts after it. Shared by
//! the compact-to-send and session-resume actors (zerus-5wmh.66.4.9).
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub(in crate::desktop) struct Clock {
    /// The operation's whole bound.
    length: Duration,
    /// Its deadline, once its first turn started it.
    started: OnceLock<Instant>,
}
impl Clock {
    pub fn new(length: Duration) -> Self {
        Self {
            length,
            started: OnceLock::new(),
        }
    }
    /// The operation's deadline; its first call, at the operation's first
    /// turn, starts it, and every later call returns the same instant.
    pub fn deadline(&self) -> Instant {
        *self.started.get_or_init(|| Instant::now() + self.length)
    }
    /// Observe clock ownership without starting or renewing the operation.
    #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
    pub(in crate::desktop) fn fixture_started(&self) -> Option<Instant> {
        self.started.get().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deadline_starts_at_the_first_call_and_is_never_renewed() {
        let clock = Clock::new(Duration::from_secs(60));
        let created = Instant::now();
        std::thread::sleep(Duration::from_millis(20));
        let first = clock.deadline();
        // Counted from the first call, not from the clock's creation.
        assert!(first >= created + Duration::from_secs(60) + Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(clock.deadline(), first);
    }
}
