//! Probe/watch helper custody, shared with the isolated test supervisor.
#[path = "../helper_group.rs"]
mod group;
pub(crate) use group::{prepare, terminate, try_wait};

#[cfg(all(test, unix))]
mod tests;
