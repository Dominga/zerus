//! The sealed negatives of the deferred-input kinds (zerus-tlb9q).
//!
//! An admission refused before its ledger admission, with the kind's ledger
//! proving no row for its request, settles as never started: the kind
//! answers `<kind>_not_admitted`. Absence alone is not a fact about the
//! request ID: two live native registrations can carry the same request, so
//! a same-ID copy queued behind the refusal could otherwise admit later.
//! Each refused ID is therefore sealed here before the reply, and a sealed
//! ID is never admitted again in this service epoch.
//!
//! The fences belong to the service epoch, not to a kind's runtime. The
//! service mints its epoch once, but it can drop the background runtime and
//! start it again under the same epoch and scope; the fences are created
//! with the epoch and passed through every such start. A new epoch refuses
//! the old scope, so nothing is kept across epochs. A fence never evicts,
//! resets or downgrades a sealed ID. When it is full, no new negative is
//! minted and the refusal stays the kind's conservative unavailable one.
use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

/// Request IDs one kind seals per epoch at most.
pub(super) const BOUND: usize = 1024;

/// One kind's sealed request IDs.
#[derive(Default)]
pub(super) struct Fence {
    ids: Mutex<BTreeSet<String>>,
}

impl Fence {
    /// Whether `id` was answered not admitted in this epoch.
    pub(super) fn sealed(&self, id: &str) -> bool {
        self.ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(id)
    }

    /// Seals `id` for the rest of the epoch: true when it is sealed now,
    /// false when the fence is full and `id` was not sealed already.
    pub(super) fn seal(&self, id: &str) -> bool {
        let mut ids = self.ids.lock().unwrap_or_else(PoisonError::into_inner);
        if ids.contains(id) {
            return true;
        }
        if ids.len() >= BOUND {
            return false;
        }
        ids.insert(id.to_owned());
        true
    }
}

/// The fences of one service epoch, one per kind that answers not admitted.
#[derive(Default)]
pub(crate) struct Fences {
    pub(super) deferred_send: Fence,
    pub(super) session_resume: Fence,
    pub(super) deferred_answer: Fence,
    pub(super) compact_to_send: Fence,
}

/// Whether the kind's ledger provably holds no row for the request: a strict
/// read in a fresh BEGIN IMMEDIATE on the service's own held store. Opening
/// the transaction, a busy, corrupt or unreadable store and any read error
/// prove nothing. The read-only transaction is rolled back.
pub(super) fn no_row<T>(
    db: &mut rusqlite::Connection,
    load: impl FnOnce(&rusqlite::Transaction<'_>) -> anyhow::Result<Option<T>>,
) -> bool {
    let Ok(tx) = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) else {
        return false;
    };
    matches!(load(&tx), Ok(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_id_stays_sealed_and_a_full_fence_mints_nothing_new() {
        let fence = Fence::default();
        assert!(!fence.sealed("a"));
        assert!(fence.seal("a"));
        assert!(fence.sealed("a"));
        for n in 1..BOUND {
            assert!(fence.seal(&format!("id-{n}")));
        }
        // Full: a new ID is refused, every sealed one stays sealed.
        assert!(!fence.seal("late"));
        assert!(!fence.sealed("late"));
        assert!(fence.seal("a"), "an already sealed ID is still sealed");
        assert!(
            fence.sealed("a") && fence.sealed("id-1") && fence.sealed(&format!("id-{}", BOUND - 1))
        );
    }

    #[test]
    fn only_a_successful_empty_read_proves_no_row() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        assert!(no_row(&mut db, |_| Ok(None::<()>)));
        assert!(!no_row(&mut db, |_| Ok(Some(()))));
        assert!(!no_row(&mut db, |_| -> anyhow::Result<Option<()>> {
            anyhow::bail!("unreadable")
        }));
        // A store another writer holds cannot open the transaction.
        let path = tempfile::NamedTempFile::new().unwrap();
        let mut first = rusqlite::Connection::open(path.path()).unwrap();
        let _held = first
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let mut second = rusqlite::Connection::open(path.path()).unwrap();
        second
            .busy_timeout(std::time::Duration::from_millis(1))
            .unwrap();
        assert!(!no_row(&mut second, |_| Ok(None::<()>)));
    }
}
