//! Service timers (package 10). A firing rechecks its condition from stored
//! facts in this transaction and writes one inbox item or one owner card
//! under a key that fires once. Times only the service saw arrive as
//! `since_ms`; the store checks their arithmetic, never their truth.
use super::*;
mod answers;
mod attempts;
mod cancel;
mod terminal;
pub(super) use answers::{drop_marks, mark_answer};
pub(super) use attempts::{conflict, person_attempt, remember, supersedes_accepted, take_over};
pub(super) use cancel::record_cancel;
pub(super) use terminal::{key_clear, late_session};

enum Made {
    Item(&'static str, String),
    Card(&'static str, String),
}

pub(super) fn fire(ctx: &mut Context<'_>) -> Result<()> {
    let Operation::FireTimer {
        timer,
        node,
        generation,
        item,
        since_ms,
    } = ctx.op
    else {
        unreachable!()
    };
    if ctx.team.status != TeamStatus::Running {
        return Err(invalid("No timer fires while the team is not running."));
    }
    let (node, generation) = (*node, *generation);
    let (made, subject) = match timer {
        TimerKind::NoReport => (no_report(ctx, node, generation, *since_ms)?, 0),
        TimerKind::NotStarted => (not_started(ctx, node, generation)?, 0),
        TimerKind::RemindLead => (remind(ctx, node, generation, *item, false)?, *item),
        TimerKind::RemindPerson => (remind(ctx, node, generation, *item, true)?, *item),
        TimerKind::SilentLead => (silent(ctx, node, generation, *item, *since_ms)?, *item),
    };
    let kind = encode(timer)?.trim_matches('"').to_owned();
    let fresh = ctx.db.execute("INSERT INTO timer_firings(team,kind,node,generation,item,target,seq,subject,created_ms) VALUES(?1,?2,?3,?4,?5,'inbox',0,?6,?7) ON CONFLICT DO NOTHING",params![ctx.team.id,kind,node,generation,subject,subject,ctx.now]).map_err(storage)?;
    if fresh != 1 {
        return Err(Problem::new(code::DUPLICATE, "This timer already fired."));
    }
    let (target, seq) = match made {
        Made::Item(kind, text) => ("inbox", ctx.inbox(node, kind, &text)?),
        // Nothing reads the decision of a timer's card: acknowledged alone.
        Made::Card(kind, text) => ("card", talk::card(ctx, kind, &text, (node != 0).then_some(node), None, &[], &[])?),
    };
    ctx.db.execute("UPDATE timer_firings SET target=?6,seq=?7 WHERE team=?1 AND kind=?2 AND node=?3 AND generation=?4 AND item=?5",params![ctx.team.id,kind,node,generation,subject,target,seq]).map_err(storage)?;
    ctx.reply.node = Some(node);
    ctx.reply.item = Some(seq);
    Ok(())
}
/// The stored time a hand-out or reopen outcome was recorded.
pub(super) fn outcome_time(ctx: &Context<'_>, id: &str) -> Result<()> {
    ctx.db
        .execute(
            "INSERT OR IGNORE INTO effect_outcomes(team,request_id,at_ms) VALUES(?1,?2,?3)",
            params![ctx.team.id, id, ctx.now],
        )
        .map_err(storage)?;
    Ok(())
}
fn current(ctx: &Context<'_>, node: u32, generation: u32) -> Result<Node> {
    let n = ctx.live_node(node)?;
    if ctx.generation(node)? != generation {
        return Err(invalid("The attempt is no longer current."));
    }
    Ok(n)
}
fn elapsed(ctx: &Context<'_>, since: i64, wait: i64, why: &str) -> Result<()> {
    if since <= 0 || since > ctx.now || ctx.now - since < wait {
        return Err(invalid(why));
    }
    Ok(())
}
/// The member of the attempt's submitted delivery and its outcome time.
fn delivered(ctx: &Context<'_>, node: u32, generation: u32) -> Result<(String, Option<i64>)> {
    ctx.db.query_row("SELECT e.member,o.at_ms FROM effects e LEFT JOIN effect_outcomes o ON o.team=e.team AND o.request_id=e.request_id WHERE e.team=?1 AND e.node=?2 AND e.generation=?3 AND e.state='submitted' ORDER BY e.seq DESC LIMIT 1",params![ctx.team.id,node,generation],|r|Ok((r.get(0)?,r.get(1)?)))
        .optional().map_err(storage)?.ok_or_else(|| invalid("The attempt has no submitted delivery."))
}
fn no_report(ctx: &Context<'_>, node: u32, generation: u32, since: i64) -> Result<Made> {
    let n = current(ctx, node, generation)?;
    let (member, at) = delivered(ctx, node, generation)?;
    if at.is_some_and(|at| since < at) {
        return Err(invalid("Idleness before the delivery does not count."));
    }
    let reported: bool = ctx
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM reports WHERE team=?1 AND node=?2 AND generation=?3)",
            params![ctx.team.id, node, generation],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if n.state != NodeState::InProgress || reported {
        return Err(invalid("The task is not in progress without a report."));
    }
    let wait = i64::from(ctx.team.settings.timers.report_timeout_seconds) * 1000;
    elapsed(ctx, since, wait, "The member was not idle long enough.")?;
    Ok(Made::Item(
        "no_report",
        format!("Member {member} ended its turn on task {node} without a report."),
    ))
}
fn not_started(ctx: &Context<'_>, node: u32, generation: u32) -> Result<Made> {
    let n = current(ctx, node, generation)?;
    let (member, at) = delivered(ctx, node, generation)?;
    if n.state != NodeState::Pending {
        return Err(invalid("The task has started or left To do."));
    }
    let at = at.ok_or_else(|| invalid("The hand-out has no recorded outcome time."))?;
    let wait = i64::from(ctx.team.settings.timers.start_timeout_seconds) * 1000;
    elapsed(ctx, at, wait, "The hand-out was submitted too recently.")?;
    Ok(Made::Item(
        "not_started",
        format!("Task {node} was handed to member {member} and has not started."),
    ))
}
fn remind(ctx: &Context<'_>, node: u32, generation: u32, item: u64, person: bool) -> Result<Made> {
    current(ctx, node, generation)?;
    let created: Option<i64> = ctx.db.query_row("SELECT created_ms FROM inbox WHERE team=?1 AND seq=?2 AND node=?3 AND generation=?4 AND kind='report' AND state='open'",params![ctx.team.id,item,node,generation],|r|r.get(0)).optional().map_err(storage)?;
    let created = created.ok_or_else(|| invalid("The report no longer waits for the lead."))?;
    let decided: bool = ctx
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM decisions WHERE team=?1 AND node=?2 AND generation=?3)",
            params![ctx.team.id, node, generation],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if decided {
        return Err(invalid("The report was decided."));
    }
    let t = &ctx.team.settings.timers;
    let minutes = if person {
        t.remind_person_minutes
    } else {
        t.remind_lead_minutes
    };
    elapsed(
        ctx,
        created,
        i64::from(minutes) * 60_000,
        "The report has not waited long enough.",
    )?;
    Ok(if person {
        Made::Card(
            "report_waiting",
            format!(
                "The report on task {node} has waited {minutes} minutes for the lead's decision."
            ),
        )
    } else {
        Made::Item(
            "report_reminder",
            format!("The report on task {node} has waited {minutes} minutes for your decision."),
        )
    })
}
fn silent(ctx: &Context<'_>, node: u32, generation: u32, item: u64, since: i64) -> Result<Made> {
    let oldest: Option<(u64, u32, u32, i64)> = ctx.db.query_row("SELECT seq,node,generation,created_ms FROM inbox WHERE team=?1 AND state='open' ORDER BY seq LIMIT 1",[&ctx.team.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(storage)?;
    let created = match oldest {
        Some((seq, n, g, created)) if (seq, n, g) == (item, node, generation) => created,
        _ => return Err(invalid("The item is no longer the oldest one waiting.")),
    };
    let minutes = ctx.team.settings.timers.lead_stall_minutes;
    let wait = i64::from(minutes) * 60_000;
    elapsed(ctx, created, wait, "The item has not waited long enough.")?;
    elapsed(ctx, since, wait, "The lead was not seen idle long enough.")?;
    let subject = if node == 0 { "the team queue".to_owned() } else { format!("task {node}") };
    Ok(Made::Card(
        "silent_lead",
        format!("The lead has not worked for {minutes} minutes while its inbox waits, oldest about {subject}."),
    ))
}

#[cfg(test)]
mod tests;
