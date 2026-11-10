//! Task 259 (owner, 2026-11-08): no idle member while a ready task with free
//! outputs waits. The dispatcher's same-role rule kept a ready task on its
//! busy assignee when the only free peer was another role (ui1's Frontend
//! SolidJS and ui2/ui3's Frontend DeepSeek were different roles), so members
//! idled for 20-60 minutes and the lead rebalanced by hand.
//!
//! A ready task now goes to an idle member of a compatible role: the same one,
//! or one doing the same kind of work (a role sharing its first word). Rights
//! decide (task 154); the exact role and the plan's assignee only order the
//! choice, and the plan's assignee is the preference - it is launched for the
//! task instead of being replaced whenever it has not started yet.
use super::model::{related_roles, InProgressOutputs, Member, Task, Team};

/// The member a ready task may move to now: `None` when its own member can
/// take it, or when no compatible member can. `available` is the dispatcher's
/// per-tick test (free, not handed another task, not held by a stage); the
/// member's own launch rights and work in progress are rechecked here.
pub(crate) fn peer<'a>(
    team: &'a Team,
    task: &Task,
    from: &Member,
    started: bool,
    available: &dyn Fn(&Member) -> bool,
    outputs: &InProgressOutputs,
) -> Option<&'a Member> {
    let role = team.role_of(&from.id);
    let compatible = |other: &Member| {
        let found = team.role_of(&other.id);
found == role || (started && related_roles(&role, &found))
    };
    team.members
        .iter()
        .enumerate()
        .filter(|(_, member)| member.id != from.id && member.id != team.lead)
        .filter(|(_, member)| compatible(member))
        .filter(|(_, member)| available(member) && !task.failed_by(&member.id))
        .filter(|(_, member)| super::stages::held(team, &member.id).is_none() && !outputs.blocks_for(&member.id, task))
        .filter(|(_, member)| super::rights::missing(task, member).is_empty())
        .min_by_key(|(index, member)| (if team.role_of(&member.id) == role { 0 } else { 1 }, *index))
        .map(|(_, member)| member)
}
