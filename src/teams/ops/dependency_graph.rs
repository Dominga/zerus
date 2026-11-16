//! Cycle checks follow actual live/archive rows and referenced drafts. The
//! scheduler still treats an unpublished prerequisite as missing work.
use super::*;
use std::collections::BTreeMap;
use zerus_team_plan::draft::DraftNode;

pub(super) fn check(ctx: &Context<'_>, node: u32, deps: &[Dep]) -> Result<()> {
    let mut target = rows::node(ctx.db, &ctx.team.id, node)?.ok_or_else(missing)?;
    target.deps = deps.to_vec();
    let mut nodes = BTreeMap::from([(node, target)]);
    let mut pending: Vec<_> = deps.iter().map(|d| d.node).collect();
    let mut bytes = 0usize;
    while let Some(id) = pending.pop() {
        if nodes.contains_key(&id) {
            continue;
        }
        let ancestor = match rows::node(ctx.db, &ctx.team.id, id)? {
            Some(node) => Some(node),
            None => draft(ctx, id)?,
        };
        let Some(ancestor) = ancestor else {
            continue;
        };
        bytes = bytes.saturating_add(encode(&ancestor)?.len());
        if nodes.len() >= 600 * (200 + 2) || bytes > 8 * 1024 * 1024 {
            return Err(over("Dependency graph exceeds its row or byte bound."));
        }
        pending.extend(ancestor.deps.iter().map(|d| d.node));
        nodes.insert(id, ancestor);
    }
    if !zerus_team_plan::graph::cycles(&nodes.into_values().collect::<Vec<_>>()).is_empty() {
        return Err(Problem::new(code::DEPENDENCY_CYCLE, "Dependency cycle."));
    }
    Ok(())
}

fn draft(ctx: &Context<'_>, id: u32) -> Result<Option<Node>> {
    let row: Option<(u32, Option<String>)> = ctx.db.query_row(
        "SELECT phase,CASE WHEN length(CAST(data AS BLOB))<=262144 THEN data ELSE NULL END FROM phase_draft_nodes WHERE team=?1 AND id=?2",
        params![ctx.team.id,id],|r| Ok((r.get(0)?,r.get(1)?)),
    ).optional().map_err(storage)?;
    row.map(|(phase, data)| {
        let data = data.ok_or_else(|| over("A draft ancestor exceeds its byte bound."))?;
        let node: DraftNode = store::decode(&data)?;
        Ok(node.node(phase))
    })
    .transpose()
}
