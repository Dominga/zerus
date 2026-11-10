//! Task #60 (team ede724c4): the window a test waits in for work that runs
//! beside it (a hub tick, a worker thread, a fixture process). Task 44's rule:
//! a test bounds its work, never by a wall-clock figure a loaded machine
//! exceeds; a wait for other work gets at least `FLOOR`, as task 32's
//! read_window does for Git. The wait still ends as soon as the work is done.
use std::time::Duration;

/// The least a test waits for work that runs beside it.
pub(crate) const FLOOR: Duration = Duration::from_secs(60);

/// `seconds`, or `FLOOR` when that is longer.
pub(crate) fn window(seconds: u64) -> Duration {
    Duration::from_secs(seconds).max(FLOOR)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_wait_gets_at_least_the_floor() {
        assert_eq!(super::window(5), super::FLOOR);
        assert_eq!(super::window(120), std::time::Duration::from_secs(120));
    }
}
