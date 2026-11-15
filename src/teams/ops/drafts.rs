use super::*;
use std::collections::{BTreeMap, BTreeSet};
use zerus_team_plan::draft::{DraftInput, DraftNode};

pub(super) fn write(ctx: &mut Context<'_>) -> Result<()> {
    if !matches!(
        ctx.team.status,
        TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused
    ) {
        return Err(invalid("Only an active team can edit a draft."));
    }
    let (phase, draft, generation) = match ctx.op {
        Operation::SubmitDraftPhase { phase, draft } => (*phase, draft, None),
        Operation::EditDraftPhase {
            phase,
            generation,
            draft,
        } => (*phase, draft, Some(*generation)),
        _ => unreachable!(),
    };
    let old: Option<Phase> = ctx
        .db
        .query_row(
            "SELECT data FROM phases WHERE team=?1 AND n=?2",
            params![ctx.team.id, phase],
            |r| store::sql_json(r, 0),
        )
        .optional()
        .map_err(storage)?;
    let old_nodes = if let Some(wanted) = generation {
        let p = old.as_ref().ok_or_else(missing)?;
        if p.kind != PhaseKind::Plan || !matches!(p.state, PhaseState::Draft | PhaseState::Rejected) {
            return Err(invalid("Only a draft or rejected planned phase can be edited."));
        }
        if store::publication::publication(ctx.db, &ctx.team.id, phase)?.is_some() {
            return Err(invalid("Publication has frozen this draft."));
        }
        let actual: Option<u64> = ctx
            .db
            .query_row(
                "SELECT generation FROM phase_drafts WHERE team=?1 AND phase=?2",
                params![ctx.team.id, phase],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if actual != Some(wanted) {
            return Err(stale());
        }
        store::drafts::nodes(ctx.db, &ctx.team.id, phase)?
    } else {
        if phase == 0 || old.is_some() || store::drafts::next_phase(ctx.db, &ctx.team.id)? != phase
        {
            return Err(invalid(
                "Use the next planned phase number from the phases read.",
            ));
        }
        let count: u32 = ctx
            .db
            .query_row(
                "SELECT count(*) FROM phases WHERE team=?1",
                [&ctx.team.id],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if count >= 200 {
            return Err(over("The team has reached its phase bound."));
        }
        Vec::new()
    };
    let mut record = old.unwrap_or(Phase {
        n: phase,
        order: phase,
        title: String::new(),
        kind: PhaseKind::Plan,
        mode: PhaseMode::Standard,
        approval: ctx.team.settings.plan_approval,
        state: PhaseState::Draft,
        acceptance: ctx.team.settings.phase_acceptance,
        barrier: false,
        paused: false,
    });
    record.state = PhaseState::Draft;
    record.title = draft.title.clone();
    record.barrier = draft.barrier;
    let next_node: u32 = ctx
        .db
        .query_row(
            "SELECT next_node FROM teams WHERE id=?1",
            [&ctx.team.id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let next_node = validate(ctx, &record, draft, &old_nodes, next_node)?;
    let next_generation = generation
        .unwrap_or(0)
        .checked_add(1)
        .filter(|g| *g <= i64::MAX as u64)
        .ok_or_else(|| over("Draft generations are exhausted."))?;
    if generation.is_none() {
        let next = phase
            .checked_add(1)
            .ok_or_else(|| over("Phase numbers are exhausted."))?;
        ctx.db
            .execute(
                "INSERT INTO phases(team,n,data) VALUES(?1,?2,?3)",
                params![ctx.team.id, phase, encode(&record)?],
            )
            .map_err(storage)?;
        ctx.db.execute("INSERT INTO phase_draft_counters(team,next_phase) VALUES(?1,?2) ON CONFLICT(team) DO UPDATE SET next_phase=excluded.next_phase",params![ctx.team.id,next]).map_err(storage)?;
        ctx.db.execute("INSERT INTO phase_drafts(team,phase,generation,actor,updated_ms) VALUES(?1,?2,?3,?4,?5)",params![ctx.team.id,phase,next_generation,encode(ctx.actor)?,ctx.now]).map_err(storage)?;
    } else {
        ctx.db
            .execute(
                "UPDATE phases SET data=?3 WHERE team=?1 AND n=?2",
                params![ctx.team.id, phase, encode(&record)?],
            )
            .map_err(storage)?;
        ctx.db.execute("UPDATE phase_drafts SET generation=?3,actor=?4,updated_ms=?5 WHERE team=?1 AND phase=?2",params![ctx.team.id,phase,next_generation,encode(ctx.actor)?,ctx.now]).map_err(storage)?;
        ctx.db
            .execute(
                "DELETE FROM phase_draft_nodes WHERE team=?1 AND phase=?2",
                params![ctx.team.id, phase],
            )
            .map_err(storage)?;
    }
    for node in &draft.nodes {
        ctx.db
            .execute(
                "INSERT INTO phase_draft_nodes(team,phase,id,data) VALUES(?1,?2,?3,?4)",
                params![ctx.team.id, phase, node.id, encode(node)?],
            )
            .map_err(storage)?;
    }
    ctx.db.execute("UPDATE phase_reviews SET rejection=NULL,comment=NULL WHERE team=?1 AND phase=?2",params![ctx.team.id,phase]).map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE teams SET next_node=?2 WHERE id=?1",
            params![ctx.team.id, next_node],
        )
        .map_err(storage)?;
    ctx.db.execute("UPDATE inbox SET state='closed' WHERE team=?1 AND kind='planning_goal' AND state='open'", [&ctx.team.id]).map_err(storage)?;
    Ok(())
}

fn named(value: &str, field: &str, bytes: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(&format!("{field} is required.")));
    }
    if let Some(p) = zerus_team_plan::name_problem(value, field, bytes) {
        return Err(p);
    }
    Ok(())
}

fn validate(
    ctx: &Context<'_>,
    phase: &Phase,
    input: &DraftInput,
    old: &[DraftNode],
    mut next: u32,
) -> Result<u32> {
    named(&input.title, "The phase title", 200)?;
    if input.nodes.is_empty()
        || input.nodes.len() > ctx.team.settings.caps.max_phase_tasks as usize
        || input.nodes.len() > 200
    {
        return Err(over(
            "A draft must contain 1 to the configured phase cap tasks.",
        ));
    }
    let total = ctx
        .task_capacity_used()?
        .saturating_sub(old.len() as u32)
        .saturating_add(input.nodes.len() as u32);
    if total > ctx.team.settings.caps.max_open_tasks {
        return Err(over("Drafts and admitted tasks exceed the team task cap."));
    }
    let old_ids: BTreeSet<_> = old.iter().map(|n| n.id).collect();
    let ids: BTreeSet<_> = input.nodes.iter().map(|n| n.id).collect();
    if ids.len() != input.nodes.len() || ids.contains(&0) {
        return Err(invalid("Draft node IDs must be positive and unique."));
    }
    for id in ids.difference(&old_ids) {
        if *id != next {
            return Err(invalid(
                "New draft nodes must use consecutive next_node IDs from the phases read.",
            ));
        }
        next = next
            .checked_add(1)
            .ok_or_else(|| over("Node IDs are exhausted."))?;
    }
    let mut graph = other_drafts(ctx, phase.n)?;
    let removed: BTreeSet<_> = old_ids.difference(&ids).copied().collect();
    if graph
        .iter()
        .any(|n| n.deps.iter().any(|d| removed.contains(&d.node)))
    {
        return Err(invalid("Another draft depends on a node being removed."));
    }
    let others: BTreeMap<_, _> = graph.iter().map(|n| (n.id, n.phase)).collect();
    for n in &input.nodes {
        named(&n.title, &format!("Task {} title", n.id), 200)?;
        text(&n.text, 16_384)?;
        let plain = n.text.replace(['\n', '\t'], "");
        if zerus_team_plan::forbidden_char(&plain).is_some() {
            return Err(invalid("The draft text contains a forbidden character."));
        }
        if n.criteria.is_empty() || n.criteria.len() > 32 {
            return Err(invalid(
                "Every draft task needs 1 to 32 acceptance criteria.",
            ));
        }
        for c in &n.criteria {
            named(c, "The acceptance criterion", 2048)?;
        }
        if n.kind == NodeKind::Stage {
            return Err(invalid("Only the service creates pipeline stages."));
        }
        validation::route_valid(ctx.team, &n.role, &n.pipeline)?;
        if let Some(id) = &n.assignee {
            let member = ctx.member(id)?;
            if member.id == ctx.team.lead
                || member.role != n.role
                || member.state == MemberState::Removed
            {
                return Err(invalid(
                    "The assignee must be a member of the draft task's role.",
                ));
            }
        }
        validation::write_paths(&n.writes)?;
        for path in &n.writes {
            if zerus_team_plan::forbidden_char(path).is_some() {
                return Err(invalid("A write path contains a forbidden character."));
            }
        }
        if n.estimate_minutes > 525_600 {
            return Err(over("The task estimate exceeds one year."));
        }
        if n.deps.len() > 200 {
            return Err(over("Too many draft dependencies."));
        }
        let mut seen = BTreeSet::new();
        for d in &n.deps {
            if !seen.insert(d.node) || d.node == n.id {
                return Err(invalid(
                    "Dependencies must be distinct and cannot name their own task.",
                ));
            }
            if ids.contains(&d.node) {
                continue;
            }
            if d.kind == DepKind::Hard {
                return Err(invalid(
                    "A hard dependency must be in this phase; use a barrier between phases.",
                ));
            }
            if !others.contains_key(&d.node) && rows::node(ctx.db, &ctx.team.id, d.node)?.is_none()
            {
                return Err(invalid("A draft dependency does not exist in this team."));
            }
        }
        graph.push(n.node(phase.n));
    }
    if zerus_team_plan::graph::cycles(&graph)
        .iter()
        .flatten()
        .any(|id| ids.contains(id))
    {
        return Err(Problem::new(
            code::DEPENDENCY_CYCLE,
            "Draft dependencies form a cycle.",
        ));
    }
    let mut phases = BTreeMap::new();
    let mut s = ctx
        .db
        .prepare("SELECT data FROM phases WHERE team=?1 ORDER BY n LIMIT 201")
        .map_err(storage)?;
    for p in s
        .query_map([&ctx.team.id], |r| store::sql_json::<Phase>(r, 0))
        .map_err(storage)?
    {
        let p = p.map_err(storage)?;
        phases.insert(p.n, p);
    }
    phases.insert(phase.n, phase.clone());
    let index: BTreeMap<_, _> = graph.iter().map(|n| (n.id, n)).collect();
    // Bound work under the writer lock, including long path components. The
    // request's byte limit alone permits a quadratic number of path pairs.
    const MAX_WRITE_WORK: u64 = 64_000_000;
    let write_sizes: BTreeMap<_, _> = graph
        .iter()
        .map(|n| {
            (
                n.id,
                n.writes.iter().map(|p| p.len() as u64 + 1).sum::<u64>(),
            )
        })
        .collect();
    let mut write_work = 0u64;
    let mut ancestry = BTreeMap::new();
    for a in graph.iter().filter(|n| n.phase == phase.n) {
        for b in graph.iter().filter(|n| n.phase != phase.n || n.id > a.id) {
            if a.writes.is_empty() || b.writes.is_empty() {
                continue;
            }
            let later = if a.phase != b.phase {
                let x = &phases[&a.phase];
                let y = &phases[&b.phase];
                Some(if phase_order(x) > phase_order(y) {
                    x
                } else {
                    y
                })
            } else {
                None
            };
            if later.is_some_and(|p| p.barrier) {
                continue;
            }
            if depends(&index, &mut ancestry, a.id, b.id)
                || depends(&index, &mut ancestry, b.id, a.id)
            {
                continue;
            }
            write_work = write_work.saturating_add(
                a.writes.len() as u64 * write_sizes[&b.id]
                    + b.writes.len() as u64 * write_sizes[&a.id],
            );
            if write_work > MAX_WRITE_WORK {
                return Err(over(
                    "The draft's write-set validation exceeds its work bound; simplify write sets or split the phase.",
                ));
            }
            if zerus_team_plan::overlap::overlaps(&a.writes, &b.writes) {
                return Err(Problem::new(
                    code::INVALID_STATE,
                    "Overlapping write sets need a dependency or a phase barrier.",
                )
                .with_refs([a.id.to_string(), b.id.to_string()]));
            }
        }
    }
    Ok(next)
}

fn phase_order(phase: &Phase) -> (u32, u32) {
    (
        if phase.order == 0 {
            phase.n
        } else {
            phase.order
        },
        phase.n,
    )
}

fn other_drafts(ctx: &Context<'_>, phase: u32) -> Result<Vec<Node>> {
    let (count,bytes):(u32,u64)=ctx.db.query_row("SELECT count(*),COALESCE(sum(length(CAST(data AS BLOB))),0) FROM phase_draft_nodes WHERE team=?1 AND phase<>?2",params![ctx.team.id,phase],|r|Ok((r.get(0)?,r.get(1)?))).map_err(storage)?;
    if count > 500 || bytes > 8 * 1024 * 1024 {
        return Err(over("Draft graph exceeds its read bound."));
    }
    let mut stmt=ctx.db.prepare("SELECT phase,data FROM phase_draft_nodes WHERE team=?1 AND phase<>?2 ORDER BY id LIMIT 501").map_err(storage)?;
    let out = stmt
        .query_map(params![ctx.team.id, phase], |r| {
            Ok(store::sql_json::<DraftNode>(r, 1)?.node(r.get(0)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage);
    out
}

fn depends(
    nodes: &BTreeMap<u32, &Node>,
    memo: &mut BTreeMap<u32, BTreeSet<u32>>,
    from: u32,
    target: u32,
) -> bool {
    memo.entry(from)
        .or_insert_with(|| ancestors(nodes, from))
        .contains(&target)
}

fn ancestors(nodes: &BTreeMap<u32, &Node>, from: u32) -> BTreeSet<u32> {
    let mut pending = vec![from];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(n) = nodes.get(&id) {
            pending.extend(n.deps.iter().map(|d| d.node));
        }
    }
    seen.remove(&from);
    seen
}

#[cfg(test)]
mod tests;
