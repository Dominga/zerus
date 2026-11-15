//! A person's answer may wait for a worker batch, within one request budget.
//! Inspection and ordinary cache writers retain their own short busy timeouts.
use super::*;
use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc,
};

const LIMIT: Duration = Duration::from_millis(750);
thread_local! {
    static UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
    static QUESTIONS: RefCell<Option<(PathBuf, f64)>> = const { RefCell::new(None) };
}

#[must_use]
pub(crate) struct Wait {
    previous: Option<Instant>,
    previous_questions: Option<(PathBuf, f64)>,
    _priority: Option<Guard>,
    // The SQLite callback and restoration both belong to this thread.
    _thread: PhantomData<Rc<()>>,
}
impl Wait {
    pub(crate) fn new() -> Self {
        Self::until(Instant::now() + LIMIT)
    }
    #[cfg(test)]
    pub(crate) fn for_answer(state: &Path) -> Result<Self> {
        let mut wait = Self::new();
        wait.prioritize(state)?;
        Ok(wait)
    }
    pub(crate) fn prioritize(&mut self, state: &Path) -> Result<()> {
        // Keep the worker handed off through all nested receipt checks and
        // canonical publication, not only one discovery walk. A cached exact
        // receipt remains readable before acquiring this optimization's lock.
        self._priority = migration::priority(state, false)?;
        Ok(())
    }
    fn until(deadline: Instant) -> Self {
        let previous = UNTIL.with(|until| {
            let previous = until.get();
            until.set(Some(previous.map_or(deadline, |old| old.min(deadline))));
            previous
        });
        Self {
            previous,
            previous_questions: if previous.is_none() {
                QUESTIONS.with(|value| value.borrow_mut().take())
            } else {
                None
            },
            _priority: None,
            _thread: PhantomData,
        }
    }
}
impl Drop for Wait {
    fn drop(&mut self) {
        UNTIL.with(|until| until.set(self.previous));
        if self.previous.is_none() {
            QUESTIONS.with(|value| *value.borrow_mut() = self.previous_questions.take());
        }
    }
}

// One root per native answer, cleared when its outer admission scope ends.
// This only permits metadata reconciliation after this same request already
// parsed the family. It never skips discovery, original stamps or coverage.
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
    if UNTIL.with(Cell::get).is_some() && question_reconciled(state).is_none() {
        QUESTIONS.with(|value| *value.borrow_mut() = Some((state.to_owned(), at)));
    }
}

pub(super) fn deadline() -> Option<Instant> {
    UNTIL.with(Cell::get)
}

pub(super) fn install(db: &Connection) -> Result<()> {
    if UNTIL.with(Cell::get).is_some() {
        sql(db.busy_handler(Some(busy)))?;
    }
    Ok(())
}
fn busy(_attempt: i32) -> bool {
    let Some(deadline) = UNTIL.with(Cell::get) else {
        return false;
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return false;
    }
    std::thread::sleep(remaining.min(Duration::from_millis(10)));
    Instant::now() < deadline
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_has_one_fixed_750ms_deadline() {
        assert!(UNTIL.with(Cell::get).is_none());
        let before = Instant::now();
        let wait = Wait::new();
        let after = Instant::now();
        let deadline = UNTIL.with(Cell::get).unwrap();
        assert!(deadline >= before + Duration::from_millis(750));
        assert!(deadline <= after + Duration::from_millis(750));
        drop(wait);
        assert!(UNTIL.with(Cell::get).is_none());
        assert!(!busy(0));
    }

    #[test]
    fn nested_checks_cannot_extend_an_expired_deadline() {
        let expired = Instant::now() - Duration::from_secs(1);
        let outer = Wait::until(expired);
        {
            let _nested = Wait::new();
            assert_eq!(UNTIL.with(Cell::get), Some(expired));
            assert!(!busy(0));
            assert!(!busy(100));
        }
        assert_eq!(UNTIL.with(Cell::get), Some(expired));
        drop(outer);
        assert!(UNTIL.with(Cell::get).is_none());
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
