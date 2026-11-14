//! The cleanup probe's one-second stop clock starts at its driver witness.
use super::*;
struct State {
    ready: PathBuf,
    started: Option<Instant>,
    expired: bool,
}
impl State {
    fn poll(&mut self, now: Instant) -> bool {
        if self.started.is_none() && self.ready.is_file() {
            self.started = Some(now);
        }
        self.expired = self
            .started
            .is_some_and(|start| now.duration_since(start) >= Duration::from_secs(1));
        self.expired
    }
}
thread_local! {
    static CLOCK: RefCell<Option<State>> = const { RefCell::new(None) };
}
pub(super) struct Clock(Option<State>);
impl Clock {
    pub(super) fn arm(ready: PathBuf) -> Self {
        Self(CLOCK.with(|slot| {
            slot.replace(Some(State {
                ready,
                started: None,
                expired: false,
            }))
        }))
    }
    pub(super) fn expired(&self) -> bool {
        CLOCK.with(|slot| slot.borrow().as_ref().is_some_and(|state| state.expired))
    }
}
impl Drop for Clock {
    fn drop(&mut self) {
        CLOCK.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}
// The production predicate stays false. Only this fixture installs a clock;
// the actual bounded runner stops and drains the original process group.
pub(in crate::teams::integrate) fn timed_out() -> bool {
    CLOCK.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .is_some_and(|state| state.poll(Instant::now()))
    })
}
#[test]
fn the_driver_clock_ignores_startup_wait_and_keeps_its_one_second_budget() {
    let temp = tempfile::tempdir().unwrap();
    let ready = temp.path().join("ready");
    let start = Instant::now();
    let mut state = State {
        ready: ready.clone(),
        started: None,
        expired: false,
    };
    assert!(!state.poll(start + Duration::from_secs(60)));
    assert!(state.started.is_none());
    fs::write(ready, "ready").unwrap();
    let begun = start + Duration::from_secs(61);
    assert!(!state.poll(begun));
    assert!(!state.poll(begun + Duration::from_millis(999)));
    assert!(state.poll(begun + Duration::from_secs(1)));
}
