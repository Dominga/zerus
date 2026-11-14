//! A person's answer may wait for a worker batch, within one request budget.
//! Inspection and ordinary cache writers retain their own short busy timeouts.
use super::*;
use std::{cell::Cell, marker::PhantomData, rc::Rc};

const LIMIT: Duration = Duration::from_millis(750);
thread_local! {
    static UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
}

#[must_use]
pub(crate) struct Wait {
    previous: Option<Instant>,
    // The SQLite callback and restoration both belong to this thread.
    _thread: PhantomData<Rc<()>>,
}
impl Wait {
    pub(crate) fn new() -> Self {
        Self::until(Instant::now() + LIMIT)
    }
    fn until(deadline: Instant) -> Self {
        let previous = UNTIL.with(|until| {
            let previous = until.get();
            until.set(Some(previous.map_or(deadline, |old| old.min(deadline))));
            previous
        });
        Self {
            previous,
            _thread: PhantomData,
        }
    }
}
impl Drop for Wait {
    fn drop(&mut self) {
        UNTIL.with(|until| until.set(self.previous));
    }
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
