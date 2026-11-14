//! A reply marks the member that asked, while the question's attempt is the
//! node's current one and that member still holds it. One mark per member,
//! the newest reply with the count of replies of that attempt, so no read
//! grows with the team's history. A reply to an ended or ending attempt
//! marks nobody, and a decision on the node drops its mark (a trigger).
use super::*;

pub(in crate::teams::ops) fn mark_answer(ctx: &Context<'_>, item: u64) -> Result<()> {
    let row: Option<(u32, u32, Option<String>)> = ctx
        .db
        .query_row(
            "SELECT node,generation,json_extract(actor,'$.member') FROM inbox WHERE team=?1 AND seq=?2",
            params![ctx.team.id, item],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage)?;
    let Some((node, generation, Some(member))) = row else {
        return Ok(());
    };
    // After an accept, a return, a reassignment or a cancel (a stop records
    // one for every node) the attempt is ending: such a reply marks nobody.
    if ctx.pending(node)?.is_some() || !holds(ctx, node, generation, &member)? {
        return Ok(());
    }
    // The team's next revision orders every reply of the team.
    let revision = ctx.team.revision + 1;
    ctx.db.execute("INSERT INTO answer_marks(team,member,node,generation,revision,replies) VALUES(?1,?2,?3,?4,?5,1) ON CONFLICT(team,member) DO UPDATE SET revision=excluded.revision,replies=CASE WHEN answer_marks.node=excluded.node AND answer_marks.generation=excluded.generation THEN answer_marks.replies+1 ELSE 1 END,node=excluded.node,generation=excluded.generation",params![ctx.team.id,member,node,generation,revision]).map_err(storage)?;
    Ok(())
}
/// The question's attempt is the node's current one and `member` holds it:
/// the newest effect of that generation is theirs and was not refused,
/// superseded or left reserved.
fn holds(ctx: &Context<'_>, node: u32, generation: u32, member: &str) -> Result<bool> {
    let live: Option<u32> = ctx
        .db
        .query_row(
            "SELECT generation FROM nodes WHERE team=?1 AND id=?2",
            params![ctx.team.id, node],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if live != Some(generation) {
        return Ok(false);
    }
    let holder: Option<(String, String)> = ctx.db.query_row("SELECT member,state FROM effects WHERE team=?1 AND node=?2 AND generation=?3 ORDER BY seq DESC LIMIT 1",params![ctx.team.id,node,generation],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
    Ok(holder.is_some_and(|(m, state)| {
        m == member && !matches!(state.as_str(), "reserved" | "refused" | "superseded")
    }))
}
/// An attempt that ends without a decision, its task Done or cancelled on
/// the board or a person's execution taken over, drops the node's marks as
/// a decision does (the trigger), beside the close of its inbox.
pub(in crate::teams::ops) fn drop_marks(ctx: &Context<'_>, node: u32) -> Result<()> {
    ctx.db
        .execute(
            "DELETE FROM answer_marks WHERE team=?1 AND node=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    Ok(())
}
