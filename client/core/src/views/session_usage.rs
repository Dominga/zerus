//! The selected session's account usage (`AccountUsageStore::bound`). The
//! screens worker owns the clock and runs these reads through the same
//! two-slot dispatch as profile usage; this module owns only their state.
use super::accounts::UsageSlot;
use super::usage::{usage, Usage};
use super::{text, FleetEntry};
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

pub const REFRESH_INTERVAL: u64 = 5 * 60 * 1000;
const RETAINED: usize = 64;

/// A session-bound `account inspect --session NAME --refresh`, pinned to the
/// listed run when the reply carries one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionUsageRef {
    pub host: ComputerId,
    pub name: String,
    pub run_id: String,
}

#[derive(Default)]
pub struct SessionUsageState {
    current: Option<SessionUsageRef>,
    slots: BTreeMap<SessionUsageRef, UsageSlot>,
    queue: VecDeque<SessionUsageRef>,
    order: VecDeque<SessionUsageRef>,
}

impl SessionUsageState {
    pub fn current(&self) -> Option<&SessionUsageRef> {
        self.current.as_ref()
    }

    /// Selects the session whose usage is shown. Reads already running finish;
    /// only the current session starts later refreshes.
    pub fn follow(&mut self, target: Option<SessionUsageRef>) {
        self.current = target;
        if let Some(target) = self.current.clone() {
            self.order.retain(|key| key != &target);
            self.order.push_back(target);
        }
        while self.order.len() > RETAINED {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if self.current.as_ref() != Some(&old) {
                self.slots.remove(&old);
                self.queue.retain(|key| key != &old);
            }
        }
    }

    /// Queues the current session when due. `force` is Refresh usage; a read
    /// already in flight is shared, never duplicated.
    pub fn ensure(&mut self, now: u64, online: bool, force: bool) {
        let Some(target) = self.current.clone() else {
            return;
        };
        let slot = self.slots.entry(target.clone()).or_default();
        if slot.pending || !online {
            return;
        }
        if !force
            && slot
                .attempted
                .is_some_and(|at| now.saturating_sub(at) < REFRESH_INTERVAL)
        {
            return;
        }
        slot.attempted = Some(now);
        slot.pending = true;
        if !self.queue.contains(&target) {
            self.queue.push_back(target);
        }
    }

    /// Takes queued reads while a shared slot is free. Offline hosts leave no
    /// pending read behind.
    pub fn dispatch(
        &mut self,
        reachable: &impl Fn(&ComputerId) -> bool,
        slots: &mut super::accounts::AccountsState,
    ) -> Vec<SessionUsageRef> {
        let mut out = Vec::new();
        while let Some(target) = self.queue.front().cloned() {
            if !reachable(&target.host) {
                self.queue.pop_front();
                if let Some(slot) = self.slots.get_mut(&target) {
                    slot.pending = false;
                }
                continue;
            }
            if !slots.claim_usage_slot() {
                break;
            }
            self.queue.pop_front();
            out.push(target);
        }
        out
    }

    /// Files one reply. A mismatched or unavailable reply keeps the last good
    /// values and marks the refresh failed, as AccountUsageStore::finish does.
    pub fn finish(
        &mut self,
        target: &SessionUsageRef,
        reply: Option<Value>,
        slots: &mut super::accounts::AccountsState,
    ) {
        slots.running = slots.running.saturating_sub(1);
        let slot = self.slots.entry(target.clone()).or_default();
        slot.pending = false;
        let matches = reply.as_ref().is_some_and(|data| {
            data.is_object()
                && data.get("status").is_some()
                && (text(data, "run_id").is_empty() || text(data, "run_id") == target.run_id)
        });
        let failed = !matches
            || reply
                .as_ref()
                .is_some_and(|data| text(data, "status") == "unavailable");
        slot.failed = failed;
        if let Some(data) = reply.filter(|_| matches) {
            if !failed || slot.data.is_none() {
                slot.data = Some(data);
            }
        } else if slot.data.is_none() {
            slot.data = Some(json!({"status": "unavailable"}));
        }
    }

    /// The selected session's current view; navigation to another run cannot
    /// reuse this snapshot.
    pub fn view(&self, online: bool) -> Usage {
        let Some(target) = self.current.as_ref() else {
            return Usage::default();
        };
        let slot = self.slots.get(target);
        let mut view = slot
            .and_then(|slot| slot.data.as_ref())
            .map(usage)
            .unwrap_or_else(|| Usage {
                status: if online { "loading" } else { "offline" }.into(),
                ..Usage::default()
            });
        view.refreshing = slot.is_some_and(|slot| slot.pending);
        view.refresh_error = slot.is_some_and(|slot| slot.failed);
        view.offline = !online;
        view
    }

    #[cfg(test)]
    fn pending(&self, target: &SessionUsageRef) -> bool {
        self.slots.get(target).is_some_and(|slot| slot.pending)
    }
}

/// Online as AccountUsageStore sees the selected host.
pub fn online(fleet: &[FleetEntry], host: &ComputerId) -> bool {
    fleet
        .iter()
        .find(|entry| &entry.computer == host)
        .is_some_and(super::accounts::reachable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::accounts::AccountsState;

    fn target(run: &str) -> SessionUsageRef {
        SessionUsageRef {
            host: ComputerId::from("studio"),
            name: "claude/sample-project/review".into(),
            run_id: run.into(),
        }
    }

    #[test]
    fn refreshes_are_bounded_shared_and_run_scoped() {
        let mut state = SessionUsageState::default();
        let mut slots = AccountsState::default();
        state.follow(Some(target("run-1")));
        state.ensure(10, true, false);
        state.ensure(11, true, true);
        let reads = state.dispatch(&|_| true, &mut slots);
        assert_eq!(reads, [target("run-1")]);
        assert_eq!(slots.running, 1);
        assert!(state.pending(&target("run-1")));
        state.finish(
            &target("run-1"),
            Some(json!({"status": "ok", "run_id": "run-2", "windows": []})),
            &mut slots,
        );
        assert_eq!(slots.running, 0, "completion returns the shared read slot");
        assert!(state.view(true).refresh_error);
        assert_eq!(state.view(true).status, "unavailable");
        state.ensure(12, true, false);
        state.finish(
            &target("run-1"),
            Some(json!({"status": "ok", "run_id": "run-1"})),
            &mut slots,
        );
        state.ensure(13, true, false);
        assert!(state.queue.is_empty(), "a failed read was not repeated");
        state.ensure(REFRESH_INTERVAL + 12, true, false);
        assert_eq!(state.queue.len(), 1, "the five-minute refresh is due");
        state.dispatch(&|_| true, &mut slots);
        state.follow(Some(target("run-2")));
        assert_eq!(state.view(true).status, "loading");
        assert!(state.view(true).windows.is_empty());
    }

    #[test]
    fn dispatch_shares_two_slots_and_skips_offline_hosts() {
        let mut state = SessionUsageState::default();
        let mut slots = AccountsState::default();
        for run in ["run-1", "run-2", "run-3"] {
            state.follow(Some(target(run)));
            state.ensure(1, true, false);
        }
        slots.running = 1;
        let first = state.dispatch(&|host| host.0 != "studio", &mut slots);
        assert!(first.is_empty());
        assert!(!state.pending(&target("run-3")));
        state.follow(Some(target("run-1")));
        state.ensure(2, true, true);
        let reads = state.dispatch(&|_| true, &mut slots);
        assert_eq!(reads, [target("run-1")]);
        assert_eq!(slots.running, 2);
        assert!(state.dispatch(&|_| true, &mut slots).is_empty());
    }

    #[test]
    fn failures_keep_last_good_values_and_cache_is_bounded() {
        let mut state = SessionUsageState::default();
        let mut slots = AccountsState::default();
        state.follow(Some(target("run-1")));
        state.ensure(1, true, false);
        state.finish(
            &target("run-1"),
            Some(json!({"status": "ok", "provider": "claude", "checked_at": 10})),
            &mut slots,
        );
        state.ensure(2, true, true);
        state.finish(&target("run-1"), None, &mut slots);
        let view = state.view(false);
        assert_eq!(view.checked_at, Some(10.0));
        assert!(view.refresh_error && view.offline && !view.refreshing);
        for index in 0..RETAINED + 2 {
            let run = format!("run-{index}");
            state.follow(Some(target(&run)));
        }
        assert!(state.order.len() <= RETAINED);
        assert!(state.slots.len() <= RETAINED);
        assert!(state.current().is_some());
    }
}
