//! Person-only dependency edits. The existing operation transaction binds the
//! revision, authority, receipt and journal to the same dependency replacement.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn set(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::SetDeps { node, deps } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    ctx.undecided(*node)?;
    let mut target = ctx.live_node(*node)?;
    if target.kind == NodeKind::Stage {
        return Err(invalid("A pipeline stage's dependencies cannot be edited."));
    }
    if deps.len() > 200 {
        return Err(over("Too many dependencies."));
    }
    let mut seen = BTreeSet::new();
    for dep in deps {
        if dep.node == *node || !seen.insert(dep.node) {
            return Err(invalid("Repeated or self dependency."));
        }
        if rows::node(ctx.db, &ctx.team.id, dep.node)?.is_none() {
            return Err(invalid("Missing dependency."));
        }
    }
    target.deps = deps.clone();
    // Walk only this candidate's ancestors by primary key, including archived
    // nodes. Historical edges can still close a cycle. Bound rows and bytes as
    // readiness does; never scan the team's complete journal or archive.
    let mut nodes = BTreeMap::from([(*node, target)]);
    let mut pending: Vec<_> = deps.iter().map(|d| d.node).collect();
    let mut bytes = 0usize;
    while let Some(id) = pending.pop() {
        if nodes.contains_key(&id) {
            continue;
        }
        let Some(ancestor) = rows::node(ctx.db, &ctx.team.id, id)? else {
            continue;
        };
        bytes += encode(&ancestor)?.len();
        if nodes.len() >= 600 * (200 + 2) || bytes > 8 * 1024 * 1024 {
            return Err(over("Dependency graph exceeds its row or byte bound."));
        }
        pending.extend(ancestor.deps.iter().map(|d| d.node));
        nodes.insert(id, ancestor);
    }
    if !zerus_team_plan::graph::cycles(&nodes.into_values().collect::<Vec<_>>()).is_empty() {
        return Err(Problem::new(code::DEPENDENCY_CYCLE, "Dependency cycle."));
    }
    ctx.db
        .execute(
            "DELETE FROM dependencies WHERE team=?1 AND node=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    for dep in deps {
        ctx.db
            .execute(
                "INSERT INTO dependencies(team,node,dependency,kind) VALUES(?1,?2,?3,?4)",
                params![ctx.team.id, node, dep.node, encode(&dep.kind)?],
            )
            .map_err(storage)?;
    }
    Ok(())
}
