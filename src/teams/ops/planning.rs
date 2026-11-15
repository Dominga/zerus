//! A separate creation intent: only the lead can be launched while Planning.
use super::*;

pub(super) fn member_lifecycle(ctx: &Context<'_>, member: &str) -> Result<()> {
    if ctx.team.status == TeamStatus::Planning && member == ctx.team.lead {
        if store::planning::paused(ctx.db, &ctx.team.id)?.is_some() {
            return Ok(());
        }
        return Err(invalid("The planning startup record is missing."));
    }
    ctx.active()
}

/// Planning can report and decide ordinary notices, never task cards.
/// Operation authority and each card's own listed decisions still apply.
pub(super) fn ordinary_card(ctx: &Context<'_>, node: Option<u32>) -> Result<()> {
    if ctx.team.status == TeamStatus::Planning && node.is_none() {
        if store::planning::paused(ctx.db, &ctx.team.id)?.is_some() {
            return Ok(());
        }
        return Err(invalid("The planning startup record is missing."));
    }
    ctx.active()
}

pub(super) fn start(ctx: &mut Context<'_>) -> Result<()> {
    if ctx.team.goal.trim().is_empty() {
        return Err(invalid("A planning goal is required."));
    }
    team::create(ctx)?;
    // The old engine refuses a fresh open before it can cause a planned
    // effect. An already-open old writer cannot resume Planning: planning
    // pauses deliberately never use its legacy Paused -> Running transition.
    store::raise_execution_floor(ctx.db, 3)?;
    ctx.db
        .execute(
            "INSERT INTO planning_startup(team) VALUES(?1)",
            [&ctx.team.id],
        )
        .map_err(storage)?;
    let brief = format!("Plan this goal:\n{}\n\nRead hgs team phases and submit complete phase drafts with hgs team submit-draft-phase. Include acceptance criteria. Drafts create no tasks; the person must approve the initial plan before members start.", ctx.team.goal);
    // Team-wide inbox notices use the existing (0,0) sentinel, just like the
    // queue's lead notice. It is not a task identity and has no task binding.
    ctx.db.execute("INSERT INTO inbox(team,kind,node,generation,actor,text,created_ms) VALUES(?1,'planning_goal',0,0,?2,?3,?4)", params![ctx.team.id,encode(ctx.actor)?,brief,ctx.now]).map_err(storage)?;
    Ok(())
}

pub(super) fn pause(ctx: &mut Context<'_>, wanted: bool) -> Result<()> {
    let current = store::planning::paused(ctx.db, &ctx.team.id)?
        .ok_or_else(|| invalid("The planning startup record is missing."))?;
    if current == wanted {
        return Err(invalid(if wanted {
            "Planning is already paused."
        } else {
            "Planning is not paused."
        }));
    }
    let person_paused: bool = ctx
        .db
        .query_row(
            "SELECT person_paused FROM teams WHERE id=?1",
            [&ctx.team.id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if !wanted && person_paused && !is_person(ctx.actor) {
        return Err(invalid("Only the person may resume a team they paused."));
    }
    ctx.db
        .execute(
            "UPDATE planning_startup SET paused=?2 WHERE team=?1",
            params![ctx.team.id, wanted],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE teams SET person_paused=?2 WHERE id=?1",
            params![ctx.team.id, wanted && is_person(ctx.actor)],
        )
        .map_err(storage)?;
    Ok(())
}

#[cfg(test)]
mod tests;
