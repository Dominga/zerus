//! Correction proposals have their own episode; the actual phase stays at
//! its acceptance gate until the exact new publication is fully confirmed.
use super::*;
use store::rework::ReworkRow;

fn phase(ctx: &Context<'_>, n: u32) -> Result<Phase> {
    ctx.db
        .query_row(
            "SELECT data FROM phases WHERE team=?1 AND n=?2",
            params![ctx.team.id, n],
            |r| store::sql_json(r, 0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(missing)
}

pub(super) fn acceptance_authority(ctx: &Context<'_>, record: &Phase) -> Result<()> {
    if is_person(ctx.actor)
        || (record.acceptance == Acceptance::Lead
            && ctx.team.settings.delegations.accept_results
            && matches!(ctx.actor,Actor::Session{team,member,..} if team==&ctx.team.id && member==&ctx.team.lead))
    {
        Ok(())
    } else {
        Err(Problem::new(
            code::NOT_ALLOWED,
            "Only the person can decide this phase's hard acceptance gate.",
        ))
    }
}

fn comment(value: &str) -> Result<()> {
    reason(value)?;
    if zerus_team_plan::forbidden_char(&value.replace(['\n', '\t'], "")).is_some() {
        return Err(invalid("The phase comment contains a forbidden character."));
    }
    Ok(())
}

pub(crate) fn controls(a: &Phase, b: &Phase) -> bool {
    a.n == b.n
        && a.order == b.order
        && a.kind == b.kind
        && a.mode == b.mode
        && a.approval == b.approval
        && a.acceptance == b.acceptance
        && a.barrier == b.barrier
        && a.paused == b.paused
}

fn bound(
    ctx: &Context<'_>,
    n: u32,
    base: &str,
    id: &str,
    generation: Option<u64>,
) -> Result<(Phase, ReworkRow)> {
    ctx.active()?;
    uuid(base)?;
    uuid(id)?;
    let record = phase(ctx, n)?;
    let row = store::rework::open(ctx.db, &ctx.team.id, n)?.ok_or_else(missing)?;
    if row.id != id
        || row.base_publication != base
        || record.kind != PhaseKind::Plan
        || record.state != PhaseState::AwaitingAcceptance
    {
        return Err(invalid("That correction episode is no longer current."));
    }
    if let Some(g) = generation {
        if row.generation != Some(g) {
            return Err(stale());
        }
    }
    Ok((record, row))
}

pub(super) fn send_back(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::SendBackPhase {
        phase: n,
        publication: base,
        comment: message,
    } = ctx.op
    else {
        unreachable!()
    };
    ctx.active()?;
    uuid(base)?;
    comment(message)?;
    let record = phase(ctx, *n)?;
    acceptance_authority(ctx, &record)?;
    let p = store::publication::publication(ctx.db, &ctx.team.id, *n)?.ok_or_else(missing)?;
    if record.kind != PhaseKind::Plan
        || record.state != PhaseState::AwaitingAcceptance
        || p.id != *base
        || !publication::complete(ctx.db, &ctx.team.id, *n)?
    {
        return Err(invalid(
            "Only the exact completed waiting publication can be sent back.",
        ));
    }
    if store::phase_snapshot::load(ctx.db, &ctx.team.id, *n, ctx.team.revision)?
        .nodes
        .len()
        >= store::phase_snapshot::MAX_NODES
    {
        return Err(over(
            "Retained phase history leaves no room for an explicit correction.",
        ));
    }
    uuid(ctx.request_id)?;
    store::rework::activate(ctx.db)?;
    ctx.db.execute("INSERT INTO phase_reworks(id,team,phase,base_publication,comment,actor,created_ms,state) VALUES(?1,?2,?3,?4,?5,?6,?7,'drafting')",params![ctx.request_id,ctx.team.id,n,base,message,encode(ctx.actor)?,ctx.now]).map_err(storage)?;
    phase_review::close(ctx, *n, "accept")?;
    ctx.db.execute("INSERT INTO inbox(team,kind,node,generation,actor,text,created_ms) VALUES(?1,?2,0,0,?3,?4,?5)",params![ctx.team.id,format!("phase_sent_back:{}",ctx.request_id),encode(ctx.actor)?,format!("Phase {n} needs explicit corrections: {message}"),ctx.now]).map_err(storage)?;
    Ok(())
}

pub(super) fn write(ctx: &mut Context<'_>) -> Result<()> {
    let (n, base, id, input, wanted) = match ctx.op {
        Operation::SubmitPhaseCorrections {
            phase,
            publication,
            rework,
            draft,
        } => (*phase, publication, rework, draft, None),
        Operation::EditPhaseCorrections {
            phase,
            publication,
            rework,
            generation,
            draft,
        } => (*phase, publication, rework, draft, Some(*generation)),
        _ => unreachable!(),
    };
    let (actual, row) = bound(ctx, n, base, id, wanted)?;
    if row.approval.is_some()
        || (wanted.is_none() && row.state != "drafting")
        || (wanted.is_some() && !matches!(row.state.as_str(), "review" | "rejected"))
    {
        return Err(invalid(
            "Only the current unpublished correction draft can be edited.",
        ));
    }
    if input.barrier != actual.barrier {
        return Err(invalid(
            "A correction draft cannot change the phase barrier or acceptance controls.",
        ));
    }
    let mut header = actual.clone();
    header.title = input.title.clone();
    header.state = PhaseState::Draft;
    let old = if wanted.is_some() {
        store::drafts::nodes(ctx.db, &ctx.team.id, n)?
    } else {
        vec![]
    };
    let next: u32 = ctx
        .db
        .query_row(
            "SELECT next_node FROM teams WHERE id=?1",
            [&ctx.team.id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let next = drafts::validate(ctx, &header, input, &old, next)?;
    let retained:u64=ctx.db.query_row("SELECT (SELECT count(*) FROM nodes WHERE team=?1 AND json_extract(data,'$.phase')=?2)+(SELECT count(*) FROM node_archive WHERE team=?1 AND json_extract(data,'$.phase')=?2)",params![ctx.team.id,n],|r|r.get(0)).map_err(storage)?;
    if retained.saturating_add(input.nodes.len() as u64) > 4096 {
        return Err(over(
            "Corrections and retained phase history exceed the complete snapshot bound.",
        ));
    }
    if let (Some(old_header), Some(g)) = (&row.header, wanted) {
        if !controls(&actual, old_header) {
            return Err(stale());
        }
        phase_review::remember(ctx, old_header, &old, g)?;
    } else {
        ctx.db
            .execute(
                "DELETE FROM phase_draft_previous WHERE team=?1 AND phase=?2",
                params![ctx.team.id, n],
            )
            .map_err(storage)?;
    }
    let current: u64 = ctx
        .db
        .query_row(
            "SELECT generation FROM phase_drafts WHERE team=?1 AND phase=?2",
            params![ctx.team.id, n],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if wanted.is_some_and(|g| g != current) {
        return Err(stale());
    }
    let generation = current
        .checked_add(1)
        .filter(|g| *g <= i64::MAX as u64)
        .ok_or_else(|| over("Draft generations are exhausted."))?;
    for node in &old {
        if !input.nodes.iter().any(|n| n.id == node.id) {
            ctx.db
                .execute(
                    "DELETE FROM phase_draft_nodes WHERE team=?1 AND phase=?2 AND id=?3",
                    params![ctx.team.id, n, node.id],
                )
                .map_err(storage)?;
        }
    }
    for node in &input.nodes {
        ctx.db.execute("INSERT INTO phase_draft_nodes(team,phase,id,data) VALUES(?1,?2,?3,?4) ON CONFLICT(team,id) DO UPDATE SET data=excluded.data",params![ctx.team.id,n,node.id,encode(node)?]).map_err(storage)?;
    }
    phase_review::close_rejection(ctx, n)?;
    ctx.db
        .execute(
            "UPDATE phase_reviews SET rejection=NULL,comment=NULL WHERE team=?1 AND phase=?2",
            params![ctx.team.id, n],
        )
        .map_err(storage)?;
    ctx.db.execute("UPDATE phase_drafts SET generation=?3,actor=?4,updated_ms=?5 WHERE team=?1 AND phase=?2",params![ctx.team.id,n,generation,encode(ctx.actor)?,ctx.now]).map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE phase_reworks SET state='review',generation=?2,header=?3 WHERE id=?1",
            params![id, generation, encode(&header)?],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE teams SET next_node=?2 WHERE id=?1",
            params![ctx.team.id, next],
        )
        .map_err(storage)?;
    Ok(())
}

pub(super) fn approve(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ApprovePhaseCorrections {
        phase: n,
        publication: base,
        rework: id,
        generation,
    } = ctx.op
    else {
        unreachable!()
    };
    let (actual, row) = bound(ctx, *n, base, id, Some(*generation))?;
    let header = row.header.as_ref().ok_or_else(missing)?;
    if row.state != "review"
        || row.approval.is_some()
        || !controls(&actual, header)
        || !publication::complete_facts(ctx.db, &ctx.team.id, *n, None)?
    {
        return Err(invalid(
            "The exact correction draft is not ready for approval.",
        ));
    }
    if matches!(ctx.actor, Actor::Service { .. })
        && (ctx.team.status != TeamStatus::Running
            || header.approval != Approval::Auto
            || ctx.team.settings.plan_approval != Approval::Auto)
    {
        return Err(invalid("Automatic correction approval is not enabled."));
    }
    let nodes = store::drafts::nodes(ctx.db, &ctx.team.id, *n)?;
    drafts::revalidate(ctx, header, &nodes)?;
    publication::reserve(ctx, *n, *generation, false, &nodes)?;
    ctx.db
        .execute(
            "UPDATE phase_reworks SET state='publishing',approval=?2 WHERE id=?1",
            params![id, ctx.request_id],
        )
        .map_err(storage)?;
    Ok(())
}

pub(super) fn reject(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::RejectPhaseCorrections {
        phase: n,
        publication: base,
        rework: id,
        generation,
        comment: message,
    } = ctx.op
    else {
        unreachable!()
    };
    let (_, row) = bound(ctx, *n, base, id, Some(*generation))?;
    if row.state != "review" || row.approval.is_some() {
        return Err(invalid(
            "Only an unpublished correction draft can be rejected.",
        ));
    }
    comment(message)?;
    uuid(ctx.request_id)?;
    let mut header = row.header.ok_or_else(missing)?;
    header.state = PhaseState::Rejected;
    phase_review::close(ctx, *n, "review")?;
    ctx.db.execute("INSERT INTO phase_reviews(team,phase,generation,rejection,comment,actor) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(team,phase) DO UPDATE SET generation=excluded.generation,rejection=excluded.rejection,comment=excluded.comment,actor=excluded.actor",params![ctx.team.id,n,generation,ctx.request_id,message,encode(ctx.actor)?]).map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE phase_reworks SET state='rejected',header=?2 WHERE id=?1",
            params![id, encode(&header)?],
        )
        .map_err(storage)?;
    ctx.db.execute("INSERT INTO inbox(team,kind,node,generation,actor,text,created_ms) VALUES(?1,?2,0,0,?3,?4,?5)",params![ctx.team.id,format!("phase_rejected:{}",ctx.request_id),encode(ctx.actor)?,format!("Phase {n} corrections were rejected: {message}"),ctx.now]).map_err(storage)?;
    Ok(())
}

pub(super) fn undo(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::UndoPhaseCorrectionRejection {
        phase: n,
        publication: base,
        rework: id,
        generation,
        rejection,
    } = ctx.op
    else {
        unreachable!()
    };
    let (_, row) = bound(ctx, *n, base, id, Some(*generation))?;
    uuid(rejection)?;
    let saved = store::publication::rejection(ctx.db, &ctx.team.id, *n)?.ok_or_else(missing)?;
    if row.state != "rejected"
        || row.approval.is_some()
        || saved.id != *rejection
        || saved.generation != *generation
    {
        return Err(invalid("That correction rejection is no longer current."));
    }
    let mut header = row.header.ok_or_else(missing)?;
    header.state = PhaseState::Draft;
    phase_review::close_rejection(ctx, *n)?;
    ctx.db
        .execute(
            "UPDATE phase_reviews SET rejection=NULL,comment=NULL WHERE team=?1 AND phase=?2",
            params![ctx.team.id, n],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE phase_reworks SET state='review',header=?2 WHERE id=?1",
            params![id, encode(&header)?],
        )
        .map_err(storage)?;
    Ok(())
}

pub(super) fn review(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ReconcileCorrectionReview {
        phase: n,
        publication: base,
        rework: id,
        generation,
    } = ctx.op
    else {
        unreachable!()
    };
    let (_, row) = bound(ctx, *n, base, id, Some(*generation))?;
    let old: Option<u64> = ctx
        .db
        .query_row(
            "SELECT generation FROM phase_cards WHERE team=?1 AND phase=?2 AND purpose='review'",
            params![ctx.team.id, n],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if row.state != "review" || row.approval.is_some() || old == Some(*generation) {
        return Err(invalid(
            "That correction generation has no new review episode.",
        ));
    }
    phase_review::close(ctx, *n, "review")?;
    phase_review::card(ctx, &row.header.ok_or_else(missing)?, *generation, "review")
}

pub(super) fn promotion(ctx: &Context<'_>, record: &Phase, publication: &str) -> Result<Phase> {
    let row = store::rework::open(ctx.db, &ctx.team.id, record.n)?.ok_or_else(missing)?;
    let header = row.header.ok_or_else(missing)?;
    if record.state != PhaseState::AwaitingAcceptance
        || row.state != "publishing"
        || row.approval.as_deref() != Some(publication)
        || !controls(record, &header)
        || !publication::complete_facts(ctx.db, &ctx.team.id, record.n, Some(publication))?
    {
        return Err(invalid(
            "The correction publication no longer matches its retained phase gate.",
        ));
    }
    let mut result = record.clone();
    result.title = header.title;
    result.state = PhaseState::Approved;
    Ok(result)
}
