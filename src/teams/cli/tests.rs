//! The CLI against a temporary state directory, with the preview on and the
//! caller chosen by the test instead of by the environment.
mod board;
mod catalog;
mod create_binding;
mod dispatch;
mod drawer_pages;
mod editing;
mod fixtures;
mod guides;
mod harness;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod hosted;
mod member_sessions;
mod progress;
mod progress157;
mod reads;
mod session_groups;
mod status_items;
mod storage_failure;
mod teams_followup;
mod teams_screen;
mod verdict;
mod writes;
