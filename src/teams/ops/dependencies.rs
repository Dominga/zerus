//! Person-only dependency edits. The existing operation transaction binds the
//! revision, authority, receipt and journal to the same dependency replacement.
use super::*;
use std::collections::BTreeSet;

pub(super) fn set(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::SetDeps { node, deps } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    ctx.undecided(*node)?;
    let target = ctx.live_node(*node)?;
    if target.source == NodeSource::Planned {
        return Err(invalid("Approved plan changes require a logged plan edit."));
    }
    if target.kind == NodeKind::Stage {
        return Err(invalid("A pipeline stage's dependencies cannot be edited."));
    }
    if !store::dependency_edit::editable(ctx.db, &ctx.team.id, *node)? {
        return Err(invalid(store::dependency_edit::PRESTART_CAUSE));
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
    dependency_graph::check(ctx, *node, deps)?;
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
