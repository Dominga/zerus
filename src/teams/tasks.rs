//! Team intake and service observations of ordinary replicated tasks.
//! Project locks precede team operations; no code here performs a session effect.
use super::{
    ops,
    store::{storage, Guard, Store},
};
use ops::{Actor, Operation, Request, TaskObservation, TaskStage};
use rusqlite::{params, OptionalExtension};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use zerus_replica::{Actor as TaskActor, ProjectStore, Stage, TaskDoc};
use zerus_team_plan::{model::TaskRef, problem::code, Problem};

type Result<T> = std::result::Result<T, Problem>;
mod creation;
mod header;
mod intake;
mod policy;
pub(crate) use intake::{preflight, send};
pub(crate) use policy::{clear, clear_named, guard, guarded, meet};

/// The person's exact unknown-attempt disposition is separate from ordinary
/// task cancellation. A live team owns its retained reply; a terminal team
/// cannot recover it. Retain this guard through the task-document save.
pub(crate) fn abandon_admission(
    state: &Path,
    machine: &str,
    task: &TaskDoc,
) -> Result<Option<super::store::actor_reads::Admission>> {
    if task.team_invalid() {
        return Err(Problem::new(code::INVALID_STATE, "The task's team key is invalid."));
    }
    let Some(key) = task.team().map_err(storage)? else { return Ok(None) };
    if key.home != machine {
        return Err(Problem::new(code::NOT_ALLOWED,
            format!("Abandon this attempt on the team's home computer {}.", key.home)));
    }
    let admission = Store::disposition_admission(state)?;
    if let Some(held) = &admission {
        if let Some(team) = header::read(held, &key.team)? {
            if team.home != machine || team.project != task.view().map_err(storage)?.project {
                return Err(Problem::new(code::NOT_ALLOWED,
                    format!("The stored team belongs to home computer {} and project {}; the task's team key does not match.", team.home, team.project)));
            }
            use zerus_team_plan::model::TeamStatus;
            if !matches!(team.status, TeamStatus::Stopped | TeamStatus::Finished | TeamStatus::Archived) {
                return Err(Problem::new(code::NOT_ALLOWED,
                    format!("The team is {:?}; stop or archive it before abandoning this attempt. Its service owns delivery recovery.", team.status)));
            }
        }
    }
    Ok(admission)
}

pub(crate) mod context;
mod report;
#[allow(unused_imports)] // Consumed by the service package.
pub(crate) use report::report;

/// Native caller proof is resolved by the task command before entering here.
pub(crate) struct Caller<'a> {
    pub state: &'a Path,
    pub machine: &'a str,
    pub actor: TaskActor,
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
fn request(store: &Store, team: &str, operation: Operation) -> Result<Request> {
    let header = header::read(store, team)?
        .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team not found."))?;
    Ok(Request {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: team.into(),
        expected_revision: header.revision,
        operation,
    })
}
fn load(store: &ProjectStore, uuid: &str) -> Result<TaskDoc> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    store
        .admit_until(deadline)
        .ok_or_else(|| {
            Problem::new(
                code::STORE_BUSY,
                "The task store is busy. Nothing was written; try again.",
            )
        })?
        .load_complete(uuid)
        .map_err(storage)
}
/// The document retains the highest claim across stage changes. A conflicted
/// projection has no current identity that the team can safely act on. A
/// never-admitted candidate is retained for display, but is no current execution.
pub(crate) fn attempt(view: &zerus_replica::TaskView) -> (u64, String, bool) {
    let highest = view.execution.as_ref().map_or(0, |e| e.highest);
    let conflict = view.execution.as_ref().is_some_and(|e| e.conflict);
    let id = view
        .execution
        .as_ref()
        .filter(|e| !e.conflict)
        .and_then(|e| e.current.as_ref())
        .filter(|current| current.phase != zerus_replica::execution::Phase::NotStarted)
        .map_or_else(String::new, |current| current.id.clone());
    (highest, id, conflict)
}
pub(crate) fn observation(document: &TaskDoc, team: &str, home: &str) -> Result<TaskObservation> {
    let view = document.view().map_err(storage)?;
    let (attempt, attempt_id, attempt_conflict) = attempt(&view);
    let key = match document.team().map_err(storage)? {
        Some(key) if key.team == team && key.home == home => ops::KeyState::Ours,
        None if !document.team_invalid() => ops::KeyState::Absent,
        _ => ops::KeyState::Other,
    };
    let current_cancelled = matches!(key, ops::KeyState::Ours)
        && view
            .execution
            .as_ref()
            .filter(|e| !e.conflict)
            .and_then(|e| e.current.as_ref())
            .is_some_and(|current| current.phase == zerus_replica::execution::Phase::Cancelled);
    Ok(TaskObservation {
        key,
        revision: document.revision(),
        previous_revision: String::new(),
        stage: match view.stage {
            Stage::Backlog => TaskStage::Backlog,
            Stage::Todo => TaskStage::Todo,
            Stage::Active => TaskStage::InProgress,
            Stage::Review => TaskStage::InReview,
            Stage::Done => TaskStage::Done,
        },
        cancelled: view.cancelled_at.is_some(),
        current_cancelled,
        rank: view.rank,
        title: view.title,
        attempt,
        attempt_id,
        attempt_conflict,
    })
}
fn record(store: &mut Store, team: &str, node: u32, document: &TaskDoc) -> Result<()> {
    let header = header::read(store, team)?.ok_or_else(|| Guard::ObservedTeam.problem())?;
    let mut observation = observation(document, team, &header.home)?;
    let previous = store.task_observation(team, node)?;
    observation.previous_revision = previous
        .as_ref()
        .map_or(String::new(), |o| o.revision.clone());
    if matches!(observation.key, ops::KeyState::Ours)
        && observation.previous_revision == observation.revision
        && previous.as_ref().is_some_and(|old| {
            old.attempt == observation.attempt
                && old.attempt_id == observation.attempt_id
                && old.attempt_conflict == observation.attempt_conflict
        })
    {
        return Ok(());
    }
    let req = Request {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: team.into(),
        expected_revision: header.revision,
        operation: Operation::ObserveTask { node, observation },
    };
    ops::apply(
        store,
        &Actor::Service {
            person: header.owner,
            team: team.into(),
        },
        &req,
        now(),
    )?;
    Ok(())
}
/// The writer a guard meeting clears through (zerus-63rru.97 F2): the
/// caller's already-open project store, or the attempt admission's own
/// writer. A meeting never opens a second `ProjectStore` under the caller's
/// project file lock, which would wait on itself.
pub(crate) enum Meeting<'a, 'b> {
    Projects(&'a ProjectStore),
    Admitted(&'a mut zerus_replica::Admitted<'b>),
}

impl Meeting<'_, '_> {
    fn load(&mut self, uuid: &str) -> Result<TaskDoc> {
        match self {
            Meeting::Projects(store) => store.load(uuid),
            Meeting::Admitted(admitted) => admitted.load(uuid),
        }
        .map_err(storage)
    }
    fn save(&mut self, task: &mut TaskDoc) -> Result<()> {
        match self {
            Meeting::Projects(store) => store.save(task),
            Meeting::Admitted(admitted) => admitted.save(task),
        }
        .map_err(storage)
    }
}

/// The returned key (zerus-63rru.97): an older replica document merged
/// after the stop sweep brought the key back. Meeting it here clears it
/// with the same write the sweep uses — only the key, once per meeting,
/// never for a live team, never for a key bound again after the stop, and
/// never for a key or a team that belongs to another computer (F3).
pub(crate) fn clear_returned_key(
    store: &Store,
    meeting: &mut Meeting<'_, '_>,
    task: &TaskRef,
    team: &str,
    machine: &str,
) -> Result<bool> {
    use zerus_team_plan::model::TeamStatus;
    let Some(header) = store.team(team)? else {
        return Ok(false);
    };
    if header.home != machine {
        // A team whose home is another computer is not this computer's to
        // clear; only that computer's own guard may clear its key.
        return Ok(false);
    }
    if !matches!(header.status, TeamStatus::Stopped | TeamStatus::Archived) {
        return Ok(false);
    }
    if binding(store, task)?.is_some() {
        // Bound again to a live team after the stop: the key is live again.
        return Ok(false);
    }
    let Ok(mut document) = meeting.load(&task.uuid) else {
        return Ok(false);
    };
    let view = document.view().map_err(storage)?;
    let Some(key) = view.team else {
        return Ok(false);
    };
    if key.team != team || key.home != machine {
        // The key is another team's, or its home is another computer, or
        // the document changed under the caller: not this meeting's key.
        return Ok(false);
    }
    document.clear_team_silent().map_err(storage)?;
    meeting.save(&mut document)?;
    Ok(true)
}

pub(crate) fn binding(store: &Store, task: &TaskRef) -> Result<Option<(String, u32)>> {
    store.db.as_ref().map_or(Ok(None), |db| {
        db.query_row(
            "SELECT team,node FROM live_tasks WHERE task_project=?1 AND task_uuid=?2",
            params![task.project, task.uuid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)
    })
}

/// Service entry: load and record under the same project lock. An observation
/// read before a later task write can never be carried into this function.
#[allow(dead_code)] // Called by the team service.
pub(crate) fn observe(state: &Path, task: &TaskRef) -> Result<()> {
    if !super::enabled(state) {
        return Err(Problem::new(code::TEAMS_INACTIVE, "Teams preview is off."));
    }
    let tasks = ProjectStore::open(&state.join("replica"), &task.project).map_err(storage)?;
    let document = load(&tasks, &task.uuid)?;
    #[cfg(test)]
    observe_pause();
    let mut store = Store::open(state)?;
    if let Some((team, node)) = binding(&store, task)? {
        record(&mut store, &team, node, &document)?;
    }
    Ok(())
}

#[cfg(test)]
fn observe_pause() {
    let Some(path) = std::env::var_os("HGS_TEST_TEAM_OBSERVE_PAUSE") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    if std::fs::write(path.join("ready"), "loaded").is_err() {
        return;
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while !path.join("release").exists() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Called by retarget while its task command retains the project lock. A
/// refused release leaves this in-memory edit unsaved, including its key.
pub(crate) fn retarget(
    state: &Path,
    here: &str,
    document: &mut TaskDoc,
    actor: &TaskActor,
) -> Result<()> {
    let marked = document.team().map_err(storage)?.is_some() || document.team_invalid();
    clear(state, document, actor)?;
    if marked && super::enabled(state) {
        let mut store = Store::open(state)?;
        let reference = TaskRef {
            project: document.view().map_err(storage)?.project,
            uuid: document.uuid().into(),
        };
        if let Some((team, node)) = binding(&store, &reference)? {
            // A replaced or malformed key cannot hide this computer's live
            // binding. The stored team's home decides who releases it.
            let header =
                header::read(&store, &team)?.ok_or_else(|| Guard::RetargetedTeam.problem())?;
            if header.home == here {
                record(&mut store, &team, node, document)?;
            }
        }
    }
    Ok(())
}

/// Package 10 clears the marker only if this exact team still owns it.
#[allow(dead_code)] // Called by package 10 after terminal-team reconciliation.
pub(crate) fn clear_team_key(
    state: &Path,
    task: &TaskRef,
    expected_team: &str,
    actor: &TaskActor,
) -> Result<bool> {
    if !super::enabled(state) {
        return Err(Problem::new(code::TEAMS_INACTIVE, "Teams preview is off."));
    }
    let tasks = ProjectStore::open(&state.join("replica"), &task.project).map_err(storage)?;
    let mut document = load(&tasks, &task.uuid)?;
    if document
        .team()
        .map_err(storage)?
        .as_ref()
        .is_none_or(|link| link.team != expected_team)
    {
        return Ok(false);
    }
    let link = document.team().map_err(storage)?.expect("checked marker");
    let store = Store::open(state)?;
    let Some(header) = header::read(&store, expected_team)? else {
        return Ok(false);
    };
    if link.home != header.home {
        return Ok(false);
    }
    if header.live() {
        return Err(Problem::new(
            code::INVALID_STATE,
            "Only a terminal team's marker is cleared by this service operation.",
        ));
    }
    clear(state, &mut document, actor)?;
    tasks.save(&mut document).map_err(storage)?;
    Ok(true)
}
#[cfg(test)]
mod tests;
