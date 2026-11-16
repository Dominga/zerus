//! The compound native original reuses ordinary intake validation and task
//! edits. These helpers operate on caller-held data; they issue no permits.
use super::*;
use zerus_replica::{Target, TaskTeam};

pub(crate) struct Previous {
    team: Option<TaskTeam>,
    name: String,
    target: Target,
}

pub(crate) fn check(store: &Store, state: &Path, machine: &str, request: &Request) -> Result<()> {
    let Operation::AddTask {
        task,
        role,
        pipeline,
    } = &request.operation
    else {
        return Err(crate::desktop::teams::person_actions::required());
    };
    let caller = Caller {
        state,
        machine,
        actor: TaskActor::Person,
    };
    intake::check(
        store,
        &caller,
        &task.project,
        &request.team,
        role.as_deref(),
        pipeline.as_deref(),
        Some(&task.uuid),
    )?;
    Ok(())
}

pub(crate) fn prepare(
    store: &Store,
    state: &Path,
    machine: &str,
    request: &Request,
    document: &mut TaskDoc,
) -> Result<Previous> {
    let Operation::AddTask {
        task,
        role,
        pipeline,
    } = &request.operation
    else {
        return Err(crate::desktop::teams::person_actions::required());
    };
    let caller = Caller {
        state,
        machine,
        actor: TaskActor::Person,
    };
    intake::foreign(&caller, document)?;
    let previous_team = document.team().map_err(storage)?;
    let previous_name = previous_team
        .as_ref()
        .map(|key| {
            header::read(store, &key.team)
                .map(|header| header.map_or_else(|| key.team.clone(), |header| header.name))
        })
        .transpose()?
        .unwrap_or(
            header::read(store, &request.team)?
                .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team not found."))?
                .name,
        );
    let (_, _, view) = intake::prepare(
        store,
        &caller,
        task,
        &request.team,
        role.as_deref(),
        pipeline.as_deref(),
        document,
    )?;
    Ok(Previous {
        team: previous_team,
        name: previous_name,
        target: view.target,
    })
}

pub(crate) fn restore(document: &mut TaskDoc, previous: &Previous, now: i64) -> Result<bool> {
    let mut changed = false;
    if document.team().map_err(storage)? != previous.team {
        document
            .set_team_named(
                previous.team.as_ref(),
                &previous.name,
                &TaskActor::Person,
                now,
            )
            .map_err(storage)?;
        changed = true;
    }
    if document.view().map_err(storage)?.target != previous.target {
        document
            .set_target(&previous.target, &TaskActor::Person, now)
            .map_err(storage)?;
        changed = true;
    }
    Ok(changed)
}

/// The exact pre-binding observation of an existing cancelled node. The
/// common Teams writer may apply only this continuing service fact before
/// adding a replacement. No caller-supplied Actor becomes service authority.
pub(crate) fn settlement(
    store: &Store,
    team: &str,
    document: &TaskDoc,
) -> Result<Option<(u32, TaskObservation)>> {
    let view = document.view().map_err(storage)?;
    let task = TaskRef {
        project: view.project.clone(),
        uuid: document.uuid().into(),
    };
    let Some((home, node)) = binding(store, &task)? else {
        return Ok(None);
    };
    if home != team {
        return Ok(None);
    }
    let cancelled = view
        .execution
        .as_ref()
        .filter(|e| !e.conflict)
        .and_then(|e| e.current.as_ref())
        .is_some_and(|c| c.phase == zerus_replica::execution::Phase::Cancelled);
    let pending = store
        .pending_cancels(team, 200, 0)?
        .iter()
        .any(|pending| pending.node == node);
    if !cancelled && !pending {
        return Ok(None);
    }
    let header = header::read(store, team)?
        .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team not found."))?;
    let mut fact = observation(document, team, &header.home)?;
    fact.key = ops::KeyState::Ours;
    fact.previous_revision = store
        .task_observation(team, node)?
        .map_or(String::new(), |o| o.revision);
    Ok(Some((node, fact)))
}

#[cfg(all(test, unix))]
pub(crate) fn record_saved(
    store: &mut Store,
    team: &str,
    node: u32,
    document: &TaskDoc,
) -> Result<()> {
    record(store, team, node, document)
}

/// The document is still under the original project lock. Read the one
/// previous observation with a SQL body bound before decoding it.
pub(crate) fn saved_observation(
    db: &rusqlite::Connection,
    team: &str,
    node: u32,
    document: &TaskDoc,
    home: &str,
) -> Result<Option<TaskObservation>> {
    let mut current = observation(document, team, home)?;
    let saved:Option<Option<String>>=db.query_row("SELECT CASE WHEN length(CAST(data AS BLOB))<=65536 THEN data END FROM task_observations WHERE team=?1 AND node=?2",params![team,node],|r|r.get(0)).optional().map_err(storage)?;
    let previous: Option<TaskObservation> = match saved {
        None => None,
        Some(Some(body)) => Some(super::super::store::decode(&body)?),
        _ => return Err(storage("The exact previous observation is unavailable.")),
    };
    if previous.as_ref().is_some_and(|old| {
        old.revision == current.revision
            && old.attempt == current.attempt
            && old.attempt_id == current.attempt_id
            && old.attempt_conflict == current.attempt_conflict
    }) {
        return Ok(None);
    }
    current.previous_revision = previous.map_or(String::new(), |old| old.revision);
    Ok(Some(current))
}
