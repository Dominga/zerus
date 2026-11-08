//! Inactive executor-only task-control bridge. Same UID is the transport
//! boundary, never Person authority. No service route is enabled by this module.
#![allow(dead_code)] // Runtime wiring remains behind the single const-false gate.
#[cfg(target_os = "linux")]
mod peer_linux;
