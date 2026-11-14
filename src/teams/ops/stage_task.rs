use super::*;

pub(super) fn intent(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::StageTaskIntent {
        node,
        effect_request_id,
    } = ctx.op
    else {
        unreachable!()
    };
    uuid(effect_request_id)?;
    let n = ctx.live_node(*node)?;
    if ctx.team.status != TeamStatus::Running || n.kind != NodeKind::Stage || n.task.is_some() {
        return Err(invalid(
            "Only a running team's wanted stage can reserve its task creation.",
        ));
    }
    let valid: bool = ctx.db.query_row("SELECT EXISTS(SELECT 1 FROM stage_tasks s JOIN pipeline_runs p ON p.team=s.team AND p.node=s.target JOIN nodes n ON n.team=p.team AND n.id=p.node WHERE s.team=?1 AND s.node=?2 AND s.state='wanted' AND p.state='active' AND p.stage=s.node AND p.generation=s.generation AND n.generation=s.generation AND n.node_state='reported')",params![ctx.team.id,node],|r|r.get(0)).map_err(storage)?;
    if !valid {
        return Err(invalid(
            "Stage creation is not wanted by the current pipeline round.",
        ));
    }
    let used: bool = ctx
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM stage_tasks WHERE request_id=?1)",
            [effect_request_id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if used {
        return Err(invalid("Stage creation request ID was already used."));
    }
    ctx.db
        .execute(
            "UPDATE stage_tasks SET request_id=?3,state='reserved' WHERE team=?1 AND node=?2",
            params![ctx.team.id, node, effect_request_id],
        )
        .map_err(storage)?;
    ctx.reply.node = Some(*node);
    Ok(())
}
pub(super) fn outcome(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::StageTaskOutcome {
        effect_request_id,
        outcome,
    } = ctx.op
    else {
        unreachable!()
    };
    uuid(effect_request_id)?;
    let row: Option<(u32, u32, String, Option<String>)> = ctx
        .db
        .query_row(
            "SELECT node,target,state,outcome FROM stage_tasks WHERE team=?1 AND request_id=?2",
            params![ctx.team.id, effect_request_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(storage)?;
    let (node, target, state, old) = row.ok_or_else(missing)?;
    if old.is_some() {
        return Err(invalid("Stage creation already has an outcome."));
    }
    let next = outcome_state(ctx, outcome)?;
    // Superseded and inactive effects still accept their one provenance fact.
    let current = state == "reserved"
        && matches!(ctx.team.status, TeamStatus::Running | TeamStatus::Paused)
        && pipeline::current_stage(ctx, node, target)?;
    ctx.db.execute("UPDATE stage_tasks SET outcome=?3,state=CASE WHEN state='reserved' THEN ?4 ELSE state END WHERE team=?1 AND node=?2",params![ctx.team.id,node,encode(outcome)?,next]).map_err(storage)?;
    if !current {
        return Ok(());
    }
    match outcome {
        StageTaskOutcome::Created { task } => bind(ctx, node, task),
        StageTaskOutcome::Refused { reason } | StageTaskOutcome::Uncertain { reason } => {
            // A creation refused or uncertain was never bound: the stage is
            // retired, which keeps a person's pending cancel for its own
            // answer and otherwise ends the stage at once, as before.
            pipeline::retire_stage(ctx, node)?;
            let target = ctx.node(target)?;
            pipeline::to_lead(ctx, &target, reason)
        }
    }
}
fn bind(ctx: &Context<'_>, node: u32, task: &TaskRef) -> Result<()> {
    let mut n = ctx.live_node(node)?;
    let bound: bool = ctx
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM live_tasks WHERE task_project=?1 AND task_uuid=?2)",
            params![task.project, task.uuid],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if bound {
        return Err(invalid("Task already belongs to a live team node."));
    }
    n.task = Some(task.clone());
    ctx.db
        .execute(
            "INSERT INTO live_tasks(task_project,task_uuid,team,node) VALUES(?1,?2,?3,?4)",
            params![task.project, task.uuid, ctx.team.id, node],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE node_identity SET task_project=?3,task_uuid=?4 WHERE team=?1 AND id=?2",
            params![ctx.team.id, node, task.project, task.uuid],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE nodes SET data=?3 WHERE team=?1 AND id=?2",
            params![ctx.team.id, node, rows::node_payload(&n)?],
        )
        .map_err(storage)?;
    Ok(())
}

fn outcome_state(ctx: &Context<'_>, outcome: &StageTaskOutcome) -> Result<&'static str> {
    Ok(match outcome {
        StageTaskOutcome::Created { task } => {
            uuid(&task.uuid)?;
            if task.project != ctx.team.project {
                return Err(invalid("Stage task belongs to another project."));
            }
            "created"
        }
        StageTaskOutcome::Refused { reason: r } => {
            reason(r)?;
            "refused"
        }
        StageTaskOutcome::Uncertain { reason: r } => {
            reason(r)?;
            "uncertain"
        }
    })
}
