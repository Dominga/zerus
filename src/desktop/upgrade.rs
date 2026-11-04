//! Replacement is a request to retire this reader, never a writer permission.
use crate::swarm::{Executable, Replacement};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) const CHECK: Duration = Duration::from_secs(5);

#[cfg(all(test, target_os = "linux"))]
pub(super) mod actor_tests;

pub(super) fn owner_loans_released(owner: &Arc<crate::platform::FileLock>) -> bool {
    Arc::strong_count(owner) == 1
}

pub(super) fn connections_pending(
    active: usize,
    replacing: bool,
    before_deadline: bool,
    shutdown: bool,
) -> bool {
    active != 0
        && if replacing {
            !shutdown
        } else {
            before_deadline
        }
}

/// These routes only observe or reduce authority. The original handlers still
/// validate their scope, UUID, receipt and physical drain; this adds no grant.
pub(super) fn allowed_during_retirement(op: &str, request: Option<&Value>) -> bool {
    (matches!(op, "status" | "ownership_status") && request.is_none())
        || (op == "request"
            && request.is_some_and(|request| {
                matches!(request["op"].as_str(), Some("revoke" | "revoke_status"))
            }))
}

#[derive(Default)]
struct Stable<T> {
    candidate: Option<T>,
    requested: bool,
}
impl<T: Copy + Eq> Stable<T> {
    fn observe(&mut self, candidate: Option<T>) -> bool {
        self.requested |= candidate.is_some() && candidate == self.candidate;
        self.candidate = candidate;
        self.requested
    }
}

pub(super) struct Upgrade {
    executable: Option<Executable>,
    stable: Stable<Replacement>,
    next: Instant,
    supervised: bool,
}
impl Upgrade {
    pub fn new() -> Self {
        Self {
            executable: Executable::current(),
            stable: Stable {
                candidate: None,
                requested: false,
            },
            next: Instant::now(),
            supervised: cfg!(any(target_os = "linux", target_os = "macos")),
        }
    }
    pub fn poll(&mut self, now: Instant) -> bool {
        if !self.requested() && now >= self.next {
            self.next = now + CHECK;
            self.stable
                .observe(self.executable.as_ref().and_then(Executable::replacement));
        }
        self.requested()
    }
    pub fn requested(&self) -> bool {
        self.stable.requested
    }
    pub fn supervised(&self) -> bool {
        // The maintained Linux/macOS registrations restart on a nonzero exit.
        // Windows task instances have no equivalent automatic restart contract.
        self.supervised
    }
    pub fn restart_ready(&self, owner: &Arc<crate::platform::FileLock>) -> bool {
        self.requested() && self.supervised() && owner_loans_released(owner)
    }
    pub fn decorate(&self, value: &mut Value) {
        if self.requested() {
            value["executable_upgrade"] = json!({"code":"hgs_replaced",
                "state":if self.supervised(){"draining"}else{"manual_refresh_required"},
                "new_admission":false});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unsupervised_reader_finishes_owner_cleanup_without_becoming_restartable() {
        let file = tempfile::tempfile().unwrap();
        let owner = Arc::new(crate::platform::FileLock::exclusive(file).unwrap());
        let mut upgrade = Upgrade::new();
        upgrade.supervised = false;
        upgrade.stable.requested = true;
        let original = owner.clone();
        assert!(!owner_loans_released(&owner));
        assert!(!upgrade.restart_ready(&owner));
        drop(original);
        assert!(owner_loans_released(&owner));
        assert!(!upgrade.restart_ready(&owner));
    }

    #[test]
    fn a_later_shutdown_ends_the_replacement_connection_wait() {
        assert!(connections_pending(1, true, false, false));
        assert!(!connections_pending(1, true, false, true));
        assert!(!connections_pending(1, true, true, true));
        assert!(!connections_pending(0, true, false, false));
        // Ordinary shutdown retains its existing bounded connection cleanup.
        assert!(connections_pending(1, false, true, true));
        assert!(!connections_pending(1, false, false, false));
    }

    #[test]
    fn recovery_and_original_revoke_routes_remain_available_without_positive_admission() {
        for op in ["status", "ownership_status"] {
            assert!(allowed_during_retirement(op, None));
            assert!(!allowed_during_retirement(op, Some(&json!({}))));
        }
        for op in ["revoke", "revoke_status"] {
            assert!(allowed_during_retirement(
                "request",
                Some(&json!({"op":op}))
            ));
        }
        for op in [
            "ping",
            "automatic_approvals",
            "read_intent",
            "desktop_ownership",
            "cache_maintenance",
        ] {
            assert!(!allowed_during_retirement(
                "request",
                Some(&json!({"op":op}))
            ));
        }
        for op in ["watch", "feed", "frontend_request", "request", "unknown"] {
            assert!(!allowed_during_retirement(op, None));
        }
        assert!(!allowed_during_retirement(
            "frontend_request",
            Some(&json!({"op":"revoke"}))
        ));
    }
    #[test]
    fn only_two_consecutive_ready_observations_retire_and_retirement_is_sticky() {
        let mut stable = Stable::<u32>::default();
        for candidate in [None, Some(1), None, Some(1), Some(2), None, Some(2)] {
            assert!(!stable.observe(candidate));
        }
        assert!(stable.observe(Some(2)));
        assert!(stable.observe(None));
        assert!(stable.observe(Some(3)));
    }
    #[cfg(unix)]
    #[test]
    fn an_original_owner_loan_prevents_restart_and_keeps_the_kernel_exclusion() {
        use fs2::FileExt;
        let file = tempfile::NamedTempFile::new().unwrap();
        let original = file.reopen().unwrap();
        original.try_lock_exclusive().unwrap();
        let owner = Arc::new(crate::platform::FileLock::held(original));
        let observer = file.reopen().unwrap();
        let mut upgrade = Upgrade::new();
        assert!(!upgrade.restart_ready(&owner));
        upgrade.stable.requested = true;
        let original_job = owner.clone();
        assert!(!upgrade.restart_ready(&owner));
        assert!(observer.try_lock_exclusive().is_err());
        drop(original_job);
        assert!(upgrade.restart_ready(&owner));
        assert!(
            observer.try_lock_exclusive().is_err(),
            "restart readiness never releases the owner"
        );
        drop(owner);
        observer.try_lock_exclusive().unwrap();
    }
}
