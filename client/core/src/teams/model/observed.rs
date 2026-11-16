//! One lazy observation page. The lease and read-retirement epochs are distinct.
use super::*;
use crate::teams::observed_timing;

#[derive(Clone, Debug)]
pub(super) struct Selection {
    phase: u32,
    node: Option<u32>,
    window: u64,
    plan_generation: Option<u64>,
}
impl TeamsModel {
    pub fn follow_observed(
        &mut self,
        team: &str,
        phase: u32,
        node: Option<u32>,
        window: u64,
        enabled: bool,
        expected: Option<u64>,
        now: Instant,
    ) -> Option<(u64, Vec<(String, TeamsUpdate)>)> {
        if self.watched() != Some(team)
            || !self.phases_enabled
            || self.phase_selected != Some(phase)
            || !(1..=observed_timing::MAX_WINDOW_MS).contains(&window)
        {
            return None;
        }
        if !enabled && expected != Some(self.observed_lease) {
            return Some((self.observed_lease, Vec::new()));
        }
        self.observed_lease = self.observed_lease.checked_add(1)?;
        self.observed_selection = enabled.then_some(Selection {
            phase,
            node,
            window,
            plan_generation: self
                .parts
                .as_ref()
                .and_then(|(_, p)| p.draft.as_ref())
                .and_then(|d| d.generation),
        });
        let updates = self.retire_observed(now);
        Some((self.observed_lease, updates))
    }
    pub(super) fn retire_observed(&mut self, now: Instant) -> Vec<(String, TeamsUpdate)> {
        self.retire_observed_state(now);
        self.observed_update()
    }
    pub(super) fn retire_observed_state(&mut self, now: Instant) {
        self.observed_generation = self.observed_generation.saturating_add(1);
        self.observed_page = None;
        self.queued
            .retain(|r| !matches!(r, TeamRead::Observed { .. }));
        if let Some(team) = self.follow.team.as_ref() {
            self.selected.remove(&team_key("observed", team));
        }
        if self.observed_selection.is_some() && !self.hidden {
            self.due.insert(Slot::Observed, now);
        } else {
            self.due.remove(&Slot::Observed);
        }
        self.dirty.remove(&Slot::Observed);
    }
    pub(crate) fn observed_update(&mut self) -> Vec<(String, TeamsUpdate)> {
        let mut updates = Vec::new();
        if let (Some(team), Some(view)) = (self.follow.team.clone(), self.team_view()) {
            self.send(
                team_key("team", &team),
                TeamsUpdate::Team(Arc::new(view)),
                &mut updates,
            );
        }
        updates
    }
    pub(super) fn observed_read(&self) -> Option<TeamRead> {
        let selection = self.observed_selection.as_ref()?;
        let team = self.follow.team.clone()?;
        Some(TeamRead::Observed {
            after: self.selected.get(&team_key("observed", &team)).cloned(),
            team,
            phase: selection.phase,
            node: selection.node,
            window_ms: selection.window,
        })
    }
    pub(super) fn observed_page_request(&mut self, key: &str, cursor: Option<&str>) -> bool {
        if self.hidden
            || self.observed_selection.is_none()
            || key.strip_prefix("observed:") != self.watched()
        {
            return false;
        }
        if let Some(cursor) = cursor {
            if cursor.is_empty()
                || cursor.len() > observed_timing::MAX_CURSOR_BYTES
                || cursor.starts_with('-')
                || cursor.chars().any(|c| c.is_control() || c.is_whitespace())
            {
                return false;
            }
        }
        // Retire only the read, preserving the UI cleanup lease.
        self.observed_generation = self.observed_generation.saturating_add(1);
        self.observed_page = None;
        if let Some(cursor) = cursor {
            self.selected.insert(key.into(), cursor.into());
        } else {
            self.selected.remove(key);
        }
        if let Some(read) = self.observed_read() {
            self.queued
                .retain(|r| !matches!(r, TeamRead::Observed { .. }));
            self.queued.push_back(read);
            true
        } else {
            false
        }
    }
    pub(super) fn apply_observed(
        &mut self,
        read: &TeamRead,
        reply: Result<Value, TeamError>,
        now: Instant,
    ) -> Vec<(String, TeamsUpdate)> {
        let TeamRead::Observed {
            team,
            phase,
            node,
            window_ms,
            after,
        } = read
        else {
            return Vec::new();
        };
        if self.observed_issued.take() != Some(self.observed_generation)
            || self.hidden
            || self.watched() != Some(team)
            || self.observed_read().as_ref() != Some(read)
        {
            return Vec::new();
        }
        self.settled(Slot::Observed, now);
        self.observed_page = None;
        let unsupported = matches!(&reply,Err(TeamError::Refused{code,..}) if code=="not_found");
        if let Ok(value) = reply {
            if let Ok(page) = observed_timing::decode(&value, team, *phase, *node, *window_ms) {
                if self.observed_scope_matches(&page) {
                    self.observed_page = Some(observed_timing::project(
                        page,
                        self.observed_lease,
                        after.clone(),
                    ));
                }
            }
        }
        if self.observed_page.is_none() {
            // A failed page is unavailable; a later fresh read cannot resurrect
            // the selected old cursor or any comparison eligibility.
            let updates = self.retire_observed(now);
            if unsupported {
                self.due.remove(&Slot::Observed);
            } else if self.observed_selection.is_some() && !self.hidden {
                self.due.insert(Slot::Observed, now + POLL);
            }
            return updates;
        }
        self.observed_update()
    }
    fn observed_scope_matches(&self, page: &observed_timing::ObservedTimingPage) -> bool {
        let Some((_, parts)) = &self.parts else {
            return false;
        };
        let home = parts
            .status
            .as_ref()
            .and_then(|v| v.get("team"))
            .and_then(|v| v.get("home"))
            .and_then(Value::as_str);
        if !self.observed_status_verified
            || home != Some(page.scope.home.as_str())
            || parts.draft_failure.is_some()
            || parts.phases_failure.is_some()
        {
            return false;
        }
        let Some(draft) = parts.draft.as_ref() else {
            return false;
        };
        let publication = draft
            .publication
            .as_ref()
            .map(|p| observed_timing::PublicationKey {
                id: p.id.clone(),
                draft_generation: p.generation,
            });
        self.observed_selection
            .as_ref()
            .is_some_and(|s| s.plan_generation == draft.generation)
            && draft.phase.n == page.scope.phase
            && publication == page.scope.publication
    }
    pub(super) fn check_observed_scope(&mut self, now: Instant) -> Vec<(String, TeamsUpdate)> {
        let generation = self
            .parts
            .as_ref()
            .and_then(|(_, p)| p.draft.as_ref())
            .and_then(|d| d.generation);
        let changed = self
            .observed_selection
            .as_ref()
            .is_some_and(|s| s.plan_generation != generation);
        let failed = self
            .parts
            .as_ref()
            .is_some_and(|(_, p)| p.draft_failure.is_some() || p.phases_failure.is_some());
        if failed
            || changed
            || self
                .observed_page
                .as_ref()
                .is_some_and(|p| !self.observed_scope_matches(&p.page))
        {
            if let Some(selected) = self.observed_selection.as_mut() {
                selected.plan_generation = generation;
            }
            self.retire_observed(now)
        } else {
            Vec::new()
        }
    }
    pub(crate) fn observed_hidden(&mut self) -> Vec<(String, TeamsUpdate)> {
        self.observed_update()
    }
}
