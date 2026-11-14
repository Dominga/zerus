use super::*;

pub(super) fn create(ctx: &mut Context<'_>) -> Result<()> {
    validation::validate_create(ctx.team)?;
    let t = ctx.team;
    if t.settings.integration == Integration::Branch {
        let used:bool=ctx.db.query_row("SELECT EXISTS(SELECT 1 FROM teams WHERE project=?1 AND home=?2 AND integration_branch=?3 AND status NOT IN (?4,?5,?6))",params![t.project,t.home,t.integration_branch,encode(&TeamStatus::Stopped)?,encode(&TeamStatus::Archived)?,encode(&TeamStatus::Finished)?],|r|r.get(0)).map_err(storage)?;
        if used {
            return Err(invalid("Another live team uses this integration branch."));
        }
    }
    ctx.db.execute("INSERT INTO teams(id,project,name,goal,home,owner,status,lead,settings,bundle,integration_branch,base_commit,revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,0)",params![t.id,t.project,t.name,t.goal,t.home,t.owner,encode(&t.status)?,t.lead,encode(&t.settings)?,encode(&t.bundle)?,t.integration_branch,t.base_commit]).map_err(storage)?;
    create_roster(ctx)?;
    if t.settings.queue {
        let phase = Phase {
            n: QUEUE_PHASE,
            order: 0,
            title: "Queue".into(),
            kind: PhaseKind::Queue,
            mode: PhaseMode::Free,
            approval: t.settings.plan_approval,
            state: PhaseState::Approved,
            acceptance: t.settings.phase_acceptance,
            barrier: false,
            paused: false,
        };
        ctx.db
            .execute(
                "INSERT INTO phases(team,n,data) VALUES(?1,0,?2)",
                params![t.id, encode(&phase)?],
            )
            .map_err(storage)?;
    }
    Ok(())
}
fn create_roster(ctx: &Context<'_>) -> Result<()> {
    let mut roster = vec![Member {
        id: LEAD.into(),
        role: LEAD.into(),
        agent: ctx.team.bundle.template.lead.agent.clone(),
        account: String::new(),
        state: MemberState::NotCreated,
        session: None,
        worktree: String::new(),
        ownership_nonce: String::new(),
    }];
    for seat in &ctx.team.bundle.template.roster {
        let role = ctx
            .team
            .bundle
            .roles
            .iter()
            .find(|r| r.id == seat.role)
            .ok_or_else(missing)?;
        for _ in 0..seat.count {
            roster.push(Member {
                id: format!("m{}", roster.len()),
                role: role.id.clone(),
                agent: role.agent.clone(),
                account: String::new(),
                state: MemberState::NotCreated,
                session: None,
                worktree: String::new(),
                ownership_nonce: String::new(),
            });
        }
    }
    for m in roster {
        ctx.db
            .execute(
                "INSERT INTO members(team,id,data,state) VALUES(?1,?2,?3,?4)",
                params![ctx.team.id, m.id, encode(&m)?, encode(&m.state)?],
            )
            .map_err(storage)?;
    }
    Ok(())
}
pub(super) fn pause(ctx: &mut Context<'_>) -> Result<()> {
    if ctx.team.status != TeamStatus::Running {
        return Err(invalid("Only a running team can be paused."));
    }
    ctx.db
        .execute(
            "UPDATE teams SET status=?2,person_paused=?3 WHERE id=?1",
            params![
                ctx.team.id,
                encode(&TeamStatus::Paused)?,
                is_person(ctx.actor)
            ],
        )
        .map_err(storage)?;
    Ok(())
}
pub(super) fn resume(ctx: &mut Context<'_>) -> Result<()> {
    if ctx.team.status != TeamStatus::Paused {
        return Err(invalid("Only a paused team can be resumed."));
    }
    let person_paused: bool = ctx
        .db
        .query_row(
            "SELECT person_paused FROM teams WHERE id=?1",
            [&ctx.team.id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if person_paused && !is_person(ctx.actor) {
        return Err(invalid("Only the person may resume a team they paused."));
    }
    ctx.db
        .execute(
            "UPDATE teams SET status=?2,person_paused=0 WHERE id=?1",
            params![ctx.team.id, encode(&TeamStatus::Running)?],
        )
        .map_err(storage)?;
    Ok(())
}
pub(super) fn stop(ctx: &mut Context<'_>) -> Result<()> {
    ctx.active()?;
    // A stop settles older decisions and records cancellation commands; the
    // existing task writer still owns their actual task-state transitions.
    // A hosted delivery whose actor outcome was never recorded is settled
    // only from a durable outcome the team already holds (the row's persisted
    // outcome or the hosted child's retained reply); it is never replayed.
    // A retained `uncertain` reply (or a missing one) leaves the delivery
    // unknown: its row and its task text stay, and the terminal prune stays
    // blocked for that team by the open effect. That is the accepted limit of
    // zerus-63rru.102 round 3, not a retry.
    rows::settle_decisions(ctx.db, &ctx.team.id, None, &encode(ctx.actor)?, true)?;
    // The stop's own identity: a monotonic sequence recorded on the team,
    // stamped on this stop's cancel decisions (zerus-63rru.52 round 3).
    // The queue read joins the recorded facts on this identity, never on
    // the decision's text.
    let stop: i64 = ctx
        .db
        .query_row("SELECT stop_seq FROM teams WHERE id=?1", [&ctx.team.id], |r| r.get(0))
        .map_err(storage)?;
    let stop = stop + 1;
    ctx.db.execute("UPDATE teams SET stop_seq=?2 WHERE id=?1", params![ctx.team.id, stop]).map_err(storage)?;
    ctx.db.execute("INSERT INTO decisions(team,node,generation,actor,kind,text,state,stop) SELECT team,id,generation,?2,'cancel','Team stopped',?3,?4 FROM nodes WHERE team=?1",params![ctx.team.id,encode(ctx.actor)?,DecisionState::Pending,stop]).map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE nodes SET requested='cancel' WHERE team=?1",
            [&ctx.team.id],
        )
        .map_err(storage)?;
    // The stop's Cancel is each node's next decision: it ends a hand-back's
    // hold (holding the node itself while it waits for its answer); an
    // explicit hold stays.
    rows::end_hand_backs(ctx.db, &ctx.team.id)?;
    ctx.db
        .execute("DELETE FROM live_tasks WHERE team=?1", [&ctx.team.id])
        .map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE teams SET status=?2 WHERE id=?1",
            params![ctx.team.id, encode(&TeamStatus::Stopped)?],
        )
        .map_err(storage)?;
    // A new stop reopens the closed-key sweep (zerus-63rru.32 round 2).
    ctx.db
        .execute(
            "DELETE FROM key_clear_finished WHERE team=?1",
            [&ctx.team.id],
        )
        .map_err(storage)?;
    Ok(())
}
pub(super) fn archive(ctx: &mut Context<'_>) -> Result<()> {
    if !matches!(ctx.team.status, TeamStatus::Stopped | TeamStatus::Finished) {
        return Err(invalid("Only a stopped or finished team can be archived."));
    }
    rows::close_cards(ctx.db, &ctx.team.id, None, &encode(ctx.actor)?)?;
    ctx.db
        .execute("DELETE FROM live_tasks WHERE team=?1", [&ctx.team.id])
        .map_err(storage)?;
    ctx.db
        .execute(
            "UPDATE teams SET status=?2 WHERE id=?1",
            params![ctx.team.id, encode(&TeamStatus::Archived)?],
        )
        .map_err(storage)?;
    // A new archive reopens the closed-key sweep (zerus-63rru.32 round 2).
    ctx.db
        .execute(
            "DELETE FROM key_clear_finished WHERE team=?1",
            [&ctx.team.id],
        )
        .map_err(storage)?;
    Ok(())
}
pub(super) fn change_settings(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::ChangeSettings { settings } = ctx.op else {
        unreachable!()
    };
    ctx.active()?;
    validation::settings(settings)?;
    let members: u32 = ctx
        .db
        .query_row(
            "SELECT count(*) FROM members WHERE team=?1 AND id<>?2 AND state<>?3",
            params![ctx.team.id, ctx.team.lead, encode(&MemberState::Removed)?],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if settings.caps.max_open_tasks < ctx.count(true)? || settings.caps.max_members < members {
        return Err(over("Caps are below the admitted live set."));
    }
    if settings == &ctx.team.settings || settings.queue != ctx.team.settings.queue {
        return Err(invalid(
            "Settings are unchanged or change queue enablement.",
        ));
    }
    if settings.integration == Integration::Branch && ctx.team.integration_branch.is_empty() {
        return Err(invalid("An integration branch is required."));
    }
    // The archive is history and keeps the value each node ended with; only
    // live nodes follow the setting, and only when it actually changed.
    if settings.integration != ctx.team.settings.integration {
        ctx.db
            .execute(
                "UPDATE nodes SET integration_required=?2 WHERE team=?1",
                params![ctx.team.id, settings.integration == Integration::Branch],
            )
            .map_err(storage)?;
    }
    if settings.integration == Integration::Branch && !settings.delegations.merge_accepted {
        delegation::revoke_merges(ctx)?;
    }
    ctx.db
        .execute(
            "UPDATE teams SET settings=?2 WHERE id=?1",
            params![ctx.team.id, encode(settings)?],
        )
        .map_err(storage)?;
    Ok(())
}
