//! Inactive native task-control support. Publication under the native writer
//! never opens the desktop database; import belongs after that writer drops.
#![allow(dead_code)] // Removed as the runtime entry points are connected.
pub(crate) mod observation;
mod stop_facts;
pub(in crate::state) mod writer;
