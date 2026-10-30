//! Atomic, close-only admission and lifetime accounting for frontend roles.
//! No I/O, worker, store barrier, identity validation or service authority.
//! Idle describes only admitted resource groups; uninstrumented work cannot
//! be inferred absent. The host must qualify every required role separately.
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

pub const MAX_ADMISSIONS: usize = 1024;
pub const ROLE_COUNT: usize = 12;

/// Fixed ownership groups. No per-session or caller-selected string keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Role {
    Fleet,
    Attention,
    NativePosting,
    Badges,
    ReadDelivery,
    Presence,
    Operations,
    Drafts,
    Providers,
    Downloads,
    Cleanup,
    AutomaticApprovals,
}
impl Role {
    pub const ALL: [Self; ROLE_COUNT] = [
        Self::Fleet,
        Self::Attention,
        Self::NativePosting,
        Self::Badges,
        Self::ReadDelivery,
        Self::Presence,
        Self::Operations,
        Self::Drafts,
        Self::Providers,
        Self::Downloads,
        Self::Cleanup,
        Self::AutomaticApprovals,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    InvalidLimit,
    Closed,
    Capacity,
    Unavailable,
}

/// Not serializable and not a runtime proof, even when closed and idle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub closed: bool,
    pub admitted: usize,
    roles: [usize; ROLE_COUNT],
}
impl Snapshot {
    pub fn count(&self, role: Role) -> usize {
        self.roles[role as usize]
    }
}

struct Shared {
    state: Mutex<Snapshot>,
    changed: Condvar,
    limit: usize,
}

#[derive(Clone)]
pub struct Gate(Arc<Shared>);
impl Gate {
    pub fn new(limit: usize) -> Result<Self, Refused> {
        if !(1..=MAX_ADMISSIONS).contains(&limit) {
            return Err(Refused::InvalidLimit);
        }
        Ok(Self(Arc::new(Shared {
            state: Mutex::new(Snapshot {
                closed: false,
                admitted: 0,
                roles: [0; ROLE_COUNT],
            }),
            changed: Condvar::new(),
            limit,
        })))
    }

    /// Charge before enqueue/spawn. Copies cover the complete admitted
    /// resource group; one group's clones keep one charge until the last drop.
    pub fn admit(&self, role: Role) -> Result<Charge, Refused> {
        let mut state = self.0.state.lock().map_err(|_| Refused::Unavailable)?;
        if state.closed {
            return Err(Refused::Closed);
        }
        if state.admitted == self.0.limit {
            return Err(Refused::Capacity);
        }
        state.admitted += 1;
        state.roles[role as usize] += 1;
        Ok(Charge(Arc::new(Admission {
            shared: self.0.clone(),
            role,
        })))
    }

    /// One-way and idempotent. No timeout, dropped handle or disconnected
    /// controller can reopen admissions. Does not cancel or signal anything.
    pub fn close(&self) -> Closed {
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
        self.0.changed.notify_all();
        Closed(self.0.clone())
    }

    pub fn snapshot(&self) -> Result<Snapshot, Refused> {
        self.0.snapshot()
    }
}

struct Admission {
    shared: Arc<Shared>,
    role: Role,
}
impl Drop for Admission {
    fn drop(&mut self) {
        // Poisoned accounting stays unknown, never reports a false idle.
        if let Ok(mut state) = self.shared.state.lock() {
            state.admitted -= 1;
            state.roles[self.role as usize] -= 1;
        }
        self.shared.changed.notify_all();
    }
}

/// The host can put a copy in every queued callback and owned I/O worker.
/// Dropping the caller's copy or joining one worker cannot release the rest.
#[derive(Clone)]
pub struct Charge(Arc<Admission>);
impl Charge {
    pub fn role(&self) -> Role {
        self.0.role
    }
}

#[derive(Clone)]
pub struct Closed(Arc<Shared>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    Idle,
    Pending,
    Unavailable,
}

impl Closed {
    pub fn snapshot(&self) -> Result<Snapshot, Refused> {
        self.0.snapshot()
    }

    /// Blocking host-side wait, off the GUI thread. The absolute deadline
    /// bounds only waiting, not the resource lifetime. Idle is not a store
    /// flush, an authenticated drain receipt or an effect permit.
    pub fn wait_until(&self, deadline: Instant) -> Observation {
        let Ok(mut state) = self.0.state.lock() else {
            return Observation::Unavailable;
        };
        loop {
            if state.admitted == 0 {
                return Observation::Idle;
            }
            let now = Instant::now();
            if now >= deadline {
                return Observation::Pending;
            }
            let Ok((next, _)) = self.0.changed.wait_timeout(state, deadline - now) else {
                return Observation::Unavailable;
            };
            state = next;
        }
    }
}

impl Shared {
    fn snapshot(&self) -> Result<Snapshot, Refused> {
        Ok(self.state.lock().map_err(|_| Refused::Unavailable)?.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisoned_accounting_never_authorizes_a_false_idle() {
        let gate = Gate::new(1).unwrap();
        let charge = gate.admit(Role::Drafts).unwrap();
        let broken = gate.clone();
        assert!(std::thread::spawn(move || {
            let _held = broken.0.state.lock().unwrap();
            panic!("synthetic accounting failure");
        })
        .join()
        .is_err());
        assert!(matches!(
            gate.admit(Role::Drafts),
            Err(Refused::Unavailable)
        ));
        let closed = gate.close();
        drop(charge);
        assert_eq!(closed.snapshot(), Err(Refused::Unavailable));
        assert_eq!(closed.wait_until(Instant::now()), Observation::Unavailable);
    }
}
