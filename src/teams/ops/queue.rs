use super::*;

pub(super) fn add(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::AddTask {
        task,
        role,
        pipeline,
    } = ctx.op
    else {
        unreachable!()
    };
    ctx.active()?;
    if !ctx.team.settings.queue || task.project != ctx.team.project {
        return Err(invalid("Team does not take this project's queued tasks."));
    }
    uuid(&task.uuid)?;
    if let Some(node) = existing_node(ctx, task)? {
        ctx.reply.node = Some(node);
        return Ok(());
    }
    let admitted = is_router(ctx);
    check_capacity(ctx, admitted)?;
    let n = new_node(ctx, task, role.as_deref(), pipeline.as_deref(), admitted)?;
    ctx.db
        .execute(
            "INSERT INTO node_identity(team,id,task_project,task_uuid) VALUES(?1,?2,?3,?4)",
            params![ctx.team.id, n.id, task.project, task.uuid],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "INSERT INTO live_tasks(task_project,task_uuid,team,node) VALUES(?1,?2,?3,?4)",
            params![task.project, task.uuid, ctx.team.id, n.id],
        )
        .map_err(storage)?;
    let routed = !n.role.is_empty();
    ctx.db.execute("INSERT INTO nodes(team,id,data,rank,admitted,routed,task_revision,task_stage,task_cancelled,integration_required) VALUES(?1,?2,?3,'',?4,?5,'','unobserved',0,?6)",params![ctx.team.id,n.id,rows::node_payload(&n)?,admitted,routed,ctx.team.settings.integration==Integration::Branch]).map_err(storage)?;
    if !is_router(ctx) && (role.is_some() || pipeline.is_some()) {
        ctx.db
            .execute(
                "INSERT INTO suggestions(team,node,role,pipeline) VALUES(?1,?2,?3,?4)",
                params![ctx.team.id, n.id, role, pipeline],
            )
            .map_err(storage)?;
    }
    ctx.db
        .execute(
            "UPDATE teams SET next_node=next_node+1 WHERE id=?1",
            [&ctx.team.id],
        )
        .map_err(storage)?;
    ctx.reply.node = Some(n.id);
    Ok(())
}
fn existing_node(ctx: &Context<'_>, task: &TaskRef) -> Result<Option<u32>> {
    let existing: Option<(String, u32)> = ctx
        .db
        .query_row(
            "SELECT team,node FROM live_tasks WHERE task_project=?1 AND task_uuid=?2",
            params![task.project, task.uuid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    match existing {
        Some((team, node)) if team == ctx.team.id => Ok(Some(node)),
        Some(_) => Err(Problem::new(
            code::DUPLICATE,
            "Task has a live node in another team.",
        )),
        None => Ok(None),
    }
}

fn new_node(
    ctx: &Context<'_>,
    task: &TaskRef,
    role: Option<&str>,
    pipeline: Option<&str>,
    admitted: bool,
) -> Result<Node> {
    let explicit = role.is_some() || pipeline.is_some();
    let routed = if is_router(ctx) {
        explicit || ctx.team.settings.routing == Routing::Direct
    } else {
        !explicit && ctx.team.settings.routing == Routing::Direct
    };
    if let Some(r) = role {
        text(r, 256)?;
    }
    if let Some(p) = pipeline {
        text(p, 256)?;
    }
    let pipe = if is_router(ctx) {
        pipeline.unwrap_or(&ctx.team.bundle.template.default_pipeline)
    } else {
        &ctx.team.bundle.template.default_pipeline
    };
    let p = ctx
        .team
        .bundle
        .pipelines
        .iter()
        .find(|p| p.id == pipe)
        .ok_or_else(|| Problem::new(code::UNKNOWN_PIPELINE, "Unknown pipeline."))?;
    let r = if is_router(ctx) {
        role.unwrap_or(&p.implementer)
    } else {
        &p.implementer
    };
    if is_router(ctx) {
        validation::route_valid(ctx.team, r, pipe)?;
    }
    let id: u32 = ctx
        .db
        .query_row(
            "SELECT next_node FROM teams WHERE id=?1",
            [&ctx.team.id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if id == u32::MAX {
        return Err(over("Node IDs are exhausted."));
    }
    Ok(node_metadata(
        id,
        task,
        if routed { r.into() } else { String::new() },
        if routed { pipe.into() } else { String::new() },
        admitted,
    ))
}
fn node_metadata(id: u32, task: &TaskRef, role: String, pipeline: String, admitted: bool) -> Node {
    Node {
        id,
        phase: QUEUE_PHASE,
        task: Some(task.clone()),
        title: String::new(),
        text: String::new(),
        source: NodeSource::Sent,
        deps: vec![],
        assignee: None,
        role,
        pipeline,
        writes: vec![],
        estimate_minutes: 0,
        kind: NodeKind::Task,
        severity: Severity::None,
        origin: None,
        stage_of: None,
        stage_index: 0,
        round: 1,
        implementer: None,
        earlier_implementers: vec![],
        hold: false,
        admitted,
        state: NodeState::Waiting,
        decided_by: None,
        integrated: false,
        rank: String::new(),
    }
}
pub(super) fn check_capacity(ctx: &Context<'_>, admitted: bool) -> Result<()> {
    let limit = if admitted {
        ctx.team.settings.caps.max_open_tasks
    } else {
        100
    };
    if ctx.count(admitted)? >= limit {
        return Err(over(if admitted {
            "Admitted task cap reached."
        } else {
            "Unadmitted submission cap reached."
        }));
    }
    Ok(())
}
pub(super) fn admit(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::Admit { node } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let n = ctx.live_node(*node)?;
    if n.admitted {
        return Err(invalid("Task is already admitted."));
    }
    check_capacity(ctx, true)?;
    ctx.db
        .execute(
            "UPDATE nodes SET admitted=1 WHERE team=?1 AND id=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    Ok(())
}
pub(super) fn route(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::Route {
        node,
        role,
        pipeline,
        deps,
        writes,
        assignee,
    } = ctx.op
    else {
        unreachable!()
    };
    ctx.active()?;
    ctx.undecided(*node)?;
    let mut n = ctx.live_node(*node)?;
    // Route places a task for the lead; a stage's role is its pipeline's
    // and its member is chosen by the hand-out (zerus-63rru.84). Whatever
    // its state, the stage's own paths are Reassign and the person's
    // decisions.
    if n.kind == NodeKind::Stage {
        return Err(invalid(
            "Route cannot move a stage; its role is the pipeline's and its member is chosen by the hand-out. Reassign the stage to another member of its role, or the person can decide.",
        ));
    }
    if !matches!(n.state, NodeState::Waiting | NodeState::Pending) {
        return Err(invalid("Only waiting or pending tasks may be routed."));
    }
    validation::route_valid(ctx.team, role, pipeline)?;
    validate_deps(ctx, *node, deps)?;
    validation::write_paths(writes)?;
    if let Some(id) = assignee {
        let m = ctx.member(id)?;
        if m.role != *role || m.id == ctx.team.lead || m.state == MemberState::Removed {
            return Err(invalid("Assignee cannot take this role."));
        }
    }
    n.role = role.clone();
    n.pipeline = pipeline.clone();
    n.writes = writes.clone();
    n.assignee = assignee.clone();
    ctx.db
        .execute(
            "UPDATE nodes SET data=?3,routed=1 WHERE team=?1 AND id=?2",
            params![ctx.team.id, node, rows::node_payload(&n)?],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "DELETE FROM dependencies WHERE team=?1 AND node=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    for d in deps {
        ctx.db
            .execute(
                "INSERT INTO dependencies(team,node,dependency,kind) VALUES(?1,?2,?3,?4)",
                params![ctx.team.id, node, d.node, encode(&d.kind)?],
            )
            .map_err(storage)?;
    }
    Ok(())
}
fn validate_deps(ctx: &Context<'_>, node: u32, deps: &[Dep]) -> Result<()> {
    if deps.len() > 200 {
        return Err(over("Too many dependencies."));
    }
    let mut seen = std::collections::BTreeSet::new();
    for d in deps {
        if d.node == node
            || !seen.insert(d.node)
            || rows::node(ctx.db, &ctx.team.id, d.node)?.is_none()
        {
            return Err(invalid("Missing, repeated or self dependency."));
        }
        let cycle:bool=ctx.db.query_row("WITH RECURSIVE ancestors(id) AS (SELECT dependency FROM dependencies WHERE team=?1 AND node=?2 UNION SELECT d.dependency FROM dependencies d JOIN ancestors a ON d.node=a.id WHERE d.team=?1) SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=?3)",params![ctx.team.id,d.node,node],|r|r.get(0)).map_err(storage)?;
        if cycle {
            return Err(Problem::new(code::DEPENDENCY_CYCLE, "Dependency cycle."));
        }
    }
    Ok(())
}
/// An explicit hold belongs to who set it, the person or the lead: no
/// decision, settlement, outcome or hand-back clears it, and the node still
/// takes decisions under it. It can be set while a decision is pending.
pub(super) fn hold(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::Hold { node } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    ctx.live_node(*node)?;
    if rows::hold_owner(ctx.db, &ctx.team.id, *node)?.is_some() {
        return Err(invalid("Task is already held."));
    }
    let owner = if is_person(ctx.actor) {
        "person"
    } else {
        "lead"
    };
    rows::set_explicit_hold(ctx.db, &ctx.team.id, *node, Some(owner))
}
/// Release ends an explicit hold and a hand-back's hold; a pending
/// decision's hold ends only with the decision. A person's hold is released
/// only by the person.
pub(super) fn release(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::Release { node } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    ctx.undecided(*node)?;
    ctx.live_node(*node)?;
    let owner = rows::hold_owner(ctx.db, &ctx.team.id, *node)?;
    let handed_back = rows::handed_back(ctx.db, &ctx.team.id, *node)?;
    if owner.is_none() && !handed_back {
        return Err(invalid("Task is not held."));
    }
    if owner.as_deref() == Some("person") && !is_person(ctx.actor) {
        return Err(Problem::new(
            code::NOT_ALLOWED,
            "The person holds this task, and only the person releases that hold; a hold kept from before holds had owners counts as the person's unless it was a hand-back to the lead.",
        ));
    }
    rows::set_explicit_hold(ctx.db, &ctx.team.id, *node, None)?;
    hand_back(ctx, *node, false)
}
/// Holds a node for the team's next step, or ends that hold, through the
/// store's one writer of a hand-back's hold.
pub(super) fn hand_back(ctx: &Context<'_>, node: u32, value: bool) -> Result<()> {
    rows::set_hand_back(ctx.db, &ctx.team.id, node, value)
}
pub(super) fn observe(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ObserveTask {
        node,
        observation: o,
    } = ctx.op
    else {
        unreachable!()
    };
    let mut n = ctx.node(*node)?;
    let table = ctx.node_table(*node)?;
    let (stage, observed_end) = validate_observation(ctx, *node, o)?;
    let released =
        rows::released(ctx.db, &ctx.team.id, *node)? || pipeline::released_stage(ctx, &n)?;
    let left = o.key != KeyState::Ours;
    if left || released {
        let release = left && !released && table == "nodes";
        rows::record_observation(ctx.db, &ctx.team.id, *node, o, release || released)?;
        if released && n.kind == NodeKind::Stage {
            ctx.db
                .execute(
                    "DELETE FROM live_tasks WHERE team=?1 AND node=?2",
                    params![ctx.team.id, node],
                )
                .map_err(storage)?;
        }
        if release {
            pipeline::observe_override(ctx, &n)?;
            leave_team(ctx, *node)?;
        }
        return Ok(());
    }
    // A conflict of attempts of a task that runs or waits decides nothing: it
    // is recorded as it came. An ended task's conflict is applied below.
    if timers::conflict(o) {
        return rows::record_observation(ctx.db, &ctx.team.id, *node, o, false);
    }
    // The node's own end counts as well as the stored observation's: a
    // conflict is recorded without ending or restoring anything, so after a
    // conflict on a finished node the stored observation no longer shows
    // the end its node keeps.
    let ended = node_ended(ctx, table, *node)?;
    let was_terminal = observed_end || ended;
    let active = matches!(ctx.team.status, TeamStatus::Running | TeamStatus::Paused);
    // A finished node takes today's restore below; a live one a person sent
    // again between two observations ends its attempt and is restored here.
    if active && table == "nodes" && !was_terminal && timers::person_attempt(ctx, *node, o)? {
        timers::take_over(ctx, *node, &mut n)?;
    }
    pipeline::before_observation(ctx, &mut n, o, table == "nodes" && !was_terminal)?;
    if !active && table == "nodes" {
        archive_inactive(ctx, *node)?;
    }
    if n.kind != NodeKind::Stage
        && (table == "node_archive" || was_terminal)
        && !o.cancelled
        && o.stage != TaskStage::Done
    {
        if active {
            restore(ctx, *node, &mut n)?;
        } else if was_terminal {
            ctx.event = Some("person_override");
        }
    }
    n.title = o.title.clone();
    // Rule 1 cleared the mark when the person moved the task back; the
    // team's own cancel is still read from its attempt (zerus-63rru.36):
    // the node whose open effect carries that attempt ends, a newer node
    // for the same task does not. The same column the cancel mark sets
    // carries it, so node_state is cancelled and the node leaves the
    // hand-out's reach (zerus-63rru.36 round 4).
    let own_cancelled = o.current_cancelled
        && !o.attempt_id.is_empty()
        && ctx
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM effects WHERE team=?1 AND node=?2 AND request_id=?3 AND state IN ('reserved','submitted','uncertain','cutting_over'))",
                params![ctx.team.id, node, o.attempt_id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(storage)?;
    let target = ctx.node_table(*node)?;
    ctx.db.execute(&format!("UPDATE {target} SET data=?3,task_revision=?4,task_stage=?5,task_cancelled=?6,rank=?7 WHERE team=?1 AND id=?2"),params![ctx.team.id,node,rows::node_payload(&n)?,o.revision,stage,o.cancelled || own_cancelled,o.rank]).map_err(storage)?;
    if o.cancelled || own_cancelled || o.stage == TaskStage::Done {
        finish_observation(ctx, *node, o.cancelled || own_cancelled)?;
    }
    rows::record_observation(
        ctx.db,
        &ctx.team.id,
        *node,
        o,
        n.kind == NodeKind::Stage && (o.cancelled || o.stage == TaskStage::Done),
    )?;
    timers::remember(ctx, *node, o)?;
    // A task back in To do without a reopen of its generation (a person's
    // run has no pipeline run to reopen), or a settled conflict: a waiting
    // Return or Reassign that lost its reason is settled here.
    handout::settle_without_reason(ctx, *node)
}
/// Whether the node's own projection shows its task ended, cancelled or done.
fn node_ended(ctx: &Context<'_>, table: &str, node: u32) -> Result<bool> {
    ctx.db
        .query_row(
            &format!(
                "SELECT task_cancelled OR task_stage='done' FROM {table} WHERE team=?1 AND id=?2"
            ),
            params![ctx.team.id, node],
            |r| r.get(0),
        )
        .map_err(storage)
}
/// The stage's text and whether the stored observation shows the task ended.
fn validate_observation(
    ctx: &Context<'_>,
    node: u32,
    o: &TaskObservation,
) -> Result<(String, bool)> {
    reason(&o.revision)?;
    text(&o.revision, 256)?;
    text(&o.previous_revision, 256)?;
    text(&o.rank, 256)?;
    text(&o.title, 1024)?;
    text(&o.attempt_id, 256)?;
    let old = rows::observation(ctx.db, &ctx.team.id, node)?;
    let stage = encode(&o.stage)?.trim_matches('"').to_owned();
    if let Some(old) = &old {
        let same = old.stage == o.stage
            && old.cancelled == o.cancelled
            && old.current_cancelled == o.current_cancelled
            && old.rank == o.rank
            && old.title == o.title
            && old.key == o.key;
        if (old.revision != o.revision && old.revision != o.previous_revision)
            || (old.revision == o.revision && !same)
            || o.attempt < old.attempt
        {
            return Err(stale());
        }
    } else if !o.previous_revision.is_empty() {
        return Err(stale());
    }
    Ok((
        stage,
        old.is_some_and(|old| old.stage == TaskStage::Done || old.cancelled),
    ))
}

fn restore(ctx: &mut Context<'_>, node: u32, n: &mut Node) -> Result<()> {
    ctx.active()?;
    let archived = ctx.node_table(node)? == "node_archive";
    if archived {
        check_capacity(ctx, n.admitted)?;
    }
    let task = n.task.as_ref().ok_or_else(missing)?;
    let binding: Option<(String, u32)> = ctx
        .db
        .query_row(
            "SELECT team,node FROM live_tasks WHERE task_project=?1 AND task_uuid=?2",
            params![task.project, task.uuid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if binding
        .as_ref()
        .is_some_and(|b| b != &(ctx.team.id.clone(), node))
    {
        return Err(invalid("Task is now owned by another live node."));
    }
    ctx.db.execute("INSERT OR IGNORE INTO live_tasks(task_project,task_uuid,team,node) VALUES(?1,?2,?3,?4)",params![task.project,task.uuid,ctx.team.id,node]).map_err(storage)?;
    if archived {
        restore_row(ctx, node)?;
    }
    ctx.settle_override(node)?;
    ctx.advance(node, n)?;
    // A person's restore ends a hand-back's hold; an explicit hold stays.
    ctx.db.execute("UPDATE nodes SET requested=NULL,decided_by=NULL,integrated=0,integration_commit=NULL,hand_back=0 WHERE team=?1 AND id=?2",params![ctx.team.id,node]).map_err(storage)?;
    ctx.event = Some("person_override");
    Ok(())
}
fn restore_row(ctx: &Context<'_>, node: u32) -> Result<()> {
    ctx.db
        .execute(
            &format!(
                "INSERT INTO nodes({0}) SELECT {0} FROM node_archive WHERE team=?1 AND id=?2",
                rows::NODE_COLUMNS
            ),
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "DELETE FROM node_archive WHERE team=?1 AND id=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    Ok(())
}
pub(super) fn finish_observation(ctx: &Context<'_>, node: u32, cancelled: bool) -> Result<()> {
    ctx.db.execute(&format!("UPDATE effects SET state='observed_done' WHERE team=?1 AND node=?2 AND state IN {OPEN_EFFECT_STATES}"),params![ctx.team.id,node]).map_err(storage)?;
    ctx.close_inbox(node)?;
    timers::drop_marks(ctx, node)?;
    if cancelled || ctx.node(node)?.kind == NodeKind::Stage {
        ctx.db
            .execute(
                "DELETE FROM live_tasks WHERE team=?1 AND node=?2",
                params![ctx.team.id, node],
            )
            .map_err(storage)?;
        ctx.settle_override(node)?;
    } else if let Some((_, kind, _)) = ctx.pending(node)? {
        let n = ctx.node(node)?;
        if kind == "accept" && (!decisions::needs_merge(ctx, &n) || n.integrated) {
            ctx.settle_override(node)?;
        }
    }
    Ok(())
}

fn archive_inactive(ctx: &Context<'_>, node: u32) -> Result<()> {
    ctx.db
        .execute(
            &format!(
                "INSERT INTO node_archive({0}) SELECT {0} FROM nodes WHERE team=?1 AND id=?2",
                rows::NODE_COLUMNS
            ),
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "DELETE FROM nodes WHERE team=?1 AND id=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    Ok(())
}

fn leave_team(ctx: &mut Context<'_>, node: u32) -> Result<()> {
    ctx.settle_override(node)?;
    ctx.close_inbox(node)?;
    ctx.db.execute(&format!("UPDATE effects SET state='superseded' WHERE team=?1 AND node=?2 AND state IN {OPEN_EFFECT_STATES}"),params![ctx.team.id,node]).map_err(storage)?;
    ctx.db
        .execute(
            "DELETE FROM live_tasks WHERE team=?1 AND node=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    // A node that left the team stays held: nothing hands it out again.
    ctx.db
        .execute(
            "UPDATE nodes SET hand_back=1 WHERE team=?1 AND id=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    archive_inactive(ctx, node)?;
    ctx.event = Some("person_override");
    Ok(())
}
