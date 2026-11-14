use super::*;
use zerus_replica::execution::{Origin, Phase};

fn acknowledgement(task: &TaskDoc, actor: &TaskActor) -> Result<bool> {
    let TaskActor::Agent(key) = actor else {
        return Ok(false);
    };
    let view = task.view().map_err(storage)?;
    if view.stage != Stage::Active {
        return Ok(false);
    }
    let same = |holder: &zerus_replica::SessionKey| {
        !key.session_id.is_empty()
            && holder.machine == key.machine
            && holder.session_id == key.session_id
    };
    // The execution projection includes legacy dispatches, and distinguishes
    // their holder from a direct claim (Origin::Start) on either fence route.
    Ok(view
        .execution
        .and_then(|e| e.current)
        .is_some_and(|attempt| {
            attempt.origin != Origin::Start
                && attempt.executor.as_ref().is_some_and(same)
                && matches!(attempt.phase, Phase::Submitted | Phase::Running)
                && (!attempt.legacy || attempt.at > 0)
        }))
}

/// An agent decides through hgs team; only the service drives a team task.
pub(crate) fn guard(
    caller: &Caller<'_>,
    meeting: &mut Meeting<'_, '_>,
    task: &TaskDoc,
    action: &str,
) -> Result<()> {
    if !matches!(
        action,
        "send"
            | "handoff"
            | "stage"
            | "done"
            | "target"
            | "start"
            | "worker"
            | "reopen"
            | "cancel"
            | "cancel-execution"
            | "move"
            | "edit"
            | "rename"
    ) {
        return Ok(());
    }
    if matches!(caller.actor, TaskActor::Person) {
        return Ok(());
    }
    let guarded = guarded(caller.state, caller.machine, task)?;
    if guarded && matches!(action, "start" | "worker") && acknowledgement(task, &caller.actor)? {
        return Ok(());
    }
    // zerus-63rru.97: meet the returned key of a team stopped or archived
    // on this computer at every agent meeting, before the early exit for a
    // non-live header. Meeting it clears it with the sweep's own write,
    // once; the task is usable again. A foreign key still refuses below.
    if meet(caller.state, caller.machine, task, meeting)? || !guarded {
        return Ok(());
    }
    Err(Problem::new(
        code::NOT_ALLOWED,
        format!(
            "An agent cannot {action} a live team's task; the team service performs this action."
        ),
    ))
}

/// The key-clear half of a guard meeting. A command whose task is not
/// guarded still meets a stopped or archived team's returned key here, so
/// the clear is reached for a local team (F1) through the caller's own
/// project writer (F2), and only for this computer's own key and team (F3).
pub(crate) fn meet(
    state: &Path,
    machine: &str,
    task: &TaskDoc,
    meeting: &mut Meeting<'_, '_>,
) -> Result<bool> {
    let Some(link) = task.team().map_err(storage)? else {
        return Ok(false);
    };
    let Ok(store) = Store::open(state) else {
        // Teams is off or its store is not opened here; no meeting.
        return Ok(false);
    };
    let reference = TaskRef {
        project: task.view().map_err(storage)?.project,
        uuid: task.uuid().into(),
    };
    super::clear_returned_key(&store, meeting, &reference, &link.team, machine)
}

/// The marker and indexed store binding decide whether a command touches a
/// team. This read grants no caller authority and never resolves a caller.
pub(crate) fn guarded(state: &Path, machine: &str, task: &TaskDoc) -> Result<bool> {
    let link = task.team().map_err(storage)?;
    if task.team_invalid() {
        return Ok(true);
    }
    let Some(link) = link else { return Ok(false) };
    if link.home != machine {
        return Ok(true);
    }
    if !super::super::enabled(state) {
        return Ok(false);
    }
    let store = Store::open(state)?;
    let Some(header) = header::read(&store, &link.team)? else {
        return Ok(false);
    };
    if !header.live() {
        return Ok(false);
    }
    let reference = TaskRef {
        project: task.view().map_err(storage)?.project,
        uuid: task.uuid().into(),
    };
    Ok(
        binding(&store, &reference)?.is_some_and(|(team, _)| team == link.team)
            || (reference.project == header.project
                && super::creation::pending(&store, &reference, &link.team)?),
    )
}

pub(crate) fn clear(state: &Path, task: &mut TaskDoc, actor: &TaskActor) -> Result<()> {
    let link = task.team().map_err(storage)?;
    if link.is_none() && !task.team_invalid() {
        return Ok(());
    }
    let mut name = link
        .as_ref()
        .map_or_else(|| "unknown team".into(), |link| link.team.clone());
    if super::super::enabled(state) {
        if let Some(link) = &link {
            if let Ok(store) = Store::open(state) {
                if let Ok(Some(header)) = header::read(&store, &link.team) {
                    name = header.name;
                }
            }
        }
    }
    clear_named(task, &name, actor)
}

/// Shared marker writer for an authority already checked by a service command.
pub(crate) fn clear_named(task: &mut TaskDoc, name: &str, actor: &TaskActor) -> Result<()> {
    task.set_team_named(None, name, actor, now())
        .map_err(storage)
}
