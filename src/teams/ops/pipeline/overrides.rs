use super::*;

pub(in crate::teams::ops) fn released_stage(ctx: &Context<'_>, n: &Node) -> Result<bool> {
    // A finished live stage still awaits the writer's Done observation.
    if n.kind != NodeKind::Stage || ctx.node_table(n.id)? == "nodes" {
        return Ok(false);
    }
    ctx.db.query_row("SELECT NOT EXISTS(SELECT 1 FROM stage_tasks WHERE team=?1 AND node=?2 AND state IN ('wanted','reserved','created'))",params![ctx.team.id,n.id],|r|r.get(0)).map_err(storage)
}

pub(in crate::teams::ops) fn override_task(ctx: &Context<'_>, target: u32) -> Result<()> {
    let stage: Option<u32> = ctx
        .db
        .query_row(
            "SELECT stage FROM pipeline_runs WHERE team=?1 AND node=?2",
            params![ctx.team.id, target],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?
        .flatten();
    if let Some(stage) = stage {
        ctx.db.execute("UPDATE stage_tasks SET state='superseded' WHERE team=?1 AND node=?2 AND state IN ('wanted','reserved','created')",params![ctx.team.id,stage]).map_err(storage)?;
        retire_stage(ctx, stage)?;
    }
    state(ctx, target, "overridden", None)?;
    ctx.db.execute("UPDATE cards SET state='closed',decision='stale',decided_actor=?3 WHERE team=?1 AND node=?2 AND kind='pipeline_acceptance' AND state='open'",params![ctx.team.id,target,encode(ctx.actor)?]).map_err(storage)?;
    Ok(())
}
pub(in crate::teams::ops) fn observed_reopen(ctx: &Context<'_>, node: u32) -> Result<bool> {
    ctx.db.query_row("SELECT EXISTS(SELECT 1 FROM pipeline_runs WHERE team=?1 AND node=?2 AND generation=?3 AND state IN ('active','person','lead','accepted'))",params![ctx.team.id,node,ctx.generation(node)?],|r|r.get(0)).map_err(storage)
}
pub(in crate::teams::ops) fn observe_override(ctx: &mut Context<'_>, n: &Node) -> Result<()> {
    if n.kind == NodeKind::Stage {
        if let Some(target) = n.stage_of {
            if current_stage(ctx, n.id, target)? {
                ctx.db
                    .execute(
                        "UPDATE stage_tasks SET state='superseded' WHERE team=?1 AND node=?2",
                        params![ctx.team.id, n.id],
                    )
                    .map_err(storage)?;
                // The current stage's own task ended: it ends here.
                end_stage(ctx, n.id)?;
                let target = ctx.node(target)?;
                to_lead(
                    ctx,
                    &target,
                    "The person overrode the pipeline's stage task.",
                )?;
            }
        }
        Ok(())
    } else {
        override_task(ctx, n.id)
    }
}
pub(in crate::teams::ops) fn before_decision(ctx: &Context<'_>, node: u32) -> Result<()> {
    if is_person(ctx.actor) {
        return override_task(ctx, node);
    }
    let handed_over:bool=ctx.db.query_row("SELECT EXISTS(SELECT 1 FROM pipeline_runs WHERE team=?1 AND node=?2 AND generation=?3 AND state='lead')",params![ctx.team.id,node,ctx.generation(node)?],|r|r.get(0)).map_err(storage)?;
    if !handed_over {
        return Err(invalid(
            "The pipeline has not handed this attempt to the lead.",
        ));
    }
    Ok(())
}
pub(in crate::teams::ops) fn decide_card(
    ctx: &mut Context<'_>,
    node: u32,
    answer: &str,
) -> Result<bool> {
    let current:bool=ctx.db.query_row("SELECT EXISTS(SELECT 1 FROM pipeline_runs p JOIN nodes n ON n.team=p.team AND n.id=p.node WHERE p.team=?1 AND p.node=?2 AND p.state='person' AND p.generation=n.generation AND n.node_state='reported')",params![ctx.team.id,node],|r|r.get(0)).map_err(storage)?;
    if !current || ctx.pending(node)?.is_some() {
        return Ok(true);
    }
    ctx.active()?;
    let n = ctx.live_node(node)?;
    match answer {
        "approve" => {
            state(ctx, node, "accepted", None)?;
            decisions::accept_result(ctx, &n, Decider::Person, true)?;
        }
        "reject" => to_lead(ctx, &n, "The person declined the pipeline's acceptance.")?,
        _ => return Err(invalid("Pipeline acceptance must be approve or reject.")),
    }
    Ok(false)
}

pub(in crate::teams::ops) fn before_observation(
    ctx: &mut Context<'_>,
    n: &mut Node,
    o: &TaskObservation,
    live: bool,
) -> Result<()> {
    let reopened = live
        && matches!(ctx.team.status, TeamStatus::Running | TeamStatus::Paused)
        && n.kind != NodeKind::Stage
        && n.state == NodeState::Reported
        && !o.cancelled
        && matches!(
            o.stage,
            TaskStage::Todo | TaskStage::InProgress | TaskStage::Backlog
        )
        && observed_reopen(ctx, n.id)?
        && !ctx.reopening(n.id)?;
    if o.cancelled || o.stage == TaskStage::Done {
        task_ended(ctx, n, o.cancelled)?;
    } else if reopened {
        observe_override(ctx, n)?;
    }
    if reopened {
        ctx.settle_override(n.id)?;
        ctx.advance(n.id, n)?;
        ctx.db.execute(&format!("UPDATE effects SET state='superseded' WHERE team=?1 AND node=?2 AND state IN {OPEN_EFFECT_STATES}"),params![ctx.team.id,n.id]).map_err(storage)?;
        queue::hand_back(ctx, n.id, false)?;
        ctx.event = Some("person_override");
    }
    Ok(())
}

/// What the end of a node's task does to its pipeline, whether the store
/// learns it from an observation or from a recorded answer: a person's end
/// overrides it (a stage hands its parent back to the lead, a task leaves
/// its pipeline); a writer-produced end of a stage does not.
pub(in crate::teams::ops) fn task_ended(
    ctx: &mut Context<'_>,
    n: &Node,
    cancelled: bool,
) -> Result<()> {
    if !team_ended_stage(ctx, n, cancelled)? {
        observe_override(ctx, n)?;
    }
    Ok(())
}

fn team_ended_stage(ctx: &Context<'_>, n: &Node, cancelled: bool) -> Result<bool> {
    if n.kind != NodeKind::Stage {
        return Ok(false);
    }
    // A writer-produced end is not a person's override. Inspect only the
    // latest decision by the node index, including settled decisions: older
    // development stores allowed the lead to end stages without a verdict.
    let decision = ctx.db.query_row(
        "SELECT generation,kind,json_extract(actor,'$.kind')='person' FROM decisions WHERE team=?1 AND node=?2 ORDER BY seq DESC LIMIT 1",
        params![ctx.team.id, n.id],
        |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?)),
    ).optional().map_err(storage)?;
    let Some((generation, kind, person)) = decision else {
        return Ok(false);
    };
    Ok(generation == ctx.generation(n.id)?
        && kind == if cancelled { "cancel" } else { "accept" }
        && !person)
}
