//! The phase pane is optional, on the same worker and the watched team's scope.
use super::*;
use crate::teams::view::phases::{self, PhaseFailure};

impl TeamsModel {
    /// Each explicit follow has an epoch, so a closed/reopened pane cannot
    /// accept an old read even when it selects the same team and phase again.
    pub fn follow_phases(
        &mut self,
        team: &str,
        phase: Option<u32>,
        enabled: bool,
        now: Instant,
    ) -> Option<(u64, Vec<(String, TeamsUpdate)>)> {
        if self.watched() != Some(team) {
            return None;
        }
        self.phase_epoch = self.phase_epoch.checked_add(1)?;
        self.phases_enabled = enabled;
        self.phase_selected = if enabled { phase } else { None };
        self.queued
            .retain(|r| !matches!(r, TeamRead::Phases { .. } | TeamRead::Draft { .. }));
        if let Some((_, parts)) = &mut self.parts {
            parts.phases = None;
            parts.draft = None;
            parts.phases_failure = None;
            parts.draft_failure = None;
        }
        for (slot, on) in [
            (Slot::Phases, enabled),
            (Slot::Draft, enabled && phase.is_some()),
        ] {
            if on {
                self.due.insert(slot, now);
                if self.in_flight.contains(&slot) {
                    self.dirty.insert(slot);
                }
            } else {
                self.due.remove(&slot);
                self.dirty.remove(&slot);
            }
        }
        if !enabled {
            self.selected.remove(&team_key("phases", team));
        }
        let mut updates = Vec::new();
        if let Some(view) = self.team_view() {
            self.send(
                team_key("team", team),
                TeamsUpdate::Team(Arc::new(view)),
                &mut updates,
            );
        }
        Some((self.phase_epoch, updates))
    }

    pub(super) fn issue_phase(&mut self, slot: Slot) {
        if matches!(slot, Slot::Phases | Slot::Draft) {
            self.phase_issued.insert(slot, self.phase_epoch);
        }
    }

    pub(super) fn apply_phase_read(
        &mut self,
        read: &TeamRead,
        reply: Result<Value, TeamError>,
        now: Instant,
    ) -> Vec<(String, TeamsUpdate)> {
        let mut updates = Vec::new();
        let slot = Slot::of(read);
        let Some(team) = read.team() else {
            return updates;
        };
        let current = self.phase_issued.remove(&slot) == Some(self.phase_epoch);
        if !self.phases_enabled || self.watched() != Some(team) {
            return updates;
        }
        if !current || matches!(read,TeamRead::Draft{phase,..} if self.phase_selected!=Some(*phase))
        {
            if self.due.contains_key(&slot) {
                self.due.insert(slot, now);
            }
            return updates;
        }
        self.settled(slot, now);
        let unsupported = matches!(&reply,Err(TeamError::Refused{code,..}) if code=="not_found");
        if unsupported {
            self.due.remove(&slot);
            if slot == Slot::Phases {
                self.due.remove(&Slot::Draft);
                self.queued
                    .retain(|r| !matches!(r, TeamRead::Phases { .. } | TeamRead::Draft { .. }));
            }
        }
        if self.parts.is_none() {
            self.parts = Some((team.to_owned(), Parts::default()));
        }
        if let Some((_, parts)) = &mut self.parts {
            match read {
                TeamRead::Phases { .. } => match reply {
                    Ok(value) => match phases::list(&value) {
                        Ok(view) => {
                            parts.phases = Some(view);
                            parts.phases_failure = None;
                        }
                        Err(message) => {
                            parts.phases_failure = Some(PhaseFailure {
                                code: "invalid_read".into(),
                                message,
                            })
                        }
                    },
                    Err(error) => {
                        if read.after().is_some() && matches!(&error, TeamError::Refused { .. }) {
                            self.selected.remove(&team_key("phases", team));
                            if !unsupported {
                                self.due.insert(slot, now);
                            }
                        }
                        parts.phases_failure = Some(PhaseFailure {
                            code: error.code().into(),
                            message: error.message().into(),
                        });
                    }
                },
                TeamRead::Draft { phase, .. } => match reply {
                    Ok(value) => match phases::draft(&value, *phase) {
                        Ok(view) => {
                            parts.draft = Some(view);
                            parts.draft_failure = None;
                        }
                        Err(message) => {
                            parts.draft_failure = Some(PhaseFailure {
                                code: "invalid_read".into(),
                                message,
                            })
                        }
                    },
                    Err(error) => {
                        parts.draft_failure = Some(PhaseFailure {
                            code: error.code().into(),
                            message: error.message().into(),
                        })
                    }
                },
                _ => {}
            }
        }
        if let Some(view) = self.team_view() {
            self.send(
                team_key("team", team),
                TeamsUpdate::Team(Arc::new(view)),
                &mut updates,
            );
        }
        updates
    }
}
