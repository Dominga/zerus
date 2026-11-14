//! Member creation is owned by the desktop service. All session effects use
//! the shared original-child runner and maintained CLI; stored plans are data,
//! never authority to continue an original after a restart.
#![cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod child;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use child::adopt_custody;
mod actor;
pub(crate) use actor::presence;
pub(crate) use actor::Ledger as ActorLedger;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(crate) use timers::native_doorbell_for_test;

/// A command's own reason as a card or a team record shows it: control
/// characters as spaces, whitespace collapsed, at most 512 characters
/// (zerus-63rru.57, .62). Empty when the command gave none.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(in crate::desktop::teams) fn reason_text(raw: &str) -> String {
    let spaced: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(512)
        .collect()
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod confirm;
mod health;
mod items;
#[cfg(test)]
pub(crate) use items::Item as ServiceItem;
pub(crate) use items::Items as ServiceItems;
/// The service items `hgs team status` shows for a team, read without
/// creating the actor ledger; None from a service that does not write them.
pub(crate) fn service_items(
    state: &std::path::Path,
    team: &str,
    running: Option<bool>,
    now: i64,
) -> Option<ServiceItems> {
    items::read(state, team, running, now)
}
/// The actor ledger of a state directory, as the service creates it, and a
/// connection for a test's own service items.
#[cfg(test)]
pub(crate) fn test_actor_ledger(state: &std::path::Path) -> rusqlite::Connection {
    actor::test_ledger(state)
}
/// The actor's real failing passes, for other components' tests.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(crate) use actor::failing_service_passes;
#[cfg(test)]
pub(crate) use items::{put as put_service_item, Failure as ServiceFailure};
mod kept;
mod launch;
mod ledger;
pub(crate) use health::Health;
/// Last launch-worker fault, read without creating a launch ledger.
pub(crate) fn health(state: &std::path::Path) -> Option<Health> {
    health::read(state)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native;
mod ports;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod timers;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod worker;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(super) use worker::entry_hold::Hold as WorkerEntryHold;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use worker::Worker;
#[cfg(test)]
mod tests;
