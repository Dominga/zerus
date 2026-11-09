//! Inactive executor-only task-control bridge. Same UID is the transport
//! boundary, never Person authority. No service route is enabled by this module.
#![allow(dead_code)] // Runtime wiring remains behind the single const-false gate.
#[cfg(any(target_os = "macos", test))]
mod args_macos;
mod authority;
mod client;
mod stop_import;
#[cfg(target_os = "linux")]
mod peer_linux;
#[cfg(target_os = "macos")]
mod peer_macos;
#[cfg(windows)]
mod peer_windows;
mod wire;
