//! One complete task source held under the existing project lock.
use super::*;
use ops::observed_timing::TaskTimingSample;
use zerus_team_plan::observed_timing::{AttemptKey, Category};

pub(crate) struct TimingRead {
    pub team: String,
    pub node: u32,
    pub sample: TaskTimingSample,
    // This lock survives through the independent optional recording call.
    pub _held: ProjectStore,
}

pub(crate) fn observe(
    state: &Path,
    task: &TaskRef,
    epoch: &str,
    generation: Option<String>,
    offset: Option<u64>,
) -> Result<Option<TimingRead>> {
    if !super::super::enabled(state) {
        return Err(Problem::new(code::TEAMS_INACTIVE, "Teams preview is off."));
    }
    let tasks = project(state, task, Some(std::time::Duration::from_secs(2)))?;
    let document = load(&tasks, &task.uuid)?;
    let mut store = Store::open(state)?;
    let Some((team, node)) = binding(&store, task)? else {
        return Ok(None);
    };
    record(&mut store, &team, node, &document)?;
    let header = header::read(&store, &team)?.ok_or_else(|| Guard::ObservedTeam.problem())?;
    let observed = observation(&document, &team, &header.home)?;
    if observed.attempt_conflict || !matches!(observed.key, ops::KeyState::Ours) {
        return Ok(None);
    }
    let original = if observed.attempt_id.is_empty() {
        None
    } else {
        document.attempt(&observed.attempt_id).map_err(storage)?
    };
    let attempt = original.as_ref().map(|a| AttemptKey {
        id: a.id.clone(),
        generation: a.generation,
        fingerprint: a.fingerprint(),
    });
    let executor = original
        .as_ref()
        .map(|a| zerus_team_plan::model::SessionKey {
            machine: a.executor.machine.clone(),
            session_id: a.executor.session_id.clone(),
        });
    let node_generation = store
        .db
        .as_ref()
        .ok_or_else(|| storage("Teams store unavailable"))?
        .query_row(
            "SELECT generation FROM nodes WHERE team=?1 AND id=?2",
            params![team, node],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let category = if observed.cancelled {
        Category::TaskCancelled
    } else {
        match observed.stage {
            TaskStage::Backlog => Category::TaskBacklog,
            TaskStage::Todo => Category::TaskTodo,
            TaskStage::InProgress => Category::TaskInProgress,
            TaskStage::InReview => Category::TaskInReview,
            TaskStage::Done => Category::TaskDone,
        }
    };
    let sample = TaskTimingSample {
        task: task.clone(),
        node_generation,
        task_revision: Some(observed.revision),
        applied_revision: header.revision,
        attempt_generation: observed.attempt,
        attempt,
        executor,
        delivery_run: original
            .as_ref()
            .and_then(|a| a.delivery.as_ref())
            .map(|d| d.run_id.clone()),
        delivery_conversation: original
            .as_ref()
            .and_then(|a| a.delivery.as_ref())
            .and_then(|d| (!d.conversation_id.is_empty()).then(|| d.conversation_id.clone())),
        category,
        observer_epoch: epoch.into(),
        replica_generation: generation,
        replica_offset: offset,
        gap: false,
    };
    Ok(Some(TimingRead {
        team,
        node,
        sample,
        _held: tasks,
    }))
}
