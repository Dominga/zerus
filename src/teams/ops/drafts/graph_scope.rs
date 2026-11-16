//! Once a phase is published its dependencies still constrain later draft
//! edits. Keep live nodes and only the archived ancestors they actually name.
use super::*;

pub(super) fn extend(
    ctx: &Context<'_>,
    graph: &mut Vec<Node>,
    candidate: &DraftInput,
) -> Result<()> {
    let mut nodes: BTreeMap<_, _> = graph.drain(..).map(|n| (n.id, n)).collect();
    let mut stmt = ctx
        .db
        .prepare(&format!(
            "SELECT {} FROM nodes WHERE team=?1 ORDER BY id LIMIT 601",
            rows::node_select("nodes")
        ))
        .map_err(storage)?;
    let mut count = 0;
    for row in stmt
        .query_map([&ctx.team.id], rows::node_row)
        .map_err(storage)?
    {
        let mut n = row.map_err(storage)?;
        count += 1;
        if count > 600 {
            return Err(over(
                "The live draft-validation scope exceeds its row bound.",
            ));
        }
        n.deps = rows::deps(ctx.db, &ctx.team.id, n.id)?;
        if nodes.insert(n.id, n).is_some() {
            return Err(invalid("A node has both a live and a draft identity."));
        }
    }
    let mut pending: Vec<_> = nodes
        .values()
        .flat_map(|n| n.deps.iter().map(|d| d.node))
        .collect();
    pending.extend(
        candidate
            .nodes
            .iter()
            .flat_map(|n| n.deps.iter().map(|d| d.node)),
    );
    let mut bytes = nodes.values().try_fold(0usize, |size, n| {
        encode(n).map(|s| size.saturating_add(s.len()))
    })?;
    while let Some(id) = pending.pop() {
        if nodes.contains_key(&id) {
            continue;
        }
        if let Some(n) = rows::node(ctx.db, &ctx.team.id, id)? {
            bytes = bytes.saturating_add(encode(&n)?.len());
            if bytes > 8 * 1024 * 1024 || nodes.len() >= 600 * (200 + 2) {
                return Err(over(
                    "Draft ancestor validation exceeds its row or byte bound.",
                ));
            }
            pending.extend(n.deps.iter().map(|d| d.node));
            nodes.insert(id, n);
        }
        // A dependency into this candidate still lives in its draft input;
        // the caller installs it before the cycle and write-set checks.
    }
    if bytes > 8 * 1024 * 1024 {
        return Err(over("Draft validation exceeds its byte bound."));
    }
    graph.extend(nodes.into_values());
    Ok(())
}
