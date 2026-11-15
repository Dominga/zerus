//! Phase review and publication use the ordinary operation transaction. Only
//! the document writer records creation; this module binds its saved facts.
use super::*;
use store::publication::{PlannedTaskRequest, PUBLICATION_LAYOUT};

fn phase(ctx: &Context<'_>, phase: u32) -> Result<Phase> {
    ctx.db
        .query_row(
            "SELECT data FROM phases WHERE team=?1 AND n=?2",
            params![ctx.team.id, phase],
            |r| store::sql_json(r, 0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(missing)
}

fn draft(ctx: &Context<'_>, phase: u32, generation: u64) -> Result<Phase> {
    if !matches!(
        ctx.team.status,
        TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused
    ) {
        return Err(invalid("The team is not active."));
    }
    let record = phase_record(ctx, phase)?;
    if record.kind != PhaseKind::Plan {
        return Err(invalid("The intake queue is not a planned phase."));
    }
    let current: Option<u64> = ctx
        .db
        .query_row(
            "SELECT generation FROM phase_drafts WHERE team=?1 AND phase=?2",
            params![ctx.team.id, phase],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if current != Some(generation) {
        return Err(stale());
    }
    if store::publication::publication(ctx.db, &ctx.team.id, phase)?.is_some() {
        return Err(invalid("Publication has frozen this draft."));
    }
    Ok(record)
}
fn phase_record(ctx: &Context<'_>, n: u32) -> Result<Phase> {
    phase(ctx, n)
}

pub(super) fn approve(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ApprovePhase {
        phase,
        generation,
        confirm_initial,
    } = ctx.op
    else {
        unreachable!()
    };
    let record = draft(ctx, *phase, *generation)?;
    if record.state != PhaseState::Draft {
        return Err(invalid("Only the current draft can be approved."));
    }
    let initial = ctx.team.status == TeamStatus::Planning;
    if initial && (!is_person(ctx.actor) || !confirm_initial) {
        return Err(invalid(
            "The person must confirm the initial plan, roster and working folders.",
        ));
    }
    if matches!(ctx.actor, Actor::Service { .. })
        && (initial
            || ctx.team.status != TeamStatus::Running
            || record.approval != Approval::Auto
            || ctx.team.settings.plan_approval != Approval::Auto)
    {
        return Err(invalid("Automatic approval is not enabled for this phase."));
    }
    let lead = ctx
        .member(&ctx.team.lead)?
        .session
        .filter(|s| s.machine == ctx.team.home && !s.session_id.is_empty())
        .ok_or_else(|| invalid("The lead needs its local session binding before approval."))?;
    let nodes = store::drafts::nodes(ctx.db, &ctx.team.id, *phase)?;
    if nodes.is_empty() {
        return Err(invalid("An empty phase cannot be approved."));
    }
    for n in &nodes {
        validation::route_valid(ctx.team, &n.role, &n.pipeline)?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    ctx.db
        .pragma_update(None, "user_version", PUBLICATION_LAYOUT)
        .map_err(storage)?;
    ctx.db.execute("INSERT INTO phase_publications(id,team,phase,generation,snapshot,initial,state,created_ms) VALUES(?1,?2,?3,?4,?5,?6,'reserved',?7)",params![id,ctx.team.id,phase,generation,encode(&nodes)?,initial,ctx.now]).map_err(storage)?;
    for node in nodes {
        let request = PlannedTaskRequest {
            publication: id.clone(),
            team: ctx.team.id.clone(),
            phase: *phase,
            task: TaskRef {
                project: ctx.team.project.clone(),
                uuid: uuid::Uuid::new_v4().to_string(),
            },
            lead: lead.clone(),
            home: ctx.team.home.clone(),
            team_name: ctx.team.name.clone(),
            text: format!(
                "{}\n\nAcceptance criteria:\n{}",
                node.text,
                node.criteria
                    .iter()
                    .map(|c| format!("- {c}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            node,
            created_ms: ctx.now,
        };
        ctx.db.execute("INSERT INTO phase_publication_nodes(publication,team,node,task_uuid,request) VALUES(?1,?2,?3,?4,?5)",params![id,ctx.team.id,request.node.id,request.task.uuid,encode(&request)?]).map_err(storage)?;
    }
    ctx.db
        .execute(
            "UPDATE phase_reviews SET rejection=NULL,comment=NULL WHERE team=?1 AND phase=?2",
            params![ctx.team.id, phase],
        )
        .map_err(storage)?;
    Ok(())
}

pub(super) fn reject(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::RejectPhase {
        phase,
        generation,
        comment,
    } = ctx.op
    else {
        unreachable!()
    };
    let mut record = draft(ctx, *phase, *generation)?;
    if record.state != PhaseState::Draft {
        return Err(invalid("Only a draft can be rejected."));
    }
    reason(comment)?;
    let id = uuid::Uuid::new_v4().to_string();
    ctx.db.execute("INSERT INTO phase_reviews(team,phase,generation,rejection,comment,actor) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(team,phase) DO UPDATE SET generation=excluded.generation,rejection=excluded.rejection,comment=excluded.comment,actor=excluded.actor",params![ctx.team.id,phase,generation,id,comment,encode(ctx.actor)?]).map_err(storage)?;
    record.state = PhaseState::Rejected;
    save_phase(ctx, &record)?;
    let text = format!("Phase {} was rejected: {}", phase, comment);
    ctx.db.execute("INSERT INTO inbox(team,kind,node,generation,actor,text,created_ms) VALUES(?1,?2,0,0,?3,?4,?5)",params![ctx.team.id,format!("phase_rejected:{id}"),encode(ctx.actor)?,text,ctx.now]).map_err(storage)?;
    Ok(())
}

pub(super) fn undo(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::UndoRejectPhase {
        phase,
        generation,
        rejection,
    } = ctx.op
    else {
        unreachable!()
    };
    let mut record = draft(ctx, *phase, *generation)?;
    let saved = store::publication::rejection(ctx.db, &ctx.team.id, *phase)?.ok_or_else(missing)?;
    if record.state != PhaseState::Rejected
        || saved.id != *rejection
        || saved.generation != *generation
    {
        return Err(invalid("That rejection is no longer current."));
    }
    ctx.db
        .execute(
            "UPDATE phase_reviews SET rejection=NULL,comment=NULL WHERE team=?1 AND phase=?2",
            params![ctx.team.id, phase],
        )
        .map_err(storage)?;
    record.state = PhaseState::Draft;
    save_phase(ctx, &record)
}

pub(super) fn problem(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::PublicationTaskProblem {
        publication,
        node,
        reason: message,
    } = ctx.op
    else {
        unreachable!()
    };
    reason(message)?;
    let changed=ctx.db.execute("UPDATE phase_publication_nodes SET reason=?4 WHERE team=?1 AND publication=?2 AND node=?3 ",params![ctx.team.id,publication,node,message]).map_err(storage)?;
    if changed != 1 {
        return Err(invalid("The publication task is no longer pending."));
    }
    Ok(())
}

pub(super) fn publish(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ReconcilePublication { phase, publication } = ctx.op else {
        unreachable!()
    };
    let mut record = phase_record(ctx, *phase)?;
    let p = store::publication::publication(ctx.db, &ctx.team.id, *phase)?.ok_or_else(missing)?;
    if p.id != *publication
        || p.state != "reserved"
        || p.total == 0
        || p.created != p.total
        || p.blocked != 0
    {
        return Err(invalid("Publication has not confirmed every task."));
    }
    if !writing_allowed(ctx.db, ctx.team)? {
        return Err(invalid("Publication is paused or stopped."));
    }
    if record.state != PhaseState::Draft {
        return Err(invalid("The publication phase is no longer a draft."));
    }
    let requests = {
        let mut stmt=ctx.db.prepare("SELECT request FROM phase_publication_nodes WHERE publication=?1 ORDER BY node LIMIT 201").map_err(storage)?;
        let rows = stmt
            .query_map([publication], |r| {
                store::sql_json::<PlannedTaskRequest>(r, 0)
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        rows
    };
    if requests.len() != p.total as usize || requests.len() > 200 {
        return Err(over("Publication exceeds its phase bound."));
    }
    // All bindings and the gate open in one operation transaction. Until it
    // commits no created task is in the live plan, even after a restart.
    for request in &requests {
        let mut n = request.node.node(*phase);
        n.task = Some(request.task.clone());
        n.state = NodeState::Waiting;
        ctx.db
            .execute(
                "INSERT INTO node_identity(team,id,task_project,task_uuid) VALUES(?1,?2,?3,?4)",
                params![ctx.team.id, n.id, request.task.project, request.task.uuid],
            )
            .map_err(storage)?;
        ctx.db
            .execute(
                "INSERT INTO live_tasks(task_project,task_uuid,team,node) VALUES(?1,?2,?3,?4)",
                params![request.task.project, request.task.uuid, ctx.team.id, n.id],
            )
            .map_err(storage)?;
        ctx.db.execute("INSERT INTO nodes(team,id,data,rank,admitted,routed,task_revision,task_stage,task_cancelled,integration_required) VALUES(?1,?2,?3,'',1,1,'','unobserved',0,?4)",params![ctx.team.id,n.id,rows::node_payload(&n)?,ctx.team.settings.integration==Integration::Branch]).map_err(storage)?;
    }
    for request in &requests {
        for d in &request.node.deps {
            ctx.db
                .execute(
                    "INSERT INTO dependencies(team,node,dependency,kind) VALUES(?1,?2,?3,?4)",
                    params![ctx.team.id, request.node.id, d.node, encode(&d.kind)?],
                )
                .map_err(storage)?;
        }
    }
    ctx.db
        .execute(
            "DELETE FROM phase_draft_nodes WHERE team=?1 AND phase=?2",
            params![ctx.team.id, phase],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE phase_publications SET state='published' WHERE id=?1",
            [publication],
        )
        .map_err(storage)?;
    record.state = PhaseState::Approved;
    save_phase(ctx, &record)?;
    if ctx.team.status == TeamStatus::Planning {
        ctx.db
            .execute(
                "UPDATE teams SET status='\"running\"' WHERE id=?1",
                [&ctx.team.id],
            )
            .map_err(storage)?;
    }
    Ok(())
}

pub(crate) fn writing_allowed(db: &Connection, team: &Team) -> Result<bool> {
    Ok(match team.status {
        TeamStatus::Running => true,
        TeamStatus::Planning => store::planning::paused(db, &team.id)? == Some(false),
        _ => false,
    })
}

fn save_phase(ctx: &Context<'_>, record: &Phase) -> Result<()> {
    ctx.db
        .execute(
            "UPDATE phases SET data=?3 WHERE team=?1 AND n=?2",
            params![ctx.team.id, record.n, encode(record)?],
        )
        .map_err(storage)?;
    Ok(())
}

/// The authoritative fact used both by reconciliation and the fresh kernel
/// projection at hand-out. Never infer completion from an empty live queue.
pub(crate) fn complete(db: &Connection, team: &str, phase: u32) -> Result<bool> {
    let Some(p) = store::publication::publication(db, team, phase)? else {
        return Ok(false);
    };
    if p.state != "published" || p.total == 0 || p.created != p.total || p.blocked != 0 {
        return Ok(false);
    }
    let snapshot = store::phase_snapshot::load(db, team, phase, 0)?;
    if !snapshot.complete || snapshot.nodes.is_empty() {
        return Ok(false);
    }
    for n in &snapshot.nodes {
        if !matches!(n.state, NodeState::Accepted | NodeState::Cancelled) || n.hold {
            return Ok(false);
        }
        let pending:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM decisions WHERE team=?1 AND node=?2 AND state<>'settled') OR EXISTS(SELECT 1 FROM effects WHERE team=?1 AND node=?2 AND state IN ('reserved','submitted','uncertain','cutting_over')) OR EXISTS(SELECT 1 FROM stage_tasks WHERE team=?1 AND target=?2 AND state IN ('wanted','reserved'))",params![team,n.id],|r|r.get(0)).map_err(storage)?;
        if pending {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn reconcile(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ReconcilePhase { phase, publication } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let mut record = phase_record(ctx, *phase)?;
    let p = store::publication::publication(ctx.db, &ctx.team.id, *phase)?.ok_or_else(missing)?;
    if p.id != *publication
        || record.kind != PhaseKind::Plan
        || record.state != PhaseState::Approved
        || !complete(ctx.db, &ctx.team.id, *phase)?
    {
        return Err(invalid(
            "The phase is not complete and awaiting reconciliation.",
        ));
    }
    record.state = PhaseState::AwaitingAcceptance;
    save_phase(ctx, &record)
}

pub(super) fn accept(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::AcceptPhase { phase, publication } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let mut record = phase_record(ctx, *phase)?;
    let p = store::publication::publication(ctx.db, &ctx.team.id, *phase)?.ok_or_else(missing)?;
    if p.id != *publication
        || record.kind != PhaseKind::Plan
        || record.state != PhaseState::AwaitingAcceptance
        || !complete(ctx.db, &ctx.team.id, *phase)?
    {
        return Err(invalid(
            "The phase's completion facts no longer permit acceptance.",
        ));
    }
    record.state = PhaseState::Done;
    save_phase(ctx, &record)
}

#[cfg(test)]
pub(crate) mod tests;
