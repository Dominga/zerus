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

fn review_id(ctx: &Context<'_>) -> Result<String> {
    if uuid::Uuid::parse_str(ctx.request_id).is_ok_and(|id| !id.is_nil()) {
        Ok(ctx.request_id.to_owned())
    } else {
        Err(invalid("A phase review needs a non-nil request UUID."))
    }
}

/// A versioned, domain-separated UUIDv8 derived from the already unique
/// approval identity. It is saved once; recovery always uses the saved input.
fn task_id(team: &str, publication: &str, node: u32) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"zerus-team-phase-task-v1\0");
    hash.update(team.as_bytes());
    hash.update(b"\0");
    hash.update(publication.as_bytes());
    hash.update(node.to_be_bytes());
    let bytes: [u8; 16] = hash.finalize()[..16]
        .try_into()
        .expect("fixed SHA-256 prefix");
    uuid::Builder::from_custom_bytes(bytes)
        .into_uuid()
        .to_string()
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
    let nodes = store::drafts::nodes(ctx.db, &ctx.team.id, *phase)?;
    if nodes.is_empty() {
        return Err(invalid("An empty phase cannot be approved."));
    }
    drafts::revalidate(ctx, &record, &nodes)?;
    reserve(ctx, *phase, *generation, initial, &nodes)
}

pub(super) fn reserve(
    ctx: &Context<'_>,
    phase: u32,
    generation: u64,
    initial: bool,
    nodes: &[zerus_team_plan::draft::DraftNode],
) -> Result<()> {
    let lead = ctx
        .member(&ctx.team.lead)?
        .session
        .filter(|s| s.machine == ctx.team.home && !s.session_id.is_empty())
        .ok_or_else(|| invalid("The lead needs its local session binding before approval."))?;
    phase_review::close(ctx, phase, "review")?;
    let id = review_id(ctx)?;
    store::raise_execution_floor(ctx.db, PUBLICATION_LAYOUT)?;
    ctx.db.execute("INSERT INTO phase_publications(id,team,phase,generation,snapshot,initial,state,created_ms) VALUES(?1,?2,?3,?4,?5,?6,'reserved',?7)",params![id,ctx.team.id,phase,generation,encode(&nodes)?,initial,ctx.now]).map_err(storage)?;
    for node in nodes.iter().cloned() {
        let request = PlannedTaskRequest {
            publication: id.clone(),
            team: ctx.team.id.clone(),
            phase,
            task: TaskRef {
                project: ctx.team.project.clone(),
                uuid: task_id(&ctx.team.id, &id, node.id),
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
    phase_review::close(ctx, *phase, "review")?;
    let id = review_id(ctx)?;
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
    phase_review::close_rejection(ctx, *phase)?;
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
        kind,
        clear,
    } = ctx.op
    else {
        unreachable!()
    };
    reason(message)?;
    text(message, 2048)?;
    let (state,failures):(String,u32)=ctx.db.query_row("SELECT state,CASE WHEN ?4 THEN clear_failures ELSE failures END FROM phase_publication_nodes WHERE team=?1 AND publication=?2 AND node=?3",params![ctx.team.id,publication,node,clear],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?.ok_or_else(missing)?;
    let terminal = matches!(ctx.team.status, TeamStatus::Stopped | TeamStatus::Archived);
    if *clear && !terminal {
        return Err(invalid("Only a stopped publication can report cleanup."));
    }
    let failures = if *kind == PublicationProblemKind::Uncertain {
        failures.saturating_add(1).min(3)
    } else {
        failures
    };
    // A known non-write caused by Pause can resume with this same request.
    // A stopped team's creation answer is not a cleanup answer, even when
    // it arrives after Stop committed.
    let paused = !*clear
        && !terminal
        && *kind == PublicationProblemKind::Refused
        && !writing_allowed(ctx.db, ctx.team)?;
    let final_state = match kind {
        _ if paused => None,
        PublicationProblemKind::Refused => Some("refused"),
        PublicationProblemKind::Uncertain if failures >= 3 => Some("uncertain"),
        _ => None,
    };
    let next = if !*clear && matches!(state.as_str(), "pending" | "writing" | "created") {
        final_state.unwrap_or(&state)
    } else {
        &state
    };
    ctx.db.execute("UPDATE phase_publication_nodes SET state=?4,reason=?5,failures=CASE WHEN ?6 THEN failures ELSE ?7 END,clear_failures=CASE WHEN ?6 THEN ?7 ELSE clear_failures END,clear_outcome=CASE WHEN ?6 THEN COALESCE(clear_outcome,?8) ELSE clear_outcome END WHERE team=?1 AND publication=?2 AND node=?3",params![ctx.team.id,publication,node,next,message,clear,failures,final_state]).map_err(storage)?;
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
        record = rework::promotion(ctx, &record, publication)?;
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
        n.text.clear();
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
    // Promote incoming edges while both the new confirmed identity and its
    // draft row exist. No transaction exposes a missing prerequisite.
    ctx.db.execute("INSERT INTO dependencies(team,node,dependency,kind) SELECT d.team,d.node,d.dependency,d.kind FROM phase_draft_dependencies d JOIN phase_publication_nodes n ON n.team=d.team AND n.node=d.dependency WHERE n.publication=?1",[publication]).map_err(storage)?;
    ctx.db.execute("DELETE FROM phase_draft_dependencies WHERE team=?1 AND dependency IN (SELECT node FROM phase_publication_nodes WHERE publication=?2)",params![ctx.team.id,publication]).map_err(storage)?;
    for request in &requests {
        for d in &request.node.deps {
            let confirmed: bool = ctx
                .db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM node_identity WHERE team=?1 AND id=?2)",
                    params![ctx.team.id, d.node],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            let table = if confirmed {
                "dependencies"
            } else {
                "phase_draft_dependencies"
            };
            ctx.db
                .execute(
                    &format!("INSERT INTO {table}(team,node,dependency,kind) VALUES(?1,?2,?3,?4)"),
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
    if let Some(row) = store::rework::open(ctx.db, &ctx.team.id, *phase)? {
        if row.approval.as_deref() != Some(publication) {
            return Err(invalid("The correction publication identity changed."));
        }
        ctx.db
            .execute(
                "UPDATE phase_reworks SET state='published' WHERE id=?1 AND approval=?2",
                params![row.id, publication],
            )
            .map_err(storage)?;
    }
    record.state = PhaseState::Approved;
    save_phase(ctx, &record)?;
    if ctx.team.status == TeamStatus::Planning {
        ctx.db
            .execute(
                "UPDATE teams SET status='\"running\"' WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM phase_publications p WHERE p.team=teams.id AND p.initial=1 AND p.state<>'published')",
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
    if store::rework::open(db, team, phase)?.is_some() {
        return Ok(false);
    }
    complete_facts(db, team, phase, None)
}

pub(super) fn complete_facts(
    db: &Connection,
    team: &str,
    phase: u32,
    excluded: Option<&str>,
) -> Result<bool> {
    let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM phase_publications WHERE team=?1 AND phase=?2 AND (?3 IS NULL OR id<>?3))",params![team,phase,excluded],|r|r.get(0)).map_err(storage)?;
    if !exists {
        return Ok(false);
    }
    let snapshot = store::phase_snapshot::load_except(db, team, phase, 0, excluded)?;
    if !snapshot.complete || snapshot.nodes.is_empty() {
        return Ok(false);
    }
    for n in &snapshot.nodes {
        if !matches!(n.state, NodeState::Accepted | NodeState::Cancelled)
            && !retired_uncreated_stage(db, team, n)?
        {
            return Ok(false);
        }
        let pending:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM decisions WHERE team=?1 AND node=?2 AND state<>'settled') OR EXISTS(SELECT 1 FROM effects WHERE team=?1 AND node=?2 AND state IN ('reserved','submitted','uncertain','cutting_over')) OR EXISTS(SELECT 1 FROM stage_tasks WHERE team=?1 AND target=?2 AND state IN ('wanted','reserved'))",params![team,n.id],|r|r.get(0)).map_err(storage)?;
        if pending {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A pipeline can retire a wanted stage before any task is bound. Its
/// archived Waiting row is not a completion fact by itself: require the
/// positive superseded lifecycle record and no task binding.
fn retired_uncreated_stage(db: &Connection, team: &str, node: &Node) -> Result<bool> {
    if node.kind != NodeKind::Stage || node.task.is_some() {
        return Ok(false);
    }
    db.query_row("SELECT EXISTS(SELECT 1 FROM node_archive a JOIN stage_tasks s ON s.team=a.team AND s.node=a.id WHERE a.team=?1 AND a.id=?2 AND s.state='superseded') AND NOT EXISTS(SELECT 1 FROM live_tasks WHERE team=?1 AND node=?2)",params![team,node.id],|r|r.get(0)).map_err(storage)
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
    phase_review::card(ctx, &record, p.generation, "accept")?;
    save_phase(ctx, &record)
}

pub(super) fn accept(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::AcceptPhase { phase, publication } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let mut record = phase_record(ctx, *phase)?;
    rework::acceptance_authority(ctx, &record)?;
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
    phase_review::close(ctx, *phase, "accept")?;
    record.state = PhaseState::Done;
    save_phase(ctx, &record)
}

#[cfg(test)]
pub(crate) mod tests;
