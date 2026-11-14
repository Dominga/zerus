//! The store follows the task's attempts. An attempt is named by the pair
//! of its generation and its ID: the current attempt can change while the
//! highest generation stays. When an observation shows an active task whose
//! current attempt is not the one the store holds and is not an effect of
//! the node, a person started a new execution between two observations, and
//! the store does at once what the two observations it missed would have
//! done in turn: the end of the earlier attempt, then the person's restore.
//! The node's own hand-out or reopen is never such an attempt, whether or not
//! its outcome is recorded, also when it comes back as current after a newer
//! attempt never started. A conflict, two attempts claiming the highest
//! generation, is its own flag: it is recorded as it came (the number, an
//! empty ID, the flag) and decides nothing while the task runs or waits.
//! The store remembers the attempt its node runs under, and that memory
//! changes only when the node takes an execution in: its own effect is
//! observed, a person's running execution is taken over or restored, or the
//! upgrade seeds it. An attempt merely observed while the task does not run
//! is not remembered, so that it is still judged when it starts.
use super::*;

/// Two attempts claim the highest generation: the flag, and nothing else. An
/// empty ID without it is an ordinary pair, a task with no current attempt.
/// A conflict of a task that still runs or waits decides nothing; the end of
/// a task (cancelled or Done) does not depend on who executed it.
pub(in crate::teams::ops) fn conflict(o: &TaskObservation) -> bool {
    o.attempt_conflict && !o.cancelled && o.stage != TaskStage::Done
}
/// A person's new execution of a live node: active, with a current attempt
/// that is neither the last current attempt the store saw nor an effect of
/// this node, whether or not the highest generation grew. A conflict (no
/// current attempt) decides nothing; when one settles back on the attempt
/// the node already runs under, that is no second override.
pub(in crate::teams::ops) fn person_attempt(
    ctx: &Context<'_>,
    node: u32,
    o: &TaskObservation,
) -> Result<bool> {
    let active = o.stage == TaskStage::InProgress && !o.cancelled && o.key == KeyState::Ours;
    if !active || o.attempt_id.is_empty() || o.attempt_id == remembered(ctx, node)? {
        return Ok(false);
    }
    Ok(!own(ctx, node, &o.attempt_id)?)
}
/// What the node took in with this observation, remembered: the node's own
/// effect, whatever the task's stage, or a running person's execution that
/// the observation took over or restored (its `person_override`).
pub(in crate::teams::ops) fn remember(
    ctx: &Context<'_>,
    node: u32,
    o: &TaskObservation,
) -> Result<()> {
    if o.attempt_id.is_empty() {
        return Ok(());
    }
    let running = o.stage == TaskStage::InProgress && !o.cancelled && o.key == KeyState::Ours;
    let taken = running && ctx.event == Some("person_override");
    if !taken && !own(ctx, node, &o.attempt_id)? {
        return Ok(());
    }
    ctx.db
        .execute(
            "UPDATE task_observations SET last_attempt_id=?3 WHERE team=?1 AND node=?2",
            params![ctx.team.id, node, o.attempt_id],
        )
        .map_err(storage)?;
    Ok(())
}
/// Whether the attempt is an effect of this node: the team's own.
fn own(ctx: &Context<'_>, node: u32, id: &str) -> Result<bool> {
    own_effect(ctx, node, id, false)
}
/// Whether the attempt is an effect of this node; with `current`, only a
/// hand-out or a reopen of the node's current generation, the team's own
/// attempt as the cancel command narrows it.
pub(in crate::teams::ops) fn own_effect(
    ctx: &Context<'_>,
    node: u32,
    id: &str,
    current: bool,
) -> Result<bool> {
    let generation = if current {
        Some(ctx.generation(node)?)
    } else {
        None
    };
    ctx.db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM effects WHERE request_id=?1 AND team=?2 AND node=?3 AND (?4 IS NULL OR (generation=?4 AND kind IN ('handout','reopen'))))",
            params![id, ctx.team.id, node, generation],
            |r| r.get(0),
        )
        .map_err(storage)
}
/// The end of the earlier attempt, as its cancelled observation would end
/// it, then the person's restore as today. A stage is never restored.
pub(in crate::teams::ops) fn take_over(
    ctx: &mut Context<'_>,
    node: u32,
    n: &mut Node,
) -> Result<()> {
    pipeline::observe_override(ctx, n)?;
    ctx.db.execute(&format!("UPDATE effects SET state='observed_done' WHERE team=?1 AND node=?2 AND state IN {OPEN_EFFECT_STATES}"),params![ctx.team.id,node]).map_err(storage)?;
    ctx.close_inbox(node)?;
    drop_marks(ctx, node)?;
    ctx.settle_override(node)?;
    if n.kind == NodeKind::Stage || ctx.node_table(node)? != "nodes" {
        return Ok(());
    }
    ctx.advance(node, n)?;
    // A person's run taken over ends a hand-back's hold; an explicit hold
    // stays.
    ctx.db.execute("UPDATE nodes SET requested=NULL,decided_by=NULL,integrated=0,integration_commit=NULL,hand_back=0 WHERE team=?1 AND id=?2",params![ctx.team.id,node]).map_err(storage)?;
    ctx.event = Some("person_override");
    Ok(())
}
/// The attempt the node runs under, as far as the store took it in; for
/// the override rule alone.
fn remembered(ctx: &Context<'_>, node: u32) -> Result<String> {
    Ok(ctx
        .db
        .query_row(
            "SELECT last_attempt_id FROM task_observations WHERE team=?1 AND node=?2",
            params![ctx.team.id, node],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?
        .unwrap_or_default())
}
/// The newest observation's attempt pair for the node, a conflict's empty ID
/// included; none before one.
pub(in crate::teams::ops) fn stored(ctx: &Context<'_>, node: u32) -> Result<(u64, String)> {
    Ok(ctx
        .db
        .query_row(
            "SELECT attempt,attempt_id FROM task_observations WHERE team=?1 AND node=?2",
            params![ctx.team.id, node],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?
        .unwrap_or_default())
}
