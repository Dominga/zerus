//! No estimate cache: retirement of the one observed page removes this output.
use super::*;
use crate::teams::{estimate_comparison, estimate_projection::EstimateContext};

impl TeamsModel {
    pub(super) fn estimates(&self) -> Option<estimate_comparison::EstimatePane> {
        if self.hidden || !self.phases_enabled {
            return None;
        }
        let observed = self.observed_page.as_ref()?;
        if observed.epoch != self.observed_lease || !self.observed_scope_matches(&observed.page) {
            return None;
        }
        let (team, parts) = self.parts.as_ref()?;
        if self.watched() != Some(team)
            || parts.phases_failure.is_some()
            || parts.draft_failure.is_some()
        {
            return None;
        }
        let status = parts.status.as_ref()?.get("team")?;
        let draft = parts.draft.as_ref()?;
        let revision = status.get("revision")?.as_u64()?;
        if status.get("id")?.as_str()? != team
            || revision != draft.revision
            || revision != observed.page.source_revision
            || self.phase_selected != Some(draft.phase.n)
        {
            return None;
        }
        let context = EstimateContext {
            team: team.clone(),
            home: status.get("home")?.as_str()?.into(),
            revision,
        };
        estimate_comparison::project(&context, draft, observed)
    }
}

#[cfg(test)]
mod tests;
