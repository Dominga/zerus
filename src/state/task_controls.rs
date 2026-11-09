//! Inactive native task-control support. Publication under the native writer
//! never opens the desktop database; import belongs after that writer drops.
#![allow(dead_code)] // Removed as the runtime entry points are connected.
pub(crate) mod observation;
mod stop_facts;
pub(crate) use stop_facts::{Captured, Fact, Source};
pub(in crate::state) mod writer;

/// Existing-run capture only, under the caller's original native exclusion.
/// Missing history refuses; inspection never creates an empty outbox.
pub(crate) fn capture_stops(
    root: &std::path::Path,
    writer: &super::storage::Guard,
    executor: &zerus_desktop_ledger::task_controls::Executor,
) -> Result<Option<Captured>, String> {
    stop_facts::open(root, writer, executor, false)?
        .map(stop_facts::Publisher::capture)
        .transpose()
}
