//! One bounded native first-send observation, outside the spawn admission.
use super::*;

pub(super) struct FirstReady {
    identity: [String; 4],
}
fn identity(row: &Value) -> [String; 4] {
    let named = super::super::fleet::Identity::of(row);
    [named.name, named.run, named.launch, named.agent].map(str::to_owned)
}
impl FirstReady {
    pub(super) fn matches(&self, row: &Value) -> bool {
        super::super::fleet::Facts::of(row).first_candidate() && self.identity == identity(row)
    }
}
impl Native {
    pub(super) fn first_on(&self, member: &Member, row: &Value) -> Option<FirstReady> {
        let launch = member
            .session
            .as_ref()?
            .session_id
            .strip_prefix("launch_id:")?;
        let named = identity(row);
        if launch.is_empty()
            || named[2] != launch
            || named[3] != member.agent
            || !matches!(member.agent.as_str(), "claude" | "codex" | "kimi")
            || !super::super::fleet::Facts::of(row).first_candidate()
        {
            return None;
        }
        if named.iter().any(String::is_empty) {
            return None;
        }
        let inspected = self
            .read(
                &["inspect".into(), named[0].clone(), "--json".into()],
                Instant::now() + Duration::from_secs(2),
            )
            .ok()?;
        // This flag is written by input::first_message_summary, which calls
        // checked_input and checked_terminal(true), including the composer
        // and wrapped-footer checks. Never infer readiness from unknown status.
        let facts = super::super::fleet::Facts::of(&inspected);
        if inspected["first_message_can_send"] != true
            || identity(&inspected) != named
            || !facts.fresh
            || facts.background.is_some_and(|n| n > 0)
            || facts.children.is_some_and(|n| n > 0)
        {
            return None;
        }
        Some(FirstReady { identity: named })
    }

    pub(super) fn first_witness(&self, team: &Team, member: &Member) -> Result<Option<FirstReady>> {
        Ok(
            super::super::availability::record(&self.config, &self.epoch, &team.id, member, false)?
                .and_then(|row| self.first_on(member, &row)),
        )
    }
}
