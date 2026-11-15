//! An explicit stopped-seat edit reserves a new original and reuses only its
//! proven existing checkout. It never continues the prior original.
use super::*;
impl Runtime {
    pub(super) fn edited_member(
        &mut self,
        team: &Team,
        member: &Member,
        old: &Row,
        now: i64,
    ) -> Result<bool> {
        let generation = member.launch.as_ref().map_or(0, |l| l.generation);
        if member.state != MemberState::NotCreated
            || member.session.is_some()
            || old.member_generation.unwrap_or(0).checked_add(1) != Some(generation)
        {
            return Err(invalid(
                "Member edit generation changed; no launch was admitted.",
            ));
        }
        let p = old
            .plan
            .as_ref()
            .ok_or_else(|| invalid("The previous launch plan is missing."))?;
        let mut previous = member.clone();
        previous.agent = p.agent.clone();
        previous.session = Some(SessionKey {
            machine: p.machine.clone(),
            session_id: format!("launch_id:{}", p.launch_id),
        });
        super::super::member_swap::known(old, team, &previous)?;
        let mut next = Row::new(team, &member.id, &self.epoch, State::Retry);
        next.member_generation = Some(generation);
        next.reuse = Some(super::super::ledger::Reuse {
            placement: p.placement.clone(),
            worktree: old
                .worktree
                .clone()
                .ok_or_else(|| invalid("The previous checkout is missing."))?,
            launch: p.launch_id.clone(),
        });
        self.ledger().replace_stopped(old, &mut next, now)?;
        self.admitted = None;
        Ok(true)
    }
}
