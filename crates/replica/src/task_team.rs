//! Optional team routing metadata. This field deliberately has no schema
//! upgrade: an older reader preserves unknown field-map entries on edits.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTeam {
    pub team: String,
    pub home: String,
}

/// Forward-compatible metadata is parsed only within this UTF-8 byte bound.
const MAX_TEAM_FIELD_BYTES: usize = 2048;

impl TaskTeam {
    fn validate(&self) -> Result<()> {
        if !ids::is_uuid(&self.team) || !ids::is_uuid(&self.home) {
            return Err(Error::new("a task team needs team and home computer UUIDs"));
        }
        Ok(())
    }
}

impl TaskDoc {
    /// The caller proves the team's stored acceptance before entering here.
    /// No report is invented on behalf of the attempt's holder.
    pub fn finish_team_accepted(&mut self, actor: &Actor, now: i64) -> Result<()> {
        self.writable()?;
        let key = self
            .team()?
            .ok_or_else(|| Error::new("A valid team key is required."))?;
        if !matches!(actor, Actor::Agent(session) if session.machine == key.home && !session.session_id.is_empty())
        {
            return Err(Error::new(
                "The team finish must be recorded as its local lead.",
            ));
        }
        if self.execution_managed()
            && (!self.executes_attempts()
                || self.needs_schema_repair()
                || (self.meta().get("floor").is_some() && self.floor_record().is_none()))
        {
            return Err(Error::new(
                "This task is cutting over to execution attempts.",
            ));
        }
        let view = self.view()?;
        if view.cancelled_at.is_some() || !matches!(view.stage, Stage::Active | Stage::Review) {
            return Err(Error::new(
                "Only an active or reviewed team task can finish.",
            ));
        }
        self.not_held()?;
        let execution = view
            .execution
            .as_ref()
            .filter(|e| !e.conflict)
            .ok_or_else(|| Error::new("Exactly one current recorded attempt is required."))?;
        let current = execution
            .current
            .as_ref()
            .filter(|c| !c.legacy && !c.id.is_empty())
            .ok_or_else(|| Error::new("Exactly one current recorded attempt is required."))?;
        let phase = match view.stage {
            Stage::Active => matches!(
                current.phase,
                execution::Phase::Submitted | execution::Phase::Running
            ),
            Stage::Review => current.phase == execution::Phase::Ready,
            _ => false,
        };
        if !phase {
            return Err(Error::new(
                "The current attempt cannot be finished by the team.",
            ));
        }
        if self.content_len() + 1024 > MAX_DOCUMENT {
            return Err(Error::new("task history is full"));
        }
        // All rule refusals precede mutation. Ready closes this exact attempt;
        // the ordinary stage entry and readable team sentence preserve both
        // existing history forms, without a synthetic holder report.
        if view.stage == Stage::Active {
            self.push_event(&current.id, execution::EventKind::Ready, actor, now)?;
        }
        self.fields().insert("stage", Stage::Done.as_str())?;
        self.fields().insert("stage_at", now)?;
        self.log("stage", actor, now, Stage::Done.as_str(), None)?;
        self.log("team", actor, now, "Finished by the team", None)?;
        self.commit()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_raw_team_for_test(&mut self, raw: &str) {
        self.fields().insert("team", raw).unwrap();
        self.doc.commit();
    }
    fn team_field(&self) -> (Option<TaskTeam>, bool) {
        let Some(value) = self.fields().get("team") else {
            return (None, false);
        };
        let Ok(LoroValue::String(raw)) = value.into_value() else {
            return (None, true);
        };
        if raw.len() > MAX_TEAM_FIELD_BYTES {
            return (None, true);
        }
        match serde_json::from_str::<TaskTeam>(&raw) {
            Ok(team) if team.validate().is_ok() => (Some(team), false),
            _ => (None, true),
        }
    }

    pub fn team(&self) -> Result<Option<TaskTeam>> {
        Ok(self.team_field().0)
    }

    /// An invalid marker is preserved and exposed, never treated as damage
    /// to the task or as permission to bypass a team.
    pub fn team_invalid(&self) -> bool {
        self.team_field().1
    }

    /// Changes only the optional field and its history. Target and schema
    /// stay independent; a hand-out still uses the ordinary task writer.
    pub fn set_team(&mut self, team: Option<&TaskTeam>, actor: &Actor, now: i64) -> Result<()> {
        let name = team
            .cloned()
            .or_else(|| self.team_field().0)
            .map_or_else(|| "unknown team".into(), |t| t.team);
        self.set_team_named(team, &name, actor, now)
    }

    pub fn set_team_named(
        &mut self,
        team: Option<&TaskTeam>,
        name: &str,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        self.writable()?;
        if let Some(team) = team {
            team.validate()?;
        }
        if name.trim().is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
            return Err(Error::new("invalid team display name"));
        }
        if !self.team_invalid() && self.team()?.as_ref() == team {
            // An unchanged binding still clears a stale mark: the team takes
            // the task anew (zerus-63rru.36 round 2). A repeat of a task
            // without a mark stays the no-op it was.
            if self.fields().get("cancelled_at").is_some() {
                self.fields().delete("cancelled_at")?;
                self.fields().delete("cancel_reason")?;
            }
            return Ok(());
        }
        if self.content_len() + 1024 > MAX_DOCUMENT {
            return Err(Error::new("task history is full"));
        }
        let raw = team
            .map(serde_json::to_string)
            .transpose()?
            .unwrap_or_default();
        if team.is_some() {
            self.fields().insert("team", raw.as_str())?;
            // A task that a team takes is not cancelled: the mark and its
            // reason go with the binding, in the same commit
            // (zerus-63rru.36). The ordinary send already clears them.
            self.fields().delete("cancelled_at")?;
            self.fields().delete("cancel_reason")?;
        } else {
            self.fields().delete("team")?;
        }
        let text = format!(
            "{} team {}",
            if team.is_some() {
                "Sent to"
            } else {
                "Removed from"
            },
            name.trim()
        );
        self.log("team", actor, now, &text, None)?;
        self.commit()
    }

    /// Clears the team key without a history entry (zerus-63rru.32 round 2):
    /// a closed task's history stays as it was, only the key changes. The
    /// revision advances, which is inherent to any replica write and is how
    /// the clear reaches other computers. No display name or actor enters
    /// the document. `None` is already clear: a no-op.
    pub fn clear_team_silent(&mut self) -> Result<()> {
        self.writable()?;
        if !self.team_invalid() && self.team()?.is_none() {
            return Ok(());
        }
        if self.content_len() + 1024 > MAX_DOCUMENT {
            return Err(Error::new("task history is full"));
        }
        self.fields().delete("team")?;
        self.commit()
    }
}
