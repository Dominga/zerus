//! One previous draft and one linked owner card per review episode. Neither
//! creates task identities, nor stores parameters inside a saved launch plan.
use super::*;
use zerus_team_plan::draft::{DraftInput, DraftNode};

pub(super) fn remember(
    ctx: &Context<'_>,
    record: &Phase,
    nodes: &[DraftNode],
    generation: u64,
) -> Result<()> {
    let input = DraftInput {
        title: record.title.clone(),
        barrier: record.barrier,
        nodes: nodes.to_vec(),
    };
    ctx.db.execute("INSERT INTO phase_draft_previous(team,phase,generation,draft) VALUES(?1,?2,?3,?4) ON CONFLICT(team,phase) DO UPDATE SET generation=excluded.generation,draft=excluded.draft",params![ctx.team.id,record.n,generation,encode(&input)?]).map_err(storage)?;
    close(ctx, record.n, "review")
}

pub(super) fn close_rejection(ctx: &Context<'_>, phase: u32) -> Result<()> {
    if let Some(rejection) = store::publication::rejection(ctx.db, &ctx.team.id, phase)? {
        ctx.db
            .execute(
                "UPDATE inbox SET state='closed' WHERE team=?1 AND kind=?2 AND state='open'",
                params![ctx.team.id, format!("phase_rejected:{}", rejection.id)],
            )
            .map_err(storage)?;
    }
    Ok(())
}

pub(super) fn close(ctx: &Context<'_>, phase: u32, purpose: &str) -> Result<()> {
    ctx.db.execute("UPDATE cards SET state='closed' WHERE team=?1 AND seq IN (SELECT card FROM phase_cards WHERE team=?1 AND phase=?2 AND purpose=?3) AND state='open'",params![ctx.team.id,phase,purpose]).map_err(storage)?;
    Ok(())
}

pub(super) fn card(ctx: &Context<'_>, phase: &Phase, generation: u64, purpose: &str) -> Result<()> {
    let body = if purpose == "accept" {
        format!("Phase {} ({}) is complete and awaits your acceptance. Open the planned phase to review its result.",phase.n,phase.title)
    } else {
        format!("Phase {} ({}) has a draft ready for your review. Open the planned phase to approve or reject it.",phase.n,phase.title)
    };
    let card = talk::card(
        ctx,
        if purpose == "accept" {
            "phase_acceptance"
        } else {
            "phase_review"
        },
        &body,
        None,
        None,
        &["acknowledged"],
        &[],
    )?;
    ctx.db.execute("INSERT INTO phase_cards(team,phase,generation,purpose,card) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(team,phase,purpose) DO UPDATE SET generation=excluded.generation,card=excluded.card",params![ctx.team.id,phase.n,generation,purpose,card]).map_err(storage)?;
    Ok(())
}

pub(super) fn review(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ReconcilePhaseReview { phase, generation } = ctx.op else {
        unreachable!()
    };
    if !matches!(
        ctx.team.status,
        TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused
    ) {
        return Err(invalid("The team is not active."));
    }
    let record: Phase = ctx
        .db
        .query_row(
            "SELECT data FROM phases WHERE team=?1 AND n=?2",
            params![ctx.team.id, phase],
            |r| store::sql_json(r, 0),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(missing)?;
    let actual: Option<u64> = ctx
        .db
        .query_row(
            "SELECT generation FROM phase_drafts WHERE team=?1 AND phase=?2",
            params![ctx.team.id, phase],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let old: Option<u64> = ctx
        .db
        .query_row(
            "SELECT generation FROM phase_cards WHERE team=?1 AND phase=?2 AND purpose='review'",
            params![ctx.team.id, phase],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if record.kind != PhaseKind::Plan
        || record.state != PhaseState::Draft
        || actual != Some(*generation)
        || old == actual
        || store::publication::publication(ctx.db, &ctx.team.id, *phase)?.is_some()
    {
        return Err(invalid("This draft is not a new review episode."));
    }
    close(ctx, *phase, "review")?;
    card(ctx, &record, *generation, "review")
}
