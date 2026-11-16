//! Resource accounting only. Exhaustion never proves a root or causal span.
use crate::{Error, Result};
use std::time::{Duration, Instant};

const WORK: usize = 131_072;
// Work is the primary bound. Thirty seconds tolerates long scheduler pauses
// on development hosts; it is only a last-resort resource stop, not a native
// request/admission deadline and never authority to accept incomplete proof.
const SAFETY_NET: Duration = Duration::from_secs(30);

pub(crate) struct Budget {
    start: Instant,
    work: usize,
}
impl Budget {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            work: 0,
        }
    }
    pub fn step(&mut self) -> Result<()> {
        #[cfg(any(test, feature = "test-support"))]
        testing::pause();
        self.work = self.work.saturating_add(1);
        let (elapsed, work) = limits(self.start.elapsed());
        if self.work > work || elapsed >= SAFETY_NET {
            return Err(Error::busy(
                "The task history proof is busy; the saved copy is kept. Please try again.",
            ));
        }
        Ok(())
    }
}

fn limits(elapsed: Duration) -> (Duration, usize) {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(value) = testing::OVERRIDE.with(|v| v.get()) {
        return (value.elapsed, value.work);
    }
    (elapsed, WORK)
}

#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    use super::*;
    #[derive(Clone, Copy)]
    pub(super) struct Limits {
        pub(super) elapsed: Duration,
        pub(super) work: usize,
    }
    thread_local! { pub(super) static OVERRIDE: std::cell::Cell<Option<Limits>> = const { std::cell::Cell::new(None) }; }
    #[derive(Clone, Copy)]
    struct Pause {
        remaining: usize,
        delay: Option<Duration>,
        steps: usize,
    }
    thread_local! { static PAUSE: std::cell::Cell<Option<Pause>> = const { std::cell::Cell::new(None) }; }
    pub(super) fn pause() {
        let Some(mut pause) = PAUSE.with(|v| v.get()) else {
            return;
        };
        pause.steps += 1;
        let delay = if pause.remaining == 0 {
            pause.delay.take()
        } else {
            pause.remaining -= 1;
            None
        };
        PAUSE.with(|v| v.set(Some(pause)));
        if let Some(delay) = delay {
            std::thread::sleep(delay);
        }
    }
    /// Pause once after `skip` proof steps, without changing evidence or results.
    /// Returns the closure result and number of visited steps, so a control can
    /// target a later real scan. Scoped and thread-local, including on unwind.
    pub fn with_proof_pause_for_test<T>(
        skip: usize,
        delay: Duration,
        run: impl FnOnce() -> T,
    ) -> (T, usize) {
        struct Restore(Option<Pause>);
        impl Drop for Restore {
            fn drop(&mut self) {
                PAUSE.with(|v| v.set(self.0));
            }
        }
        let _restore = Restore(PAUSE.with(|v| {
            v.replace(Some(Pause {
                remaining: skip,
                delay: Some(delay),
                steps: 0,
            }))
        }));
        let result = run();
        (result, PAUSE.with(|v| v.get().expect("scoped pause").steps))
    }
    /// Scoped, thread-local test accounting. No environment/configuration knob.
    pub fn with_proof_budget_for_test<T>(
        elapsed: Duration,
        work: usize,
        run: impl FnOnce() -> T,
    ) -> T {
        struct Restore(Option<Limits>);
        impl Drop for Restore {
            fn drop(&mut self) {
                OVERRIDE.with(|v| v.set(self.0));
            }
        }
        let _restore = Restore(OVERRIDE.with(|v| v.replace(Some(Limits { elapsed, work }))));
        run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_original_work_cap_is_still_enforced() {
        testing::with_proof_budget_for_test(Duration::ZERO, 131_072, || {
            let mut budget = Budget::new();
            for _ in 0..131_072 {
                budget.step().unwrap();
            }
            assert_eq!(budget.step().unwrap_err().kind(), crate::ErrorKind::Busy);
        });
    }
    #[test]
    fn stored_block_exhaustion_is_not_a_false_corruption_result() {
        use crate::{Actor, NewTask, ProjectStore, Stage, Target, TaskDoc};
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        let mut task = TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-a1b2c3",
            text: "Original",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        store.save(&mut task).unwrap();
        let before = task.snapshot().unwrap();
        let error = testing::with_proof_budget_for_test(Duration::from_secs(31), 131_072, || {
            task.import_block(&before)
        })
        .unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::Busy);
        assert_eq!(task.snapshot().unwrap(), before);
    }
}
