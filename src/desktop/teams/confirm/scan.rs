//! One bounded roster page on the existing service worker, with progress kept
//! across passes. Large rosters cannot starve later members or later teams.
use super::{engine, native::Native, ports::Result};
use crate::{
    config::Config,
    teams::store::{Cursor, Store},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[derive(Default)]
pub(in crate::desktop::teams) struct Scan {
    teams: Option<Cursor>,
    team: Option<String>,
    members: Option<Cursor>,
}
pub(in crate::desktop::teams) fn step(
    config: &Config,
    epoch: &str,
    machine: &str,
    store: &mut Store,
    stop: &Arc<AtomicBool>,
    cursor: &mut Scan,
) -> Result<()> {
    if stop.load(Ordering::Acquire) || !crate::teams::enabled(&config.state) {
        return Ok(());
    }
    store.refresh().map_err(|e| e.to_string())?;
    if cursor.team.is_none() {
        let page = store
            .live_teams(1, cursor.teams.as_ref())
            .map_err(|e| e.to_string())?;
        cursor.teams = page.next;
        cursor.team = page.rows.first().map(|team| team.id.clone());
    }
    let Some(team) = &cursor.team else {
        return Ok(());
    };
    let page = store
        .members(team, 16, cursor.members.as_ref())
        .map_err(|e| e.to_string())?;
    let mut native = Native::new(config, machine, stop, store)?;
    let mut fault = None;
    for member in page.rows {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if member.session.is_none() || member.agent == "dsh" {
            continue;
        }
        let Ok((binding, _)) = native.binding(team, &member.id) else {
            continue;
        };
        native.reset();
        if let Err(error) = engine::step(
            &mut native,
            &binding,
            epoch,
            chrono::Utc::now().timestamp_millis(),
        ) {
            fault = Some(error);
        }
    }
    cursor.members = page.next;
    if cursor.members.is_none() {
        cursor.team = None;
    }
    fault.map_or(Ok(()), Err)
}
