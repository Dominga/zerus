use super::*;

// At most 500 admitted and 100 unadmitted live nodes, one open decision each.
pub(super) const MAX_OPEN_DECISIONS: usize = 600;

pub(super) fn revoke_merges(ctx: &Context<'_>) -> Result<()> {
    for (seq, node) in delegated_accepts(ctx)? {
        ctx.db
            .execute(
                "UPDATE decisions SET state=?2,stopped_ready=1 WHERE seq=?1",
                params![seq, DecisionState::AwaitingOwner],
            )
            .map_err(storage)?;
        talk::card(
            ctx,
            "integration",
            "Approve integration of the accepted result.",
            Some(node),
            Some(seq),
            &["approve", "reject"],
            &[],
        )?;
    }
    Ok(())
}

fn delegated_accepts(ctx: &Context<'_>) -> Result<Vec<(u64, u32)>> {
    // Limit the open set before filtering: a long settled history is never
    // visited, and malformed extra open rows fail closed within the same bound.
    let mut statement = ctx.db.prepare(&revocation_sql()).map_err(storage)?;
    let candidates = statement
        .query_map(
            params![ctx.team.id, DecisionState::Ready, MAX_OPEN_DECISIONS + 1],
            |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            },
        )
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    if candidates.len() > MAX_OPEN_DECISIONS {
        return Err(over("Too many open decisions in the delegation view."));
    }
    Ok(candidates
        .into_iter()
        .filter_map(|(seq, node, eligible)| eligible.then_some((seq, node)))
        .collect())
}

pub(super) fn revocation_sql() -> String {
    format!(
        "SELECT d.seq,d.node,
            d.kind='accept' AND d.state=?2
            AND json_extract(d.actor,'$.kind')<>'person'
            AND NOT EXISTS(SELECT 1 FROM integration_facts f WHERE f.decision=d.seq)
            AND NOT EXISTS(SELECT 1 FROM cards c
                WHERE c.team=d.team AND c.decision_seq=d.seq
                    AND c.kind='integration' AND c.state='closed' AND c.decision='approve'
                    AND json_extract(c.decided_actor,'$.kind')='person')
            AND EXISTS(
                SELECT 1 FROM nodes n WHERE n.team=d.team AND n.id=d.node
                    AND COALESCE(json_extract(n.data,'$.kind'),'task')<>'stage'
                UNION ALL
                SELECT 1 FROM node_archive n WHERE n.team=d.team AND n.id=d.node
                    AND COALESCE(json_extract(n.data,'$.kind'),'task')<>'stage')
         FROM decisions d
         WHERE d.team=?1 AND d.state<>'{}'
         ORDER BY d.node LIMIT ?3",
        DecisionState::Settled.as_str()
    )
}
