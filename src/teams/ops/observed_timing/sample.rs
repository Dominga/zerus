//! One service-owned task snapshot, never a client-supplied duration.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTimingSample {
    pub task: TaskRef,
    pub node_generation: u32,
    pub task_revision: Option<String>,
    pub applied_revision: u64,
    pub attempt_generation: u64,
    pub attempt: Option<AttemptKey>,
    pub executor: Option<SessionKey>,
    pub delivery_run: Option<String>,
    pub delivery_conversation: Option<String>,
    pub category: Category,
    pub observer_epoch: String,
    pub replica_generation: Option<String>,
    pub replica_offset: Option<u64>,
    pub gap: bool,
}

fn checked(ctx: &Context<'_>, node: u32, sample: &TaskTimingSample) -> Result<Node> {
    uuid(&sample.task.uuid)?;
    uuid(&sample.observer_epoch)?;
    let n = ctx.node(node)?;
    if n.task.as_ref() != Some(&sample.task) || ctx.generation(node)? != sample.node_generation {
        return Err(invalid("The observation's exact task generation changed."));
    }
    if sample.applied_revision > ctx.team.revision {
        return Err(invalid("The source projection revision is unavailable."));
    }
    if sample.gap {
        if sample.category != Category::Gap
            || sample.task_revision.is_some()
            || sample.attempt.is_some()
            || sample.executor.is_some()
            || sample.delivery_run.is_some()
            || sample.delivery_conversation.is_some()
        {
            return Err(invalid(
                "An unavailable source cannot supply positive observations.",
            ));
        }
        return Ok(n);
    }
    let revision = sample
        .task_revision
        .as_deref()
        .filter(|r| !r.is_empty())
        .ok_or_else(|| invalid("The observed task revision is missing."))?;
    text(revision, 4096)?;
    if !matches!(
        sample.category,
        Category::TaskBacklog
            | Category::TaskTodo
            | Category::TaskInProgress
            | Category::TaskInReview
            | Category::TaskDone
            | Category::TaskCancelled
    ) {
        return Err(invalid("Unsupported task observation category."));
    }
    let observed = store::rows::observation(ctx.db, &ctx.team.id, node)?;
    let Some(observed) = observed else {
        return Err(invalid("The task observation is not current."));
    };
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
    if observed.revision != revision
        || observed.attempt != sample.attempt_generation
        || observed.attempt_id != sample.attempt.as_ref().map_or("", |a| a.id.as_str())
        || category != sample.category
    {
        return Err(invalid("A newer task observation owns this unit."));
    }
    if let Some(attempt) = &sample.attempt {
        uuid(&attempt.id)?;
        if attempt.generation != sample.attempt_generation
            || attempt.fingerprint.len() != 64
            || !attempt
                .fingerprint
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || sample.executor.is_none()
        {
            return Err(invalid("The task attempt identity is incomplete."));
        }
    } else if sample.executor.is_some()
        || sample.delivery_run.is_some()
        || sample.delivery_conversation.is_some()
    {
        return Err(invalid("A missing attempt cannot establish a binding."));
    }
    Ok(n)
}

pub(super) fn unchanged(ctx: &Context<'_>, node: u32, sample: &TaskTimingSample) -> Result<bool> {
    checked(ctx, node, sample)?;
    same(ctx.db, &ctx.team.id, node, sample, ctx.now)
}
pub(super) fn same(
    db: &Connection,
    team: &str,
    node: u32,
    sample: &TaskTimingSample,
    now: i64,
) -> Result<bool> {
    let Some(old) = data::latest_task(db, team, node)? else {
        return Ok(false);
    };
    if old.key.node_generation != sample.node_generation {
        return Ok(false);
    }
    if let (Some(old_offset), Some(new)) = (old.source.replica_offset, sample.replica_offset) {
        if old.source.replica_generation == sample.replica_generation && new < old_offset {
            return Ok(true);
        }
    }
    let same = old.source.kind == SourceKind::TaskDocument
        && old.source.observer_epoch.as_deref() == Some(sample.observer_epoch.as_str())
        && old.source.task_revision == sample.task_revision
        && (old.category == sample.category || old.category == Category::Watermark)
        && (sample.gap || old.key.attempt == sample.attempt)
        && old.source.replica_generation == sample.replica_generation;
    Ok(
        same && (sample.gap
            || now >= old.observed_at_ms && now - old.observed_at_ms < WATERMARK_MS),
    )
}

pub(super) fn build(ctx: &Context<'_>, node: u32, sample: &TaskTimingSample) -> Result<TimingFact> {
    let n = checked(ctx, node, sample)?;
    let old = data::latest_task(ctx.db, &ctx.team.id, node)?
        .filter(|f| f.key.node_generation == sample.node_generation);
    let mut reasons = vec![
        Reason::NativeUnsupported,
        Reason::SampledSource,
        Reason::UnsequencedSource,
    ];
    let effect = sample.attempt.as_ref().map(|a| a.id.as_str());
    let original = effect
        .map(|id| data::reserved(ctx.db, &ctx.team.id, id))
        .transpose()?
        .flatten();
    let original = if sample.gap { old.clone() } else { original };
    let (mut key, snapshot) = if let Some(original) = original {
        if original.key.node_generation != sample.node_generation
            || original.key.task != sample.task
        {
            return Err(invalid("The reserved source attempt changed."));
        }
        (original.key, original.snapshot)
    } else {
        reasons.extend([
            Reason::OldData,
            Reason::PublicationUnavailable,
            Reason::MissingBinding,
        ]);
        (
            ObservedUnitKey {
                team: ctx.team.id.clone(),
                home: ctx.team.home.clone(),
                node,
                task: sample.task.clone(),
                kind: match n.kind {
                    NodeKind::Task => UnitKind::Implementation,
                    NodeKind::Check => UnitKind::Check,
                    NodeKind::Stage => UnitKind::Stage,
                },
                phase: n.phase,
                publication: None,
                node_generation: sample.node_generation,
                attempt: None,
                effect: None,
                binding: None,
            },
            HistoricalSnapshot {
                title: n.title,
                role: None,
                configured_model: None,
                native_model: None,
            },
        )
    };
    if !sample.gap {
        key.attempt = sample.attempt.clone();
    }
    if let (Some(binding), Some(executor)) = (&mut key.binding, &sample.executor) {
        let current = ctx.member(&binding.member)?;
        if &binding.session != executor || current.session.as_ref() != Some(executor) {
            return Err(invalid("The exact source member binding changed."));
        }
        binding.kind = BindingKind::TaskDelivery;
        binding.run_id = sample.delivery_run.clone();
        binding.conversation_id = sample.delivery_conversation.clone();
    } else if sample.attempt.is_none() {
        reasons.push(Reason::MissingAttempt);
    }
    if key.binding.is_none() && !reasons.contains(&Reason::MissingBinding) {
        reasons.push(Reason::MissingBinding);
    }
    if key.publication.is_none() && !reasons.contains(&Reason::PublicationUnavailable) {
        reasons.push(Reason::PublicationUnavailable);
    }
    if key.attempt.is_none() && !reasons.contains(&Reason::MissingAttempt) {
        reasons.push(Reason::MissingAttempt);
    }
    let restart = old.as_ref().is_some_and(|old| {
        old.source.observer_epoch.as_deref() != Some(sample.observer_epoch.as_str())
    });
    let clock = old
        .as_ref()
        .is_some_and(|old| ctx.now < old.observed_at_ms.max(old.last_verified_ms.unwrap_or(0)));
    if restart {
        reasons.push(Reason::ObserverRestart);
    }
    if clock {
        reasons.push(Reason::ClockDiscontinuity);
    }
    if sample.gap {
        reasons.push(Reason::SourceUnavailable);
    }
    let last_verified_ms = if sample.gap || clock {
        old.as_ref().and_then(|o| o.last_verified_ms)
    } else {
        Some(ctx.now)
    };
    let identity = data::extend(ctx.db)?;
    Ok(TimingFact {
        schema: SCHEMA,
        seq: 0,
        unit: 0,
        key,
        snapshot,
        source: SourcePosition {
            schema: SCHEMA,
            kind: SourceKind::TaskDocument,
            identity,
            team_revision: sample.applied_revision,
            event_id: None,
            event_sequence: None,
            task_revision: sample.task_revision.clone(),
            replica_generation: sample.replica_generation.clone(),
            replica_offset: sample.replica_offset,
            observer_epoch: Some(sample.observer_epoch.clone()),
        },
        category: sample.category,
        event_at_ms: None,
        observed_at_ms: ctx.now,
        last_verified_ms,
        break_before: true,
        reasons,
        implementation_observed_ms: None,
    })
}
