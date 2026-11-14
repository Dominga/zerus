//! A ready node that no member can take (zerus-63rru.45). The plan kernel
//! names these nodes (`ready::unstaffed`); the lead is told once per
//! episode, which is a node, its generation and the kernel's reason.
//!
//! The service's hand-out pass compares the kernel's set, over its uncooled
//! readiness view, with the told items through [`unstaffed_changes`], and
//! asks for `ReconcileUnstaffed` only when that finds a change. The store
//! never takes the service's list: it recomputes the set from its own
//! readiness view, members and open effects through [`unstaffed_of`] and
//! applies the changes of the same function in one operation. An episode
//! ends when its node leaves the set or its reason or generation changes,
//! and only the reconcile ends it: it closes the item with a reply that says
//! why (see `store::unstaffed`). The lead closing its item ends nothing and
//! is not told again. An unstaffed stage is retired and its parent handed to
//! the lead with the kernel's reason, as at the stage's creation.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use store::unstaffed::Told;
use zerus_team_plan::ready::{self, MemberView, PlanView, Unstaffed};

/// The prefix of an unstaffed item's kind. The item's kind is
/// `unstaffed:<revision>`, the reconciling operation's revision; the inbox
/// keys an item by team, kind, node and generation, so the nodes of one
/// reconcile never share an item.
pub const UNSTAFFED_KIND: &str = "unstaffed:";
const LEFT: &str =
    "The service ended this item: a member can take the task again, or it is no longer ready.";
const REPLACED: &str = "The service ended this item: the task's reason or attempt changed, and a newer item states it.";
const STALE: &str = "The service ended this item: a newer item of the same task stands for it.";

/// What a reconcile would write: the episodes it ends (an item and why),
/// the nodes it tells (with the item's text), and the stages it retires.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UnstaffedChanges {
    pub end: Vec<(u64, &'static str)>,
    pub tell: Vec<(u32, String)>,
    pub stages: Vec<(u32, String)>,
}
impl UnstaffedChanges {
    pub fn is_empty(&self) -> bool {
        self.end.is_empty() && self.tell.is_empty() && self.stages.is_empty()
    }
}
fn text(node: u32, reason: &str) -> String {
    format!(
        "Task {node} is ready, but no member can take it: {reason} Route it to another role, reassign or cancel it."
    )
}
/// The one comparison of the kernel's set with the told episodes, for the
/// store's reconcile and the service's pass alike. A told item stands while
/// it is its node's newest, the node is in the set, and its generation and
/// text are the node's now; every other told item is ended. A node of the
/// set without a standing item is told. Whether the item is open does not
/// matter: the lead closing it ends nothing.
pub fn unstaffed_changes(
    now: &[Unstaffed],
    stages: &BTreeSet<u32>,
    told: &[Told],
) -> UnstaffedChanges {
    let wanted: BTreeMap<u32, String> = now
        .iter()
        .filter(|u| !stages.contains(&u.node))
        .map(|u| (u.node, text(u.node, &u.reason)))
        .collect();
    let mut newest: BTreeMap<u32, &Told> = BTreeMap::new();
    for t in told {
        let entry = newest.entry(t.node).or_insert(t);
        if t.seq > entry.seq {
            *entry = t;
        }
    }
    let stands = |t: &Told| {
        newest.get(&t.node).is_some_and(|n| n.seq == t.seq)
            && wanted.get(&t.node) == Some(&t.text)
            && t.current == Some(t.generation)
    };
    let mut changes = UnstaffedChanges::default();
    for t in told.iter().filter(|t| !stands(t)) {
        let why = if wanted.contains_key(&t.node) {
            REPLACED
        } else {
            LEFT
        };
        changes.end.push((t.seq, why));
    }
    for (node, text) in &wanted {
        if !newest.get(node).is_some_and(|t| stands(t)) {
            changes.tell.push((*node, text.clone()));
        }
    }
    changes.stages = now
        .iter()
        .filter(|u| stages.contains(&u.node))
        .map(|u| (u.node, u.reason.clone()))
        .collect();
    changes
}

/// The ready nodes that no member can take, by the kernel's rule, without
/// the nodes that have an open effect: such a node is in somebody's hands or
/// on its way and is never reconciled. Busy, paused and not yet created
/// members count as members who could take a node, as the kernel says. One
/// function for the store's reconcile and the service's comparison, so the
/// two cannot drift.
pub fn unstaffed_of(
    team: &Team,
    readiness: &Readiness,
    members: &[Member],
    open_effects: &BTreeSet<u32>,
) -> Vec<Unstaffed> {
    let members: Vec<MemberView> = members
        .iter()
        .map(|m| MemberView {
            id: m.id.clone(),
            role: m.role.clone(),
            state: m.state,
            free: false,
        })
        .collect();
    let view = PlanView {
        phases: &readiness.phases,
        nodes: &readiness.nodes,
        members: &members,
        lead: &team.lead,
        integration: team.settings.integration,
    };
    ready::unstaffed(&view)
        .into_iter()
        .filter(|u| !open_effects.contains(&u.node))
        .collect()
}

pub(super) fn reconcile(ctx: &mut Context<'_>) -> Result<()> {
    if ctx.team.status != TeamStatus::Running {
        return Err(invalid(
            "Only a running team's unstaffed nodes are reconciled.",
        ));
    }
    let readiness = readiness::load(ctx.db, &ctx.team.id)?;
    let members = members(ctx)?;
    let effects = store::unstaffed::open_effect_nodes(ctx.db, &ctx.team.id)?;
    let now = unstaffed_of(ctx.team, &readiness, &members, &effects);
    if now.len() > store::unstaffed::MAX_OPEN {
        return Err(over(
            "More than 500 unstaffed nodes: the reconcile fails closed.",
        ));
    }
    let told = store::unstaffed::told(ctx.db, &ctx.team.id)?;
    let changes = unstaffed_changes(&now, &unstaffed_stages(&readiness), &told);
    if changes.is_empty() {
        return Err(Problem::new(
            code::UNSTAFFED_UNCHANGED,
            "The lead's items already show every unstaffed node; nothing was written.",
        ));
    }
    // The reconcile's own close: the reply marks the episode as ended.
    for (seq, why) in &changes.end {
        ctx.db
            .execute(
                "UPDATE inbox SET state='closed',reply=?3,reply_actor=?4 WHERE team=?1 AND seq=?2",
                params![ctx.team.id, seq, why, encode(ctx.actor)?],
            )
            .map_err(storage)?;
    }
    let revision = ctx
        .team
        .revision
        .checked_add(1)
        .ok_or_else(|| over("Revision is exhausted."))?;
    for (node, text) in &changes.tell {
        ctx.inbox(*node, &format!("{UNSTAFFED_KIND}{revision}"), text)?;
    }
    for (stage, reason) in &changes.stages {
        stage_to_lead(ctx, *stage, reason)?;
    }
    Ok(())
}

/// Ends a page of stale items, oldest first: unended items older than their
/// node's newest one, which stand for no episode. Its own operation, so that
/// its writes are kept whatever the reconcile that follows answers.
pub(super) fn end_stale(ctx: &mut Context<'_>) -> Result<()> {
    if ctx.team.status != TeamStatus::Running {
        return Err(invalid(
            "Only a running team's unstaffed nodes are reconciled.",
        ));
    }
    let stale = store::unstaffed::stale(ctx.db, &ctx.team.id, store::unstaffed::STALE_PAGE)?;
    if stale.is_empty() {
        return Err(Problem::new(
            code::UNSTAFFED_UNCHANGED,
            "No stale unstaffed item waits to be ended; nothing was written.",
        ));
    }
    for seq in stale {
        ctx.db
            .execute(
                "UPDATE inbox SET state='closed',reply=?3,reply_actor=?4 WHERE team=?1 AND seq=?2",
                params![ctx.team.id, seq, STALE, encode(ctx.actor)?],
            )
            .map_err(storage)?;
    }
    Ok(())
}

/// The stage nodes of a readiness view, which are retired, never told.
pub fn unstaffed_stages(readiness: &Readiness) -> BTreeSet<u32> {
    readiness
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Stage)
        .map(|n| n.id)
        .collect()
}

/// An unstaffed stage goes the way a stage nobody could take goes at its
/// creation: it is taken from its pipeline as an override takes it (its
/// creation superseded), retired through the one retirement function, and
/// its parent is handed to the lead with the kernel's reason. A stage that is
/// no longer its parent's current one is retired alone.
fn stage_to_lead(ctx: &mut Context<'_>, stage: u32, reason: &str) -> Result<()> {
    let n = ctx.node(stage)?;
    let parent = n.stage_of.ok_or_else(missing)?;
    let current = pipeline::current_stage(ctx, stage, parent)?;
    ctx.db.execute("UPDATE stage_tasks SET state='superseded' WHERE team=?1 AND node=?2 AND state IN ('wanted','reserved','created')",params![ctx.team.id,stage]).map_err(storage)?;
    pipeline::retire_stage(ctx, stage)?;
    if current {
        let parent = ctx.node(parent)?;
        // The parent's hold while its stage runs is the pipeline's own
        // hand-back (`want_stage`); handing the run to the lead ends that
        // hand-back, as at the stage's creation, and never an explicit
        // hold: a person's or the lead's Hold stays its owner's.
        pipeline::to_lead(ctx, &parent, reason)?;
    }
    Ok(())
}

/// Every member of the team, bounded as the pipeline's staffing read is.
fn members(ctx: &Context<'_>) -> Result<Vec<Member>> {
    let mut s = ctx
        .db
        .prepare("SELECT data FROM members WHERE team=?1 ORDER BY id LIMIT 602")
        .map_err(storage)?;
    let members = s
        .query_map([&ctx.team.id], |r| store::sql_json::<Member>(r, 0))
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    if members.len() > 601 {
        return Err(over("Too many members in the unstaffed view."));
    }
    Ok(members)
}
