use super::*;

pub(super) enum FactKind {
    Merge,
    Conflict,
}

/// Facts grant no write authority. A merge may have happened before revocation;
/// a person's rejection keeps that receipt as provenance only.
pub(super) struct Permit {
    pub decision: u64,
    pub late: bool,
    pub generation: u32,
    pub provenance_only: bool,
    awaiting_owner: bool,
    card: Option<u64>,
}
pub(super) fn permit(
    ctx: &Context<'_>,
    node: u32,
    decision: u64,
    kind: FactKind,
) -> Result<Permit> {
    let (generation, state, preserved): (u32, DecisionState, bool) = ctx.db
        .query_row(
            "SELECT generation,state,stopped_ready FROM decisions d WHERE seq=?1 AND team=?2 AND node=?3 AND kind='accept' AND NOT EXISTS(SELECT 1 FROM integration_facts f WHERE f.decision=d.seq)",
            params![decision, ctx.team.id, node],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage)?
        .ok_or_else(|| invalid("The named decision has no unrecorded integration authority."))?;
    let card = integration_card(ctx, decision)?;
    let rejected = card.as_ref().is_some_and(|c| c.rejected);
    let merge = matches!(kind, FactKind::Merge);
    let awaiting_owner = merge && state == DecisionState::AwaitingOwner && preserved;
    let late = state == DecisionState::Settled && preserved;
    if !(state == DecisionState::Ready || awaiting_owner || late) || (!merge && rejected) {
        return Err(invalid(
            "The named decision has no unrecorded integration authority.",
        ));
    }
    Ok(Permit {
        decision,
        generation,
        late,
        provenance_only: state == DecisionState::Settled && rejected,
        awaiting_owner,
        card: card.filter(|c| c.open).map(|c| c.seq),
    })
}

pub(super) const INTEGRATION_CARD_SQL: &str = "SELECT seq,state,decision,COALESCE(json_extract(decided_actor,'$.kind')='person',0) FROM cards WHERE decision_seq=?1 AND team=?2 AND kind='integration' LIMIT 1";
struct IntegrationCard {
    seq: u64,
    open: bool,
    rejected: bool,
}
fn integration_card(ctx: &Context<'_>, decision: u64) -> Result<Option<IntegrationCard>> {
    ctx.db
        .query_row(INTEGRATION_CARD_SQL, params![decision, ctx.team.id], |r| {
            let state: String = r.get(1)?;
            let answer: Option<String> = r.get(2)?;
            Ok(IntegrationCard {
                seq: r.get(0)?,
                open: state == "open",
                rejected: state == "closed"
                    && answer.as_deref() == Some("reject")
                    && r.get::<_, bool>(3)?,
            })
        })
        .optional()
        .map_err(storage)
}

pub(super) fn complete_merge(ctx: &Context<'_>, permit: &Permit) -> Result<()> {
    if !permit.awaiting_owner {
        return Ok(());
    }
    ctx.db
        .execute(
            "UPDATE decisions SET state=?2 WHERE seq=?1",
            params![permit.decision, DecisionState::Ready],
        )
        .map_err(storage)?;
    if let Some(card) = permit.card {
        ctx.db.execute("UPDATE cards SET state='closed',decision='stale',decided_actor=?3 WHERE seq=?1 AND team=?2 AND state='open'",params![card,ctx.team.id,encode(ctx.actor)?]).map_err(storage)?;
    }
    Ok(())
}
pub(super) fn record(
    ctx: &Context<'_>,
    permit: &Permit,
    node: u32,
    commit: Option<&str>,
    conflicts: &[String],
) -> Result<()> {
    ctx.db.execute("INSERT INTO integration_facts(decision,team,node,actor,commit_id,conflicts,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![permit.decision,ctx.team.id,node,encode(ctx.actor)?,commit,encode(&conflicts)?,ctx.now]).map_err(storage)?;
    Ok(())
}
