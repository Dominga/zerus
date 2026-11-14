//! The CLI against a temporary state directory, with the preview on and the
//! caller chosen by the test instead of by the environment.
mod board;
mod catalog;
mod create_binding;
mod dispatch;
mod fixtures;
mod guides;
mod harness;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod hosted;
mod member_sessions;
mod reads;
mod session_groups;
mod status_items;
mod storage_failure;
mod verdict;
mod writes;
