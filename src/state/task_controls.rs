//! Inactive native task-control support. Publication under the native writer
//! never opens the desktop database; import belongs after that writer drops.
#![allow(dead_code)] // Removed as the runtime entry points are connected.
mod stop_facts;
