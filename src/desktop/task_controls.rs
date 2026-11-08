//! Inactive executor-only task-control bridge. Same UID is the transport
//! boundary, never Person authority. The runtime route is const-gated off.
#![allow(dead_code)] // Runtime wiring remains behind the single const-false gate.
#[cfg(any(target_os = "macos", test))]
mod args_macos;
mod authority;
mod client;
mod guard;
#[cfg(target_os = "linux")]
mod peer_linux;
#[cfg(target_os = "macos")]
mod peer_macos;
#[cfg(windows)]
mod peer_windows;
mod runtime;
mod server;
mod stop_import;
mod wire;
mod worker;

pub(super) use runtime::{Custody, Runtime};
pub(super) use server::{client, Incoming};
pub(super) use server::{line as read_until, write as write_until};
pub(super) use wire::OP;

pub(crate) fn request(
    config: &crate::config::Config,
    command: crate::tasks::controls::wire::Command,
) -> anyhow::Result<serde_json::Value> {
    client::request(config, command)
}

pub(super) fn enabled() -> bool {
    zerus_desktop_ledger::task_controls::TASK_CONTROLS
}
pub(super) fn server_total() -> std::time::Duration {
    server::TOTAL
}
pub(super) fn decorate(value: &mut serde_json::Value) {
    if enabled() {
        value["task_controls"] = serde_json::json!({"schema":1,"capability":"task-control-v1"});
    }
}
