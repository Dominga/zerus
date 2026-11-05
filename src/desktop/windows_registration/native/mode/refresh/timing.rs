//! Bounded diagnostics only. Wall time never participates in admission,
//! deadline selection, replay or an original-process observation.
use super::*;
use std::{
    cell::RefCell,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Timing {
    start: Instant,
    phase: RefCell<String>,
    events: RefCell<Vec<Value>>,
}
impl Timing {
    pub(super) fn new() -> Self {
        Self {
            start: Instant::now(),
            phase: RefCell::new("Preflight".into()),
            events: RefCell::new(Vec::new()),
        }
    }
    pub(super) fn phase(&self, phase: Budget, until: Instant) {
        *self.phase.borrow_mut() = format!("{phase:?}");
        self.mark("phase.begin", until, None);
    }
    pub(super) fn mark(&self, name: &str, until: Instant, observed: Option<bool>) {
        let now = Instant::now();
        let mut events = self.events.borrow_mut();
        if events.len() == 64 {
            events.remove(0);
        }
        events.push(json!({"event":name,"phase":*self.phase.borrow(),
            "elapsed_ms":now.duration_since(self.start).as_millis() as u64,
            "utc_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            "remaining_ms":until.saturating_duration_since(now).as_millis() as u64,
            "observed":observed}));
    }
    pub(super) fn snapshot(&self) -> Value {
        json!({"abi":"desktop-refresh-timing-v1","events":*self.events.borrow()})
    }
}
