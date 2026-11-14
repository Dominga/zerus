//! What a stop or a released stage leaves behind: the final answer for each
//! task key the service cleared or was refused, and the provenance of a
//! member launch that completed after the stop, which never makes the
//! session a member again.
use super::*;
use store::timers::{settle, sql::WAITING_KEY};

/// The native session name contract's own bound (a session name as the
/// launch records hold it); a name is recorded whole, never cut.
const NAME_BYTES: usize = 512;

pub(in crate::teams::ops) fn key_clear(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::RecordKeyClear {
        node,
        state,
        reason: why,
    } = ctx.op
    else {
        unreachable!()
    };
    if !clearable(ctx, *node)? {
        return Err(invalid(
            "The task key was released, recorded, or is not this service's to clear.",
        ));
    }
    match state {
        KeyClearState::Cleared if !why.is_empty() => {
            return Err(invalid("A cleared key carries no reason."))
        }
        KeyClearState::Cleared => (),
        KeyClearState::Refused => {
            reason(why)?;
            text(why, 4096)?;
        }
    }
    ctx.db
        .execute(
            "INSERT INTO key_clears(team,node,state,reason,at_ms) VALUES(?1,?2,?3,?4,?5)",
            params![
                ctx.team.id,
                node,
                encode(state)?.trim_matches('"'),
                why,
                ctx.now
            ],
        )
        .map_err(storage)?;
    ctx.db
        .execute(
            "DELETE FROM key_clear_work WHERE team=?1 AND node=?2",
            params![ctx.team.id, node],
        )
        .map_err(storage)?;
    settle(ctx.db, &ctx.team.id)?;
    ctx.reply.node = Some(*node);
    Ok(())
}
/// Only a node of the work frontier has a key to record: a released stage in
/// a team of any status, or a stopped or archived team's node whose cancel
/// has settled. A second record finds no row and is refused.
fn clearable(ctx: &Context<'_>, node: u32) -> Result<bool> {
    ctx.db
        .query_row(WAITING_KEY, params![ctx.team.id, node], |r| r.get(0))
        .map_err(storage)
}

pub(in crate::teams::ops) fn late_session(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::RecordLateSession {
        member,
        launch_id,
        session,
        name,
    } = ctx.op
    else {
        unreachable!()
    };
    let m = ctx.member(member)?;
    let terminal = matches!(ctx.team.status, TeamStatus::Stopped | TeamStatus::Archived);
    if !terminal && m.state != MemberState::Removed {
        return Err(invalid("The member is live; its launch binds normally."));
    }
    uuid(launch_id)?;
    if session.machine != ctx.team.home || session.session_id != format!("launch_id:{launch_id}") {
        return Err(invalid(
            "The session must be this launch on the team's home.",
        ));
    }
    reason(name)?;
    text(name, NAME_BYTES)?;
    let known: bool = ctx
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM late_sessions WHERE team=?1 AND member=?2 AND launch=?3)",
            params![ctx.team.id, member, launch_id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if known {
        return Err(Problem::new(
            code::DUPLICATE,
            "This late session was already recorded.",
        ));
    }
    let body = format!("Member {member} started session {name} after the team stopped or the member was removed. Zerus does not stop it; end that agent if it is not needed.");
    // Nothing reads its decision: acknowledged alone.
    let card = talk::card(ctx, "late_session", &body, None, None, &[], &[])?;
    ctx.db.execute("INSERT INTO late_sessions(team,member,launch,machine,session_id,name,card,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![ctx.team.id,member,launch_id,session.machine,session.session_id,name,card,ctx.now]).map_err(storage)?;
    ctx.reply.item = Some(card);
    Ok(())
}
