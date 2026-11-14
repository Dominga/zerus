//! A roster edit reads exact original receipts. It never opens a writer or
//! constructs permission to finish an old launch or delivery.
use super::{
    ledger::{Row, State, FILE},
    ports::Result,
};
use crate::teams::store::storage;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::path::Path;
use zerus_team_plan::{
    model::{Member, Team},
    problem::code,
    Problem,
};

fn refused(text: &str) -> Problem {
    Problem::new(code::INVALID_STATE, text)
}
fn read(path: &Path) -> Result<Option<Connection>> {
    match crate::platform::verify_private(path, crate::platform::Private::File) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(true) => (),
        Ok(false) => return Err(refused("The member receipt file is not private.")),
        Err(e) => return Err(storage(e)),
    }
    let parent = path
        .parent()
        .ok_or_else(|| refused("The member receipt directory is missing."))?;
    if !crate::platform::verify_private(parent, crate::platform::Private::Directory)
        .map_err(storage)?
    {
        return Err(refused("The member receipt directory is not private."));
    }
    #[cfg(unix)]
    let identity = crate::sqlite_file::private(path).map_err(storage)?;
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(storage)?;
    #[cfg(unix)]
    crate::sqlite_file::bind_private(&db, path, identity).map_err(storage)?;
    db.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(storage)?;
    Ok(Some(db))
}
/// A known completed launch, with an exact binding and unchanged checkout.
/// The same predicate gates retirement by the launch worker after the edit.
pub(super) fn known(row: &Row, team: &Team, member: &Member) -> Result<()> {
    let p = row
        .plan
        .as_ref()
        .ok_or_else(|| refused("The member has no saved launch plan."))?;
    let tree = row
        .worktree
        .as_ref()
        .ok_or_else(|| refused("The member has no saved checkout."))?;
    let observed = row
        .observed
        .as_ref()
        .ok_or_else(|| refused("The member has no confirmed launch receipt."))?;
    if !(row.state == State::Done || (row.state == State::Bound && p.parent.is_none()))
        || row.problem.is_some()
        || row.session_request.is_some()
        || row.close_request.is_some()
        || row
            .parent_result
            .as_ref()
            .is_some_and(|r| r.contains("unknown"))
        || row.team != team.id
        || row.member != member.id
        || p.team != team.id
        || p.member != member.id
        || p.project != team.project
        || p.machine != team.home
        || p.person != team.owner
        || p.role != member.role
        || p.agent != member.agent
        || member.session.as_ref().is_none_or(|s| {
            s.machine != p.machine || s.session_id != format!("launch_id:{}", p.launch_id)
        })
        || observed.launch_id != p.launch_id
        || observed.machine != p.machine
        || observed.agent != p.agent
        || observed.account != p.account
        || observed.worktree != *tree
        || tree.path != member.worktree
        || tree.path != p.placement.destination
        || tree.ownership_nonce != member.ownership_nonce
    {
        return Err(refused(
            "The member has an unsettled or unknown launch receipt; swap is refused.",
        ));
    }
    Ok(())
}
pub(crate) fn check(state: &Path, team: &Team, member: &Member) -> Result<()> {
    let state = crate::platform::canonical(state).map_err(storage)?;
    let db = read(&state.join("teams").join(FILE))?
        .ok_or_else(|| refused("The member has no saved launch receipt."))?;
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(storage)?;
    if version != super::ledger::SCHEMA {
        return Err(refused(
            "The member launch receipt needs a compatible Teams service.",
        ));
    }
    let raw:Option<String>=db.query_row("SELECT CAST(substr(CAST(data AS BLOB),1,262145) AS TEXT) FROM originals WHERE team=?1 AND member=?2",rusqlite::params![team.id,member.id],|r|r.get(0)).optional().map_err(storage)?;
    let raw = raw.ok_or_else(|| refused("The member has no saved launch receipt."))?;
    if raw.len() > 262144 {
        return Err(refused("The member launch receipt exceeds its bound."));
    }
    let row: Row = serde_json::from_str(&raw).map_err(storage)?;
    known(&row, team, member)?;
    let Some(db) = read(&state.join("teams/standing-v1/journal.sqlite3"))? else {
        return Ok(());
    };
    let version: u32 = db
        .query_row("SELECT version FROM standing_meta WHERE id=1", [], |r| {
            r.get(0)
        })
        .map_err(storage)?;
    if version != 1 {
        return Err(refused(
            "The member delivery receipt needs a compatible Teams service.",
        ));
    }
    let indexed:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='standing_unresolved')",[],|r|r.get(0)).map_err(storage)?;
    if !indexed {
        return Err(refused(
            "Let the updated Teams service read this member's receipts before swapping.",
        ));
    }
    let launch = &row.plan.as_ref().expect("known launch").launch_id;
    let unknown:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM standing_effects INDEXED BY standing_unresolved WHERE team=?1 AND member=?2 AND launch=?3 AND (pending=1 OR json_extract(body,'$.state')='unknown') LIMIT 1)",rusqlite::params![team.id,member.id,launch],|r|r.get(0)).map_err(storage)?;
    if unknown {
        return Err(refused(
            "The member has an unsettled or unknown delivery receipt; swap is refused.",
        ));
    }
    Ok(())
}
