//! An answer has one bounded waiting budget shared by nested receipt checks.
//! CPU work does not spend that budget; only actual lock waiting does.
use super::*;
use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc,
};

const LIMIT: Duration = Duration::from_millis(750);
thread_local! {
    static BUDGET: Cell<Option<Duration>> = const { Cell::new(None) };
    static HARD_UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
    static PRIORITIZED: Cell<usize> = const { Cell::new(0) };
    static QUESTIONS: RefCell<Option<(PathBuf,f64)>> = const { RefCell::new(None) };
}
#[must_use]
pub(crate) struct Wait {
    outer: bool,
    previous_questions: Option<(PathBuf, f64)>,
    _priority: Option<Guard>,
    _thread: PhantomData<Rc<()>>,
}
impl Wait {
    pub(crate) fn new() -> Self {
        let outer = BUDGET.with(|budget| {
            let outer = budget.get().is_none();
            if outer {
                budget.set(Some(LIMIT));
            }
            outer
        });
        Self {
            outer,
            previous_questions: if outer {
                QUESTIONS.with(|value| value.borrow_mut().take())
            } else {
                None
            },
            _priority: None,
            _thread: PhantomData,
        }
    }
    #[cfg(test)]
    fn until(deadline: Instant) -> Self {
        let wait = Self::new();
        assert!(wait.outer);
        HARD_UNTIL.with(|until| until.set(Some(deadline)));
        wait
    }
    #[cfg(test)]
    pub(crate) fn for_answer(state: &Path) -> Result<Self> {
        let mut wait = Self::new();
        wait.prioritize(state)?;
        Ok(wait)
    }
    pub(crate) fn prioritize(&mut self, state: &Path) -> Result<()> {
        // Exact cached receipts return before this optional-index custody.
        if self._priority.is_none() {
            self._priority = migration::priority(state, false)?;
            if self._priority.is_some() {
                PRIORITIZED.with(|count| count.set(count.get() + 1));
            }
        }
        Ok(())
    }
}
impl Drop for Wait {
    fn drop(&mut self) {
        if self._priority.is_some() {
            PRIORITIZED.with(|count| count.set(count.get() - 1));
        }
        if self.outer {
            BUDGET.with(|budget| budget.set(None));
            HARD_UNTIL.with(|until| until.set(None));
            QUESTIONS.with(|value| *value.borrow_mut() = self.previous_questions.take());
        }
    }
}
pub(super) fn prioritized() -> bool {
    PRIORITIZED.with(|count| count.get() > 0)
}
pub(super) fn remaining() -> Option<Duration> {
    BUDGET.with(Cell::get).map(|left| {
        HARD_UNTIL.with(Cell::get).map_or(left, |until| {
            left.min(until.saturating_duration_since(Instant::now()))
        })
    })
}
pub(super) fn deadline() -> Option<Instant> {
    remaining().map(|left| Instant::now() + left)
}
pub(super) fn pause(maximum: Duration) -> bool {
    let Some(left) = remaining().filter(|left| !left.is_zero()) else {
        return false;
    };
    let started = Instant::now();
    std::thread::sleep(maximum.min(left));
    BUDGET.with(|budget| {
        budget.set(
            budget
                .get()
                .map(|left| left.saturating_sub(started.elapsed())),
        )
    });
    // Permit one immediate retry even when this consumed the last slice.
    // A further busy callback then refuses without another sleep.
    true
}
// This one-root hint only enables metadata reconciliation after this same
// answer parsed the complete family. It ends with the outer request scope.
pub(super) fn question_reconciled(state: &Path) -> Option<f64> {
    QUESTIONS.with(|value| {
        value
            .borrow()
            .as_ref()
            .filter(|(root, _)| root == state)
            .map(|(_, at)| *at)
    })
}
pub(super) fn reconciled_question(state: &Path, at: f64) {
    if BUDGET.with(Cell::get).is_some() && question_reconciled(state).is_none() {
        QUESTIONS.with(|value| *value.borrow_mut() = Some((state.to_owned(), at)));
    }
}
pub(super) fn install(db: &Connection) -> Result<()> {
    if BUDGET.with(Cell::get).is_some() {
        sql(db.busy_handler(Some(busy)))?;
    }
    Ok(())
}
fn busy(_attempt: i32) -> bool {
    pause(Duration::from_millis(10))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_shares_one_750ms_waiting_budget_without_charging_cpu_work() {
        assert_eq!(remaining(), None);
        let answer = Wait::new();
        assert_eq!(remaining(), Some(LIMIT));
        std::thread::sleep(Duration::from_millis(3));
        assert_eq!(
            remaining(),
            Some(LIMIT),
            "ordinary processing is not SQLite waiting"
        );
        assert!(pause(Duration::from_millis(1)));
        let before_nested = remaining();
        let left;
        {
            let _nested = Wait::new();
            assert_eq!(
                remaining(),
                before_nested,
                "nesting must not replenish the budget"
            );
            assert!(pause(Duration::from_millis(1)));
            left = remaining().unwrap();
            assert!(left < LIMIT);
        }
        assert_eq!(
            remaining(),
            Some(left),
            "a nested check must not restore spent waiting time"
        );
        drop(answer);
        assert_eq!(remaining(), None);
        assert!(!busy(0));
    }
    #[test]
    fn nested_checks_cannot_extend_an_expired_budget() {
        let outer = Wait::until(Instant::now() - Duration::from_secs(1));
        {
            let _nested = Wait::new();
            assert_eq!(remaining(), Some(Duration::ZERO));
            assert!(!busy(0));
            assert!(!busy(100));
        }
        assert_eq!(remaining(), Some(Duration::ZERO));
        drop(outer);
        assert_eq!(remaining(), None);
    }
    #[test]
    fn new_connections_use_the_scope_and_ordinary_writers_keep_their_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let timeout = |db: &Connection| {
            db.query_row("PRAGMA busy_timeout", [], |row| row.get::<_, u64>(0))
                .unwrap()
        };
        let ordinary = Catalog::open(temp.path(), true).unwrap();
        assert_eq!(timeout(&ordinary.db), 20);
        {
            let _wait = Wait::until(Instant::now() - Duration::from_secs(1));
            for _ in 0..2 {
                let scoped = Catalog::open(temp.path(), true).unwrap();
                // Installing a callback clears SQLite's per-statement timer.
                assert_eq!(timeout(&scoped.db), 0);
            }
            let writer = Connection::open(temp.path().join(DATABASE)).unwrap();
            writer
                .execute_batch(
                    "BEGIN IMMEDIATE; UPDATE receipt_meta SET value=value WHERE key='epoch'",
                )
                .unwrap();
            let error = Catalog::open(temp.path(), true).err().unwrap();
            assert!(error.contains("SQLite DatabaseBusy"), "{error}");
        }
        let ordinary = Catalog::open(temp.path(), true).unwrap();
        assert_eq!(timeout(&ordinary.db), 20);
        let reader = Catalog::open(temp.path(), false).unwrap();
        assert_eq!(timeout(&reader.db), 20);
        assert!(reader.db.is_readonly(rusqlite::DatabaseName::Main).unwrap());
    }
}

#[cfg(test)]
mod request_tests {
    use super::*;
    #[test]
    fn the_whole_answer_holds_priority_and_its_first_validation_time() {
        let temp = tempfile::tempdir().unwrap();
        assert!(question_reconciled(temp.path()).is_none());
        let answer = Wait::for_answer(temp.path()).unwrap();
        assert!(migration::priority(temp.path(), true).unwrap().is_none());
        let first = now() - 1.0;
        reconciled_question(temp.path(), first);
        {
            let _nested = Wait::new();
            reconciled_question(temp.path(), now());
            assert_eq!(question_reconciled(temp.path()), Some(first));
        }
        assert_eq!(question_reconciled(temp.path()), Some(first));
        drop(answer);
        assert!(question_reconciled(temp.path()).is_none());
        assert!(migration::priority(temp.path(), true).unwrap().is_some());
    }
}
