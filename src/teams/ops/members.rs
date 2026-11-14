use super::*;

pub(super) fn set_session(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::SetMemberSession {
        member,
        session,
        account,
        worktree,
        ownership_nonce,
    } = ctx.op
    else {
        unreachable!()
    };
    ctx.active()?;
    let mut m = ctx.member(member)?;
    if m.state == MemberState::Removed || m.session.is_some() {
        return Err(invalid("Member is removed or already bound."));
    }
    uuid(&session.machine)?;
    if let Some(id) = session.session_id.strip_prefix("launch_id:") {
        uuid(id)?;
    } else if let Some(id) = session.session_id.strip_prefix("run_id:") {
        reason(id)?;
        text(id, 256)?;
    } else {
        return Err(invalid("Session must use launch_id or run_id."));
    }
    if session.machine != ctx.team.home || worktree.is_empty() || ownership_nonce.is_empty() {
        return Err(invalid(
            "A local session with an owned worktree is required.",
        ));
    }
    text(account, 256)?;
    text(worktree, 4096)?;
    text(ownership_nonce, 256)?;
    let bound: bool = ctx
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM members WHERE machine=?1 AND session_id=?2)",
            params![session.machine, session.session_id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if bound {
        return Err(invalid("Session is already bound to a member."));
    }
    m.session = Some(session.clone());
    m.account = account.clone();
    m.worktree = worktree.clone();
    m.ownership_nonce = ownership_nonce.clone();
    m.state = MemberState::Active;
    save(ctx, &m)
}
/// The longest session name a handle keeps as its label hint.
const HINT_BYTES: usize = 512;
/// Records the durable handle of a member bound to a launch: once, while the
/// member and its team are live, and only for a session no other member's
/// handle holds. A member bound by run has no launch and gets none.
pub(super) fn set_handle(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::SetMemberHandle {
        member,
        session_id,
        label_hint,
    } = ctx.op
    else {
        unreachable!()
    };
    ctx.active()?;
    let m = ctx.member(member)?;
    let Some((machine, launch)) = m
        .session
        .as_ref()
        .filter(|_| m.state != MemberState::Removed)
        .and_then(|s| Some((&s.machine, s.session_id.strip_prefix("launch_id:")?)))
    else {
        return Err(invalid("Only a member bound to a launch has a handle."));
    };
    uuid(session_id)?;
    text(label_hint, HINT_BYTES)?;
    let own: bool = ctx
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM member_handles WHERE team=?1 AND member=?2)",
            params![ctx.team.id, member],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if own {
        return Err(Problem::new(
            code::DUPLICATE,
            "The member already has a handle.",
        ));
    }
    let held: bool = ctx
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM member_handles WHERE machine=?1 AND session_id=?2)",
            params![machine, session_id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if held {
        return Err(invalid("The session is another member's."));
    }
    ctx.db
        .execute(
            "INSERT INTO member_handles(team,member,machine,session_id,launch,label_hint,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![ctx.team.id, member, machine, session_id, launch, label_hint, ctx.now],
        )
        .map_err(storage)?;
    Ok(())
}
pub(super) fn set_state(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::SetMemberState { member, state } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let mut m = ctx.member(member)?;
    if m.state == *state
        || m.state == MemberState::Removed
        || matches!(state, MemberState::Removed | MemberState::NotCreated)
    {
        return Err(invalid("Invalid member state transition."));
    }
    if *state == MemberState::Active && (m.session.is_none() || ctx.person_stopped(member)?) {
        return Err(invalid(
            "The member has no session or was stopped by the person.",
        ));
    }
    m.state = *state;
    save(ctx, &m)
}
pub(super) fn pause(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::PauseMember { member } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let mut m = ctx.member(member)?;
    if m.state != MemberState::Active {
        return Err(invalid("Only an active member can be paused."));
    }
    m.state = MemberState::Paused;
    save(ctx, &m)
}
pub(super) fn resume(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ResumeMember { member } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let mut m = ctx.member(member)?;
    if m.state != MemberState::Paused
        || m.session.is_none()
        || (ctx.person_stopped(member)? && !is_person(ctx.actor))
    {
        return Err(invalid("The member cannot be resumed by this actor."));
    }
    ctx.db
        .execute(
            "UPDATE members SET person_stopped=0 WHERE team=?1 AND id=?2",
            params![ctx.team.id, member],
        )
        .map_err(storage)?;
    m.state = MemberState::Active;
    save(ctx, &m)
}
pub(super) fn stop(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::StopMember { member } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let mut m = ctx.member(member)?;
    if !matches!(m.state, MemberState::Active | MemberState::Paused)
        || ctx.person_stopped(member)?
    {
        return Err(invalid("The member cannot be stopped."));
    }
    ctx.db
        .execute(
            "UPDATE members SET person_stopped=1 WHERE team=?1 AND id=?2",
            params![ctx.team.id, member],
        )
        .map_err(storage)?;
    m.state = MemberState::Paused;
    save(ctx, &m)
}
pub(super) fn archive(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ArchiveMember { member } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    let mut m = ctx.member(member)?;
    if m.state == MemberState::Removed || m.id == ctx.team.lead {
        return Err(invalid("The member cannot be removed."));
    }
    m.state = MemberState::Removed;
    save(ctx, &m)?;
    hand_back(ctx, &m)
}
/// Removal settles the Returns and Reassigns aimed at the member that have
/// not started, or whose reopen is only reserved, and hands each node back to
/// the lead as a reopen that could not be delivered does: the decision
/// settled, the node held, and one inbox item. Then it settles the member's
/// open hand-out of a task that has not started or is in progress
/// (zerus-63rru.77). A stopped or busy member keeps them; that wait is
/// intended.
fn hand_back(ctx: &mut Context<'_>, removed: &Member) -> Result<()> {
    let member = removed.id.as_str();
    let mut handed = std::collections::BTreeSet::new();
    for (seq, node, kind, state) in reopen_decisions(ctx)? {
        let reserved = state == DecisionState::EffectReserved.as_str();
        if (!reserved && ctx.unstarted(node)?.map(|(s, _)| s) != Some(seq))
            || aimed_at(ctx, node, &kind)?.as_deref() != Some(member)
        {
            continue;
        }
        let body = if reserved {
            // Its delivery may already be under way: the reopen is settled
            // with the decision, so nothing delivers or replays it.
            supersede(ctx, node, member)?;
            format!(
                "Member {member} was removed while the {kind} of task {node} was being delivered; {UNKNOWN} The decision is settled and the task waits for a new one."
            )
        } else {
            format!(
                "Member {member} was removed before the {kind} of task {node} started; the decision is settled and the task waits for a new one."
            )
        };
        hand_to_lead(ctx, node, &format!("member_removed:{seq}"), &body)?;
        handed.insert(node);
    }
    settle_handout(ctx, removed, &handed)
}
/// What a removal says about a delivery it settles: the store cannot know
/// whether the prompt arrived, since a delivery child that passed its own
/// last check before the removal still types it.
const UNKNOWN: &str =
    "the delivery's result is not known: the prompt may have reached a session that is now gone.";
/// The member's open hand-out (reserved, submitted, uncertain or cutting
/// over; there is at most one) of a live task that has not started, or is in
/// progress, is settled in the removal: the effect superseded, so no pass
/// hands the task out again, nothing replays the delivery and a later report
/// of the member is not the team's, and the node handed back to the lead
/// with one item. An uncertain delivery keeps its outcome and its earlier
/// item; this item does not repeat it as a second fact. A task already in
/// review stays as it is: its member's report was written before the
/// removal. A node with another unsettled decision keeps it: a Cancel still
/// ends its task.
fn settle_handout(
    ctx: &mut Context<'_>,
    removed: &Member,
    handed: &std::collections::BTreeSet<u32>,
) -> Result<()> {
    let member = removed.id.as_str();
    let open: Option<(u32, String, String)> = ctx.db.query_row(&format!("SELECT e.node,e.request_id,n.node_state FROM effects e INDEXED BY effect_member JOIN nodes n ON n.team=e.team AND n.id=e.node WHERE e.team=?1 AND e.member=?2 AND e.state IN {OPEN_EFFECT_STATES} AND e.kind IN ('handout','reopen') AND n.node_state IN ('pending','waiting','in_progress')"),params![ctx.team.id,member],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)?;
    let Some((node, id, state)) = open else {
        return Ok(());
    };
    if ctx.pending(node)?.is_some() {
        return Ok(());
    }
    supersede(ctx, node, member)?;
    // A node the decision loop handed back keeps that one item; its open
    // hand-out is settled all the same, so no timer speaks of it later.
    if handed.contains(&node) {
        return Ok(());
    }
    let body = if state == "in_progress" {
        // The decisions it names are the ones that work for its node kind,
        // read from the node itself: only the person cancels a stage.
        let decisions = if ctx.node(node)?.kind == NodeKind::Stage {
            "Reassign gives the task to another member, and a later report from that session will not move it. The lead cannot cancel a stage; the person's Cancel stops that session and ends the task, and its parent comes back to the lead."
        } else {
            "Reassign gives the task to another member, and a later report from that session will not move it; Cancel stops that session and ends the task."
        };
        format!(
            "Member {member} was removed while task {node} was in progress; the task is with the lead again. {} {decisions}",
            still_working(&removed.worktree)
        )
    } else {
        format!(
            "Member {member} was removed before task {node} started; {UNKNOWN} The task waits for the lead."
        )
    };
    hand_to_lead(ctx, node, &format!("member_removed:{id}"), &body)?;
    Ok(())
}
/// Where a removed member's work is, as far as the store knows it: the
/// worktree path its members row has, and nothing else (no branch, no head).
fn still_working(worktree: &str) -> String {
    if worktree.is_empty() {
        "Its session was not told and may still be working on it, in its worktree.".into()
    } else {
        format!(
            "Its session was not told and may still be working on it, in the worktree at {worktree}."
        )
    }
}
/// The sentence for a report the removed member wrote after its removal: not
/// the team's, so not recorded; the work stays where the store says it is.
pub(super) fn late_report_text(member: &str, node: u32, worktree: &str) -> String {
    let at = if worktree.is_empty() {
        "in its worktree".to_owned()
    } else {
        format!("in the worktree at {worktree}")
    };
    format!(
        "Member {member} reported task {node} after it was removed; the team does not record that report. Its work is {at}."
    )
}
/// Closes the member's open effects of the node; a late outcome is still
/// recorded as a fact and never reopens them.
fn supersede(ctx: &Context<'_>, node: u32, member: &str) -> Result<()> {
    ctx.db.execute(&format!("UPDATE effects SET state='superseded' WHERE team=?1 AND node=?2 AND member=?3 AND state IN {OPEN_EFFECT_STATES}"),params![ctx.team.id,node,member]).map_err(storage)?;
    Ok(())
}
/// The member a pending Return or Reassign is aimed at, read only to hand
/// its node back when that member is removed: a Reassign's assignee, and for
/// a Return the member of the newest effect of the node's current
/// generation, whatever that effect's state. That is wider than the holder a
/// reopen is delivered to (`handout::reopen_target`), so that a Return
/// admitted while its member's hand-out was only reserved is still found. It
/// never chooses a recipient for a delivery.
fn aimed_at(ctx: &Context<'_>, node: u32, kind: &str) -> Result<Option<String>> {
    if kind == "reassign" {
        return Ok(ctx.node(node)?.assignee);
    }
    ctx.db
        .query_row(
            "SELECT member FROM effects WHERE team=?1 AND node=?2 AND generation=?3 ORDER BY seq DESC LIMIT 1",
            params![ctx.team.id, node, ctx.generation(node)?],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)
}
/// The member a Return of the node would go back to, its holder, when that
/// member was removed from the team: such a Return could never be carried
/// out. Every writer of a Return asks it.
pub(super) fn removed_holder(ctx: &Context<'_>, node: u32) -> Result<Option<String>> {
    let Some(holder) = handout::holder(ctx, node)? else {
        return Ok(None);
    };
    Ok((ctx.member(&holder)?.state == MemberState::Removed).then_some(holder))
}
/// Hands a node to the lead instead of a Return or Reassign that its member
/// can no longer carry out, through the store's one writer of that outcome
/// (`rows::hand_back`): its unsettled decision settled, the node held, one
/// inbox item of the cause's own kind. The operation moves the revision.
/// A hand-out reserved for a member that left the active set (paused,
/// stopped, unbound) is settled at the next pass instead of being replayed
/// through the non-start tiers (zerus-63rru.85): the effect is superseded,
/// the node handed back to the lead with one item, and the next pass can
/// hand the task to another free member. Whether the person paused the
/// member is not stored, so the lead is told in every case. A member that
/// is Active and bound is refused and nothing is written, whatever a
/// request's reason says (zerus-63rru.85 round 3): the store's member state
/// is the only proof of a departure, and a reason cannot substitute for it.
pub(super) fn settle_left(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::SettleLeftHandout { node, reason } = ctx.op else {
        unreachable!()
    };
    let node = *node;
    // Round 2's actor-read cause never decides a settlement: an Active bound
    // member is refused below whatever it says, and the store's own member
    // state names the cause for the others.
    let _ = reason;
    let open: Option<(String, String)> = ctx
        .db
        .query_row(
            &format!(
                "SELECT e.member,e.request_id FROM effects e WHERE e.team=?1 AND e.node=?2 AND e.state IN {OPEN_EFFECT_STATES} AND e.kind='handout'"
            ),
            params![ctx.team.id, node],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    let Some((member, id)) = open else {
        return Ok(());
    };
    let m = ctx.member(&member)?;
    if m.state == MemberState::Active && m.session.is_some() {
        // The member is still in the active set and its binding is the
        // store's: nothing settles, even when a reason claims a departure.
        return Err(invalid(
            "A hand-out of a member that is still active and bound is not settled.",
        ));
    }
    let how = if m.state == MemberState::Paused {
        if ctx.person_stopped(&member).unwrap_or(false) {
            "stopped"
        } else {
            "paused"
        }
    } else {
        "unbound"
    };
    supersede(ctx, node, &member)?;
    // The item without the hand-back's hold: the task stays in the queue,
    // so the next pass can hand it to another free member.
    rows::hand_back_item(
        ctx.db,
        &ctx.team.id,
        node,
        &encode(ctx.actor)?,
        &format!("member_left:{id}"),
        &format!(
            "Member {member} was {how} before the hand-out of task {node} reached it; the task is offered to another member."
        ),
        ctx.now,
    )?;
    Ok(())
}

/// The service's own binding observation read the member's binding as
/// decisively ended while the store still held it (zerus-63rru.85 round 3):
/// the member is recorded unbound — its binding, account, worktree and
/// ownership nonce cleared, and its durable handle deleted — so every later
/// reader, the settlement above included, reads the departure from the
/// store. The state stays Active: the member is in the roster but can take
/// no task until it is bound again. A member that is not Active and bound is
/// refused with `invalid_state` and nothing changes; that is also the answer
/// to a repeated call, so the end is recorded once per binding.
pub(super) fn unbind(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::UnbindMember { member } = ctx.op else {
        unreachable!()
    };
    let member = member.as_str();
    ctx.active()?;
    let mut m = ctx.member(member)?;
    if m.state != MemberState::Active || m.session.is_none() {
        return Err(invalid("Only an active bound member can be unbound."));
    }
    m.session = None;
    m.account.clear();
    m.worktree.clear();
    m.ownership_nonce.clear();
    save(ctx, &m)?;
    ctx.db
        .execute(
            "DELETE FROM member_handles WHERE team=?1 AND member=?2",
            params![ctx.team.id, member],
        )
        .map_err(storage)?;
    Ok(())
}

/// The lead is told once that a member out of the active set holds a
/// started task (zerus-63rru.86): one member_started item per member and
/// task, naming the member, the task, and that the work stays with the
/// member until it is resumed or the person moves the task. The task
/// stays with the member; nothing else changes. The operation is
/// idempotent by itself: an open item of the path for that member and
/// node means the lead was already told, and a crash between the apply
/// and the actor's own memory cannot tell twice. A call that finds no
/// such task, or a member that is in the active set again, writes
/// nothing.
pub(super) fn tell_paused_started(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::TellPausedStarted { member, node } = ctx.op else {
        unreachable!()
    };
    let member = member.as_str();
    let node = *node;
    let m = ctx.member(member)?;
    if m.state == MemberState::Active && m.session.is_some() {
        // The member is back in the active set: nothing to tell.
        return Ok(());
    }
    let holder = handout::holder(ctx, node)?;
    if holder.as_deref() != Some(member) {
        // No started work of this member on this task.
        return Ok(());
    }
    // The store's own once-only (zerus-63rru.86 round 2): the kind family
    // is member_started:{node}:{member}:{n}, n being the count of earlier
    // items of the family for that member and node plus one. The exact
    // prefix comparison never treats the member's characters as wildcards.
    let prefix = format!("member_started:{node}:{member}:");
    let told: i64 = ctx
        .db
        .query_row(
            "SELECT COUNT(*) FROM inbox WHERE team=?1 AND node=?2 AND state='open' AND substr(kind,1,?3)=?4",
            params![ctx.team.id, node, prefix.len() as i64, prefix],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if told > 0 {
        return Ok(());
    }
    let n: i64 = ctx
        .db
        .query_row(
            "SELECT COUNT(*) FROM inbox WHERE team=?1 AND node=?2 AND substr(kind,1,?3)=?4",
            params![ctx.team.id, node, prefix.len() as i64, prefix],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let how = if m.state == MemberState::Paused {
        if ctx.person_stopped(member)? {
            "stopped"
        } else {
            "paused"
        }
    } else {
        "unbound"
    };
    rows::hand_back_item(
        ctx.db,
        &ctx.team.id,
        node,
        &encode(ctx.actor)?,
        &format!("{prefix}{}", n + 1),
        &format!(
            "Member {member} was {how} while task {node} is in progress; the work stays with the member until it is resumed or the person moves the task."
        ),
        ctx.now,
    )?;
    Ok(())
}

pub(super) fn close_paused_started(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ClosePausedStarted { member } = ctx.op else {
        unreachable!()
    };
    ctx.db
        .execute(
            "UPDATE inbox SET state='closed' WHERE team=?1 AND state='open' AND substr(kind,1,15)='member_started:' AND substr(kind,16+instr(substr(kind,16),':'),length(?2)+1)=?2||':'",
            params![ctx.team.id, member],
        )
        .map_err(storage)?;
    Ok(())
}


pub(super) fn hand_to_lead(
    ctx: &mut Context<'_>,
    node: u32,
    kind: &str,
    text: &str,
) -> Result<u64> {
    rows::hand_back(
        ctx.db,
        &ctx.team.id,
        node,
        &encode(ctx.actor)?,
        kind,
        text,
        ctx.now,
    )
}
/// The team's unsettled Returns and Reassigns with their states, from the
/// partial index of open decisions and within its bound.
fn reopen_decisions(ctx: &Context<'_>) -> Result<Vec<(u64, u32, String, String)>> {
    let mut s = ctx.db.prepare("SELECT seq,node,kind,state FROM decisions INDEXED BY decision_pending WHERE team=?1 AND state<>'settled' AND kind IN ('return','reassign') ORDER BY node LIMIT ?2").map_err(storage)?;
    let rows = s
        .query_map(
            params![ctx.team.id, delegation::MAX_OPEN_DECISIONS + 1],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    if rows.len() > delegation::MAX_OPEN_DECISIONS {
        return Err(over("Too many open decisions to settle for the member."));
    }
    Ok(rows)
}
fn save(ctx: &Context<'_>, m: &Member) -> Result<()> {
    ctx.db
        .execute(
            "UPDATE members SET data=?3,machine=?4,session_id=?5,state=?6 WHERE team=?1 AND id=?2",
            params![
                ctx.team.id,
                m.id,
                encode(m)?,
                m.session.as_ref().map(|s| s.machine.as_str()),
                m.session.as_ref().map(|s| s.session_id.as_str()),
                encode(&m.state)?
            ],
        )
        .map_err(storage)?;
    Ok(())
}
