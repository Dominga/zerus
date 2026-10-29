//! When the screen reads run: pure decisions over a millisecond clock the
//! hub passes in (client/src-tauri/src/screens.rs runs the reads). Sources:
//! AccountsPage::ensureCatalogs and requestQueuedCatalog
//! (AccountsPage.cpp:450-468), AccountUsageStore::ensure, dispatch and
//! finish (AccountUsageStore.cpp:24-64), SwarmController's 5 s poll.

use serde::{Deserialize, Serialize};

/// The shared project catalog's poll (SwarmController pollInterval).
pub const SWARM_INTERVAL: u64 = 5_000;

/// A screen whose reads the hub runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    Overview,
    Projects,
    Machines,
    Accounts,
}

impl Screen {
    /// The name the `screens-changed` event carries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Projects => "projects",
            Self::Machines => "machines",
            Self::Accounts => "accounts",
        }
    }
}

/// A read that runs at most once at a time, again `interval` after the
/// previous start.
#[derive(Clone, Debug, Default)]
pub struct Poll {
    interval: u64,
    started: Option<u64>,
    pending: bool,
}

impl Poll {
    pub fn new(interval: u64) -> Self {
        Self {
            interval,
            started: None,
            pending: false,
        }
    }

    /// Whether a read starts now; when it does, it is marked running.
    pub fn due(&mut self, now: u64, force: bool) -> bool {
        if self.pending {
            return false;
        }
        if !force
            && self
                .started
                .is_some_and(|at| now.saturating_sub(at) < self.interval)
        {
            return false;
        }
        self.started = Some(now);
        self.pending = true;
        true
    }

    pub fn finish(&mut self) {
        self.pending = false;
    }

    pub fn pending(&self) -> bool {
        self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Review Focus 5 (the timing half; the hub test in Task 3 covers visibility).
    #[test]
    fn the_swarm_poll_runs_every_five_seconds_and_never_twice_at_once() {
        let mut poll = Poll::new(SWARM_INTERVAL);
        assert!(poll.due(0, false));
        assert!(!poll.due(6_000, false), "one runs");
        assert!(
            !poll.due(6_000, true),
            "a forced read waits for the one running"
        );
        poll.finish();
        assert!(poll.due(6_000, false));
        poll.finish();
        assert!(!poll.due(10_999, false));
        assert!(poll.due(11_000, false));
        poll.finish();
        assert!(poll.due(11_001, true), "forced: at once");
    }

    #[test]
    fn the_screen_names_match_the_screens_changed_event() {
        assert_eq!(
            serde_json::to_value(Screen::Overview).unwrap(),
            json!("overview")
        );
        assert_eq!(Screen::Projects.name(), "projects");
    }
}
