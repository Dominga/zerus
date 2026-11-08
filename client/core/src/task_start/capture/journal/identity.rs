//! Native-only Check identity proposals. These are data, never execution permits.
use super::*;

/// A fresh native ID, optionally bound to the retained original terminal journal.
/// No string/renderer deserialization can select a previously cancelled ID.
///
/// ```compile_fail
/// use zerus_client_core::task_start::capture::journal::CheckId;
/// let _: CheckId = "00000009-1111-4111-8111-111111111111".into();
/// ```
///
/// ```compile_fail
/// use zerus_client_core::task_start::capture::journal::CheckId;
/// let _: CheckId = serde_json::from_str("{}").unwrap();
/// ```
pub struct CheckId {
    id: String,
    before: Option<Before>,
}
struct Before {
    file: File,
    version: FileVersion,
    sha256: String,
}
impl CheckId {
    /// Only a new empty slot may adopt this proposal. Reopening an existing
    /// Prepared/Offered original observes its saved ID instead.
    pub fn fresh() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            before: None,
        }
    }
    /// Data for the existing complete-envelope preflight, not an admission.
    pub fn as_str(&self) -> &str {
        &self.id
    }
    pub(super) fn check_before(&self, disk: Option<&Disk>) -> Result<(), Error> {
        match (&self.before, disk) {
            (None, None) => Ok(()),
            (Some(before), Some(disk))
                if file_version(&before.file, BYTES).ok().as_ref() == Some(&before.version)
                    && disk.version == before.version
                    && sha256(&disk.bytes) == before.sha256 =>
            {
                Ok(())
            }
            _ => Err(Error::Unavailable),
        }
    }
    #[cfg(test)]
    pub(super) fn fixture_initial(id: &str) -> Self {
        Self {
            id: id.into(),
            before: None,
        }
    }
}
impl Saved {
    /// Read-only proposal for a separately explicit Check after cancellation or
    /// a completed old delivery. The SAME file object is held, not reopened by
    /// native object number. Actual adoption rechecks it under the metadata flock.
    pub fn next_check_id(&self) -> Result<CheckId, Error> {
        if !matches!(self.phase(), Phase::CancelledBeforeOffer | Phase::Completed)
            || !self.current()
        {
            return Err(Error::Unavailable);
        }
        let file = self.disk.file.try_clone().map_err(|_| Error::Unavailable)?;
        if file_version(&file, BYTES).ok().as_ref() != Some(&self.disk.version) || !self.current() {
            return Err(Error::Unavailable);
        }
        Ok(CheckId {
            id: uuid::Uuid::new_v4().to_string(),
            before: Some(Before {
                file,
                version: self.disk.version.clone(),
                sha256: sha256(&self.disk.bytes),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_start::capture::tests::{retry_busy, Fixture};
    use std::fs;

    fn saved(f: &Fixture) -> Saved {
        Saved::read(
            &f.store,
            &f.intent.binding.key.project,
            &f.intent.binding.key.task,
        )
        .unwrap()
        .unwrap()
    }
    // Every lock after a fixture's first may meet a parallel fork's inherited
    // description (zerus-p1l5z): those steps wait for Busy, the no-write
    // refusal, and keep every other result exactly.
    fn cancelled(f: &Fixture) -> Saved {
        drop(
            Journal::prepare(
                f.capture().unwrap(),
                &CheckId::fixture_initial(&f.intent.reconcile_id),
            )
            .unwrap(),
        );
        retry_busy(|| saved(f).discard_prepared(&f.intent.reconcile_id)).unwrap();
        saved(f)
    }
    #[test]
    fn stale_proposal_cannot_adopt_after_a_later_discard_cycle() {
        let f = Fixture::new();
        let original = cancelled(&f);
        let stale = original.next_check_id().unwrap();
        let next = original.next_check_id().unwrap();
        drop(
            retry_busy(|| {
                Journal::prepare_after_discard(f.capture().unwrap(), &next, original.reconcile_id())
            })
            .unwrap(),
        );
        retry_busy(|| saved(&f).discard_prepared(next.as_str())).unwrap();
        let before = fs::read(f.file.with_extension("reconcile")).unwrap();
        assert_eq!(
            retry_busy(|| Journal::prepare_after_discard(
                f.capture().unwrap(),
                &stale,
                next.as_str()
            ))
            .err(),
            Some(Error::Unavailable)
        );
        assert_eq!(
            fs::read(f.file.with_extension("reconcile")).unwrap(),
            before
        );
        let fresh = saved(&f).next_check_id().unwrap();
        let current = retry_busy(|| {
            Journal::prepare_after_discard(f.capture().unwrap(), &fresh, next.as_str())
        })
        .unwrap();
        assert_eq!(current.saved().reconcile_id(), fresh.as_str());
    }
    /// The reported step (zerus-p1l5z): while a parallel fork's child still
    /// holds an inherited description of the metadata lock, a later discard
    /// cycle's prepare refuses Busy before any write; the shared retry waits
    /// for that description and then prepares the exact next ID.
    #[test]
    fn a_later_discard_cycle_waits_for_an_inherited_lock_description() {
        let f = Fixture::new();
        let original = cancelled(&f);
        let next = original.next_check_id().unwrap();
        // Like fork before exec, this clone shares the lock's open file description.
        let lock = retry_busy(|| original.slot.lock(false)).unwrap();
        let inherited = lock.try_clone().unwrap();
        drop(lock);
        let path = f.file.with_extension("reconcile");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            Journal::prepare_after_discard(f.capture().unwrap(), &next, original.reconcile_id())
                .err(),
            Some(Error::Busy)
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(saved(&f).phase(), Phase::CancelledBeforeOffer);
        let (busy, observed) = std::sync::mpsc::sync_channel(1);
        std::thread::scope(|threads| {
            let waiter = threads.spawn(|| {
                retry_busy(|| {
                    let result = Journal::prepare_after_discard(
                        f.capture().unwrap(),
                        &next,
                        original.reconcile_id(),
                    );
                    if matches!(result, Err(Error::Busy)) {
                        let _ = busy.try_send(());
                    }
                    result.map(|journal| journal.saved().reconcile_id().to_owned())
                })
            });
            observed
                .recv_timeout(std::time::Duration::from_secs(8))
                .unwrap();
            assert_eq!(fs::read(&path).unwrap(), bytes);
            drop(inherited);
            assert_eq!(waiter.join().unwrap().unwrap(), next.as_str());
        });
    }
    #[test]
    fn original_file_and_version_are_retained_across_identical_replacement_or_edit() {
        for replace in [true, false] {
            let f = Fixture::new();
            let original = cancelled(&f);
            let proposal = original.next_check_id().unwrap();
            let held = &proposal.before.as_ref().unwrap().file;
            // SAFETY: observes only this retained descriptor's close-on-exec flag.
            let flags = unsafe { libc::fcntl(held.as_raw_fd(), libc::F_GETFD) };
            assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
            let path = f.file.with_extension("reconcile");
            let bytes = fs::read(&path).unwrap();
            if replace {
                fs::rename(&path, f.base.join("kept-old-journal")).unwrap();
                crate::fsx::write_private_durable(&path, &bytes).unwrap();
            } else {
                fs::write(&path, &bytes).unwrap();
                held.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
                    .unwrap();
            }
            assert_eq!(
                retry_busy(|| Journal::prepare_after_discard(
                    f.capture().unwrap(),
                    &proposal,
                    original.reconcile_id()
                ))
                .err(),
                Some(Error::Unavailable)
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn a_bound_proposal_cannot_seed_another_empty_slot() {
        let f = Fixture::new();
        let proposal = cancelled(&f).next_check_id().unwrap();
        let other = Fixture::new();
        assert_eq!(
            Journal::prepare(other.capture().unwrap(), &proposal).err(),
            Some(Error::Unavailable)
        );
        assert!(!other.file.with_extension("reconcile").exists());
        assert!(!other.file.with_extension("reconcile-lock").exists());
    }
    #[test]
    fn prepared_and_offered_originals_cannot_issue_replacement_proposals() {
        let f = Fixture::new();
        let mut original = Journal::prepare(f.capture().unwrap(), &CheckId::fresh()).unwrap();
        assert_eq!(
            original.saved().next_check_id().err(),
            Some(Error::Unavailable)
        );
        let id = original.saved().reconcile_id().to_owned();
        original.offer().unwrap();
        assert_eq!(
            original.saved().next_check_id().err(),
            Some(Error::Unavailable)
        );
        assert_eq!(original.saved().reconcile_id(), id);
    }
}
