//! Data custody between reads; one typed current scope spans each logical use.
use super::stop_facts::Fact;
use std::cell::Cell;
use zerus_desktop_ledger::task_controls::Executor;

type Result<T> = std::result::Result<T, String>;

/// No Clone, deserialization, public constructor or unleased facts getter.
/// Native workers return this only after their Writer has physically retired.
pub(crate) struct Capture {
    #[cfg(not(windows))]
    legacy: super::stop_facts::Captured,
    #[cfg(windows)]
    owned: super::stop_facts::SelectedCapture,
    failed: Cell<bool>,
}

impl Capture {
    #[cfg(test)]
    pub(crate) fn fixture(
        root: &std::path::Path,
        fact: &Fact,
        until: std::time::Instant,
    ) -> Result<(Self, std::path::PathBuf)> {
        super::stop_facts::fixture_capture(root, fact, until)
    }

    #[cfg(not(windows))]
    pub(super) fn legacy(legacy: super::stop_facts::Captured) -> Self {
        Self {
            legacy,
            failed: Cell::new(false),
        }
    }

    #[cfg(windows)]
    pub(super) fn owned(owned: super::stop_facts::SelectedCapture) -> Self {
        Self {
            owned,
            failed: Cell::new(false),
        }
    }

    /// The callback and every final check share the SAME physical lease.
    /// Its borrowed arguments cannot escape through T. Callers must retain
    /// this complete scope through their SQL COMMIT, not just its precheck.
    ///
    /// A post-callback refusal can occur AFTER COMMIT. It never proves that
    /// a durable write rolled back: callers retain Unknown/lookup-only state.
    pub(crate) fn with_current<T>(
        &self,
        expected: &Executor,
        read: impl FnOnce(&Executor, &[Fact], &dyn Fn() -> Result<()>) -> Result<T>,
    ) -> Result<T> {
        if self.failed.get() {
            return Err("The original Stop capture already refused its scope".into());
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            #[cfg(not(windows))]
            {
                if self.legacy.executor() != expected {
                    return Err("The captured Stop executor changed".into());
                }
                self.legacy.verify()?;
                let check = || self.legacy.verify();
                let value = read(self.legacy.executor(), self.legacy.facts(), &check);
                self.legacy.verify()?;
                value
            }
            #[cfg(windows)]
            {
                let lease = self.owned.lease(expected).map_err(str::to_owned)?;
                lease
                    .read_scoped(|executor, facts| {
                        let check = || lease.final_check().map_err(str::to_owned);
                        read(executor, facts, &check)
                    })
                    .map_err(str::to_owned)?
            }
        }))
        .unwrap_or_else(|_| Err("The original Stop read scope panicked".into()));
        if result.is_err() {
            self.failed.set(true);
        }
        result
    }
}

#[cfg(test)]
#[path = "capture/windows_lease.rs"]
mod current_scope;
