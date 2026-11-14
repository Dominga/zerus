use super::*;
use crate::teams::store::{actor_reads::Admission, Decision};
impl Native {
    pub(super) fn admit(
        &self,
        expected: &Team,
        node: &Node,
        member: Option<&Member>,
        id: &str,
        decision: Option<&Decision>,
    ) -> Result<Admission> {
        let store = Store::open(&self.config.state)?.admission()?;
        let team = store
            .team(&expected.id)?
            .ok_or_else(|| problem("Team disappeared"))?;
        if team.home != self.machine
            || team.status != TeamStatus::Running
            || team.owner != expected.owner
            || team.project != expected.project
        {
            return Err(problem("Team effect admission changed"));
        }
        let current = store
            .node(&team.id, node.id)?
            .ok_or_else(|| problem("Node disappeared"))?;
        if current.task != node.task {
            return Err(problem("Effect task binding changed"));
        }
        if let Some(member) = member {
            self.admit_delivery(&store, &team, node, member, id)?;
        } else {
            self.admit_finish(
                &store,
                &team,
                &current,
                decision.ok_or_else(|| problem("Finishing permit is missing"))?,
            )?;
        }
        if let Some(member) = member {
            if !super::super::availability::free(
                &self.config,
                &self.epoch,
                &team.id,
                member,
                false,
            )? {
                return Err(problem(
                    "Member is no longer live and idle in the exact current run",
                ));
            }
        }
        Ok(store)
    }
    fn admit_delivery(
        &self,
        store: &Admission,
        team: &Team,
        node: &Node,
        member: &Member,
        id: &str,
    ) -> Result<()> {
        if member.state != MemberState::Active
            || store.member(&team.id, &member.id)?.as_ref() != Some(member)
        {
            return Err(problem("Member effect admission changed"));
        }
        let generation = store.generation(&team.id, node.id)?;
        let mut cursor = None;
        for _ in 0..4 {
            let page = store.open_effects(&team.id, 200, cursor.as_ref())?;
            if let Some(effect) = page.rows.iter().find(|e| {
                e.request_id == id
                    && e.node == node.id
                    && e.member == member.id
                    && Some(e.generation) == generation
                    && e.outcome.is_none()
                    && e.state == "reserved"
            }) {
                if effect.kind == "handout" {
                    store.handout_admissible(team, node.id, &member.id)?;
                } else if effect.kind == "reopen"
                    && !store.own_task_current(&team.id, node.id, effect.generation)?
                {
                    return Err(problem("The reopened task's reviewed attempt changed"));
                }
                return Ok(());
            }
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        Err(problem("Delivery effect is not reserved"))
    }
    fn admit_finish(
        &self,
        store: &Admission,
        team: &Team,
        node: &Node,
        decision: &Decision,
    ) -> Result<()> {
        if decision.node != node.id
            || store.generation(&team.id, node.id)? != Some(decision.generation)
        {
            return Err(problem("Finishing generation changed"));
        }
        if !store.reviewed_task_current(&team.id, node.id, decision.generation)? {
            return Err(problem("The accepted task's reviewed attempt changed"));
        }
        if team.settings.integration == Integration::Branch
            && node.kind != NodeKind::Stage
            && !node.integrated
        {
            return Err(problem("Finishing integration fact is missing"));
        }
        let mut cursor = None;
        for _ in 0..4 {
            let page = store.pending_accepts(&team.id, 200, cursor.as_ref())?;
            if page.rows.iter().any(|d| {
                d.seq == decision.seq && d.node == node.id && d.generation == decision.generation
            }) {
                return Ok(());
            }
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        Err(problem("Task has no exact Ready acceptance"))
    }
}
