use super::super::super::ledger::{Ledger as Launches, State};
use super::*;

pub(in crate::desktop::teams::confirm) fn launch_binding(
    config: &Config,
    team: &str,
    member: &str,
) -> Result<(Binding, String)> {
    let row = Launches::open(&config.state)
        .map_err(|e| e.to_string())?
        .get(team, member)
        .map_err(|e| e.to_string())?
        .ok_or("Member has no launch provenance")?;
    if !matches!(row.state, State::Bound | State::ParentIntent | State::Done) {
        return Err("The launch is not bound".into());
    }
    let p = row.plan.ok_or("Missing launch plan")?;
    let s = row.observed.ok_or("Missing bound native session")?;
    let tree = row.worktree.ok_or("Missing created worktree")?;
    if p.team != team
        || p.member != member
        || p.launch_id != s.launch_id
        || p.machine != s.machine
        || p.agent != s.agent
        || p.account != s.account
        || tree != s.worktree
        || tree.path != p.placement.destination
        || s.session_id.is_empty()
        || s.run_id.is_empty()
    {
        return Err("The original member binding is inconsistent".into());
    }
    Ok((
        Binding {
            team: p.team,
            member: p.member,
            person: p.person,
            machine: p.machine,
            role: p.role,
            agent: p.agent,
            account: p.account,
            launch: p.launch_id,
            session: s.session_id,
            run: s.run_id,
            conversation: s.conversation_id,
            folder: tree.path,
            common: p.placement.git_dir,
            nonce: tree.ownership_nonce,
        },
        s.name,
    ))
}
impl Native<'_> {
    pub fn binding(&self, team: &str, member: &str) -> Result<(Binding, String)> {
        let (initial, name) = launch_binding(self.config, team, member)?;
        let Some(latest) = self.ledger.binding(&initial)? else {
            return Ok((initial, name));
        };
        let mut stem = latest.clone();
        stem.run = initial.run.clone();
        stem.conversation = initial.conversation.clone();
        if stem != initial {
            return Err("The resumed binding has different launch provenance".into());
        }
        Ok((latest, name))
    }
    pub fn fresh_facts(&self, b: &Binding) -> Result<Facts> {
        let store = Store::open(&self.config.state).map_err(|e| e.to_string())?;
        let team = store
            .team(&b.team)
            .map_err(|e| e.to_string())?
            .ok_or("Team disappeared")?;
        let member = store
            .member(&b.team, &b.member)
            .map_err(|e| e.to_string())?
            .ok_or("Member disappeared")?;
        let binding = self.binding(&b.team, &b.member)?.0;
        Ok(Facts {
            team,
            member,
            binding,
            preview: crate::teams::enabled(&self.config.state)
                && !self.stop.load(Ordering::Acquire)
                && b.machine == self.machine,
        })
    }
}
